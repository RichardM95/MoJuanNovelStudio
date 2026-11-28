export const MOCK_URL = "http://127.0.0.1:4173/mock/index.html";
export const DEMO_CHAPTER = {
  book: "试验书", volume: "卷甲", number: "12", title: "潮声",
  body: "雾散时，灯还亮着。\n\n「靠岸了。」🌊",
};

export function parseChapter(raw) {
  if (typeof raw !== "string" || raw.length > 800000) throw Error("章稿格式不符或过长。");
  let data;
  try { data = JSON.parse(raw); } catch { throw Error("需要单章 JSON，可先载入假稿。"); }
  const keys = ["book", "volume", "number", "title", "body"];
  if (!data || typeof data !== "object" || Object.keys(data).length !== keys.length ||
      keys.some(key => !Object.hasOwn(data, key) || typeof data[key] !== "string")) {
    throw Error("须提供作品、目标卷、章号、标题和正文，不含其他指令。");
  }
  if (!/^[1-9][0-9]{0,8}$/.test(data.number) ||
      ["book", "volume", "title"].some(k => !data[k].trim() || data[k].length > 200 || /[\r\n\u0000]/.test(data[k])) ||
      !data.body.trim() || data.body.length > 250000 || data.body.includes("\u0000")) {
    throw Error("请核对章号、标题长度和正文。");
  }
  return {...data, body: data.body.replace(/\r\n?/g, "\n")};
}

// executeScript serializes this function: all dependencies are defined inside it.
// It does not click, submit, navigate, run site code on demand, or perform network I/O.
export async function fillCurrent(data) {
  const url = "http://127.0.0.1:4173/mock/index.html";
  if (location.href !== url || window.top !== window) return {code:"wrong_page"};
  const form = document.getElementById("chapter-form");
  const ids = ["number", "title", "body"];
  const fields = ids.map(id => document.getElementById(id));
  const out = code => ({code});
  const sameTarget = () =>
    document.getElementById("book")?.textContent.trim() === data.book &&
    document.getElementById("volume")?.textContent.trim() === data.volume;
  const usable = (control, i) => {
    const selector = i === 2 ? "textarea" : 'input[type="text"],input:not([type])';
    return control?.matches(selector) &&
      document.querySelectorAll("#" + ids[i]).length === 1 &&
      control.form === form && !control.matches(":disabled,[readonly],[hidden]") &&
      (control.maxLength < 0 || data[ids[i]].length <= control.maxLength);
  };
  if (!sameTarget()) return out("wrong_target");
  if (!(form instanceof HTMLFormElement) || !fields.every(usable)) return out("unavailable");
  if (fields.some(control => control.value !== "")) return out("nonempty");

  for (let i = 0; i < fields.length; i++) {
    if (location.href !== url || !sameTarget() || !fields.every((control,j) =>
      control.isConnected && document.getElementById(ids[j]) === control && usable(control,j) &&
      control.value === (j < i ? data[ids[j]] : ""))) return out("review");
    const control = fields[i];
    const prototype = i === 2 ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(prototype, "value").set.call(control, data[ids[i]]);
    control.dispatchEvent(new Event("input", {bubbles:true}));
  }
  await new Promise(resolve => setTimeout(resolve, 50));
  return out(location.href === url && sameTarget() && fields.every((control,i)=>
    control.isConnected && document.getElementById(ids[i]) === control && usable(control,i) &&
    control.value === data[ids[i]]) ? "filled" : "review");
}
