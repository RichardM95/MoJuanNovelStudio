/// <reference types="vite/client" />
import html from "../qa/fanqie/index.html?raw";
import script from "../qa/fanqie/lab.js?raw";
import { beforeEach, expect, it } from "vitest";

beforeEach(() => {
  document.body.innerHTML = html.match(/<body>([\s\S]*)<\/body>/)![1];
  new Function(script)();
});
const get = <T extends HTMLElement>(id: string) =>
  document.getElementById(id) as T;
const click = (id: string) => get<HTMLButtonElement>(id).click();
const set = (id: string, value: string) => {
  const field = get<HTMLInputElement>(id);
  field.value = value;
  field.dispatchEvent(new Event("input", { bubbles: true }));
};

it("fills exactly one chapter in B and cannot silently overwrite or skip", () => {
  click("start-b");
  click("create");
  set("title", "原有内容");
  click("fill");
  expect(get<HTMLInputElement>("title").value).toBe("原有内容");
  expect(get("status").textContent).toContain("已有内容");
  click("readback");
  expect(get("progress").textContent).toContain("第 1 / 6");
  set("title", "");
  click("fill");
  expect(get<HTMLInputElement>("number").value).toBe("11");
  expect(get<HTMLTextAreaElement>("body").value).toBe(
    "雾散时，灯还亮着。\n\n「靠岸了。」🌊",
  );
  click("readback");
  expect(get("progress").textContent).toContain("第 1 / 6");
  click("save");
  click("save");
  expect(get("progress").textContent).toContain("第 2 / 6");
});
it("C reaches six real draft records in order across two volumes and exports three metrics", () => {
  click("start-c");
  for (let i = 0; i < 6; i++) {
    click("one");
    expect(get<HTMLSelectElement>("volume").value).toBe(
      i < 3 ? "归途" : "远航",
    );
    click("readback");
    click("save");
  }
  expect(get("progress").textContent).toContain("完成 6 / 6");
  expect(get("results").querySelectorAll("tbody tr")).toHaveLength(6);
  expect(get("summary").textContent).toContain("中位耗时");
});
it("A cannot automatically fill and never advances for a field mismatch", () => {
  click("start-a");
  click("create");
  expect(get<HTMLButtonElement>("fill").hidden).toBe(true);
  set("number", "12");
  click("readback");
  click("save");
  expect(get("progress").textContent).toContain("第 1 / 6");
  expect(get("status").textContent).toContain("不一致");
});

it("stops a run on request and never counts a filled unsaved chapter", () => {
  click("start-c");
  click("one");
  click("readback");
  click("interrupt");
  expect(get("work").hidden).toBe(true);
  expect(get("results").hidden).toBe(true);
  click("start-c");
  expect(get("progress").textContent).toContain("第 1 / 6");
  expect(get<HTMLInputElement>("title").value).toBe("");
});
