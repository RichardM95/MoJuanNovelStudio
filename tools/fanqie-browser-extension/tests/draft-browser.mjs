import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { draftPage } from "../extension/draft-page.mjs";
import { deliverDraft } from "../extension/draft-run.mjs";
const { chromium } = createRequire(import.meta.url)(
  process.env.PLAYWRIGHT_MODULE || "playwright",
);
const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM_PATH || undefined,
});
const target = {
  book: "试验书",
  volume: "第一卷",
  list_url: "https://fanqienovel.com/main/writer/draft-list",
};
const body = "「第一段。」🙂\n\n第二段 <script> & 正文";
const job = { target, number: "12", title: "潮声", body, verify_only: false };
let saved = new Map(),
  saves = 0,
  opens = 0;
const esc = (s) =>
  s.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll('"', "&quot;");
const context = await browser.newContext();
await context.route("**/*", async (route) => {
  const u = new URL(route.request().url());
  if (u.origin !== "https://fanqienovel.com") return route.abort();
  let html =
    "<header><h1>试验书</h1><span aria-current='true'>第一卷</span></header>";
  if (u.pathname.endsWith("/draft-list")) {
    html += "<h2>草稿箱</h2><span>共 " + saved.size + " 章</span><ul>";
    for (const n of saved.keys())
      html += "<li>第 " + n + " 章 <a href='edit?n=" + n + "'>编辑</a></li>";
    html += "</ul><a href='edit'>新建章节</a>";
  } else if (u.pathname.endsWith("/edit")) {
    const n = u.searchParams.get("n");
    const chapter = saved.get(n) ?? { title: "未命名草稿", body: "" };
    if (n === null) opens++;
    html +=
      "<p>正文字数 0</p><div>第<input id=n value='" +
      (n ?? "") +
      "'>章</div><label>标题<input id=t value=\"" +
      esc(chapter.title) +
      '"></label>' +
      "<div id=b contenteditable=true style='white-space:pre-wrap'>" +
      esc(chapter.body).replaceAll("\n", "<br>") +
      "</div>" +
      "<button id=s>存草稿</button><button id=publish>下一步</button>" +
      "<script>s.onclick=()=>fixtureSave(n.value,t.value,b.innerText); publish.onclick=()=>fixturePublish()</script>";
  } else return route.abort();
  return route.fulfill({ contentType: "text/html; charset=utf-8", body: html });
});
await context.exposeFunction("fixtureSave", (n, title, body) => {
  saves++;
  saved.set(n, { title, body });
});
let published = 0;
await context.exposeFunction("fixturePublish", () => published++);
let interrupt = false;
async function deliver(j) {
  return deliverDraft(j, {
    validate: async () => {},
    listPage: async (url) => {
      const page = await context.newPage();
      await page.goto(url);
      return {
        step: async (action) => {
          const result = await page.evaluate(
            ([fn, a, j, url]) => (0, eval)("(" + fn + ")")(a, j, url),
            [draftPage.toString(), action, j, page.url()],
          );
          if (!result.ok) throw Error(result.error);
          if (action === "save" && interrupt)
            throw Error("fixture disconnected after save");
          return result.value;
        },
      };
    },
  });
}
try {
  assert.equal((await deliver(job)).state, "verified");
  assert.deepEqual(saved.get("12"), { title: job.title, body });
  assert.equal((await deliver(job)).state, "exists");
  await assert.rejects(deliver({ ...job, body: "冲突稿" }), /内容或标题不同/);
  await assert.rejects(
    deliver({ ...job, number: "13", verify_only: true }),
    /未找到/,
  );
  interrupt = true;
  assert.equal((await deliver({ ...job, number: "13" })).state, "verified");
  assert.equal(
    (await deliver({ ...job, number: "13", verify_only: true })).state,
    "exists",
  );
  await assert.rejects(
    deliver({ ...job, target: { ...target, volume: "第二卷" } }),
    /当前卷/,
  );
  assert.equal(opens, 2);
  assert.equal(saves, 2);
  assert.equal(published, 0);
  console.log(
    "通过：隔离番茄页面双章保存、重新打开逐字核验、重复请求、冲突、错卷和保存后断线核对。",
  );
} finally {
  await browser.close();
}
