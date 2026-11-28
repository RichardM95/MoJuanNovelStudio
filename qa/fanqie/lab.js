(() => {
  "use strict";
  const fixture = [
    ["归途", "11", "靠岸", "雾散时，灯还亮着。\n\n「靠岸了。」🌊"],
    ["归途", "12", "归来", "她把伞留在门口。\n　　潮声比往常低。"],
    ["归途", "13", "空信封", "「这封信里没有字。」\n纸角印着一枚蓝灯。"],
    ["远航", "11", "归来", "十一盏灯依次亮起。\n\n她没有回头。"],
    ["远航", "12", "风清", "第一部分资料归她保管。\n钟响了两次。"],
    ["远航", "13", "次日", "天边浮着一点白。\n「今夜换我守灯。」"],
  ];
  const $ = (id) => document.getElementById(id);
  const fields = ["volume", "number", "title", "body"].map($);
  const modes = ["a", "b", "c"];
  const data = Object.fromEntries(modes.map((m) => [m, []]));
  const drafts = new Map();
  let mode, position, open, started, actions, errors, reviewed;
  function start(m) {
    if (mode) return;
    mode = m;
    data[m] = [];
    drafts.clear();
    for (const key of modes) $("start-" + key).disabled = true;
    position = 0;
    $("work").hidden = false;
    show();
  }
  function show() {
    const f = fixture[position];
    $("progress").textContent =
      mode.toUpperCase() +
      " 组 · 第 " +
      (position + 1) +
      " / " +
      fixture.length +
      " 章";
    $("expected").textContent = f[0] + " · 第 " + f[1] + " 章 · " + f[2];
    $("source").textContent = f[3];
    for (const x of fields) x.value = "";
    $("status").textContent = "";
    $("fields").disabled = true;
    $("create").hidden = mode === "c";
    $("one").hidden = mode !== "c";
    $("create").disabled = false;
    $("one").disabled = false;
    $("fill").hidden = mode !== "b";
    $("save").disabled = true;
    actions = 0;
    errors = 0;
    open = false;
    reviewed = null;
    started = performance.now();
  }
  function create() {
    if (!mode || open) return false;
    open = true;
    actions++;
    $("fields").disabled = false;
    $("create").disabled = true;
    $("one").disabled = true;
    if (mode !== "c") fields[0].focus();
    return true;
  }
  function fill(count) {
    if (!open || mode === "a") return;
    if (count) actions++;
    if (fields.some((x) => x.value !== "")) {
      errors++;
      $("status").textContent = "编辑器已有内容，未覆盖。";
      return;
    }
    fields.forEach((field, i) => (field.value = fixture[position][i]));
    $("status").textContent = "已填入，请核对三栏和目标卷。";
  }
  function equal() {
    return fields.every((x, i) => x.value === fixture[position][i]);
  }
  const seconds = (ms) => Math.round(ms) / 1000;
  function verify() {
    if (!open) return;
    actions++;
    if (!equal()) {
      errors++;
      reviewed = null;
      $("save").disabled = true;
      $("status").textContent = "与样稿不一致，请核对全部字段。";
      return;
    }
    reviewed = seconds(performance.now() - started);
    $("save").disabled = false;
    $("status").textContent = "核对通过。请手动存模拟草稿。";
  }
  function save() {
    if (!open || reviewed === null || !equal()) return;
    const [volume, n] = fixture[position];
    const key = JSON.stringify([volume, n]);
    if (drafts.has(key)) return;
    drafts.set(
      key,
      fields.map((x) => x.value),
    );
    data[mode].push([
      mode.toUpperCase(),
      position + 1,
      reviewed,
      actions,
      errors,
      seconds(performance.now() - started),
    ]);
    open = false;
    if (++position < fixture.length) show();
    else {
      $("progress").textContent =
        mode.toUpperCase() +
        " 组 · 完成 " +
        drafts.size +
        " / " +
        fixture.length;
      $("work").hidden = true;
      for (const key of modes) $("start-" + key).disabled = false;
      mode = null;
      display();
    }
  }
  function records() {
    return modes.flatMap((m) =>
      data[m].length === fixture.length ? data[m] : [],
    );
  }
  function display() {
    const tbody = $("results").querySelector("tbody");
    tbody.replaceChildren();
    for (const row of records()) {
      const tr = document.createElement("tr");
      for (const value of row) {
        const td = document.createElement("td");
        td.textContent = value;
        tr.append(td);
      }
      tbody.append(tr);
    }
    $("summary").textContent = modes
      .filter((m) => data[m].length === fixture.length)
      .map((m) => {
        const times = data[m].map((r) => r[2]).sort((a, b) => a - b);
        return (
          m.toUpperCase() +
          " 组中位耗时：" +
          ((times[2] + times[3]) / 2).toFixed(2) +
          " 秒"
        );
      })
      .join("；");
    $("results").hidden = false;
  }
  function exportCsv() {
    const a = document.createElement("a");
    const csv =
      "\ufeff组,章,核对秒数,操作,核对失败,总秒数\r\n" +
      records()
        .map((row) => row.join(","))
        .join("\r\n");
    const url = URL.createObjectURL(
      new Blob([csv], { type: "text/csv;charset=utf-8" }),
    );
    a.href = url;
    a.download = "mojuan-fanqie-lab.csv";
    a.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
  $("interrupt").addEventListener("click", () => {
    if (!mode) return;
    data[mode] = [];
    mode = null;
    drafts.clear();
    open = false;
    $("work").hidden = true;
    $("progress").textContent = "已中止本组；本组不会计入对照。";
    for (const key of modes) $("start-" + key).disabled = false;
    if (records().length) display();
    else $("results").hidden = true;
  });
  $("create").addEventListener("click", create);
  $("one").addEventListener("click", () => {
    if (create()) fill(false);
  });
  $("fill").addEventListener("click", () => fill(true));
  $("readback").addEventListener("click", verify);
  $("save").addEventListener("click", save);
  $("copy-results").addEventListener("click", exportCsv);
  for (const m of modes)
    $("start-" + m).addEventListener("click", () => start(m));
  for (const x of fields) {
    let editing = false;
    x.addEventListener("input", () => {
      if (!editing || x === fields[0]) {
        actions++;
        editing = true;
      }
      reviewed = null;
      $("save").disabled = true;
    });
    x.addEventListener("blur", () => {
      editing = false;
    });
  }
})();
