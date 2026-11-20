// UI-only fictitious history. This does not exercise SQLite persistence.
import type {HistoryAPI,VersionSummary} from '../src/historyTypes';
import type {LibraryState} from '../src/types';
import type {ChapterVersion} from '../src/workflowTypes';
export function historyFixture(read:()=>LibraryState,write:(state:LibraryState)=>void):HistoryAPI{
 const epoch='fictional-history-session';
 const versions=new Map<string,ChapterVersion>();
 const labels=new Map<string,{label:string|null;label_revision:number}>();
 function scope(book:string,chapter:string){const s=read();const c=s.chapters.find(c=>c.id===chapter&&!c.deleted_at&&s.volumes.some(v=>v.id===c.volume_id&&v.book_id===book&&!v.deleted_at));if(!c||!s.books.some(b=>b.id===book&&!b.deleted_at))throw Error('章节不可访问');return c;}
 function seed(book:string,chapter:string){const c=scope(book,chapter);if(![...versions.values()].some(v=>v.chapter_id===chapter)){const id=`fixture-${chapter}-${c.revision}`;versions.set(id,{id,chapter_id:chapter,revision:c.revision,body:c.body,source:'gui',created_at:Math.floor(Date.now()/1000)});}}
 function metadata(v:ChapterVersion):VersionSummary{const{body,...m}=v;return{...m,...(labels.get(v.id)??{label:null,label_revision:0})};}
 function lookup(book:string,chapter:string,id:string){seed(book,chapter);const v=versions.get(id);if(!v||v.chapter_id!==chapter)throw Error('版本不可访问');return v;}
 return{
  list:async(book,chapter,cursor,limit,markedOnly)=>{seed(book,chapter);const rows=[...versions.values()].filter(v=>v.chapter_id===chapter).map(metadata).filter(v=>!markedOnly||v.label!==null).sort((a,b)=>b.revision-a.revision);const before=cursor?Number(cursor):Infinity;const items=rows.filter(v=>v.revision<before).slice(0,limit);return{items,next_cursor:items.length===limit?String(items[items.length-1].revision):null,epoch};},
  get:async(book,chapter,id)=>{const v=lookup(book,chapter,id);const m=metadata(v);return{version:structuredClone(v),label:m.label,label_revision:m.label_revision,epoch};},
  label:async(book,chapter,id,label,expected,expectedEpoch)=>{const v=lookup(book,chapter,id);if(expectedEpoch!==epoch||metadata(v).label_revision!==expected)throw Error('名称已变化，请刷新');const value=label?.trim()||null;if(value&&(Array.from(value).length>120||new TextEncoder().encode(value).length>480||/\p{Cc}/u.test(value)))throw Error('名称无效');labels.set(id,{label:value,label_revision:expected+1});return metadata(v);},
  restore:async(book,chapter,id,expected,expectedEpoch)=>{const v=lookup(book,chapter,id);const c=scope(book,chapter);if(expectedEpoch!==epoch||c.revision!==expected)throw Error('章节已变化，不能覆盖');const next={...c,body:v.body,revision:c.revision+1};write({...read(),chapters:read().chapters.map(c=>c.id===chapter?next:c)});const newId=`fixture-${chapter}-${next.revision}`;versions.set(newId,{id:newId,chapter_id:chapter,revision:next.revision,body:next.body,source:'history',created_at:Math.floor(Date.now()/1000)});return structuredClone(read());}
 };
}
