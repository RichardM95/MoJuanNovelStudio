import React,{useEffect,useRef,useState} from "react";
import type {Material} from "./materialTypes";
import type {MaterialHistoryAPI,MaterialContentVersion,MaterialVersionPage} from "./materialHistoryTypes";
import {errorMessage} from "./workflowTypes";
export default function MaterialHistoryPanel({api,card,onReload,onClose,onPendingChange}:{api:MaterialHistoryAPI;card:Material;onReload:()=>Promise<Material>;onClose:()=>void;onPendingChange:(v:boolean)=>void}){
 const [page,setPage]=useState<MaterialVersionPage|null>(null),[detail,setDetail]=useState<MaterialContentVersion|null>(null),[busy,setBusy]=useState(false),[message,setMessage]=useState(""),[confirm,setConfirm]=useState<"restore"|"ack"|null>(null),[needsRead,setNeedsRead]=useState(false);
 const alive=useRef(true),seq=useRef(0),lock=useRef(false),readRequired=useRef(false),known=useRef<Material|null>(null),actionError=useRef(""),callbacks=useRef({onReload,onClose,onPendingChange});
 callbacks.current={onReload,onClose,onPendingChange};
 function pending(v:boolean){setBusy(v);callbacks.current.onPendingChange(v||readRequired.current);}
 async function load(before:number|null=null){
 if(lock.current||readRequired.current)return;lock.current=true;pending(true);const g=++seq.current;setDetail(null);setConfirm(null);setMessage("");
 try{const p=await api.list(card.book_id,card.id,before,20);if(alive.current&&g===seq.current)setPage(p);}catch(e){if(alive.current&&g===seq.current)setMessage(errorMessage(e));}finally{lock.current=false;if(alive.current)pending(false);}
 }
 useEffect(()=>{alive.current=true;void load();return()=>{alive.current=false;seq.current++;callbacks.current.onPendingChange(false);};},[api,card.id]);
 async function choose(id:string){
 if(lock.current||readRequired.current)return;lock.current=true;pending(true);const g=++seq.current;setDetail(null);setConfirm(null);
 try{const d=await api.get(card.book_id,card.id,id);if(d.material_id!==card.id||d.id!==id)throw Error("历史归属不匹配");if(alive.current&&g===seq.current)setDetail(d);}catch(e){if(alive.current&&g===seq.current)setMessage(errorMessage(e));}finally{lock.current=false;if(alive.current)pending(false);}
 }
 function close(){if(!lock.current&&!readRequired.current)callbacks.current.onClose();}
 async function reconcile(){
 if(lock.current)return;lock.current=true;pending(true);
 try{const current=await callbacks.current.onReload();if(!current||current.id!==card.id||current.book_id!==card.book_id||current.deleted_at!==null||(known.current&&current.revision<known.current.revision))throw Error("尚未读到本次结果，请重新核对");
 if(alive.current){readRequired.current=false;setNeedsRead(false);setPage(null);setDetail(null);setConfirm(null);setMessage(known.current?"操作完成，已重新读取当前资料。":"结果未确认，已重新读取，请核对当前资料；未自动重试。"+actionError.current);}
 }catch(e){if(alive.current)setMessage(errorMessage(e));}finally{lock.current=false;if(alive.current)pending(false);}
 }
 async function act(){
 if(lock.current||readRequired.current||!confirm||!page||(confirm==="restore"&&!detail))return;
 const mode=confirm;setConfirm(null);lock.current=true;readRequired.current=true;setNeedsRead(true);pending(true);setMessage("");actionError.current="";
 try{const r=mode==="restore"?await api.restore(card.book_id,card.id,detail!.id,card.revision,page.epoch):await api.acknowledge(card.book_id,card.id,card.revision,page.epoch);
 known.current=r&&typeof r==="object"&&r.id===card.id&&r.book_id===card.book_id&&Number.isSafeInteger(r.revision)&&r.revision>card.revision?r:null;
 }catch(e){known.current=null;actionError.current=errorMessage(e);}
 finally{lock.current=false;if(alive.current)await reconcile();}
 }
 return <section className="material-history" aria-label="资料内容历史" onKeyDown={e=>{if(e.key==="Escape"){e.preventDefault();e.stopPropagation();close();}if((e.ctrlKey||e.metaKey)&&e.key.toLowerCase()==="s"){e.preventDefault();e.stopPropagation();}}}>
 <header><h3>资料内容历史</h3><button onClick={close} disabled={busy||needsRead}>关闭资料历史</button></header>
 <p>仅恢复标题和文字字段；章节关联、来源版本、故事位置和剧情状态不回滚。恢复后必须人工复核关联，旧来源相同也不自动视为有效。</p>
 {card.content_review_required&&<p role="status">内容已恢复，关联待人工复核。请核对文字与两组来源，过期来源需先在编辑器更新并保存。</p>}
 <button onClick={()=>void load()} disabled={busy||needsRead}>刷新历史</button>
 {page?.items.map(v=><button key={v.id} disabled={busy||needsRead} onClick={()=>void choose(v.id)}>查看版本 {v.material_revision}</button>)}
 {page?.next_cursor!==null&&page?.next_cursor!==undefined&&<button disabled={busy||needsRead} onClick={()=>void load(page.next_cursor)}>下一页历史</button>}
 {detail&&<div className="history-comparison"><article><h4>当前内容：{card.title}</h4>{Object.entries(card.fields).map(([k,v])=><p key={k}><strong>{k}</strong><span>{v}</span></p>)}</article><article><h4>历史内容：{detail.title}</h4>{Object.entries(detail.fields).map(([k,v])=><p key={k}><strong>{k}</strong><span>{v}</span></p>)}</article><button disabled={busy||needsRead} onClick={()=>setConfirm("restore")}>恢复此版本内容</button></div>}
 {card.content_review_required&&page&&<button disabled={busy||needsRead} onClick={()=>setConfirm("ack")}>已核对内容与关联</button>}
 {confirm&&<div><p>{confirm==="restore"?"将恢复标题和文字，保留当前关联及状态，并标记待复核。":"确认已人工核对当前文字和全部关联；系统还会校验来源仍为当前版本。"}</p><button disabled={busy} onClick={()=>void act()}>{confirm==="restore"?"确认恢复内容":"确认完成关联复核"}</button><button disabled={busy} onClick={()=>setConfirm(null)}>取消确认</button></div>}
 {needsRead&&<button disabled={busy} onClick={()=>void reconcile()}>重新读取核对</button>}
 {message&&<p role="status">{message}</p>}
 </section>;
}
