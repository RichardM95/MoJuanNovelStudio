import { DEMO_CHAPTER, MOCK_URL, fillCurrent, parseChapter } from "./fill.mjs";
import { openWriterAssistant } from "./writer.mjs";
const $ = id => document.getElementById(id);
async function send(action,target) {
  const result=await chrome.runtime.sendMessage({action,target});
  if(result?.error) throw Error(result.error);
  return result?.value;
}
async function show(action) {
  try { $("connection").textContent=await action(); }
  catch(e){ $("connection").textContent=e.message; }
}
void show(()=>send("state"));
$("connect").onclick=()=>void show(async()=>{
  const allowed=await chrome.permissions.request({origins:["https://fanqienovel.com/*"]});
  if(!allowed) throw Error("番茄作家页面访问权限未开启");
  return send("connect");
});
$("load-volumes").onclick=()=>void show(async()=>{
  $("binding").hidden=true;
  const data=await send("volumes");
  const select=$("local-volume");
  select.replaceChildren(...Object.entries(data.volumes).map(([id,title])=>new Option(title,id)));
  $("remote-book").value=data.book;
  const suggest=()=>{$("remote-volume").value=select.selectedOptions[0]?.text ?? "";};
  select.onchange=suggest; suggest();
  $("binding").hidden=false;
  return "核对当前草稿箱的作品、卷名与编号。";
});
$("binding").onsubmit=e=>{
  e.preventDefault();
  void show(()=>send("bind",{volume_id:$("local-volume").value,book:$("remote-book").value.trim(),
    volume:$("remote-volume").value.trim(),first_number:Number($("first-number").value)}));
};
const packet = $("packet"), fill = $("fill"), status = $("status"), target = $("target");
let prepared = null;
function invalidate() {
  prepared = null;
  target.hidden = fill.hidden = true;
  status.textContent = "";
}
packet.addEventListener("input", invalidate);
$("example").addEventListener("click", () => {
  packet.value = JSON.stringify(DEMO_CHAPTER, null, 2);
  invalidate();
});
$("preview").addEventListener("click", () => {
  invalidate();
  try {
    prepared = parseChapter(packet.value);
    target.textContent = prepared.book + " · " + prepared.volume + " · 第 " + prepared.number + " 章：" + prepared.title;
    target.hidden = fill.hidden = false;
  } catch (e) { status.textContent = e.message; }
});
fill.addEventListener("click", async () => {
  if (!prepared || fill.disabled) return;
  const value = prepared;
  const controls = [packet, $("example"), $("preview"), fill];
  controls.forEach(c => c.disabled = true);
  try {
    const [tab] = await chrome.tabs.query({active:true,currentWindow:true});
    if (!tab?.id || !tab.url) throw Error("无法识别当前标签页。");
    const url = new URL(tab.url);
    const local = tab.url === MOCK_URL;
    const writer = url.origin === "https://fanqienovel.com" && url.pathname.startsWith("/main/writer/");
    if (!local && !writer) throw Error("请打开番茄作家专区的空白章节编辑页。");
    const [response] = await chrome.scripting.executeScript({
      target:{tabId:tab.id}, func:local ? fillCurrent : openWriterAssistant,
      args:local ? [value] : [value,tab.url],
    });
    if (response?.result?.code === "opened") { window.close(); return; }
    status.textContent = {
      already_open:"网页上已有填稿面板。请在网页关闭旧面板，再尝试。",
      filled:"已填并核对三栏；未提交。请回页面人工检查。",
      editor_unverified:"没有找到唯一可见的「存草稿」。请打开空白章节编辑器，此页未填。",
      wrong_page:"不是预期测试页，未填。",
      wrong_target:"书名或卷名不一致，未填。",
      unavailable:"有字段不可编辑或长度不符，未填。",
      nonempty:"有字段已有内容，未覆盖。",
      review:"填入时页面有变化，可能只填入部分内容。请核对，勿重复点击。",
    }[response?.result?.code] ?? "没有收到核对结果。请检查当前页，勿自动重试。";
  } catch (e) {
    status.textContent = (e.message ?? String(e)) + " 请核对当前页，不要自动重试。";
  } finally {
    prepared = null;
    fill.hidden = true;
    controls.forEach(c => c.disabled = false);
  }
});
