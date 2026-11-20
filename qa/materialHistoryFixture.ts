// Fictional UI fixture only. No SQLite, files or model calls.
import type {Material} from "../src/materialTypes";
import type {MaterialContentVersion,MaterialHistoryAPI} from "../src/materialHistoryTypes";
export function materialHistoryFixture(rows:()=>Material[],chapterRevision:(id:string)=>number|undefined){
 const versions:MaterialContentVersion[]=[];
 function record(m:Material){if(versions.some(v=>v.material_id===m.id&&v.material_revision===m.revision))return;versions.push({id:crypto.randomUUID(),material_id:m.id,material_revision:m.revision,title:m.title,fields:structuredClone(m.fields),created_at:1,source:"虚构示例",byte_count:0});}
 rows().forEach(record);
 function scope(book:string,id:string){const m=rows().find(m=>m.id===id&&m.book_id===book&&m.deleted_at===null);if(!m)throw Error("资料不可用");return m;}
 const api:MaterialHistoryAPI={
 list:async(book,id,before,limit)=>{scope(book,id);const all=versions.filter(v=>v.material_id===id&&(before===null||v.material_revision<before)).sort((a,b)=>b.material_revision-a.material_revision);const items=all.slice(0,limit).map(({fields,material_id,byte_count,...v})=>v);return{items,next_cursor:all.length>limit?items.at(-1)!.material_revision:null,epoch:"fixture"};},
 get:async(book,id,version)=>{scope(book,id);const v=versions.find(v=>v.material_id===id&&v.id===version);if(!v)throw Error("历史不存在");return structuredClone(v);},
 restore:async(book,id,version,revision,epoch)=>{const m=scope(book,id);if(epoch!=="fixture"||m.revision!==revision)throw Error("版本冲突");const v=versions.find(v=>v.id===version&&v.material_id===id);if(!v)throw Error("历史不存在");record(m);m.title=v.title;m.fields=structuredClone(v.fields);m.revision++;m.content_review_required=true;record(m);return structuredClone(m);},
 acknowledge:async(book,id,revision,epoch)=>{const m=scope(book,id);if(epoch!=="fixture"||m.revision!==revision||!m.content_review_required)throw Error("版本冲突");for(const [id,r]of[[m.source_chapter_id,m.source_revision],[m.payoff_chapter_id,m.payoff_revision]]as const){if(id&&chapterRevision(id)!==r)throw Error("来源待复核");}m.content_review_required=false;m.revision++;return structuredClone(m);}
 };return{api,record};
}
