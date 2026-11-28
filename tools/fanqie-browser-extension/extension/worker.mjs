import { draftPage } from "./draft-page.mjs";
import { deliverDraft } from "./draft-run.mjs";

let port,
  timer,
  running = false,
  serial = 0,
  status = "未连接墨卷";
const replies = new Map();
async function rpc(method, params = {}) {
  const connection = port;
  const deadline = Date.now() + 65_000;
  for (;;) {
    try {
      return await request(method, params);
    } catch (e) {
      if (
        e.code !== "rate_limited" ||
        Date.now() >= deadline ||
        port !== connection ||
        !port
      )
        throw e;
      await new Promise((resolve) => setTimeout(resolve, 2000));
      if (port !== connection) throw Error("本机连接已改变");
    }
  }
}
function request(method, params) {
  if (!port) return Promise.reject(Error("请先连接墨卷"));
  return new Promise((resolve, reject) => {
    const id = ++serial;
    const timeout = setTimeout(() => {
      replies.delete(id);
      reject(Error("本机连接超时"));
    }, 15_000);
    replies.set(id, { resolve, reject, timeout });
    try {
      port.postMessage({ id, method, params });
    } catch (e) {
      replies.delete(id);
      clearTimeout(timeout);
      reject(e);
    }
  });
}
function connect() {
  if (port) return;
  port = chrome.runtime.connectNative("com.mojuan.fanqie");
  status = "请在墨卷的智能体连接面板授权此扩展";
  port.onMessage.addListener(({ id, reply }) => {
    const p = replies.get(id);
    if (!p) return;
    clearTimeout(p.timeout);
    replies.delete(id);
    if (reply.error) {
      p.reject(
        Object.assign(Error(reply.error.message), { code: reply.error.code }),
      );
      if (reply.error.code === "invalid_session") {
        port.disconnect();
        port = null;
        clearInterval(timer);
        status = "授权失效，请重新连接并在墨卷授权";
      }
    } else p.resolve(reply.result);
  });
  port.onDisconnect.addListener(() => {
    status =
      chrome.runtime.lastError?.message || "连接已断开，请重新连接并授权";
    port = null;
    clearInterval(timer);
    for (const p of replies.values()) {
      clearTimeout(p.timeout);
      p.reject(Error(status));
    }
    replies.clear();
  });
  timer = setInterval(() => void tick(), 4000);
  void tick();
}
function writerUrl(value) {
  const url = new URL(value);
  if (
    url.origin !== "https://fanqienovel.com" ||
    !url.pathname.startsWith("/main/writer/") ||
    url.username ||
    url.password
  )
    throw Error("请打开番茄作家后台的草稿箱");
  return url.href;
}
async function step(tabId, action, job, expectedEditor) {
  const tab = await chrome.tabs.get(tabId);
  const url = writerUrl(tab.url);
  if (expectedEditor && url !== expectedEditor) throw Error("章节编辑页已改变");
  if (tab.status !== "complete") throw Error("正在载入番茄页面");
  const [reply] = await chrome.scripting.executeScript({
    target: { tabId },
    func: draftPage,
    args: [
      action,
      ["inspect", "fill", "save"].includes(action)
        ? job
        : { target: job.target, number: job.number },
      url,
    ],
  });
  if (!reply?.result?.ok) throw Error(reply?.result?.error ?? "番茄页面已改变");
  return reply.result.value;
}
async function execute(job) {
  const tabs = [];
  async function listPage(url) {
    const tab = await chrome.tabs.create({
      url: writerUrl(url),
      active: false,
    });
    tabs.push(tab.id);
    const listIdentity = (value) => {
      const parsed = new URL(writerUrl(value));
      for (const key of ["page", "pageIndex", "pageNo", "page_num"])
        parsed.searchParams.delete(key);
      return parsed.href;
    };
    let editorUrl;
    return {
      step: async (action) => {
        if (action === "inspect") {
          const url = writerUrl((await chrome.tabs.get(tab.id)).url);
          const result = await step(tab.id, action, job, url);
          editorUrl = url;
          return result;
        }
        if (
          ["list", "new", "open", "next"].includes(action) &&
          listIdentity((await chrome.tabs.get(tab.id)).url) !==
            listIdentity(url)
        )
          throw Error("已离开绑定的草稿箱");
        return step(
          tab.id,
          action,
          job,
          ["fill", "save"].includes(action) ? editorUrl : undefined,
        );
      },
    };
  }
  let result;
  try {
    if (job.body_bytes !== undefined) {
      const chunks = [];
      let cursor = 0;
      while (cursor < job.body_bytes) {
        const part = await rpc("browser_body_chunk", {
          job_id: job.job_id,
          claim: job.claim,
          cursor,
        });
        if (part.next <= cursor || part.next > job.body_bytes)
          throw Error("墨卷章稿分段异常");
        chunks.push(part.text);
        cursor = part.next;
      }
      job.body = chunks.join("");
    }
    result = await deliverDraft(job, {
      listPage,
      validate: () =>
        rpc("browser_validate", { job_id: job.job_id, claim: job.claim }),
    });
  } catch (e) {
    result = { state: "review", detail: String(e.message).slice(0, 240) };
  }
  await rpc("browser_finish", {
    job_id: job.job_id,
    claim: job.claim,
    ...result,
  });
  status = result.detail;
  // Retain the last page for inspection if the site needs attention.
  for (const id of result.state === "review" ? tabs.slice(0, -1) : tabs) {
    try {
      writerUrl((await chrome.tabs.get(id)).url);
      await chrome.tabs.remove(id);
    } catch {}
  }
}
async function tick() {
  if (running || !port) return;
  running = true;
  try {
    const session = await rpc("session_status");
    if (session.status !== "granted") {
      status = "请在墨卷授权浏览器连接";
      return;
    }
    const job = await rpc("browser_claim");
    if (job) await execute(job);
    else if (status.includes("授权")) status = "已连接，等候指定章节";
  } catch (e) {
    status = e.message;
  } finally {
    running = false;
  }
}
chrome.runtime.onMessage.addListener((msg, sender, respond) => {
  if (
    sender.id !== chrome.runtime.id ||
    sender.url !== chrome.runtime.getURL("popup.html")
  )
    return false;
  (async () => {
    switch (msg.action) {
      case "connect":
        connect();
        return status;
      case "state":
        return status;
      case "volumes":
        return rpc("browser_volumes");
      case "bind": {
        const [tab] = await chrome.tabs.query({
          active: true,
          currentWindow: true,
        });
        const list_url = writerUrl(tab.url);
        const target = { ...msg.target, list_url };
        await step(tab.id, "list", { target });
        await rpc("browser_bind", target);
        return "已绑定本卷。可以在 Codex 中点名存稿。";
      }
      default:
        throw Error("未知操作");
    }
  })().then(
    (value) => respond({ value }),
    (e) => respond({ error: e.message }),
  );
  return true;
});
