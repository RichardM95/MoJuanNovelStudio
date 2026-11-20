import React, { useState } from "react";
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { Command } from "../src/types";
import ForeshadowProposalPanel, { type ForeshadowReviewSession } from "../src/ForeshadowProposalPanel";
import type { ForeshadowProposal, ForeshadowProposalAPI, OccurrenceView } from "../src/foreshadowTypes";

const nativeWindow = vi.hoisted(() => ({
  onCloseRequested: vi.fn<(close: (event: {preventDefault(): void}) => Promise<void>) => Promise<() => void>>(),
  destroy: vi.fn(async () => {}),
}));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => nativeWindow }));
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

const source = { chapter_id:"c", chapter_title:"钟楼", chapter_revision:2, chapter_sort_order:1, volume_id:"v", volume_title:"第一卷", volume_sort_order:0 };
const receipt = {id:"p",book_id:"b",material_id:"m",material_revision:3,chapter_id:"c",chapter_revision:2,status:"pending" as const,created_at:1,decided_at:null,accepted_occurrence_id:null};
const row: ForeshadowProposal = { receipt, material_title:"旧钟", source, source_status:"current", card_current:true, archived:false, start:1,end:5,quote:"😀\r\n钟",role:"hint",note:"标记",reason:"也许已经给出提示",accepted_occurrence:null };
const linked: OccurrenceView = {occurrence:{id:"o",book_id:"b",material_id:"m",source,start:1,end:5,quote:row.quote,role:row.role,note:row.note,revision:0,deleted_at:null,created_at:2},source_status:"current",current:source};
const accepted: ForeshadowProposal = {...row,card_current:false,receipt:{...receipt,status:"accepted",decided_at:2,accepted_occurrence_id:"o"},accepted_occurrence:linked};
const rejected: ForeshadowProposal = {...row,receipt:{...receipt,status:"rejected",decided_at:2}};

function fixture(overrides: Partial<ForeshadowProposalAPI> = {}, initialMode: {materialId?: string} | null = null) {
  const api: ForeshadowProposalAPI = {
    list: vi.fn(async () => ({items:[{receipt,material_title:row.material_title,archived:false}],pending_count:1,next_after_id:null})),
    get: vi.fn(async () => row),
    decide: vi.fn(async input => input.action==="accept"?accepted:rejected),
    ...overrides,
  };
  const epoch = vi.fn(async () => "epoch");
  const occurrences = vi.fn<() => Promise<OccurrenceView[]>>(async () => [linked]);
  const block = vi.fn(), change = vi.fn(), invalidate = vi.fn();
  const session: ForeshadowReviewSession = new Map();
  function View() {
    const [mode, setMode] = useState<{materialId?: string} | null>(initialMode);
    const [mounted, setMounted] = useState(true);
    const [bookId,setBookId] = useState("b");
    const [libraryEpoch,setLibraryEpoch] = useState("epoch");
    const [invalidated,setInvalidated] = useState(false);
    return <><button onClick={()=>setMounted(x=>!x)}>{mounted ? "关闭整个测试面板" : "重开测试面板"}</button>
    <button onClick={()=>setMode(null)}>外部关闭审阅</button>
    <button onClick={()=>setBookId("new-book")}>换书</button>
    <button onClick={()=>setLibraryEpoch("new-epoch")}>换代际</button>
    <button onClick={()=>setInvalidated(true)}>另一读取发现换库</button>
    {mounted && <ForeshadowProposalPanel {...{session}} api={api} bookId={bookId} epoch={libraryEpoch} getEpoch={epoch} listOccurrences={occurrences}
      mode={mode} onOpen={() => setMode({})} onClose={() => setMode(null)}
      onBlocking={block} onChanged={change} onEpochChanged={invalidate} disabled={false} invalidated={invalidated} isEpochInvalidated={() => false}/>}</>;
  }
  render(<View/>);
  return {api,epoch,occurrences,block,change,invalidate,session};
}
const pick = async () => {
  fireEvent.click(await screen.findByRole("button",{name:/出现建议 · 1 条待审/}));
  fireEvent.click(await screen.findByRole("button",{name:/旧钟 · 待审/}));
  await screen.findByText(row.reason);
};
async function confirm() {
  await waitFor(() => expect(screen.getByRole("button",{name:"审阅后采纳"})).toBeEnabled());
  fireEvent.click(screen.getByRole("button",{name:"审阅后采纳"}));
  return screen.getByRole("button",{name:"确认并登记出现"});
}

test("shows frozen evidence and existing occurrences, then confirms once despite repeated clicks", async () => {
  const {api,change,block} = fixture();
  await pick();
  expect(screen.getByText(/第一卷.*钟楼.*已保存正文/)).toBeInTheDocument();
  expect(screen.getByText(row.reason)).toBeInTheDocument();
  expect(screen.getByLabelText("正式出现对照")).toBeInTheDocument();
  expect(screen.getByText(/不是已回收/)).toBeInTheDocument();
  expect(api.decide).not.toHaveBeenCalled();
  const decide = await confirm();
  fireEvent.click(decide);
  fireEvent.click(decide);
  await screen.findByText("已登记出现。");
  expect(api.decide).toHaveBeenCalledExactlyOnceWith({book_id:"b",material_id:"m",proposal_id:"p",expected_epoch:"epoch",expected_material_revision:3,action:"accept"});
  expect(change).toHaveBeenCalledOnce();
  expect(block).toHaveBeenCalledWith(true);
});

test("stale source stays readable and can be rejected without formal comparison", async () => {
  const {api, occurrences} = fixture({get:vi.fn(async()=>({...row,source_status:"unavailable" as const}))});
  occurrences.mockRejectedValue(Error("测试读取断开"));
  await pick();
  expect(await screen.findByText(/对照读取失败/)).toBeInTheDocument();
  expect(screen.getByRole("button",{name:"审阅后采纳"})).toBeDisabled();
  fireEvent.click(screen.getByRole("button",{name:"拒绝建议"}));
  await screen.findByText("已拒绝建议。");
  expect(api.decide).toHaveBeenCalledWith(expect.objectContaining({action:"reject"}));
});

test("lost response remains write-locked until exact same-epoch readback succeeds", async () => {
  let saved = false;
  const {api,block} = fixture({get:vi.fn(async()=>saved?accepted:row),decide:vi.fn(async()=>{throw Error("连接断开")})});
  await pick();
  fireEvent.click(await confirm());
  const check = await screen.findByRole("button",{name:"只读核对决定"});
  expect(screen.getByRole("button",{name:"返回全书伏笔"})).toBeDisabled();
  fireEvent.click(check);
  await screen.findByText(/仍待审/);
  saved = true;
  fireEvent.click(check);
  await screen.findByText("已登记出现。");
  expect(api.decide).toHaveBeenCalledOnce();
  expect(block).toHaveBeenLastCalledWith(false);
});

test("mismatched linked evidence is never success and epoch change never writes in the new library", async () => {
  const {api,epoch,invalidate} = fixture({decide:vi.fn(async()=>({...accepted,accepted_occurrence:{...linked,occurrence:{...linked.occurrence,quote:"不同片段"}}}))});
  await pick();
  fireEvent.click(await confirm());
  const check = await screen.findByRole("button",{name:"只读核对决定"});
  expect(screen.queryByText("已登记出现。")).not.toBeInTheDocument();
  epoch.mockResolvedValue("later");
  fireEvent.click(check);
  await screen.findByText(/仅保留旧书库证据/);
  expect(api.decide).toHaveBeenCalledOnce();
  expect(api.get).toHaveBeenCalledOnce();
  expect(invalidate).toHaveBeenCalledOnce();
  expect(screen.getByRole("button",{name:"返回全书伏笔"})).toBeEnabled();
});

test("list failure does not masquerade as empty and a failed refresh does not clear a verified receipt", async () => {
  const listing = vi.fn<ForeshadowProposalAPI["list"]>().mockRejectedValue(Error("读取断开"));
  const {api} = fixture({list:listing});
  fireEvent.click(await screen.findByRole("button",{name:/出现建议 · 数量不可用/}));
  await screen.findByText(/建议列表读取失败/);
  expect(screen.queryByText("没有待审出现建议。")).not.toBeInTheDocument();
  listing.mockResolvedValue({items:[{receipt,material_title:row.material_title,archived:false}],pending_count:1,next_after_id:null});
  fireEvent.click(screen.getByRole("button",{name:"刷新建议"}));
  fireEvent.click(await screen.findByRole("button",{name:/旧钟 · 待审/}));
  await screen.findByText(row.reason);
  listing.mockRejectedValue(Error("后续列表断开"));
  fireEvent.click(await confirm());
  await screen.findByText(/后续列表断开/);
  expect(screen.getByText("已登记出现。")).toBeInTheDocument();
  expect(api.decide).toHaveBeenCalledOnce();
});

test("App waits for the editor flush before showing a suggestion tied to an older saved revision", async () => {
  const { default: App } = await import("../src/App");
  const initial = {
    books: [{id:"b",title:"虚构书",author:"",deleted_at:null}],
    volumes: [{id:"v",book_id:"b",title:"第一卷",sort_order:0,deleted_at:null}],
    chapters: [{id:"c",volume_id:"v",title:"钟楼",revision:2,sort_order:0,body:"旧存稿",deleted_at:null}],
  };
  let state = initial;
  let finishSave = () => {};
  const save = vi.fn(async (command: Command) => {
    if(command.type==="save_chapter") {
      await new Promise<void>(resolve=>{finishSave=resolve;});
      state = {...initial,chapters:[{...initial.chapters[0],revision:3,body:String(command.body)}]};
    }
    return state;
  });
  const proposals: ForeshadowProposalAPI = {
    list:vi.fn(async()=>({items:[{receipt,material_title:"旧钟",archived:false}],pending_count:1,next_after_id:null})),
    get:vi.fn(async()=>({...row,source_status:state.chapters[0].revision===2 ? "current" as const : "stale" as const})),
    decide:vi.fn(async()=>rejected),
  };
  const unavailable = async () => { throw Error("fixture operation was not requested"); };
  render(<App api={{load:async()=>state,apply:save,backup:async()=>null,restore:async()=>null,exportBook:async()=>null,
    foreshadow:{proposals,epoch:async()=>"epoch",overview:async()=>[],directory:async()=>[],list:async()=>[],
      chapter:unavailable,get:unavailable,lookup:unavailable,create:unavailable,setDeleted:unavailable,
      material:unavailable,createCard:unavailable,saveCard:unavailable}}}/>);
  fireEvent.click(await screen.findByRole("button",{name:"打开 虚构书"}));
  fireEvent.change(await screen.findByRole("textbox",{name:"章节正文"}),{target:{value:"还没保存的新稿"}});
  fireEvent.click(screen.getByRole("button",{name:"全书伏笔"}));
  await waitFor(()=>expect(save).toHaveBeenCalledExactlyOnceWith({type:"save_chapter",id:"c",body:"还没保存的新稿",expected_revision:2}));
  expect(screen.queryByRole("dialog",{name:"全书伏笔"})).not.toBeInTheDocument();
  expect(proposals.list).not.toHaveBeenCalled();
  finishSave();
  await pick();
  expect(screen.getByRole("button",{name:"审阅后采纳"})).toBeDisabled();
  fireEvent.click(screen.getByRole("button",{name:"拒绝建议"}));
  expect(await screen.findByText("已拒绝建议。")).toBeInTheDocument();
});

test("history keeps archived evidence and does not claim a missing formal row is still registered", async () => {
  const history = { ...accepted, archived:true, accepted_occurrence:null };
  const {api} = fixture({
    list:vi.fn(async()=>({items:[{receipt:history.receipt,material_title:"旧钟",archived:true}],pending_count:0,next_after_id:null})),
    get:vi.fn(async()=>history),
  }, {materialId:"m"});
  fireEvent.click(await screen.findByRole("button",{name:/旧钟 · 已采纳（归档）/}));
  await screen.findByText("关联正式出现：o（现已不可用）");
  expect(screen.queryByRole("button",{name:"审阅后采纳"})).not.toBeInTheDocument();
  expect(screen.queryByRole("button",{name:"拒绝建议"})).not.toBeInTheDocument();
  expect(api.list).toHaveBeenCalledWith("b","m",false,null,"epoch");
});

test("a late detail response cannot replace a newer selection", async () => {
  let release: (p: ForeshadowProposal) => void = () => {};
  const other = {...row,receipt:{...receipt,id:"newer"},reason:"另一个依据"};
  fixture({
    list:vi.fn(async()=>({items:[{receipt,material_title:"旧钟",archived:false},{receipt:other.receipt,material_title:"新建议",archived:false}],pending_count:2,next_after_id:null})),
    get:vi.fn(async (_book,_card,id)=>id===receipt.id ? new Promise<ForeshadowProposal>(resolve=>{release=resolve;}) : other),
  });
  fireEvent.click(await screen.findByRole("button",{name:/出现建议 · 2 条待审/}));
  fireEvent.click(await screen.findByRole("button",{name:/旧钟 · 待审/}));
  fireEvent.click(screen.getByRole("button",{name:/新建议 · 待审/}));
  await screen.findByText(other.reason);
  release(row);
  await waitFor(()=>expect(screen.getByText(other.reason)).toBeInTheDocument());
  expect(screen.queryByText(row.reason)).not.toBeInTheDocument();
});

test("exact accepted evidence remains visible if a different epoch appears immediately after the decision", async () => {
  const {api,epoch,invalidate} = fixture();
  await pick();
  epoch.mockResolvedValueOnce("epoch").mockResolvedValueOnce("other");
  fireEvent.click(await confirm());
  await screen.findByText(/仅保留旧书库证据/);
  expect(screen.getByText("关联正式出现：o（已登记）")).toBeInTheDocument();
  expect(screen.queryByText("已登记出现。")).not.toBeInTheDocument();
  expect(invalidate).toHaveBeenCalledOnce();
  expect(api.decide).toHaveBeenCalledOnce();
});

test("an independently verified opposite terminal outcome unlocks without replaying", async () => {
  const cancelled: ForeshadowProposal = {...row,receipt:{...receipt,status:"cancelled",decided_at:2}};
  const get = vi.fn<ForeshadowProposalAPI["get"]>().mockResolvedValueOnce(row).mockResolvedValue(cancelled);
  const {api} = fixture({get,decide:vi.fn(async()=>{throw Error("响应断开")})});
  await pick();
  fireEvent.click(await confirm());
  fireEvent.click(await screen.findByRole("button",{name:"只读核对决定"}));
  await screen.findByText(/已撤回.*本次采纳未生效/);
  expect(api.decide).toHaveBeenCalledOnce();
  expect(screen.getByRole("button",{name:"返回全书伏笔"})).toBeEnabled();
});

test("verified old-epoch readback is kept before its second epoch check", async () => {
  const get = vi.fn<ForeshadowProposalAPI["get"]>().mockResolvedValueOnce(row).mockResolvedValue(accepted);
  const {api,epoch} = fixture({get,decide:vi.fn(async()=>{throw Error("响应断开")})});
  await pick();
  fireEvent.click(await confirm());
  const check = await screen.findByRole("button",{name:"只读核对决定"});
  epoch.mockResolvedValueOnce("epoch").mockResolvedValue("restored");
  fireEvent.click(check);
  await screen.findByText(/仅保留旧书库证据/);
  expect(screen.getByText("关联正式出现：o（已登记）")).toBeInTheDocument();
  expect(api.decide).toHaveBeenCalledOnce();
});

test("quarantine survives leaving and reopening the whole modal, without locking other suggestions", async () => {
  const other = {...row,receipt:{...receipt,id:"other"},reason:"另一条可以正常审阅"};
  const {api} = fixture({
    list:vi.fn(async()=>({items:[{receipt,material_title:"旧钟",archived:false},{receipt:other.receipt,material_title:"另一卡",archived:false}],pending_count:2,next_after_id:null})),
    get:vi.fn(async (_book,_card,id)=>id===receipt.id ? row : other),
    decide:vi.fn(async()=>{throw Error("响应断开")}),
  });
  fireEvent.click(await screen.findByRole("button",{name:/出现建议 · 2 条待审/}));
  fireEvent.click(await screen.findByRole("button",{name:/旧钟 · 待审/}));
  fireEvent.click(await confirm());
  await screen.findByRole("button",{name:"只读核对决定"});
  fireEvent.click(screen.getByRole("button",{name:"暂离，稍后核对"}));
  fireEvent.click(screen.getByRole("button",{name:"关闭整个测试面板"}));
  fireEvent.click(screen.getByRole("button",{name:"重开测试面板"}));
  fireEvent.click(await screen.findByRole("button",{name:/出现建议 · 2 条待审/}));
  fireEvent.click(await screen.findByRole("button",{name:/旧钟 · 待审/}));
  const check = await screen.findByRole("button",{name:"只读核对决定"});
  expect(screen.getByRole("button",{name:"审阅后采纳"})).toBeDisabled();
  fireEvent.click(check);
  await screen.findByText(/仍待审/);
  fireEvent.click(screen.getByRole("button",{name:/另一卡 · 待审/}));
  await screen.findByText(other.reason);
  await waitFor(()=>expect(screen.getByRole("button",{name:"审阅后采纳"})).toBeEnabled());
  expect(api.decide).toHaveBeenCalledOnce();
});

test("successful receipt survives subsequent detail failures and stale pending responses", async () => {
  const list = vi.fn<ForeshadowProposalAPI["list"]>()
    .mockResolvedValueOnce({items:[{receipt,material_title:"旧钟",archived:false}],pending_count:1,next_after_id:null})
    .mockResolvedValueOnce({items:[{receipt,material_title:"旧钟",archived:false}],pending_count:1,next_after_id:null})
    .mockRejectedValue(Error("模拟队列断开"));
  const get = vi.fn<ForeshadowProposalAPI["get"]>().mockResolvedValueOnce(row).mockRejectedValueOnce(Error("模拟详情断开")).mockResolvedValue(row);
  const {api} = fixture({list,get});
  await pick();
  fireEvent.click(await confirm());
  await screen.findByText("已登记出现。");
  await screen.findByText(/模拟队列断开/);
  expect(screen.queryByRole("button",{name:/旧钟 · 待审/})).not.toBeInTheDocument();
  const read = screen.getByRole("button",{name:"重读建议详情"});
  fireEvent.click(read);
  await screen.findByText(/模拟详情断开/);
  expect(screen.getByText("关联正式出现：o（已登记）")).toBeInTheDocument();
  fireEvent.click(read);
  await screen.findByText(/已核实的终态/);
  expect(screen.getByText("已登记出现。")).toBeInTheDocument();
  expect(screen.getByText("关联正式出现：o（已登记）")).toBeInTheDocument();
  expect(api.decide).toHaveBeenCalledOnce();
});

test("a recycled linked occurrence verifies historical acceptance without claiming current registration", async () => {
  const recycled = {...accepted,accepted_occurrence:{...linked,occurrence:{...linked.occurrence,deleted_at:3,revision:1}}};
  const get = vi.fn<ForeshadowProposalAPI["get"]>().mockResolvedValueOnce(row).mockResolvedValue(recycled);
  const {api} = fixture({get,decide:vi.fn(async()=>{throw Error("响应断开")})});
  await pick();
  fireEvent.click(await confirm());
  fireEvent.click(await screen.findByRole("button",{name:"只读核对决定"}));
  await screen.findByText(/已核实采纳，关联出现现已回收/);
  expect(screen.queryByText("已登记出现。")).not.toBeInTheDocument();
  expect(api.decide).toHaveBeenCalledOnce();
});

test("an unresolved attempt remains discoverable even after the live pending queue has emptied", async () => {
  const listing = vi.fn<ForeshadowProposalAPI["list"]>()
    .mockResolvedValue({items:[{receipt,material_title:"旧钟",archived:false}],pending_count:1,next_after_id:null});
  const {api} = fixture({
    list:listing,
    get:vi.fn<ForeshadowProposalAPI["get"]>().mockResolvedValueOnce(row).mockResolvedValue(accepted),
    decide:vi.fn(async()=>{throw Error("响应断开")}),
  });
  await pick();
  fireEvent.click(await confirm());
  await screen.findByRole("button",{name:"只读核对决定"});
  listing.mockResolvedValue({items:[],pending_count:0,next_after_id:null});
  fireEvent.click(screen.getByRole("button",{name:"暂离，稍后核对"}));
  fireEvent.click(screen.getByRole("button",{name:"关闭整个测试面板"}));
  fireEvent.click(screen.getByRole("button",{name:"重开测试面板"}));
  fireEvent.click(await screen.findByRole("button",{name:/出现建议 · 0 条待审/}));
  fireEvent.click(await screen.findByRole("button",{name:/旧钟 · 待核对/}));
  fireEvent.click(await screen.findByRole("button",{name:"只读核对决定"}));
  await screen.findByText("已登记出现。");
  expect(api.decide).toHaveBeenCalledOnce();
});

test("an emptied projected page with a next cursor does not say the whole queue is empty", async () => {
  const {api} = fixture({
    list:vi.fn(async()=>({items:[{receipt,material_title:"旧钟",archived:false}],pending_count:1,next_after_id:receipt.id})),
  });
  await pick();
  fireEvent.click(await confirm());
  await screen.findByText("已登记出现。");
  expect(await screen.findByRole("button",{name:"下一页建议"})).toBeEnabled();
  await screen.findByText(/本页没有可显示的建议/);
  expect(screen.queryByText("没有待审出现建议。")).not.toBeInTheDocument();
  expect(api.decide).toHaveBeenCalledOnce();
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res,rej) => { resolve = res; reject = rej; });
  return {promise,resolve,reject};
}
const unknownResponse = () => { throw Error("决定响应断开"); };
const stopName = "停止等待核对";
const reconcileName = "只读核对决定";

test.each(["first epoch","detail","final epoch"])("can stop a readback hung at %s without settling or replaying the attempt", async (at) => {
  const epochWait = deferred<string>();
  const detailWait = deferred<ForeshadowProposal>();
  const get = vi.fn<ForeshadowProposalAPI["get"]>().mockResolvedValueOnce(row)
    .mockImplementation(() => at === "detail" ? detailWait.promise : Promise.resolve(accepted));
  const {api,epoch,session,block,change,invalidate} = fixture({get,decide:vi.fn(async()=>unknownResponse())});
  await pick();
  fireEvent.click(await confirm());
  const check = await screen.findByRole("button",{name:reconcileName});
  if (at === "final epoch") epoch.mockResolvedValueOnce("epoch");
  if (at !== "detail") epoch.mockReturnValueOnce(epochWait.promise);
  const epochCalls = epoch.mock.calls.length;
  const listCalls = vi.mocked(api.list).mock.calls.length;
  fireEvent.click(check);
  fireEvent.click(check);
  await waitFor(() => expect(get).toHaveBeenCalledTimes(at === "first epoch" ? 1 : 2));
  await waitFor(() => expect(epoch).toHaveBeenCalledTimes(epochCalls + (at === "final epoch" ? 2 : 1)));
  expect(screen.getByRole("button",{name:"暂离，稍后核对"})).toBeDisabled();
  expect(block).toHaveBeenLastCalledWith(true);
  fireEvent.click(screen.getByRole("button",{name:stopName}));

  expect(screen.getByText(/停止等待不代表原写入失败/)).toBeInTheDocument();
  await act(async()=> {
    if (at === "detail") detailWait.resolve(accepted);
    else epochWait.resolve("epoch");
  });
  expect(get).toHaveBeenCalledTimes(at === "first epoch" ? 1 : 2);
  expect(epoch).toHaveBeenCalledTimes(epochCalls + (at === "final epoch" ? 2 : 1));
  expect(check).toBeEnabled();
  expect(screen.getByRole("button",{name:"暂离，稍后核对"})).toBeEnabled();
  expect(block).toHaveBeenLastCalledWith(false);
  expect(session.values().next().value?.attempt?.evidence).toEqual(row);
  if (at === "final epoch") {
    expect(session.values().next().value?.terminal).toEqual(accepted);
    expect(screen.getByText("关联正式出现：o（已登记）")).toBeInTheDocument();
    expect(screen.getByText(/终态证据.*未完成.*核对/)).toBeInTheDocument();
  } else {
    expect(session.values().next().value?.terminal).toBeUndefined();
    expect(screen.queryByText(/关联正式出现：/)).not.toBeInTheDocument();
  }
  expect(screen.queryByText("已登记出现。")).not.toBeInTheDocument();
  expect(api.list).toHaveBeenCalledTimes(listCalls);
  expect(change).not.toHaveBeenCalled();
  expect(invalidate).not.toHaveBeenCalled();
  expect(api.decide).toHaveBeenCalledOnce();
});

test.each(["resolve", "reject"])("a cancelled readback's late %s cannot interfere with a fresh readback", async (late) => {
  const old = deferred<ForeshadowProposal>();
  const fresh = deferred<ForeshadowProposal>();
  const get = vi.fn<ForeshadowProposalAPI["get"]>().mockResolvedValueOnce(row)
    .mockReturnValueOnce(old.promise).mockReturnValueOnce(fresh.promise);
  const {api,session,change,invalidate,block,epoch,occurrences} = fixture({get,decide:vi.fn(async()=>unknownResponse())});
  await pick();
  fireEvent.click(await confirm());
  const check = await screen.findByRole("button",{name:reconcileName});
  fireEvent.click(check);
  await waitFor(()=>expect(get).toHaveBeenCalledTimes(2));
  fireEvent.click(screen.getByRole("button",{name:stopName}));
  fireEvent.click(check);
  fireEvent.click(check);
  await waitFor(()=>expect(get).toHaveBeenCalledTimes(3));
  const epochCalls = epoch.mock.calls.length;
  const listCalls = vi.mocked(api.list).mock.calls.length;
  const comparisonCalls = occurrences.mock.calls.length;
  await act(async()=> {
    if (late === "resolve") old.resolve(accepted);
    else old.reject({code:"epoch_changed",message:"过期核对错误"});
  });
  expect(epoch).toHaveBeenCalledTimes(epochCalls);
  expect(api.list).toHaveBeenCalledTimes(listCalls);
  expect(occurrences).toHaveBeenCalledTimes(comparisonCalls);
  expect(session.values().next().value?.terminal).toBeUndefined();
  expect(session.values().next().value?.attempt).toBeDefined();
  expect(screen.queryByText(/旧书库证据|过期核对错误|已登记出现|关联正式出现：/)).not.toBeInTheDocument();
  expect(screen.getByRole("button",{name:stopName})).toBeEnabled();
  expect(check).toBeDisabled();
  expect(block).toHaveBeenLastCalledWith(true);
  expect(change).not.toHaveBeenCalled();
  expect(invalidate).not.toHaveBeenCalled();

  await act(async()=>fresh.resolve(rejected));
  await screen.findByText("已拒绝建议。本次采纳未生效。");
  expect(session.values().next().value?.attempt).toBeUndefined();
  expect(session.values().next().value?.terminal).toEqual(rejected);
  expect(change).toHaveBeenCalledOnce();
  expect(block).toHaveBeenLastCalledWith(false);
  expect(api.decide).toHaveBeenCalledOnce();
});

test.each(["换书","换代际","外部关闭审阅","另一读取发现换库","关闭整个测试面板"])(
  "invalidates a readback before its late result after %s",
  async (leave) => {
    const old = deferred<ForeshadowProposal>();
    // Use an empty list in the new book, with the old attempt kept in its own scope.
    const list = vi.fn<ForeshadowProposalAPI["list"]>(async book => book === "b"
      ? {items:[{receipt,material_title:row.material_title,archived:false}],pending_count:1,next_after_id:null}
      : {items:[],pending_count:0,next_after_id:null});
    const get = vi.fn<ForeshadowProposalAPI["get"]>().mockResolvedValueOnce(row).mockReturnValueOnce(old.promise);
    const {api,session,change,invalidate,block,epoch} = fixture({list,get,decide:vi.fn(async()=>unknownResponse())});
    await pick();
    fireEvent.click(await confirm());
    fireEvent.click(await screen.findByRole("button",{name:reconcileName}));
    await waitFor(()=>expect(get).toHaveBeenCalledTimes(2));
    expect(block).toHaveBeenLastCalledWith(true);
    fireEvent.click(screen.getByRole("button",{name:leave}));
    const epochCalls = epoch.mock.calls.length;
    await act(async()=>old.resolve(accepted));
    expect(epoch).toHaveBeenCalledTimes(epochCalls);
    expect(screen.queryByText(/已登记出现|关联正式出现：/)).not.toBeInTheDocument();
    expect(screen.queryByRole("button",{name:stopName})).not.toBeInTheDocument();
    expect([...session.values()].some(value=>value.terminal)).toBe(false);
    if (leave === "换代际") expect(session.size).toBe(0);
    else expect(session.values().next().value?.attempt).toBeDefined();
    expect(change).not.toHaveBeenCalled();
    expect(invalidate).not.toHaveBeenCalled();
    expect(block).toHaveBeenLastCalledWith(false);
    expect(api.decide).toHaveBeenCalledOnce();
  },
);

test("stopping after exact evidence lets the real App leave and safely reopen, but a real decide still guards closing", async () => {
  const {default:App} = await import("../src/App");
  const initial = {books:[{id:"b",title:"虚构书",author:"",deleted_at:null}],volumes:[],chapters:[]};
  const write = deferred<ForeshadowProposal>();
  const finalEpoch = deferred<string>();
  const other = {...row,material_title:"另一条",receipt:{...receipt,id:"other"},reason:"另一条的依据"};
  const get = vi.fn<ForeshadowProposalAPI["get"]>()
    .mockResolvedValueOnce(row).mockResolvedValueOnce(accepted).mockImplementation(async (_b,_m,id) => id === "other" ? other : accepted);
  const list = vi.fn<ForeshadowProposalAPI["list"]>().mockResolvedValue({
    items:[{receipt,material_title:row.material_title,archived:false},{receipt:other.receipt,material_title:other.material_title,archived:false}],
    pending_count:2,next_after_id:null,
  });
  const proposals = {list,get,decide:vi.fn<ForeshadowProposalAPI["decide"]>().mockReturnValue(write.promise)};
  const unavailable = async () => { throw Error("fixture operation was not requested"); };
  const epoch = vi.fn(async()=>"epoch");
  vi.stubGlobal("__TAURI_INTERNALS__", {});
  nativeWindow.onCloseRequested.mockReset().mockResolvedValue(vi.fn());
  nativeWindow.destroy.mockClear();
  render(<App api={{load:async()=>initial,apply:unavailable,backup:async()=>null,restore:async()=>null,exportBook:async()=>null,
    foreshadow:{proposals,epoch,overview:async()=>[],directory:async()=>[],list:async()=>[],
      chapter:unavailable,get:unavailable,lookup:unavailable,create:unavailable,setDeleted:unavailable,
      material:unavailable,createCard:unavailable,saveCard:unavailable}}}/>);
  fireEvent.click(await screen.findByRole("button",{name:"打开 虚构书"}));
  const full = await screen.findByRole("button",{name:"全书伏笔"});
  const open = async () => {
    fireEvent.click(full);
    await screen.findByRole("dialog",{name:"全书伏笔"});
    fireEvent.click(await screen.findByRole("button",{name:/出现建议 · 2 条待审/}));
    fireEvent.click(await screen.findByRole("button",{name:/旧钟 · 待审/}));
  };
  const modalClose = () => fireEvent.click(screen.getByRole("button",{name:"关闭全书伏笔"}));
  const leaving = (blocked: boolean) => {
    const event = new Event("beforeunload",{cancelable:true});
    window.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(blocked);
  };
  const nativeClose = async () => {
    const event = {preventDefault:vi.fn()};
    await act(async()=>nativeWindow.onCloseRequested.mock.calls[0][0](event));
    expect(event.preventDefault).toHaveBeenCalledOnce();
  };

  await open();
  fireEvent.click(await confirm());
  await waitFor(()=>expect(proposals.decide).toHaveBeenCalledOnce());
  expect(screen.queryByRole("button",{name:stopName})).not.toBeInTheDocument();
  leaving(true);
  modalClose();
  expect(screen.getByRole("dialog",{name:"全书伏笔"})).toBeInTheDocument();
  await nativeClose();
  expect(nativeWindow.destroy).not.toHaveBeenCalled();
  await act(async()=>write.reject(Error("决定响应断开")));

  epoch.mockResolvedValueOnce("epoch").mockReturnValueOnce(finalEpoch.promise);
  fireEvent.click(await screen.findByRole("button",{name:reconcileName}));
  await screen.findByText("关联正式出现：o（已登记）");
  const epochCalls = epoch.mock.calls.length;
  leaving(true);
  fireEvent.keyDown(screen.getByRole("button",{name:stopName}),{key:"Escape"});
  expect(screen.getByRole("dialog",{name:"全书伏笔"})).toBeInTheDocument();
  await nativeClose();
  expect(nativeWindow.destroy).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button",{name:stopName}));
  leaving(false);
  modalClose();
  expect(screen.queryByRole("dialog",{name:"全书伏笔"})).not.toBeInTheDocument();

  await open();
  expect(screen.getByRole("button",{name:reconcileName})).toBeEnabled();
  expect(screen.getByText(/终态证据.*未完成.*核对/)).toBeInTheDocument();
  expect(screen.getByText("关联正式出现：o（已登记）")).toBeInTheDocument();
  await act(async()=>finalEpoch.reject({code:"epoch_changed",message:"迟到的旧代际"}));
  expect(epoch).toHaveBeenCalledTimes(epochCalls+1); // Only the reopened modal read.
  expect(screen.queryByText(/书库已更换|已登记出现。/)).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button",{name:/另一条 · 待审/}));
  await screen.findByText(other.reason);
  expect(screen.getByRole("button",{name:"拒绝建议"})).toBeEnabled();
  expect(proposals.decide).toHaveBeenCalledOnce();
  await nativeClose();
  expect(nativeWindow.destroy).toHaveBeenCalledOnce();
});

test("parent epoch invalidation blocks stale final readback before React commits", async () => {
  const {default:ForeshadowPanel} = await import("../src/ForeshadowPanel");
  const epoch = vi.fn(async()=>"epoch");
  const directory = deferred<never>();
  const finalEpoch = deferred<string>();
  const unavailable = async () => { throw Error("unexpected fixture operation"); };
  const session: ForeshadowReviewSession = new Map();
  const overview = vi.fn(async()=>[]);
  const compare = vi.fn(async()=>[]);
  const close = vi.fn(), blocking = vi.fn();
  const decide = vi.fn(async()=>unknownResponse());
  const get = vi.fn<ForeshadowProposalAPI["get"]>().mockResolvedValueOnce(row).mockResolvedValueOnce(accepted);
  const list = vi.fn<ForeshadowProposalAPI["list"]>().mockResolvedValue({
    items:[{receipt,material_title:row.material_title,archived:false}],pending_count:1,next_after_id:null,
  });
  render(<ForeshadowPanel bookId="b" onClose={close} onJump={async()=>false} onPendingChange={blocking} proposalSession={session}
    api={{epoch,overview,directory:()=>directory.promise,list:compare,
      chapter:unavailable,get:unavailable,lookup:unavailable,create:unavailable,setDeleted:unavailable,
      material:unavailable,createCard:unavailable,saveCard:unavailable,proposals:{list,get,decide}}}/>);
  await pick();
  fireEvent.click(await confirm());
  const check = await screen.findByRole("button",{name:reconcileName});
  epoch.mockResolvedValueOnce("epoch").mockReturnValueOnce(finalEpoch.promise);
  fireEvent.click(check);
  await screen.findByText(/终态证据.*未完成.*核对/);
  const held = session.values().next().value;
  expect(held?.attempt).toBeDefined();
  expect(held?.terminal).toEqual(accepted);
  expect(blocking).toHaveBeenLastCalledWith(true);
  expect(overview).toHaveBeenCalledOnce();
  const listCalls = list.mock.calls.length, compareCalls = compare.mock.calls.length;
  await act(async()=>{
    // The parent's directory catch learns the library changed before the
    // already-issued final check returns its old epoch, in the same batch.
    directory.reject({code:"epoch_changed",message:"当前书库已变化"});
    finalEpoch.resolve("epoch");
  });

  expect(screen.getByText(/仅保留旧书库证据/)).toBeInTheDocument();
  expect(screen.getByText("关联正式出现：o（已登记）")).toBeInTheDocument();
  expect.soft(overview).toHaveBeenCalledOnce();
  expect.soft(list).toHaveBeenCalledTimes(listCalls);
  expect.soft(compare).toHaveBeenCalledTimes(compareCalls);
  expect.soft(session.values().next().value).toEqual(held);
  expect.soft(screen.queryByText("已登记出现。")).not.toBeInTheDocument();
  expect(blocking).toHaveBeenLastCalledWith(false);
  expect(decide).toHaveBeenCalledOnce();
  fireEvent.click(screen.getByRole("button",{name:"关闭全书伏笔"}));
  expect(close).toHaveBeenCalledOnce();
});
