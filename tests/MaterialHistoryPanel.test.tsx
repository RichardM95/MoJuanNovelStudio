import React from "react";
import {render,screen,fireEvent,waitFor,cleanup} from "@testing-library/react";
import {describe,it,expect,vi,afterEach} from "vitest";
import MaterialHistoryPanel from "../src/MaterialHistoryPanel";
afterEach(cleanup);
const card:any={id:"m",book_id:"b",kind:"setting",title:"当前",fields:{synopsis:"新"},revision:1,deleted_at:null};
const version:any={id:"v",material_id:"m",material_revision:0,title:"旧",fields:{synopsis:"旧正文"},created_at:0,source:"create",byte_count:9};
function fixture(){return {list:vi.fn().mockResolvedValue({items:[version],next_cursor:null,epoch:"e"}),get:vi.fn().mockResolvedValue(version),restore:vi.fn().mockResolvedValue({...card,revision:2,content_review_required:true}),acknowledge:vi.fn()};}
describe("material content history",()=>{
 it("loads only index until selected, confirms then reloads without repeating write",async()=>{
 const api=fixture();const reload=vi.fn().mockRejectedValueOnce(Error("读取失败")).mockResolvedValue({...card,revision:2,content_review_required:true});
 render(<MaterialHistoryPanel api={api} card={card} onReload={reload} onClose={()=>{}} onPendingChange={()=>{}}/>);
 await screen.findByText("查看版本 0");expect(api.get).not.toHaveBeenCalled();
 fireEvent.click(screen.getByText("查看版本 0"));await screen.findByText("旧正文");
 fireEvent.click(screen.getByText("恢复此版本内容"));expect(api.restore).not.toHaveBeenCalled();
 fireEvent.click(screen.getByText("确认恢复内容"));await screen.findByText("读取失败");
 fireEvent.click(screen.getByText("重新读取核对"));await waitFor(()=>expect(reload).toHaveBeenCalledTimes(2));expect(api.restore).toHaveBeenCalledTimes(1);
 });
 it("unknown response is reconciled, not called successful",async()=>{
 const api=fixture();api.restore.mockResolvedValue(undefined);
 render(<MaterialHistoryPanel api={api} card={card} onReload={async()=>card} onClose={()=>{}} onPendingChange={()=>{}}/>);
 fireEvent.click(await screen.findByText("查看版本 0"));await screen.findByText("旧正文");
 fireEvent.click(screen.getByText("恢复此版本内容"));fireEvent.click(screen.getByText("确认恢复内容"));
 await screen.findByText(/结果未确认，已重新读取/);expect(api.restore).toHaveBeenCalledTimes(1);
 });
});
import MaterialsPanel from "../src/MaterialsPanel";
it("opens history only from a clean material and locks parent close",async()=>{
 const h=fixture();const api:any={list:async()=>[card],get:async()=>card,history:h};
 const close=vi.fn();
 render(<MaterialsPanel api={api} bookId="b" chapters={[]} volumes={[]} onClose={close}/>);
 fireEvent.click(await screen.findByRole("button",{name:"当前"}));
 fireEvent.click(await screen.findByText("内容历史与复核"));
 await screen.findByText("查看版本 0");
 expect(h.list).toHaveBeenCalledTimes(1);
});

it("acknowledgement is explicit and cannot repeat or close while pending",async()=>{
 const api=fixture();let resolve:any;api.acknowledge.mockImplementation(()=>new Promise(r=>resolve=r));const close=vi.fn();
 render(<MaterialHistoryPanel api={api} card={{...card,content_review_required:true}} onReload={async()=>({...card,revision:2})} onClose={close} onPendingChange={()=>{}}/>);
 fireEvent.click(await screen.findByText("已核对内容与关联"));
 expect(api.acknowledge).not.toHaveBeenCalled();
 fireEvent.click(screen.getByText("确认完成关联复核"));
 fireEvent.click(screen.getByText("关闭资料历史"));
 expect(close).not.toHaveBeenCalled();expect(api.acknowledge).toHaveBeenCalledTimes(1);
 resolve({...card,revision:2,content_review_required:false});
 await screen.findByText("操作完成，已重新读取当前资料。");
 expect(api.acknowledge).toHaveBeenCalledTimes(1);
});
it("does not render a late detail after unmount",async()=>{
 const api=fixture();let resolve:any;api.get.mockImplementation(()=>new Promise(r=>resolve=r));
 const view=render(<MaterialHistoryPanel api={api} card={card} onReload={async()=>card} onClose={()=>{}} onPendingChange={()=>{}}/>);
 fireEvent.click(await screen.findByText("查看版本 0"));view.unmount();resolve(version);
 await waitFor(()=>expect(screen.queryByText("旧正文")).toBeNull());
 expect(api.restore).not.toHaveBeenCalled();
});
it("keeps unsaved material text and disables history entry",async()=>{
 const h=fixture();const api:any={list:async()=>[card],get:async()=>card,history:h};
 render(<MaterialsPanel api={api} bookId="b" chapters={[]} volumes={[]} onClose={()=>{}}/>);
 fireEvent.click(await screen.findByRole("button",{name:"当前"}));
 const title=await screen.findByDisplayValue("当前");fireEvent.change(title,{target:{value:"未保存标题"}});
 const entry=screen.getByText("内容历史与复核");expect((entry as HTMLButtonElement).disabled).toBe(true);fireEvent.click(entry);
 expect(h.list).not.toHaveBeenCalled();expect(screen.getByDisplayValue("未保存标题")).toBeTruthy();
});
