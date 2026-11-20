// Fictional UI-only operations; no files are created or restored here.
import type {BackupAPI,BackupJob,RestoreSummary} from '../src/backupTypes';
import type {LibraryState} from '../src/types';
export function backupFixture(read:()=>LibraryState,write:(s:LibraryState)=>void):BackupAPI{
 let job:BackupJob|null=null;let candidate:LibraryState|null=null;
 const now=()=>Math.floor(Date.now()/1000), clone=()=>structuredClone(job);
 const summary=():RestoreSummary=>({format:'sqlite',books:read().books.length,volumes:read().volumes.length,chapters:read().chapters.length,history:0,materials:0,version_labels:0,canonical_bytes:new TextEncoder().encode(JSON.stringify(read())).length});
 const find=(id:string)=>{if(!job||job.id!==id)throw Error('演示任务不存在');return job};
 function start(kind:'restore'|'export_sqlite'){
  if(job&&['running','prepared','committing'].includes(job.status))throw Error('已有演示任务');
  job={id:crypto.randomUUID(),kind,status:'running',progress:{phase:kind==='restore'?'prepare':'export',completed:0,total:null},cancel_requested:false,created_at:now(),updated_at:now(),expires_at:null,source_name:kind==='restore'?'虚构备份示例.sqlite':null,preview:null,confirmation_token:null,result:null,error_code:null,error_message:null};const id=job.id;
  queueMicrotask(()=>{const j=find(id);if(j.status!=='running')return;if(kind==='restore'){candidate=structuredClone(read());j.status='prepared';j.progress={phase:'prepared',completed:read().chapters.length,total:read().chapters.length};j.preview=summary();j.confirmation_token=crypto.randomUUID();j.expires_at=now()+600;}else{j.status='done';j.progress={phase:'export',completed:1,total:1};j.result={output_path:'虚构演示路径/墨卷书库.sqlite',recovery_path:null,summary:null}}});return clone()!;
 }
 return{
  current:async()=>clone(),startExport:async()=>start('export_sqlite'),startPrepare:async()=>start('restore'),status:async id=>structuredClone(find(id)),
  cancel:async id=>{const j=find(id);if(['running','prepared'].includes(j.status)){j.status='cancelled';j.cancel_requested=true;j.confirmation_token=null;candidate=null;}return structuredClone(j)},
  confirm:async(id,token)=>{const j=find(id);if(j.status!=='prepared'||j.confirmation_token!==token||!candidate)throw Error('演示候选已失效');j.confirmation_token=null;j.status='committing';j.progress={phase:'commit',completed:0,total:1};const frozen=candidate;queueMicrotask(()=>{write(structuredClone(frozen));j.status='done';j.progress={phase:'commit',completed:1,total:1};j.result={output_path:null,recovery_path:'虚构演示路径/恢复前留底.sqlite',summary:j.preview};candidate=null;});return structuredClone(j)},
  exportJson:async()=> '虚构演示路径/墨卷备份.json',
 };
}
