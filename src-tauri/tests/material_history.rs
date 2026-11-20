use novel_core::*;
use serde_json::json;
fn setup()->(Service,String,Material){
 let mut s=Service::open(":memory:").unwrap();
 let state=s.apply(Command::CreateBook{title:"测试".into(),author:"".into()}).unwrap();
 let b=state.books[0].id.clone();
 let m=s.create_material(serde_json::from_value(json!({"book_id":b,"kind":"setting","title":"原名","fields":{"synopsis":"原内容"},"volume_id":null,"chapter_id":null,"source_chapter_id":state.chapters[0].id,"source_revision":0})).unwrap()).unwrap();
 (s,b,m)
}
fn edit(m:&Material,title:&str)->SaveMaterialInput{
 serde_json::from_value(json!({"id":m.id,"title":title,"fields":m.fields,"volume_id":m.volume_id,"chapter_id":m.chapter_id,"expected_revision":m.revision,"source_chapter_id":m.source_chapter_id,"source_revision":m.source_revision})).unwrap()
}
#[test] fn history_restore_requires_explicit_provenance_review(){
 let(mut s,b,m)=setup();
 let first=s.list_material_content_versions(&b,&m.id,None,20).unwrap();
 assert_eq!(first.items.len(),1);
 let m=s.save_material(edit(&m,"新名")).unwrap();
 let p=s.list_material_content_versions(&b,&m.id,None,20).unwrap();assert_eq!(p.items.len(),2);
 let restored=s.restore_material_content(&b,&m.id,&first.items[0].id,m.revision,&p.epoch).unwrap();
 assert_eq!(restored.title,"原名");assert!(restored.content_review_required);
 assert_eq!(restored.source_status,"stale");
 assert_eq!(restored.source_chapter_id,m.source_chapter_id);
 let saved=s.save_material(edit(&restored,"再编辑")).unwrap();assert!(saved.content_review_required);
 let ack=s.acknowledge_material_content(&b,&m.id,saved.revision,&p.epoch).unwrap();
 assert!(!ack.content_review_required);assert_eq!(ack.source_status,"current");
 assert!(s.acknowledge_material_content(&b,&m.id,saved.revision,&p.epoch).is_err());
 let backup=s.backup().unwrap();assert_eq!(serde_json::from_str::<serde_json::Value>(&backup).unwrap()["version"],13);
 let mut r=Service::open(":memory:").unwrap();r.restore(&backup).unwrap();
 assert_eq!(r.list_material_content_versions(&b,&m.id,None,50).unwrap().items.len(),s.list_material_content_versions(&b,&m.id,None,50).unwrap().items.len());
}
#[test] fn sqlite_old_and_new_history_and_review_roundtrip(){
 let(mut s,b,m)=setup();let p=s.list_material_content_versions(&b,&m.id,None,20).unwrap();let changed=s.save_material(edit(&m,"改")).unwrap();
 let restored=s.restore_material_content(&b,&m.id,&p.items[0].id,changed.revision,&p.epoch).unwrap();
 let d=std::env::temp_dir().join(format!("mh-{}",uuid::Uuid::new_v4()));std::fs::create_dir(&d).unwrap();
 let file=d.join("new.sqlite");s.export_sqlite_backup(&file,|_|Ok(())).unwrap();
 let prepared=prepare_restore(&file,|_|Ok(())).unwrap();assert_eq!(prepared.summary().material_history,3);
 let mut dest=Service::open(":memory:").unwrap();let guard=dest.restore_guard().unwrap();dest.commit_prepared_restore(prepared,&guard,&d.join("before.sqlite"),|_|Ok(())).unwrap();
 assert!(dest.get_material(&m.id,false).unwrap().content_review_required);assert_eq!(dest.get_material(&m.id,false).unwrap().source_status,"stale");
 assert_eq!(dest.list_material_content_versions(&b,&m.id,None,50).unwrap().items.len(),3);
 // Genuine old schema has neither new table nor new review column.
 let c=rusqlite::Connection::open(&file).unwrap();c.execute_batch("DROP TABLE material_content_versions; ALTER TABLE materials DROP COLUMN content_review_required;").unwrap();drop(c);
 let old=prepare_restore(&file,|_|Ok(())).unwrap();assert_eq!(old.summary().material_history,1);
 let guard=dest.restore_guard().unwrap();dest.commit_prepared_restore(old,&guard,&d.join("before2.sqlite"),|_|Ok(())).unwrap();
 let current=dest.get_material(&m.id,false).unwrap();assert_eq!(current.title,restored.title);assert!(!current.content_review_required);
 std::fs::remove_dir_all(d).unwrap();
}
#[test] fn stale_provenance_cannot_be_acknowledged_and_cas_epoch_and_scope_are_checked(){
 let(mut s,b,m)=setup();let p=s.list_material_content_versions(&b,&m.id,None,20).unwrap();
 let r=s.restore_material_content(&b,&m.id,&p.items[0].id,0,&p.epoch).unwrap();
 assert!(s.restore_material_content("foreign",&m.id,&p.items[0].id,r.revision,&p.epoch).is_err());
 assert!(s.restore_material_content(&b,&m.id,&p.items[0].id,0,&p.epoch).is_err());
 assert!(s.restore_material_content(&b,&m.id,&p.items[0].id,r.revision,"old").is_err());
 s.apply(Command::SaveChapter{id:m.source_chapter_id.clone().unwrap(),body:"changed".into(),expected_revision:0}).unwrap();
 assert!(s.acknowledge_material_content(&b,&m.id,r.revision,&p.epoch).is_err());
 assert!(s.get_material(&m.id,false).unwrap().content_review_required);
 let t=s.trash_material(&m.id,r.revision).unwrap();assert!(s.list_material_content_versions(&b,&m.id,None,20).is_err());
 let restored=s.restore_material(&m.id,t.revision).unwrap();assert!(restored.content_review_required);
}
#[test] fn json_legacy_baseline_and_tampered_history_reject(){
 let(mut s,b,m)=setup();let mut backup:serde_json::Value=serde_json::from_str(&s.backup().unwrap()).unwrap();
 backup["material_content_history"][0]["byte_count"]=json!(0);let before=s.backup().unwrap();assert!(s.restore(&backup.to_string()).is_err());assert_eq!(s.backup().unwrap(),before);
 let mut old:serde_json::Value=serde_json::from_str(&before).unwrap();old["version"]=json!(7);old.as_object_mut().unwrap().remove("foreshadow_proposals");old.as_object_mut().unwrap().remove("material_content_history");old["materials"][0].as_object_mut().unwrap().remove("content_review_required");
 s.restore(&old.to_string()).unwrap();assert_eq!(s.list_material_content_versions(&b,&m.id,None,20).unwrap().items.len(),1);
}
#[test] fn accepted_material_proposals_keep_review_flag_and_write_history(){
 let(mut s,b,m)=setup();let p=s.list_material_content_versions(&b,&m.id,None,20).unwrap();
 let r=s.restore_material_content(&b,&m.id,&p.items[0].id,0,&p.epoch).unwrap();
 let input=MaterialProposalInput{client_id:"test".into(),idempotency_key:"x".into(),material_id:m.id.clone(),base_revision:r.revision,new_title:"agent".into(),new_fields:r.fields.clone(),new_source_revision:r.source_revision,reason:"test".into()};
 let proposal=s.propose_material_edit(input).unwrap();
 let accepted=s.decide_material_proposal(&proposal.id,true,r.revision).unwrap();
 assert!(accepted.content_review_required);assert_eq!(accepted.source_status,"stale");
 assert_eq!(s.list_material_content_versions(&b,&m.id,None,20).unwrap().items.len(),3);
 assert_eq!(s.list_materials(&b,false).unwrap()[0].source_status,"stale");
}
#[test] fn pagination_scope_and_full_details(){
 let(mut s,b,mut m)=setup();for n in 0..24{m=s.save_material(edit(&m,&format!("title{n}"))).unwrap();}
 let p=s.list_material_content_versions(&b,&m.id,None,20).unwrap();assert_eq!(p.items.len(),20);assert!(p.next_cursor.is_some());
 let p2=s.list_material_content_versions(&b,&m.id,p.next_cursor,20).unwrap();assert_eq!(p2.items.len(),5);assert!(p2.next_cursor.is_none());
 assert!(s.get_material_content_version("foreign",&m.id,&p.items[0].id).is_err());
 assert!(s.list_material_content_versions(&b,&m.id,None,51).is_err());
 let d=s.get_material_content_version(&b,&m.id,&p.items[0].id).unwrap();assert_eq!(d.fields,m.fields);
}
#[test] fn agent_reads_review_flag_and_old_proposal_cannot_be_accepted(){
 use std::sync::{Arc,Mutex};use novel_core::agent::AgentBroker;
 let(mut s,b,m)=setup();let proposal=s.propose_material_edit(MaterialProposalInput{client_id:"test".into(),idempotency_key:"old".into(),material_id:m.id.clone(),base_revision:0,new_title:"old suggestion".into(),new_fields:m.fields.clone(),new_source_revision:m.source_revision,reason:"test".into()}).unwrap();
 let p=s.list_material_content_versions(&b,&m.id,None,20).unwrap();let r=s.restore_material_content(&b,&m.id,&p.items[0].id,0,&p.epoch).unwrap();
 assert!(s.decide_material_proposal(&proposal.id,true,r.revision).is_err());
 let broker=Arc::new(AgentBroker::new(Arc::new(Mutex::new(s))));let client=broker.connect().unwrap();
 client.call(json!({"method":"request_access","params":{"client_name":"test"}}));broker.grant_session(client.id(),&b).unwrap();
 let read=client.call(json!({"method":"get_material","params":{"material_id":m.id}}));
 assert_eq!(read["result"]["content_review_required"],true);assert_eq!(read["result"]["source_status"],"stale");
}
#[test] fn restored_unsourced_material_is_flagged_in_context_task(){
 let(mut s,b,m)=setup();let mut input=edit(&m,"无来源");input.source_chapter_id=None;input.source_revision=None;let m=s.save_material(input).unwrap();
 let page=s.list_material_content_versions(&b,&m.id,None,20).unwrap();
 let r=s.restore_material_content(&b,&m.id,&page.items[0].id,m.revision,&page.epoch).unwrap();assert!(r.content_review_required);assert_eq!(r.source_status,"none");
 let task=s.start_context_task(&b,"review-flag").unwrap();while s.step_next_task().unwrap().is_some(){}
 let report=s.get_context_task(&b,&task.id).unwrap();assert!(report.items.iter().find(|i|i.item_id==m.id).unwrap().needs_review);
}

#[test]
fn large_history_titles_roundtrip_sqlite_without_spending_metadata_budget() {
 let (mut s,b,m)=setup();
 let mut backup:serde_json::Value=serde_json::from_str(&s.backup().unwrap()).unwrap();
 let title="文".repeat(1365)+"x"; // 4096 UTF-8 bytes, legal title limit
 backup["materials"][0]["title"]=json!(title);
 backup["materials"][0]["fields"]=json!({});
 backup["materials"][0]["revision"]=json!(8499);
 let mut template=backup["material_content_history"][0].clone();
 template["title"]=json!(title);template["fields"]=json!({});template["byte_count"]=json!(4098);
 backup["material_content_history"]=json!((0..8500).map(|n|{
   let mut v=template.clone();v["id"]=json!(format!("large-history-{n}"));v["material_revision"]=json!(n);v
 }).collect::<Vec<_>>());
 s.restore(&backup.to_string()).unwrap();drop(backup);
 let dir=std::env::temp_dir().join(format!("mh-large-{}",uuid::Uuid::new_v4()));
 std::fs::create_dir(&dir).unwrap();let file=dir.join("history.sqlite");
 s.export_sqlite_backup(&file,|_|Ok(())).unwrap();
 assert!(std::fs::metadata(&file).unwrap().len()>32*1024*1024);
 let prepared=prepare_restore(&file,|_|Ok(())).unwrap();
 assert_eq!(prepared.summary().material_history,8500);
 let mut dest=Service::open(":memory:").unwrap();let guard=dest.restore_guard().unwrap();
 dest.commit_prepared_restore(prepared,&guard,&dir.join("before.sqlite"),|_|Ok(())).unwrap();
 assert_eq!(dest.get_material(&m.id,false).unwrap().title,title);
 let page=dest.list_material_content_versions(&b,&m.id,None,50).unwrap();
 assert_eq!(page.items[0].material_revision,8499);
 let restored:serde_json::Value=serde_json::from_str(&dest.backup().unwrap()).unwrap();
 assert_eq!(restored["material_content_history"].as_array().unwrap().len(),8500);
 drop(restored);
 // Actual metadata remains capped; history byte_count cannot bypass validation.
 let c=rusqlite::Connection::open(&file).unwrap();
 c.execute("UPDATE material_content_versions SET byte_count=0",[]).unwrap();
 assert!(prepare_restore(&file,|_|Ok(())).is_err());
 c.execute("UPDATE material_content_versions SET byte_count=4098,source=?1",["x".repeat(5000)]).unwrap();
 let error=match prepare_restore(&file,|_|Ok(())){Ok(_)=>panic!("accepted excess metadata"),Err(e)=>e};
 assert!(error.contains("metadata32MiB"),"{error}");
 drop(c);std::fs::remove_dir_all(dir).unwrap();
}
