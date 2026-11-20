// Fictional in-memory UI fixture only. No native file reading or SQLite writes.
import type {ImportAPI,ImportPreview} from '../src/importTypes';
import type {LibraryState} from '../src/types';
export function importFixture(read:()=>LibraryState,write:(s:LibraryState)=>void):ImportAPI {
 const text='序言文字。\r\n第1卷 归途\r\n第一章 来信\r\n林溪收到一封信。\r\n\r\n第二章 回家\n她踏上归途。\n';
 let candidate:{preview:ImportPreview;bodies:string[]}|null=null;
 function make(title:string,author:string,mode:'headings'|'single'):ImportPreview {
   const rows=mode==='single'?[{title:'正文',chapters:[{title:'正文',body:text}]}]:[{title:'正文',chapters:[{title:'前言',body:'序言文字。\r\n'}]},{title:'归途',chapters:[{title:'来信',body:'林溪收到一封信。\r\n\r\n'},{title:'回家',body:'她踏上归途。\n'}]}];
   const bodies:string[]=[];
   const volumes=rows.map(v=>({title:v.title,chapters:v.chapters.map(c=>{const index=bodies.length;bodies.push(c.body);return {index,title:c.title,bytes:new TextEncoder().encode(c.body).length,snippet:c.body}})}));
   const preview:ImportPreview={id:crypto.randomUUID(),source_name:'虚构导入示例.txt',title,author,mode,expires_at:Math.floor(Date.now()/1000)+600,volumes,total_chapters:bodies.length,total_bytes:bodies.reduce((n,b)=>n+new TextEncoder().encode(b).length,0)};
   candidate={preview,bodies};return structuredClone(preview);
 }
 return {
   async choose(mode){return make('虚构导入示例','',mode)},
   async revise(id,title,author,mode){if(candidate?.preview.id!==id)throw Error('候选失效');if(!title.trim())throw Error('请输入书名');return make(title,author,mode)},
   async detail(id,index){if(candidate?.preview.id!==id||candidate.bodies[index]===undefined)throw Error('候选失效');return {index,body:candidate.bodies[index]}},
   async discard(id){if(candidate?.preview.id===id)candidate=null},
   async commit(id){if(candidate?.preview.id!==id)throw Error('候选已处理');const c=candidate;candidate=null;const next=structuredClone(read()),book_id=crypto.randomUUID();next.books.push({id:book_id,title:c.preview.title,author:c.preview.author,deleted_at:null});c.preview.volumes.forEach((v,vi)=>{const volume_id=crypto.randomUUID();next.volumes.push({id:volume_id,book_id,title:v.title,sort_order:vi,deleted_at:null});v.chapters.forEach((ch,ci)=>next.chapters.push({id:crypto.randomUUID(),volume_id,title:ch.title,body:c.bodies[ch.index],sort_order:ci,revision:0,deleted_at:null}))});write(next);return {book_id,volumes:c.preview.volumes.length,chapters:c.preview.total_chapters}}
 };
}
