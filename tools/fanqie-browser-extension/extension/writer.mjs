// Serialized by Chrome scripting.executeScript. No imports or cross-origin I/O inside.
export function openWriterAssistant(chapter, expectedUrl) {
  const url = new URL(location.href);
  if (window !== window.top || url.origin !== "https://fanqienovel.com" ||
      !url.pathname.startsWith("/main/writer/") || location.href !== expectedUrl) return {code:"wrong_page"};
  if (document.querySelector("mojuan-chapter-assistant")) return {code:"already_open"};
  const data = ["number","title","body"].map(key => chapter[key]);
  const normalize = s => (s ?? "").replace(/\r\n?/g,"\n");
  const label = el => (el.innerText ?? el.textContent ?? "").replace(/\s+/g," ").trim();
  const visible = el => el instanceof HTMLElement && el.isConnected &&
    !el.closest("[inert],[hidden],[aria-hidden=true]") &&
    el.getClientRects().length > 0 && getComputedStyle(el).visibility === "visible";
  // The uniquely visible draft affordance is a page-identity check only.
  // It is never focused, clicked, or used as a response signal.
  const draftActionVisible = () => [...document.querySelectorAll("button,[role=button]")]
    .filter(el => visible(el) && label(el)==="存草稿").length === 1;
  if (!draftActionVisible()) return {code:"editor_unverified"};
  const validHeader = el => {
    if (!visible(el) || el === document.body || el === document.documentElement) return false;
    const s = label(el);
    return !!s && s.length < 260 && s.includes(chapter.book.replace(/\s+/g," ").trim()) &&
      s.includes(chapter.volume.replace(/\s+/g," ").trim());
  };
  const rich = el => el instanceof HTMLElement && el.isContentEditable &&
    el.matches('[contenteditable="true"],[contenteditable="plaintext-only"],[contenteditable=""]');
  const read = el => normalize(rich(el) ? el.innerText : el.value);
  const empty = el => !rich(el) ? el.value === "" :
    !el.textContent && ["","\n","\n\n"].includes(read(el)) &&
    /^(?:<br\s*\/?>|<(?:p|div)><br\s*\/?><\/(?:p|div)>)?$/i.test(el.innerHTML);
  const validField = (el,i) => {
    if (!visible(el) || el.matches(":disabled,[readonly],[aria-readonly=true],[aria-disabled=true]")) return false;
    if (i === 2 && rich(el)) return !el.parentElement?.isContentEditable && !el.matches("body,html");
    const allowed = i === 2 ? el instanceof HTMLTextAreaElement :
      el instanceof HTMLInputElement && (el.type === "text" || (i === 0 && el.type === "number"));
    return allowed &&
      (!el.closest('form[role="search"]')) &&
      (el.maxLength < 0 || data[i].length <= el.maxLength);
  };

  const host = document.createElement("mojuan-chapter-assistant");
  const shadow = host.attachShadow({mode:"open"});
  const style = document.createElement("style");
  style.textContent = `
  :host{all:initial;position:fixed;z-index:2147483647;right:12px;top:12px;width:min(360px,calc(100vw - 24px));}
  section{border:2px solid #447258;background:#fbfff9;color:#192d21;padding:12px;border-radius:10px;box-shadow:0 5px 24px #0005;font:14px/1.5 system-ui,sans-serif;}
  strong{font-size:16px} p{margin:6px 0;overflow-wrap:anywhere} button{font:inherit;margin:2px 6px 2px 0;padding:4px 8px;color:inherit;cursor:pointer} [hidden]{display:none!important;}
  `;
  const section = document.createElement("section");
  section.setAttribute("role","region");
  section.setAttribute("aria-label","墨卷章稿填表");
  const heading = document.createElement("strong");
  heading.textContent = "墨卷 · 当前章";
  const target = document.createElement("p");
  target.textContent = chapter.book + " · " + chapter.volume + " · 第 " + chapter.number + " 章 " + chapter.title;
  const message = document.createElement("p");
  message.setAttribute("role","status");
  const button = text => {
    const b = document.createElement("button"); b.type="button"; b.textContent=text; return b;
  };
  const fill = button("一键填三栏");
  const reset = button("重新选字段");
  const close = button("关闭");
  section.append(heading,target,message,fill,reset,close);
  shadow.append(style,section);
  document.documentElement.append(host);

  const cache = "__mojuanChapterMappingV1";
  const existing = globalThis[cache];
  let header = null, headerLabel = "", fields = [], picking = true, pending = false, completed = false;
  function state(text) { message.textContent = text; }
  function closePanel() {
    window.removeEventListener("click", choose, true);
    window.removeEventListener("keydown", escape, true);
    host.remove();
  }
  function escape(e) {
    if (e.key === "Escape" && host.isConnected) {
      e.preventDefault(); e.stopImmediatePropagation(); closePanel();
    }
  }
  function advance() {
    picking = !header || fields.length < 3;
    fill.hidden = picking || completed;
    if (!header) state("先点网页顶部显示作品和卷名的位置。");
    else if (picking) state("请点网页上的" + ["章号输入框","标题输入框","正文编辑区"][fields.length] + "。");
    else state("已选三栏。核对作品和卷名、确认编辑器空白，再点填入。");
  }
  function choose(e) {
    if (!picking || !e.isTrusted || e.composedPath().includes(host) || !host.isConnected) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    if (location.href !== expectedUrl) { state("页面已变化，请关闭后重新打开扩展。"); return; }
    let el = e.target;
    if (!(el instanceof HTMLElement)) return;
    if (!header) {
      for (let depth=0; el && depth<5; depth++,el=el.parentElement) {
        if (validHeader(el)) {header=el;headerLabel=label(el);break;}
      }
      if (!header) {state("作品或卷名不符。请点显示这两个名字的位置。");return;}
    } else {
      const i=fields.length;
      el=el.closest(i===2 ? "textarea,[contenteditable]" : "input");
      if (!validField(el,i) || fields.includes(el)) {state("未识别到这个可编辑字段，请点输入区本身。");return;}
      fields.push(el);
    }
    advance();
  }
  function headerUnchanged() {
    return location.href===expectedUrl && draftActionVisible() && validHeader(header) && label(header)===headerLabel;
  }
  function safeState(i) {
    return host.isConnected && headerUnchanged() && fields.every((el,j)=>validField(el,j) &&
      (j<i ? read(el)===data[j] : empty(el)));
  }
  reset.addEventListener("click",()=>{
    if (pending) return;
    delete globalThis[cache]; header=null; headerLabel=""; fields=[]; completed=false; advance();
  });
  close.addEventListener("click",closePanel);
  fill.addEventListener("click",async e=>{
    if (!e.isTrusted || pending || completed || picking) return;
    if (!headerUnchanged() || !fields.every(validField)) {state("页面或字段已变化，请重新选取。");return;}
    if (fields.some(el=>!empty(el))) {state("已有内容，未覆盖。请人工检查。");return;}
    pending=true; fill.disabled=true; reset.disabled=true;
    try {
      for (let i=0;i<fields.length;i++) {
        if (!safeState(i)) throw Error();
        const el=fields[i];
        if (rich(el)) {
          el.focus();
          if (!safeState(i) || document.activeElement!==el) throw Error();
          const plain = document.createElement("span");
          plain.textContent = data[i];
          const insert = el.getAttribute("contenteditable")==="plaintext-only"
            ? document.execCommand("insertText",false,data[i])
            : document.execCommand("insertHTML",false,plain.innerHTML.replace(/\n/g,"<br>"));
          if (!insert) throw Error();
        } else {
          const prototype = i===2 ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
          Object.getOwnPropertyDescriptor(prototype,"value").set.call(el,data[i]);
          el.dispatchEvent(new Event("input",{bubbles:true,composed:true}));
        }
        await new Promise(resolve=>setTimeout(resolve,60));
        if (!safeState(i+1)) throw Error();
      }
      if (!safeState(fields.length)) throw Error();
      completed=true; fill.hidden=true; reset.hidden=true;
      state("三栏已核对");
      const notice=document.createElement("p");
      notice.textContent="请回网页看一遍，再手动点「存草稿」。工具没有点击保存或提审。";
      section.insertBefore(notice,close);
    } catch {
      completed=true; fill.hidden=true;
      state("页面发生变化，可能只填入部分内容。请检查，不要重复填入。");
    } finally {
      pending=false; fill.disabled=false; reset.disabled=false;
    }
  });
  // Cache only explicitly selected DOM references and their visible header, never the draft.
  if (existing?.url===expectedUrl && validHeader(existing.header) &&
      label(existing.header)===existing.label && existing.fields.length===3 &&
      existing.fields.every(validField)) {
    header=existing.header; headerLabel=existing.label; fields=[...existing.fields];
  }
  fill.addEventListener("click",e=>{
    if (e.isTrusted && !picking && headerUnchanged() && fields.every(validField)) {
      globalThis[cache]={url:expectedUrl,header,label:headerLabel,fields:[...fields]};
    }
  });
  window.addEventListener("click",choose,true);
  window.addEventListener("keydown",escape,true);
  advance();
  return {code:"opened"};
}
