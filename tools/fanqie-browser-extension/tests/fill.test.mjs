import test from "node:test";
import assert from "node:assert/strict";
import {JSDOM} from "jsdom";
import {parseChapter, fillCurrent} from "../extension/fill.mjs";
const sample = {book:"试验书", volume:"卷甲", number:"12", title:"潮声", body:"  一行。\n\n「两行。」🙂\n"};
const url="http://127.0.0.1:4173/mock/index.html";
async function setup(f, location=url) {
  const html='<div id="book">试验书</div><div id="volume">卷甲</div><form id="chapter-form"><input id="number"><input id="title"><textarea id="body"></textarea><button type="submit" id="submit">模拟提交</button></form>';
  const dom = new JSDOM(html,{url:location, runScripts:"outside-only"});
  const fn = dom.window.eval("("+fillCurrent.toString()+")");
  return f(dom.window.document, fn);
}
test("validates shape and normalizes newlines only",()=>{
  assert.deepEqual(parseChapter(JSON.stringify({...sample,body:"甲\r\n\r乙"})),{...sample,body:"甲\n\n乙"});
  for (const bad of [{...sample,number:"0"},{...sample,number:12},{...sample,title:"甲\n乙"},{...sample,body:"  "},{...sample,submit:true}]) assert.throws(()=>parseChapter(JSON.stringify(bad)));
});
test("fills exactly, repeat never overwrites and there is no submit",()=> setup(async(d,f)=>{
  let count=0; d.querySelector('form').addEventListener('submit',()=>count++);
  assert.equal((await f(sample)).code,"filled");
  for (const k of ["number","title","body"]) assert.equal(d.getElementById(k).value,sample[k]);
  assert.equal((await f(sample)).code,"nonempty");
  assert.equal(count,0);
}));
test("refuses wrong origin/path, target, nonempty and controlled inputs which change the result", async()=>{
  await setup(async(d,f)=>assert.equal((await f(sample)).code,"wrong_page"),"https://example.com/mock/index.html");
  await setup(async(d,f)=>assert.equal((await f(sample)).code,"wrong_page"),"http://127.0.0.1:4173/else");
  await setup(async(d,f)=>assert.equal((await f({...sample,volume:"卷乙"})).code,"wrong_target"));
  await setup(async(d,f)=>{
    d.getElementById("body").value="已有正文";
    assert.equal((await f(sample)).code,"nonempty");
    assert.equal(d.getElementById("number").value,"");
  });
  await setup(async(d,f)=>{
    d.getElementById("title").maxLength=1;
    assert.equal((await f(sample)).code,"unavailable");
    assert.equal(d.getElementById("body").value,"");
  });
  await setup(async(d,f)=>{
    d.getElementById("title").addEventListener("input",()=>d.getElementById("title").value="平台改写");
    assert.equal((await f(sample)).code,"review");
    assert.equal(d.getElementById("body").value,"");
  });
});
