import React from 'react';
import {render,screen,fireEvent,cleanup} from '@testing-library/react';
import {it,expect,afterEach,vi} from 'vitest';
import App from '../src/App';
import type {API,LibraryState} from '../src/types';
vi.mock('../src/ReviewPanel',()=>({default:({onPendingChange,onClose}:any)=><section aria-label="历史测试"><button onClick={()=>onPendingChange?.(true)}>未保存名称</button><button onClick={()=>onPendingChange?.(false)}>取消名称草稿</button><button onClick={onClose}>尝试关闭历史</button></section>}));
afterEach(cleanup);
it('protects pending history labels from unload and modal close until explicitly cancelled',async()=>{
const state:LibraryState={books:[{id:'b',title:'书',author:'',deleted_at:null}],volumes:[{id:'v',book_id:'b',title:'卷',sort_order:0,deleted_at:null}],chapters:[{id:'c',volume_id:'v',title:'章',body:'原稿',revision:0,sort_order:0,deleted_at:null}]};
const api:API={load:async()=>state,apply:async()=>state,backup:async()=>null,restore:async()=>null,exportBook:async()=>null,workflow:{history:async()=>[],listProposals:async()=>[],decideProposal:async()=>state,restoreVersion:async()=>state}};
render(<App api={api}/>);fireEvent.click(await screen.findByRole('button',{name:'打开 书'}));fireEvent.click(await screen.findByRole('button',{name:'改稿与历史'}));fireEvent.click(await screen.findByRole('button',{name:'未保存名称'}));
const dirty=new Event('beforeunload',{cancelable:true});window.dispatchEvent(dirty);expect(dirty.defaultPrevented).toBe(true);
fireEvent.click(screen.getByRole('button',{name:'尝试关闭历史'}));expect(screen.getByLabelText('历史测试')).toBeTruthy();
fireEvent.click(screen.getByRole('button',{name:'取消名称草稿'}));const clean=new Event('beforeunload',{cancelable:true});window.dispatchEvent(clean);expect(clean.defaultPrevented).toBe(false);fireEvent.click(screen.getByRole('button',{name:'尝试关闭历史'}));expect(screen.queryByLabelText('历史测试')).toBeNull();
});
