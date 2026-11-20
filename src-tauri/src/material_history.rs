//! Trusted local content history. References and plot state never roll back.
use crate::{Material,Service,WorkflowError,WorkflowResult,SaveMaterialInput};
use rusqlite::{Connection,OptionalExtension,params,TransactionBehavior};
use serde::{Serialize,Deserialize};
use std::collections::{BTreeMap,HashMap,HashSet};
const COUNT:i64=100000;
const BYTES:i64=256*1024*1024;
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(deny_unknown_fields)]
pub struct MaterialContentVersion {pub id:String,pub material_id:String,pub material_revision:i64,pub title:String,pub fields:BTreeMap<String,String>,pub created_at:i64,pub source:String,pub byte_count:i64}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct MaterialVersionSummary {pub id:String,pub material_revision:i64,pub title:String,pub created_at:i64,pub source:String}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct MaterialVersionPage {pub items:Vec<MaterialVersionSummary>,pub next_cursor:Option<i64>,pub epoch:String}
fn fail(s:&str)->WorkflowError{WorkflowError{code:"material_history".into(),message:s.into()}}
fn db(_:rusqlite::Error)->WorkflowError{fail("资料历史操作失败，未完成更改")}
fn size(title:&str,fields:&BTreeMap<String,String>)->WorkflowResult<i64>{Ok((title.len()+serde_json::to_string(fields).map_err(|_|fail("历史内容无效"))?.len())as i64)}
pub(crate) fn migrate(tx:&Connection)->WorkflowResult<()>{
 tx.execute_batch("CREATE TABLE IF NOT EXISTS material_content_versions(id TEXT PRIMARY KEY,material_id TEXT NOT NULL REFERENCES materials(id) ON DELETE CASCADE,material_revision INTEGER NOT NULL,title TEXT NOT NULL,fields TEXT NOT NULL,created_at INTEGER NOT NULL,source TEXT NOT NULL,byte_count INTEGER NOT NULL,UNIQUE(material_id,material_revision));").map_err(db)?;
 // Preflight legacy scale before collecting IDs; all changes remain in caller transaction.
 let (existing,used):(i64,i64)=tx.query_row("SELECT count(*),coalesce(sum(byte_count),0) FROM material_content_versions",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(db)?;
 let (missing,raw_bytes):(i64,i64)=tx.query_row("SELECT count(*),coalesce(sum(length(CAST(title AS BLOB))+length(CAST(fields AS BLOB))),0) FROM materials m WHERE NOT EXISTS(SELECT 1 FROM material_content_versions h WHERE h.material_id=m.id)",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(db)?;
 if existing.checked_add(missing).is_none_or(|v|v>COUNT)||used.checked_add(raw_bytes).is_none_or(|v|v>BYTES){return Err(fail("旧书库资料基线超过历史100000条或256MiB上限，升级已回滚；请保留原库并用旧版导出备份"));}
 let ids=tx.prepare("SELECT id FROM materials m WHERE NOT EXISTS(SELECT 1 FROM material_content_versions h WHERE h.material_id=m.id)").map_err(db)?.query_map([],|r|r.get::<_,String>(0)).map_err(db)?.collect::<rusqlite::Result<Vec<_>>>().map_err(db)?;
 for id in ids {let m=crate::materials::raw(tx,&id)?;crate::materials::shape(&m.kind,&m.title,&m.fields,m.volume_id.as_deref(),m.chapter_id.as_deref())?;insert_snapshot(tx,&m,"baseline",size(&m.title,&m.fields)?)?;}
 Ok(())
}
pub(crate) fn snapshot(c:&Connection,m:&Material,source:&str)->WorkflowResult<()>{
 let exists:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM material_content_versions WHERE material_id=?1 AND material_revision=?2)",params![m.id,m.revision],|r|r.get(0)).map_err(db)?;
 if exists{return Ok(());}
 crate::materials::shape(&m.kind,&m.title,&m.fields,m.volume_id.as_deref(),m.chapter_id.as_deref())?;
 let bytes=size(&m.title,&m.fields)?;
 let(n,b):(i64,i64)=c.query_row("SELECT count(*),coalesce(sum(byte_count),0) FROM material_content_versions",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(db)?;
 if n>=COUNT || b.checked_add(bytes).is_none_or(|v|v>BYTES){return Err(fail("资料历史达到100000条或256MiB上限，旧数据未更改；请先备份，不会自动清理"));}
 insert_snapshot(c,m,source,bytes)
}
fn insert_snapshot(c:&Connection,m:&Material,source:&str,bytes:i64)->WorkflowResult<()> {
 c.execute("INSERT INTO material_content_versions VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![uuid::Uuid::new_v4().to_string(),m.id,m.revision,m.title,serde_json::to_string(&m.fields).map_err(|_|fail("历史内容无效"))?,m.updated_at,source,bytes]).map_err(db)?;Ok(())
}

fn row(r:&rusqlite::Row<'_>)->rusqlite::Result<MaterialContentVersion>{
 let text:String=r.get(4)?;
 let fields=serde_json::from_str(&text).map_err(|e|rusqlite::Error::FromSqlConversionFailure(4,rusqlite::types::Type::Text,Box::new(e)))?;
 Ok(MaterialContentVersion{id:r.get(0)?,material_id:r.get(1)?,material_revision:r.get(2)?,title:r.get(3)?,fields,created_at:r.get(5)?,source:r.get(6)?,byte_count:r.get(7)?})
}
pub(crate) fn all(c:&Connection)->crate::Result<Vec<MaterialContentVersion>>{
 c.prepare("SELECT id,material_id,material_revision,title,fields,created_at,source,byte_count FROM material_content_versions").map_err(|e|e.to_string())?.query_map([],row).map_err(|e|e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e|e.to_string())
}
fn valid(m:&Material,v:&MaterialContentVersion)->WorkflowResult<()>{
 if v.id.is_empty()||v.id.len()>128||v.material_id!=m.id||v.material_revision<0||v.material_revision>m.revision||v.created_at<0||v.source.is_empty()||v.source.len()>256||v.byte_count!=size(&v.title,&v.fields)?{return Err(fail("资料历史字段无效"));}
 crate::materials::shape(&m.kind,&v.title,&v.fields,m.volume_id.as_deref(),m.chapter_id.as_deref())?;
 if v.material_revision==m.revision && (v.title!=m.title||v.fields!=m.fields){return Err(fail("当前资料与当前历史不一致"));}
 Ok(())
}
pub(crate) fn validate(materials:&[Material],versions:&[MaterialContentVersion])->crate::Result<()>{
 if versions.len()>COUNT as usize{return Err("资料历史记录过多".into())}
 let map:HashMap<_,_>=materials.iter().map(|m|(m.id.as_str(),m)).collect();let mut ids=HashSet::new();let mut revs=HashSet::new();let mut total=0i64;
 for v in versions {let m=map.get(v.material_id.as_str()).ok_or("资料历史归属不存在")?;valid(m,v).map_err(|e|e.message)?;if !ids.insert(&v.id)||!revs.insert((&v.material_id,v.material_revision)){return Err("资料历史重复".into());}total=total.checked_add(v.byte_count).ok_or("资料历史容量溢出")?;if total>BYTES{return Err("资料历史超过256MiB".into());}}
 let covered:HashSet<_>=versions.iter().map(|v|v.material_id.as_str()).collect();for m in materials {if !covered.contains(m.id.as_str()){return Err("资料历史缺少当前资料基线".into());}}
 Ok(())
}
pub(crate) fn validate_stream(c:&Connection)->crate::Result<u64>{
 let(n,b):(i64,i64)=c.query_row("SELECT count(*),coalesce(sum(byte_count),0) FROM material_content_versions",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())?;
 if n>COUNT||b>BYTES||b<0{return Err("资料历史超过限制".into());}
 let materials=crate::materials::all(c)?;let by_id:HashMap<_,_>=materials.iter().map(|m|(m.id.as_str(),m)).collect();
 let mut st=c.prepare("SELECT id,material_id,material_revision,title,CASE WHEN length(CAST(fields AS BLOB))<=2097152 THEN fields ELSE NULL END,created_at,source,byte_count FROM material_content_versions").map_err(|e|e.to_string())?;
 let mut rows=st.query([]).map_err(|e|e.to_string())?;
 while let Some(r)=rows.next().map_err(|e|e.to_string())?{let v=row(r).map_err(|_|"资料历史格式或大小无效")?;let m=by_id.get(v.material_id.as_str()).ok_or("资料历史归属不存在")?;valid(m,&v).map_err(|e|e.message)?;}
 let missing:i64=c.query_row("SELECT count(*) FROM materials m WHERE NOT EXISTS(SELECT 1 FROM material_content_versions h WHERE h.material_id=m.id)",[],|r|r.get(0)).map_err(|e|e.to_string())?;if missing!=0{return Err("资料历史缺少基线".into());}
 Ok(n as u64)
}
pub(crate) fn insert_all(c:&Connection,vs:&[MaterialContentVersion])->crate::Result<()>{
 for v in vs{c.execute("INSERT INTO material_content_versions VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![v.id,v.material_id,v.material_revision,v.title,serde_json::to_string(&v.fields).map_err(|e|e.to_string())?,v.created_at,v.source,v.byte_count]).map_err(|e|e.to_string())?;}Ok(())
}
fn scoped(c:&Connection,book:&str,id:&str)->WorkflowResult<Material>{
 let m=crate::materials::get_material_in(c,id,false)?;
 if m.book_id!=book{return Err(fail("资料不属于该作品"));}Ok(m)
}
fn epoch(c:&Connection)->WorkflowResult<String>{c.query_row("SELECT value FROM workflow_meta WHERE key='epoch'",[],|r|r.get(0)).map_err(db)}
fn check(c:&Connection,book:&str,id:&str,revision:i64,expected_epoch:&str)->WorkflowResult<Material>{
 if epoch(c)?!=expected_epoch{return Err(fail("书库已恢复，请重新读取资料历史"));}
 let m=scoped(c,book,id)?;if m.revision!=revision||revision<0||revision==i64::MAX{return Err(fail("资料已改变，请重新读取"))}Ok(m)
}
fn detail(c:&Connection,book:&str,id:&str,version:&str)->WorkflowResult<MaterialContentVersion>{
 let m=scoped(c,book,id)?;
 let v=c.query_row("SELECT id,material_id,material_revision,title,CASE WHEN length(CAST(fields AS BLOB))<=2097152 THEN fields ELSE NULL END,created_at,source,byte_count FROM material_content_versions WHERE id=?1 AND material_id=?2",params![version,id],row).optional().map_err(db)?.ok_or_else(||fail("资料历史不存在或不属于该资料"))?;
 valid(&m,&v)?;Ok(v)
}
impl Service {
 pub fn list_material_content_versions(&self,book:&str,id:&str,before:Option<i64>,limit:u32)->WorkflowResult<MaterialVersionPage>{
  if limit==0||limit>50||before.is_some_and(|n|n<0){return Err(fail("历史分页参数无效"));}
  let tx=self.conn.unchecked_transaction().map_err(db)?;scoped(&tx,book,id)?;
  let mut st=tx.prepare("SELECT id,material_revision,title,created_at,source FROM material_content_versions WHERE material_id=?1 AND (?2 IS NULL OR material_revision<?2) ORDER BY material_revision DESC LIMIT ?3").map_err(db)?;
  let mut items=st.query_map(params![id,before,limit+1],|r|Ok(MaterialVersionSummary{id:r.get(0)?,material_revision:r.get(1)?,title:r.get(2)?,created_at:r.get(3)?,source:r.get(4)?})).map_err(db)?.collect::<rusqlite::Result<Vec<_>>>().map_err(db)?;
  let next=if items.len()>limit as usize {items.pop();items.last().map(|v|v.material_revision)}else{None};let e=epoch(&tx)?;drop(st);tx.commit().map_err(db)?;
  Ok(MaterialVersionPage{items,next_cursor:next,epoch:e})
 }
 pub fn get_material_content_version(&self,book:&str,id:&str,version:&str)->WorkflowResult<MaterialContentVersion>{detail(&self.conn,book,id,version)}
 pub fn restore_material_content(&mut self,book:&str,id:&str,version:&str,revision:i64,expected_epoch:&str)->WorkflowResult<Material>{
  let tx=self.conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db)?;
  let m=check(&tx,book,id,revision,expected_epoch)?;let v=detail(&tx,book,id,version)?;
  snapshot(&tx,&m,"before_restore")?;
  let input=SaveMaterialInput{id:m.id.clone(),title:v.title,fields:v.fields,volume_id:m.volume_id.clone(),chapter_id:m.chapter_id.clone(),expected_revision:m.revision,source_chapter_id:m.source_chapter_id.clone(),source_revision:m.source_revision,story_order:m.story_order,story_time:m.story_time.clone(),tracking_status:m.tracking_status.clone(),payoff_chapter_id:m.payoff_chapter_id.clone(),payoff_revision:m.payoff_revision};
  let mut restored=crate::materials::save_material_in_tx(&tx,input)?;
  tx.execute("UPDATE materials SET content_review_required=1 WHERE id=?1",[id]).map_err(db)?;
  restored.content_review_required=true;crate::materials::review_status(&mut restored);snapshot(&tx,&restored,"restore")?;
  tx.execute("UPDATE material_content_versions SET source='restore' WHERE material_id=?1 AND material_revision=?2",params![id,restored.revision]).map_err(db)?;
  tx.commit().map_err(db)?;Ok(restored)
 }
 pub fn acknowledge_material_content(&mut self,book:&str,id:&str,revision:i64,expected_epoch:&str)->WorkflowResult<Material>{
  let tx=self.conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(db)?;
  let m=check(&tx,book,id,revision,expected_epoch)?;
  if !m.content_review_required{return Err(fail("资料无需内容复核"));}
  crate::materials::require_current_source(&tx,book,m.source_chapter_id.as_deref(),m.source_revision)?;
  crate::materials::require_current_source(&tx,book,m.payoff_chapter_id.as_deref(),m.payoff_revision)?;
  tx.execute("UPDATE materials SET content_review_required=0,revision=revision+1,updated_at=MAX(updated_at+1,?1) WHERE id=?2",params![std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_|fail("时钟无效"))?.as_secs()as i64,id]).map_err(db)?;
  let result=crate::materials::get_material_in_tx(&tx,id,false)?;tx.commit().map_err(db)?;Ok(result)
 }
}
#[cfg(test)] mod tests{
 use super::*;use crate::Command;use serde_json::json;
 fn setup()->(Service,String,Material){let mut s=Service::open(":memory:").unwrap();let st=s.apply(Command::CreateBook{title:"x".into(),author:"".into()}).unwrap();let b=st.books[0].id.clone();let m=s.create_material(serde_json::from_value(json!({"book_id":b,"kind":"setting","title":"a","fields":{"synopsis":"old"},"volume_id":null,"chapter_id":null})).unwrap()).unwrap();(s,b,m)}
 fn edit(m:&Material)->SaveMaterialInput{serde_json::from_value(json!({"id":m.id,"title":"new","fields":{"synopsis":"new"},"volume_id":null,"chapter_id":null,"expected_revision":m.revision})).unwrap()}
 #[test] fn history_failure_rolls_back_material_and_restore_flag(){
 let(mut s,b,m)=setup();let before=s.backup().unwrap();
 s.conn.execute_batch("CREATE TRIGGER reject_history BEFORE INSERT ON material_content_versions BEGIN SELECT RAISE(ABORT,'injected'); END").unwrap();
 assert!(s.save_material(edit(&m)).is_err());assert_eq!(s.backup().unwrap(),before);
 let p=s.list_material_content_versions(&b,&m.id,None,20).unwrap();assert!(s.restore_material_content(&b,&m.id,&p.items[0].id,0,&p.epoch).is_err());assert_eq!(s.backup().unwrap(),before);
 }
 #[test] fn quota_refuses_without_mutating_or_silent_history_cleanup(){
 let(mut s,_,m)=setup();s.conn.execute("UPDATE material_content_versions SET byte_count=?1",[BYTES]).unwrap();
 assert!(s.save_material(edit(&m)).is_err());assert_eq!(s.get_material(&m.id,false).unwrap(),m);
 assert_eq!(s.conn.query_row("SELECT count(*) FROM material_content_versions",[],|r|r.get::<_,i64>(0)).unwrap(),1);
 }
 #[test] fn migration_is_atomic_and_includes_recycled_material(){
 let(mut s,_,m)=setup();let old=s.trash_material(&m.id,m.revision).unwrap();
 s.conn.execute_batch("DROP TABLE material_content_versions;ALTER TABLE materials DROP COLUMN content_review_required").unwrap();
 crate::materials::migrate(&s.conn).unwrap();assert_eq!(s.conn.query_row("SELECT count(*) FROM material_content_versions WHERE material_id=?1",[&m.id],|r|r.get::<_,i64>(0)).unwrap(),1);
 assert_eq!(s.get_material(&m.id,true).unwrap().deleted_at,old.deleted_at);
 s.conn.execute_batch("DROP TABLE material_content_versions;ALTER TABLE materials DROP COLUMN content_review_required;UPDATE materials SET title=''").unwrap();
 assert!(crate::materials::migrate(&s.conn).is_err());
 let count:i64=s.conn.query_row("SELECT count(*) FROM sqlite_schema WHERE name='material_content_versions'",[],|r|r.get(0)).unwrap();assert_eq!(count,0);
 let count:i64=s.conn.query_row("SELECT count(*) FROM pragma_table_info('materials') WHERE name='content_review_required'",[],|r|r.get(0)).unwrap();assert_eq!(count,0);
 }
}
