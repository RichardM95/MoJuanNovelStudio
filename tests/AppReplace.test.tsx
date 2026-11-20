import React from 'react';
import {render,screen,fireEvent,cleanup,waitFor} from '@testing-library/react';
import {it,expect,afterEach,vi} from 'vitest';
import App from '../src/App';
import type {API,LibraryState} from '../src/types';
afterEach(cleanup);
const original:LibraryState={books:[{id:'b',title:'小说',author:'',deleted_at:null}],volumes:[{id:'v',book_id:'b',title:'卷',sort_order:0,deleted_at:null}],chapters:[{id:'c',volume_id:'v',title:'章',body:'小林看海',revision:0,sort_order:0,deleted_at:null}]};
it('flush failure keeps the manuscript and prevents opening batch replace',async()=>{
const apply=vi.fn(async()=>{throw Error('保存失败')});
const api:API={load:async()=>original,apply,backup:async()=>null,restore:async()=>null,exportBook:async()=>null};Object.assign(api,{replace:{}});
render(<App api={api}/>);fireEvent.click(await screen.findByRole('button',{name:'打开 小说'}));fireEvent.change(await screen.findByRole('textbox',{name:'章节正文'}),{target:{value:'小林还在看海'}});fireEvent.click(screen.getByRole('button',{name:'批量替换'}));await screen.findByText('保存失败');expect(screen.queryByRole('dialog',{name:'作品正文替换'})).toBeNull();expect(screen.getByRole('textbox',{name:'章节正文'})).toHaveProperty('value','小林还在看海');expect(apply).toHaveBeenCalledTimes(1);
});
it('saves before preview and reload failure retries only reads then edits at the new revision',async()=>{
let state=structuredClone(original),reads=0;
const apply=vi.fn(async(command: any)=>{state={...state,chapters:state.chapters.map(c=>c.id===command.id?{...c,body:command.body,revision:c.revision+1}:c)};return state});
const commit=vi.fn(async()=>{state={...state,chapters:state.chapters.map(c=>({...c,body:c.body.replaceAll('小林','阿舟'),revision:c.revision+1}))};return {chapters:1,matches:2}});
const api:API={load:async()=>{reads++;if(reads===2)throw Error('读取失败');return state},apply,backup:async()=>null,restore:async()=>null,exportBook:async()=>null,replace:{
preview:async(book_id,query,replacement)=>{expect(apply).toHaveBeenCalledTimes(1);return {id:'candidate',book_id,query,replacement,expires_at:Date.now()/1000+600,total_matches:2,chapters:[{id:'c',title:'章',revision:1,matches:2,before_snippet:state.chapters[0].body,after_snippet:'阿舟看海和阿舟'}]}},
detail:async()=>({chapter_id:'c',before:'小林看海和小林',after:'阿舟看海和阿舟'}),commit,discard:async()=>{}}};
render(<App api={api}/>);fireEvent.click(await screen.findByRole('button',{name:'打开 小说'}));fireEvent.change(await screen.findByRole('textbox',{name:'章节正文'}),{target:{value:'小林看海和小林'}});fireEvent.click(screen.getByRole('button',{name:'批量替换'}));await screen.findByRole('dialog',{name:'作品正文替换'});
fireEvent.change(screen.getByRole('textbox',{name:'查找内容'}),{target:{value:'小林'}});fireEvent.change(screen.getByRole('textbox',{name:'替换为'}),{target:{value:'阿舟'}});fireEvent.click(screen.getByRole('button',{name:'预览替换'}));fireEvent.click(await screen.findByRole('button',{name:'替换这些正文'}));fireEvent.click(screen.getByRole('button',{name:'确认替换'}));await screen.findByText('读取失败');expect(commit).toHaveBeenCalledTimes(1);expect(screen.queryByRole('textbox',{name:'章节正文'})).toBeNull();
const leave=new Event('beforeunload',{cancelable:true});window.dispatchEvent(leave);expect(leave.defaultPrevented).toBe(true);
fireEvent.click(screen.getByRole('button',{name:'重新加载正文'}));await waitFor(()=>expect(reads).toBe(3));await waitFor(()=>expect((screen.getByRole('button',{name:'关闭替换面板'}) as HTMLButtonElement).disabled).toBe(false));fireEvent.click(screen.getByRole('button',{name:'关闭替换面板'}));expect(screen.getByRole('textbox',{name:'章节正文'})).toHaveProperty('value','阿舟看海和阿舟');expect(commit).toHaveBeenCalledTimes(1);
fireEvent.change(screen.getByRole('textbox',{name:'章节正文'}),{target:{value:'阿舟回家'}});fireEvent.keyDown(window,{key:'s',ctrlKey:true});await waitFor(()=>expect(apply).toHaveBeenCalledTimes(2));expect(apply.mock.calls[1][0].expected_revision).toBe(2);
});
