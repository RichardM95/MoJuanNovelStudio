// UI-only fictional data; never imported by production and never writes SQLite/files.
import type {LibraryState} from '../src/types';
import type {ReplaceAPI,ReplacePreview} from '../src/replaceTypes';
export function replaceFixture(read:()=>LibraryState,write:(next:LibraryState)=>void):ReplaceAPI {
  let candidate:{preview:ReplacePreview;fingerprint:string;changes:{id:string;before:string;after:string}[]}|null=null;
  return {
    async preview(bookId,query,replacement){
      if(!query||query===replacement)throw Error('请输入不同的查找词和替换词');
      const state=read();const volumes=new Set(state.volumes.filter(v=>v.book_id===bookId&&!v.deleted_at).map(v=>v.id));
      const changes=state.chapters.filter(c=>!c.deleted_at&&volumes.has(c.volume_id)&&c.body.includes(query)).map(c=>({id:c.id,before:c.body,after:c.body.split(query).join(replacement)}));
      const chapters=changes.map(c=>{const row=state.chapters.find(x=>x.id===c.id)!;return {id:c.id,title:row.title,revision:row.revision,matches:c.before.split(query).length-1,before_snippet:c.before.slice(0,200),after_snippet:c.after.slice(0,200)}});
      const preview:ReplacePreview={id:crypto.randomUUID(),book_id:bookId,query,replacement,expires_at:Math.floor(Date.now()/1000)+600,chapters,total_matches:chapters.reduce((n,c)=>n+c.matches,0)};
      candidate={preview,fingerprint:JSON.stringify(state),changes};return structuredClone(preview);
    },
    async detail(id,chapterId){if(candidate?.preview.id!==id)throw Error('候选失效');const c=candidate.changes.find(c=>c.id===chapterId);if(!c)throw Error('章节不存在');return {chapter_id:c.id,before:c.before,after:c.after};},
    async commit(id){if(candidate?.preview.id!==id)throw Error('候选失效');const pending=candidate;candidate=null;if(pending.fingerprint!==JSON.stringify(read()))throw Error('作品已变化，请重新预览');const next=structuredClone(read());for(const change of pending.changes){const row=next.chapters.find(c=>c.id===change.id)!;row.body=change.after;row.revision++;}write(next);return {chapters:pending.changes.length,matches:pending.preview.total_matches};},
    async discard(id){if(candidate?.preview.id===id)candidate=null;}
  };
}
