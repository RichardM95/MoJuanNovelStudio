import React from 'react';
import {render,screen,fireEvent,cleanup,waitFor} from '@testing-library/react';
import {it,expect,afterEach,vi} from 'vitest';
import App from '../src/App';
import type {API,Command,LibraryState} from '../src/types';
vi.mock('../src/BackupPanel',()=>({default:({onPendingChange,onRestored,onClose}:any)=>{const[error,setError]=React.useState('');return <section aria-label="备份测试"><button onClick={()=>onPendingChange(true)}>后台运行</button><button onClick={()=>{onPendingChange(false);void onRestored().catch((e:Error)=>setError(e.message));}}>已提交恢复</button><button onClick={()=>void onRestored().catch((e:Error)=>setError(e.message))}>仅刷新书架</button><button onClick={onClose}>关闭备份</button>{error&&<p>{error}</p>}</section>}}));
afterEach(cleanup);
it('flushes before backup work and distinguishes committed restore from failed reload without retaining old editor',async()=>{
const state:LibraryState={books:[{id:'b',title:'旧书',author:'',deleted_at:null}],volumes:[{id:'v',book_id:'b',title:'卷',sort_order:0,deleted_at:null}],chapters:[{id:'c',volume_id:'v',title:'章',body:'旧正文',revision:0,sort_order:0,deleted_at:null}]};let reads=0;const apply=vi.fn(async(cmd:Command)=>({...state,chapters:state.chapters.map(c=>c.id===cmd.id?{...c,body:String(cmd.body),revision:c.revision+1}:c)}));const api:API={load:async()=>{reads++;if(reads===2)throw Error('书架读取失败');return reads===1?state:{books:[],volumes:[],chapters:[]}},apply,backup:async()=>null,restore:async()=>null,exportBook:async()=>null};Object.assign(api,{backups:{}});
render(<App api={api}/>);fireEvent.click(await screen.findByRole('button',{name:'打开 旧书'}));fireEvent.change(await screen.findByRole('textbox',{name:'章节正文'}),{target:{value:'新正文'}});fireEvent.click(screen.getByRole('button',{name:'备份书库'}));await screen.findByLabelText('备份测试');expect(apply).toHaveBeenCalledTimes(1);
fireEvent.click(screen.getByRole('button',{name:'后台运行'}));const e=new Event('beforeunload',{cancelable:true});window.dispatchEvent(e);expect(e.defaultPrevented).toBe(true);fireEvent.click(screen.getByRole('button',{name:'关闭备份'}));expect(screen.getByLabelText('备份测试')).toBeTruthy();
fireEvent.click(screen.getByRole('button',{name:'已提交恢复'}));await screen.findByText('书架读取失败');expect(screen.queryByRole('textbox',{name:'章节正文'})).toBeNull();expect(screen.queryByRole('button',{name:'打开 旧书'})).toBeNull();expect(apply).toHaveBeenCalledTimes(1);
fireEvent.click(screen.getByRole('button',{name:'仅刷新书架'}));await waitFor(()=>expect(reads).toBe(3));expect(apply).toHaveBeenCalledTimes(1);
});
