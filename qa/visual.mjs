import { mkdir } from "node:fs/promises";
import { chromium } from "@playwright/test";
const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM_PATH,
  headless: true,
  args: ["--no-sandbox"],
});
const page = await browser.newPage({
  viewport: { width: 1280, height: 850 },
  deviceScaleFactor: 1,
});
await mkdir("qa-output", { recursive: true });
const errors = [];
page.on("pageerror", (e) => errors.push(e.message));
await page.goto("http://127.0.0.1:1420/qa/index.html");
await page.getByRole("button", { name: "打开 远山来信" }).waitFor();
await page.screenshot({ path: "qa-output/shelf.png" });
await page.getByRole("button", { name: "打开 远山来信" }).click();
await page.getByRole("textbox", { name: "章节正文" }).waitFor();
await page.screenshot({ path: "qa-output/editor.png" });
await page.getByRole("textbox", { name: "章节正文" }).fill("界面验收测试稿。");
await page.waitForTimeout(1000);
if (!(await page.getByText("已保存", { exact: true }).isVisible()))
  throw Error("save status absent");
await page.getByRole("button", { name: "返回书架" }).click();
await page.getByRole("heading", { name: "我的书架" }).waitFor();
if (errors.length) throw Error(errors.join("\n"));
console.log(
  "Visual fixture: shelf, editor, text input/save, navigation passed. Not native SQLite/Windows verification.",
);
await browser.close();
