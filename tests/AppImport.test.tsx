import React from 'react';
import {render,screen,fireEvent,cleanup,waitFor} from '@testing-library/react';
import {it,expect,afterEach,vi} from 'vitest';
import App from '../src/App';
import type {API,LibraryState} from '../src/types';
afterEach(cleanup);
it('offers TXT import on the shelf and does not change the library merely by opening',async()=>{
 let writes=0;const api:API={load:async()=>({books:[],volumes:[],chapters:[]}),apply:async()=>{writes++;return {books:[],volumes:[],chapters:[]}},backup:async()=>null,restore:async()=>null,exportBook:async()=>null};Object.assign(api,{txtImport:{}});
 render(<App api={api}/>);const open=await screen.findByRole('button',{name:'导入TXT'});await waitFor(()=>expect((open as HTMLButtonElement).disabled).toBe(false));fireEvent.click(open);await screen.findByRole('dialog',{name:'导入TXT新作品'});expect(writes).toBe(0);
});
it('reloads an imported work without repeating commit after a read failure and preserves old works',async()=>{
 let state:LibraryState={books:[{id:'old',title:'旧书',author:'',deleted_at:null}],volumes:[{id:'ov',book_id:'old',title:'卷',sort_order:0,deleted_at:null}],chapters:[{id:'oc',volume_id:'ov',title:'章',body:'旧稿不变',revision:0,sort_order:0,deleted_at:null}]};let reads=0;
 const commit=vi.fn(async()=>{state={books:[...state.books,{id:'new',title:'新书',author:'',deleted_at:null}],volumes:[...state.volumes,{id:'av',book_id:'new',title:'后卷',sort_order:1,deleted_at:null},{id:'nv',book_id:'new',title:'正文',sort_order:0,deleted_at:null}],chapters:[...state.chapters,{id:'later',volume_id:'av',title:'后章',body:'后卷正文',revision:0,sort_order:0,deleted_at:null},{id:'nc',volume_id:'nv',title:'正文',body:'导入正文',revision:0,sort_order:0,deleted_at:null}]};return {book_id:'new',volumes:2,chapters:2}});
 const apply=vi.fn(async(cmd:any)=>{state={...state,chapters:state.chapters.map(c=>c.id===cmd.id?{...c,body:cmd.body,revision:c.revision+1}:c)};return state});
 const preview={id:'import',source_name:'新书.txt',title:'新书',author:'',mode:'headings' as const,expires_at:Date.now()/1000+600,volumes:[{title:'正文',chapters:[{index:0,title:'正文',bytes:12,snippet:'导入正文'}]},{title:'后卷',chapters:[{index:1,title:'后章',bytes:12,snippet:'后卷正文'}]}],total_chapters:2,total_bytes:24};
 const api:API={load:async()=>{reads++;if(reads===2)throw Error('读取失败');return state},apply,backup:async()=>null,restore:async()=>null,exportBook:async()=>null,txtImport:{choose:async()=>preview,revise:async()=>preview,detail:async()=>({index:0,body:'导入正文'}),commit,discard:async()=>{}}};
 render(<App api={api}/>);const open=await screen.findByRole('button',{name:'导入TXT'});await waitFor(()=>expect((open as HTMLButtonElement).disabled).toBe(false));fireEvent.click(open);fireEvent.click(await screen.findByRole('button',{name:'选择TXT文件'}));fireEvent.click(await screen.findByRole('button',{name:'导入为新作品'}));fireEvent.click(screen.getByRole('button',{name:'确认导入'}));await screen.findByText('读取失败');expect(commit).toHaveBeenCalledTimes(1);
 const e=new Event('beforeunload',{cancelable:true});window.dispatchEvent(e);expect(e.defaultPrevented).toBe(true);
 fireEvent.click(screen.getByRole('button',{name:'重新加载书架'}));await waitFor(()=>expect(reads).toBe(3));await waitFor(()=>expect((screen.getByRole('button',{name:'关闭导入面板'}) as HTMLButtonElement).disabled).toBe(false));fireEvent.click(screen.getByRole('button',{name:'关闭导入面板'}));expect(screen.getByRole('textbox',{name:'章节正文'})).toHaveProperty('value','导入正文');expect(commit).toHaveBeenCalledTimes(1);expect(state.books).toHaveLength(2);expect(state.chapters.find(c=>c.id==='oc')?.body).toBe('旧稿不变');
 fireEvent.change(screen.getByRole('textbox',{name:'章节正文'}),{target:{value:'继续写作'}});fireEvent.keyDown(window,{key:'s',ctrlKey:true});await waitFor(()=>expect(apply).toHaveBeenCalledTimes(1));expect(apply.mock.calls[0][0].expected_revision).toBe(0);
});
