// Injected as a function into the isolated world of a single top-level writer tab.
export async function draftPage(action, job, expectedUrl) {
  async function perform() {
    const { target } = job;
    if (
      location.href !== expectedUrl ||
      location.origin !== "https://fanqienovel.com" ||
      !location.pathname.startsWith("/main/writer/") ||
      window !== window.top
    ) {
      throw Error("番茄作家页面已改变");
    }
    const visible = (el) =>
      el instanceof HTMLElement &&
      el.getClientRects().length > 0 &&
      !el.closest("[hidden],[inert],[aria-hidden=true]") &&
      getComputedStyle(el).visibility === "visible";
    const all = (selector) =>
      [...document.querySelectorAll(selector)].filter(visible);
    const text = (el) => (el.innerText ?? el.textContent ?? "").trim();
    const controls = () => all("button,a,[role=button]");
    const unique = (nodes, label) => {
      if (nodes.length !== 1) throw Error("无法唯一识别" + label);
      return nodes[0];
    };
    const exact = (label) => controls().filter((el) => text(el) === label);
    function checkTarget() {
      const scope = all(
        'h1,h2,h3,[aria-current]:not([aria-current=false]),[aria-selected=true],.active,.selected,[class*="breadcrumb" i],[class*="book-name" i],[class*="volume-name" i],select',
      );
      const labels = scope.flatMap((el) =>
        el instanceof HTMLSelectElement
          ? [...el.selectedOptions].map((o) => o.text.trim())
          : [text(el)],
      );
      if (!labels.includes(target.book) || !labels.includes(target.volume)) {
        // The editor also renders the book and volume as adjacent plain labels.
        const compact = (value) => value.replace(/\s+/g, "");
        const pair = compact(target.book + target.volume);
        const headers = all("header,div,span").filter(
          (el) =>
            !el.closest("main,article,[contenteditable]") &&
            el.getBoundingClientRect().top < 180 &&
            el.children.length > 0 &&
            compact(text(el)).replace(/[\uE000-\uF8FF]/g, "") === pair,
        );
        if (!headers.length) throw Error("页面上的作品或当前卷与绑定不符");
      }
      if (all("[role=dialog],[aria-modal=true]").length)
        throw Error("番茄页面有待处理提示");
    }
    function click(el) {
      if (!visible(el) || el.matches(":disabled,[aria-disabled=true]"))
        throw Error("页面操作当前不可用");
      if (el instanceof HTMLAnchorElement && el.hasAttribute("href")) {
        const url = new URL(el.href);
        if (
          url.origin !== location.origin ||
          !url.pathname.startsWith("/main/writer/")
        )
          throw Error("目标链接超出作家后台");
        if (el.target === "_blank") {
          location.assign(url.href);
          return;
        }
      }
      el.click();
    }
    function list() {
      checkTarget();
      const markers = all(
        'h1,h2,h3,[aria-current],[aria-selected=true],.active,.selected,[class*="tab-active"],[class*="tab_active"]',
      );
      if (!markers.some((el) => /^草稿箱(?:[（(]\d+[）)])?$/.test(text(el))))
        throw Error("请打开当前作品和卷的草稿箱");
      const selector =
        'tr,[role=row],li,article,[class*="chapter-item"],[class*="chapter_item"]';
      const rowNumber = (el) =>
        text(el).match(/^第\s*([1-9]\d*)\s*章(?:\s|[：:、.]|$)/)?.[1];
      const candidates = all(selector).filter((el) => rowNumber(el));
      const rows = candidates.filter(
        (el) => !candidates.some((other) => other !== el && el.contains(other)),
      );
      const counts = all("span,p,small,div,h1,h2,h3,[role=tab]")
        .map(text)
        .map((t) =>
          t.match(/^(?:共\s*(\d+)\s*(?:章|条)|草稿箱[（(](\d+)[）)])$/),
        )
        .filter(Boolean)
        .map((m) => Number(m[1] ?? m[2]));
      const empty = all("p,span,div").some((el) =>
        /^(?:暂无草稿|草稿箱为空|暂无章节)$/.test(text(el)),
      );
      const totals = new Set(counts);
      if (!totals.size && empty && !rows.length) totals.add(0);
      if (totals.size !== 1) throw Error("无法核对草稿箱总数");
      const total = [...totals][0];
      if (rows.length > total || (total && !rows.length))
        throw Error("草稿列表仍在加载或格式不受支持");
      return { rows, numbers: rows.map(rowNumber), total };
    }
    if (["list", "next", "new", "open"].includes(action)) {
      const { rows, numbers, total } = list();
      if (action === "list") return { numbers, total };
      if (action === "next") {
        const next = controls().filter(
          (el) =>
            (text(el) === "下一页" ||
              el.getAttribute("aria-label") === "下一页") &&
            !el.matches(":disabled,[aria-disabled=true]"),
        );
        click(unique(next, "下一页"));
        return;
      }
      const hits = rows.filter((_, i) => numbers[i] === job.number);
      if (action === "open") {
        const row = unique(hits, "目标草稿");
        const links = [
          ...row.querySelectorAll("a,button,[role=button]"),
        ].filter(visible);
        const edit = links.filter((el) => ["编辑", "修改"].includes(text(el)));
        const chapterLinks = links.filter(
          (el) => el.matches("a[href]") && /^第\s*\d+\s*章/.test(text(el)),
        );
        click(unique(edit.length ? edit : chapterLinks, "草稿编辑入口"));
        return;
      }
      if (hits.length) throw Error("列表中新出现同号草稿，请核对");
      click(
        unique([...exact("新建章节"), ...exact("创建章节")], "新建章节按钮"),
      );
      return;
    }

    const normalize = (value) => value.replace(/\r\n?/g, "\n");
    const values = [job.number, job.title, normalize(job.body)];
    function fields() {
      checkTarget();
      unique(exact("存草稿"), "存草稿按钮");
      const inputs = all(
        "input,textarea,[contenteditable=true],[contenteditable=plaintext-only]",
      );
      const inputLabel = (el) =>
        [
          el.placeholder,
          el.getAttribute("aria-label"),
          ...Array.from(el.labels ?? [], text),
        ]
          .filter(Boolean)
          .join(" ");
      const number = unique(
        inputs.filter(
          (el) =>
            el instanceof HTMLInputElement &&
            (/章号|章节序号|章节号|章节编号/.test(inputLabel(el)) ||
              /^第\s*章$/.test(text(el.parentElement))),
        ),
        "章号",
      );
      const title = unique(
        inputs.filter(
          (el) =>
            el instanceof HTMLInputElement &&
            /章节名|章名|标题/.test(inputLabel(el)),
        ),
        "章节标题",
      );
      const body = unique(
        inputs.filter(
          (el) =>
            (el.isContentEditable && !el.parentElement?.isContentEditable) ||
            (el instanceof HTMLTextAreaElement &&
              /正文|章节内容/.test(inputLabel(el))),
        ),
        "正文",
      );
      if (number === title) throw Error("章号和标题字段重合");
      return [number, title, body];
    }
    const read = (el) =>
      normalize(el.isContentEditable ? el.innerText : el.value);
    const empty = (el, index) =>
      el.isContentEditable
        ? !el.textContent &&
          [...el.querySelectorAll("*")].every((node) =>
            ["P", "DIV", "BR"].includes(node.tagName),
          )
        : el.value === "" ||
          (index === 1 &&
            el.value === "未命名草稿" &&
            /正文字数\s*0(?:\D|$)/.test(document.body.innerText));
    const matches = (els) => els.every((el, i) => read(el) === values[i]);
    const els = fields();
    if (action === "inspect")
      return { matches: matches(els), empty: els.every(empty) };
    if (action === "save") {
      if (!matches(els)) throw Error("保存前页面内容发生变化");
      click(unique(exact("存草稿"), "存草稿按钮"));
      return;
    }
    if (action !== "fill") throw Error("未知草稿操作");
    function check(index) {
      const current = fields();
      if (
        !current.every(
          (el, i) =>
            el === els[i] &&
            !el.matches(":disabled,[readonly],[aria-readonly=true]") &&
            (i < index ? read(el) === values[i] : empty(el, i)),
        )
      )
        throw Error("页面或章稿发生变化，请检查草稿箱");
    }
    for (let i = 0; i < els.length; i++) {
      check(i);
      const el = els[i];
      if (el.isContentEditable) {
        el.focus();
        check(i);
        const plain = document.createElement("span");
        plain.textContent = values[i];
        if (
          document.activeElement !== el ||
          !document.execCommand(
            el.contentEditable === "plaintext-only"
              ? "insertText"
              : "insertHTML",
            false,
            el.contentEditable === "plaintext-only"
              ? values[i]
              : plain.innerHTML.replace(/\n/g, "<br>"),
          )
        )
          throw Error("无法写入正文");
      } else {
        const proto =
          el instanceof HTMLInputElement
            ? HTMLInputElement.prototype
            : HTMLTextAreaElement.prototype;
        Object.getOwnPropertyDescriptor(proto, "value").set.call(el, values[i]);
        el.dispatchEvent(new Event("input", { bubbles: true, composed: true }));
      }
      await new Promise((resolve) => setTimeout(resolve, 75));
    }
    check(els.length);
  }
  try {
    return { ok: true, value: await perform() };
  } catch (e) {
    return { ok: false, error: e.message };
  }
}
