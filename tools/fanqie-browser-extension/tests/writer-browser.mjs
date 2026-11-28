// Install Playwright separately. This test fulfills the writer URL with a synthetic page.
// It never loads the real site. CHROMIUM_PATH can point to an existing Chromium executable.
import { chromium } from "playwright";
import assert from "node:assert/strict";
import {openWriterAssistant} from "../extension/writer.mjs";
const body = "第一段。\n\n「第二段。」🙂\n末行。";
const packet = {book:"试验书",volume:"卷甲",number:"12",title:"潮声",body};
const url = "https://fanqienovel.com/main/writer/mock-editor";
const pageSource = `<!doctype html><style>label{display:block;margin:12px} #body{display:block;height:90px;white-space:pre-wrap;border:1px solid #222}</style>
<p id="scope">试验书　卷甲</p>
<label>章号<input id="number" type="number"></label><label>标题<input id="title"></label>
<div id="body" role="textbox" contenteditable="true"></div><button id="save">存草稿</button>
<output id="count">0</output><script>document.querySelector("#save").onclick=()=>document.querySelector("#count").value=String(1+Number(document.querySelector("#count").value));</script>`;
const options = process.env.CHROMIUM_PATH ? {executablePath:process.env.CHROMIUM_PATH,args:["--no-sandbox","--disable-dev-shm-usage"]} : {};
const browser = await chromium.launch(options);
try {
 const context=await browser.newContext();
 await context.route("**/*", route => route.request().url() === url ? route.fulfill({contentType:"text/html; charset=utf-8",body:pageSource}) : route.abort());
 const page = await context.newPage(); page.setDefaultTimeout(5000); await page.goto(url);
 const panel=page.locator("mojuan-chapter-assistant");
 const invoke = () => page.evaluate(([fn,p,u])=>eval("("+fn+")")(p,u),[openWriterAssistant.toString(),packet,url]);
 const selectFields = async () => {
   for(const field of ["scope","number","title","body"]) await page.locator("#"+field).click();
 };
 await invoke(); await selectFields();
 await panel.getByRole("button",{name:"一键填三栏"}).click();
 await panel.getByText("三栏已核对").waitFor().catch(async e=>{console.error(await panel.locator("p").allTextContents(),await page.locator("#body").innerText());throw e;});
 assert.equal(await page.locator("#number").inputValue(),"12");
 assert.equal(await page.locator("#title").inputValue(),"潮声");
 assert.equal(await page.locator("#body").innerText(),body);
 assert.equal(await page.locator("#count").textContent(),"0");
 await panel.getByRole("button",{name:"关闭"}).click();
 await invoke();
 await panel.getByRole("button",{name:"一键填三栏"}).click();
 await panel.getByText("已有内容", {exact:false}).waitFor();
 assert.equal(await page.locator("#count").textContent(),"0");
 await panel.getByRole("button",{name:"关闭"}).click();
 await page.locator("#save").click();
 assert.equal(await page.locator("#count").textContent(),"1");
 await page.reload(); await invoke(); await page.locator("#scope").evaluate(el=>el.textContent="试验书　卷乙");
 await page.locator("#scope").click(); await panel.getByText("作品或卷名不符",{exact:false}).waitFor();
 assert.equal(await page.locator("#title").inputValue(),"");
 await page.reload(); await invoke(); await selectFields();
 await page.locator("#title").evaluate(el=>{el.addEventListener("input",()=>el.value="自动改写");});
 await panel.getByRole("button",{name:"一键填三栏"}).click();
 await panel.getByText("可能只填入部分内容",{exact:false}).waitFor();
 assert.equal(await page.locator("#body").innerText(),"");
 assert.equal(await page.locator("#count").textContent(),"0");
 console.log("Chromium synthetic writer: exact multiline rich text, no overwrite, no Save click, author save only; wrong volume and intermediate rewrite halted.");
} finally {await browser.close();}
