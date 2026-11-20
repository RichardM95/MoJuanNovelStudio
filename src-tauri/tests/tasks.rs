use novel_core::*;
use std::collections::BTreeMap;
use std::path::PathBuf;
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let d = std::env::temp_dir().join(format!("context-task-fixture-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&d).unwrap();
        Self(d)
    }
    fn db(&self) -> PathBuf {
        self.0.join("library.sqlite")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn setup(f: &Fixture) -> (Service, LibraryState) {
    let mut s = Service::open(f.db()).unwrap();
    let a = s
        .apply(Command::CreateBook {
            title: "上下文虚构书".into(),
            author: "隔离测试".into(),
        })
        .unwrap();
    (s, a)
}
fn note(a: &LibraryState) -> CreateMaterialInput {
    CreateMaterialInput {
        book_id: a.books[0].id.clone(),
        kind: "summary".into(),
        title: "虚构摘要卡".into(),
        fields: BTreeMap::from([(
            "content".into(),
            "SECRET_MATERIAL_BODY_NOT_IN_REPORT".into(),
        )]),
        volume_id: None,
        chapter_id: None,
        source_chapter_id: Some(a.chapters[0].id.clone()),
        source_revision: Some(0),
        story_order: 0,
        story_time: None,
        tracking_status: None,
        payoff_chapter_id: None,
        payoff_revision: None,
    }
}
#[test]
fn actual_steps_produce_metadata_only_report_and_idempotent_requests() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = &a.books[0].id;
    let m = s.create_material(note(&a)).unwrap();
    s.apply(Command::SaveChapter {
        id: a.chapters[0].id.clone(),
        body: "中 文\n🙂".into(),
        expected_revision: 0,
    })
    .unwrap();
    let t = s.start_context_task(b, "start-one").unwrap();
    assert_eq!(t.status, "queued");
    assert_eq!((t.done, t.total), (0, 2));
    assert_eq!(s.start_context_task(b, "start-one").unwrap().id, t.id);
    let t = s.step_next_task().unwrap().unwrap();
    assert_eq!(t.done, 1);
    assert_eq!(t.status, "running");
    let t = s.step_next_task().unwrap().unwrap();
    assert_eq!(t.status, "completed");
    assert_eq!((t.done, t.total), (2, 2));
    assert!(s.step_next_task().unwrap().is_none());
    let detail = s.get_context_task(b, &t.id).unwrap();
    assert_eq!(detail.items.len(), 2);
    assert_eq!(detail.items[0].char_count, Some(3));
    assert_eq!(detail.items[1].item_id, m.id);
    assert_eq!(detail.items[1].source_status.as_deref(), Some("stale"));
    assert!(detail.items[1].needs_review);
    assert!(!serde_json::to_string(&detail)
        .unwrap()
        .contains("SECRET_MATERIAL_BODY_NOT_IN_REPORT"));
}
#[test]
fn cancel_retry_and_scope_never_mix_attempts() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = &a.books[0].id;
    let t = s.start_context_task(b, "original").unwrap();
    let cancelled = s.cancel_context_task(b, &t.id).unwrap();
    assert_eq!(cancelled.status, "cancelled");
    assert_eq!(s.cancel_context_task(b, &t.id).unwrap(), cancelled);
    assert!(s.step_next_task().unwrap().is_none());
    let retry = s.retry_context_task(b, &t.id, "retry-one").unwrap();
    assert_ne!(retry.id, t.id);
    assert_eq!(retry.done, 0);
    assert_eq!(retry.retry_of, Some(t.id.clone()));
    assert_eq!(
        s.retry_context_task(b, &t.id, "retry-one").unwrap().id,
        retry.id
    );
    assert!(s.start_context_task(b, "retry-one").is_err());
    assert_eq!(
        s.get_context_task(b, &t.id).unwrap().task.status,
        "cancelled"
    );
    let other = s
        .apply(Command::CreateBook {
            title: "别书".into(),
            author: "".into(),
        })
        .unwrap();
    let otherbook = &other.books.iter().find(|x| x.id != *b).unwrap().id;
    assert!(s.get_context_task(otherbook, &t.id).is_err());
    assert!(s.cancel_context_task(otherbook, &retry.id).is_err());
}
#[test]
fn reopened_queued_and_running_are_interrupted_and_resume_only_matching_snapshot() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = a.books[0].id.clone();
    s.create_material(note(&a)).unwrap();
    let first = s.start_context_task(&b, "running").unwrap();
    let second = s.start_context_task(&b, "queued").unwrap();
    s.step_next_task().unwrap();
    drop(s);
    let mut s = Service::open(f.db()).unwrap();
    assert!(s
        .list_context_tasks(&b)
        .unwrap()
        .iter()
        .all(|t| t.status == "interrupted"));
    assert!(s.step_next_task().unwrap().is_none());
    let resumed = s.resume_context_task(&b, &first.id).unwrap();
    assert_eq!(resumed.done, 1);
    while s.step_next_task().unwrap().is_some() {}
    assert_eq!(
        s.get_context_task(&b, &first.id).unwrap().task.status,
        "completed"
    );
    s.apply(Command::SaveChapter {
        id: a.chapters[0].id.clone(),
        body: "改稿".into(),
        expected_revision: 0,
    })
    .unwrap();
    assert!(s.resume_context_task(&b, &second.id).is_err());
    assert_eq!(
        s.get_context_task(&b, &second.id).unwrap().task.status,
        "interrupted"
    );
}
#[test]
fn final_manifest_rejects_previously_processed_change() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = &a.books[0].id;
    s.create_material(note(&a)).unwrap();
    let t = s.start_context_task(b, "changing").unwrap();
    s.step_next_task().unwrap();
    s.apply(Command::SaveChapter {
        id: a.chapters[0].id.clone(),
        body: "已处理章节后来更改".into(),
        expected_revision: 0,
    })
    .unwrap();
    while s.step_next_task().unwrap().is_some() {}
    let result = s.get_context_task(b, &t.id).unwrap();
    assert_ne!(result.task.status, "completed");
    assert_eq!(result.task.reason_code.as_deref(), Some("input_changed"));
    assert!(s.resume_context_task(b, &t.id).is_err());
}
#[test]
fn restore_clears_runtime_tasks_and_json_does_not_export_them() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = &a.books[0].id;
    let t = s.start_context_task(b, "runtime").unwrap();
    let json = s.backup().unwrap();
    assert!(serde_json::from_str::<serde_json::Value>(&json)
        .unwrap()
        .get("tasks")
        .is_none());
    s.restore(&json).unwrap();
    assert!(s.list_context_tasks(b).unwrap().is_empty());
    assert!(s.get_context_task(b, &t.id).is_err());
    assert!(s.step_next_task().unwrap().is_none());
}
#[test]
fn two_workers_share_persistent_cursor_without_duplicate_results() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = a.books[0].id.clone();
    for n in 0..8 {
        s.apply(Command::CreateChapter {
            volume_id: a.volumes[0].id.clone(),
            title: format!("章{n}"),
        })
        .unwrap();
    }
    let t = s.start_context_task(&b, "workers").unwrap();
    let shared = std::sync::Arc::new(std::sync::Mutex::new(s));
    let run = |shared: std::sync::Arc<std::sync::Mutex<Service>>| {
        std::thread::spawn(move || loop {
            let next = shared.lock().unwrap().step_next_task().unwrap();
            if next.is_none() {
                break;
            }
            std::thread::yield_now();
        })
    };
    let first = run(shared.clone());
    let second = run(shared.clone());
    first.join().unwrap();
    second.join().unwrap();
    let detail = shared.lock().unwrap().get_context_task(&b, &t.id).unwrap();
    assert_eq!(detail.task.status, "completed");
    assert_eq!((detail.task.done, detail.items.len()), (9, 9));
    let ids: std::collections::HashSet<_> = detail.items.iter().map(|x| &x.item_id).collect();
    assert_eq!(ids.len(), 9);
}
#[test]
fn unavailable_source_dependency_changes_even_when_public_status_stays_unavailable() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = &a.books[0].id;
    s.create_material(note(&a)).unwrap();
    s.apply(Command::TrashChapter {
        id: a.chapters[0].id.clone(),
    })
    .unwrap();
    let t = s.start_context_task(b, "dependency").unwrap();
    assert_eq!(t.total, 1);
    s.apply(Command::PurgeChapter {
        id: a.chapters[0].id.clone(),
    })
    .unwrap();
    let result = s.step_next_task().unwrap().unwrap();
    assert_eq!(result.status, "interrupted");
    assert_eq!(result.reason_code.as_deref(), Some("input_changed"));
    assert_eq!(result.done, 0);
}
#[test]
fn active_and_historical_task_caps_never_delete_or_reuse_old_reports() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = &a.books[0].id;
    let mut ids = Vec::new();
    for n in 0..8 {
        ids.push(s.start_context_task(b, &format!("active-{n}")).unwrap().id);
    }
    assert_eq!(
        s.start_context_task(b, "active-over").unwrap_err().code,
        "limit_exceeded"
    );
    for id in ids {
        s.cancel_context_task(b, &id).unwrap();
    }
    for n in 8..100 {
        let t = s.start_context_task(b, &format!("history-{n}")).unwrap();
        s.cancel_context_task(b, &t.id).unwrap();
    }
    assert_eq!(s.list_context_tasks(b).unwrap().len(), 100);
    assert_eq!(
        s.start_context_task(b, "total-over").unwrap_err().code,
        "limit_exceeded"
    );
    assert_eq!(
        s.start_context_task(b, "active-0").unwrap().status,
        "cancelled"
    );
    assert_eq!(s.list_context_tasks(b).unwrap().len(), 100);
}
#[test]
fn unit_sql_failure_rolls_back_progress_results_and_canonical_data() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = &a.books[0].id;
    let t = s.start_context_task(b, "rollback").unwrap();
    let canonical = s.backup().unwrap();
    let observer = rusqlite::Connection::open(f.db()).unwrap();
    observer.execute_batch("CREATE TRIGGER fail_context_result BEFORE INSERT ON context_task_items BEGIN SELECT RAISE(ABORT,'fixture result failure'); END;").unwrap();
    assert!(s.step_next_task().is_err());
    let d = s.get_context_task(b, &t.id).unwrap();
    assert_eq!(d.task.status, "queued");
    assert_eq!(d.task.done, 0);
    assert!(d.items.is_empty());
    assert_eq!(s.backup().unwrap(), canonical);
    observer
        .execute_batch("DROP TRIGGER fail_context_result;")
        .unwrap();
    assert_eq!(s.step_next_task().unwrap().unwrap().status, "completed");
}
#[test]
fn managed_storage_limit_marks_failed_without_fake_progress_or_manuscript_changes() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = &a.books[0].id;
    let t = s.start_context_task(b, "space").unwrap();
    let before = s.backup().unwrap();
    let observer = rusqlite::Connection::open(f.db()).unwrap();
    observer
        .execute(
            "UPDATE context_tasks SET storage_bytes=?1 WHERE id=?2",
            rusqlite::params![64 * 1024 * 1024 - 16 * 1024, t.id],
        )
        .unwrap();
    let stopped = s.step_next_task().unwrap().unwrap();
    assert_eq!(stopped.status, "failed");
    assert_eq!(stopped.reason_code.as_deref(), Some("storage_limit"));
    assert_eq!(stopped.done, 0);
    assert!(s.get_context_task(b, &t.id).unwrap().items.is_empty());
    assert_eq!(s.backup().unwrap(), before);
}
#[test]
fn task_manifest_limits_items_and_encoded_metadata_before_inserting_job() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = &a.books[0].id;
    let mut observer = rusqlite::Connection::open(f.db()).unwrap();
    let tx = observer.transaction().unwrap();
    for n in 1..=5000 {
        tx.execute("INSERT INTO chapters(id,volume_id,title,body,revision,sort_order,deleted_at) VALUES(?1,?2,'fixture','',0,?3,NULL)",rusqlite::params![format!("extra-{n}"),a.volumes[0].id,n]).unwrap();
    }
    tx.commit().unwrap();
    assert_eq!(
        s.start_context_task(b, "too-many").unwrap_err().code,
        "limit_exceeded"
    );
    assert!(s.list_context_tasks(b).unwrap().is_empty());
    observer
        .execute("DELETE FROM chapters WHERE id LIKE 'extra-%'", [])
        .unwrap();
    let tx = observer.transaction().unwrap();
    let title = "字".repeat(1300);
    for n in 1..=1100 {
        tx.execute("INSERT INTO chapters(id,volume_id,title,body,revision,sort_order,deleted_at) VALUES(?1,?2,?3,'',0,?4,NULL)",rusqlite::params![format!("wide-{n}"),a.volumes[0].id,title,n]).unwrap();
    }
    tx.commit().unwrap();
    assert_eq!(
        s.start_context_task(b, "too-wide").unwrap_err().code,
        "limit_exceeded"
    );
    assert!(s.list_context_tasks(b).unwrap().is_empty());
}
#[test]
fn source_reports_are_hidden_after_book_recycle_and_snapshot_retains_durable_checkpoint() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = &a.books[0].id;
    s.create_material(note(&a)).unwrap();
    let t = s.start_context_task(b, "snapshot").unwrap();
    s.step_next_task().unwrap();
    let path = s.recovery_snapshot(f.0.join("recovery.sqlite")).unwrap();
    let recovered = Service::open(path).unwrap();
    let detail = recovered.get_context_task(b, &t.id).unwrap();
    assert_eq!(detail.task.status, "interrupted");
    assert_eq!(detail.task.done, 1);
    assert_eq!(detail.items.len(), 1);
    s.apply(Command::TrashBook { id: b.clone() }).unwrap();
    assert_eq!(s.get_context_task(b, &t.id).unwrap_err().code, "deleted");
    assert_eq!(s.list_context_tasks(b).unwrap_err().code, "deleted");
}
#[test]
fn cancelling_after_completion_returns_actual_completed_not_a_false_cancelled_state() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = &a.books[0].id;
    let t = s.start_context_task(b, "finish-race").unwrap();
    let completed = s.step_next_task().unwrap().unwrap();
    assert_eq!(completed.status, "completed");
    let returned = s.cancel_context_task(b, &t.id).unwrap();
    assert_eq!(returned, completed);
    assert_eq!(s.get_context_task(b, &t.id).unwrap().items.len(), 1);
}
#[test]
fn unresolved_foreshadow_is_actionable_without_claiming_a_source_revision_conflict() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = &a.books[0].id;
    let mut card = note(&a);
    card.kind = "foreshadow".into();
    card.fields = BTreeMap::from([
        ("setup".into(), "未来要铺设".into()),
        ("resolution".into(), "".into()),
    ]);
    card.source_chapter_id = None;
    card.source_revision = None;
    card.tracking_status = Some("planned".into());
    let m = s.create_material(card).unwrap();
    let t = s.start_context_task(b, "unresolved").unwrap();
    while s.step_next_task().unwrap().is_some() {}
    let detail = s.get_context_task(b, &t.id).unwrap();
    let row = detail.items.iter().find(|x| x.item_id == m.id).unwrap();
    assert!(row.needs_review);
    assert_eq!(row.source_status.as_deref(), Some("none"));
    assert_eq!(row.tracking_status.as_deref(), Some("planned"));
}
#[test]
fn step_reads_only_current_plan_and_plan_bytes_are_accounted() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = &a.books[0].id;
    s.create_material(note(&a)).unwrap();
    let t = s.start_context_task(b, "plans").unwrap();
    let observer = rusqlite::Connection::open(f.db()).unwrap();
    let (stored,base):(i64,i64)=observer.query_row("SELECT storage_bytes,length(CAST(record AS BLOB))+length(CAST(manifest AS BLOB)) FROM context_tasks WHERE id=?1",[&t.id],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    let plans: i64 = observer
        .query_row(
            "SELECT sum(length(CAST(record AS BLOB))) FROM context_task_plan WHERE task_id=?1",
            [&t.id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(stored >= base + plans);
    let encoded: String = observer
        .query_row(
            "SELECT group_concat(record,'') FROM context_task_plan WHERE task_id=?1",
            [&t.id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!encoded.contains("SECRET_MATERIAL_BODY_NOT_IN_REPORT"));
    observer.execute("UPDATE context_task_plan SET record='fixture invalid metadata' WHERE task_id=?1 AND ordinal=1",[&t.id]).unwrap();
    assert_eq!(s.step_next_task().unwrap().unwrap().done, 1);
    assert!(s.step_next_task().is_err());
    assert_eq!(s.get_context_task(b, &t.id).unwrap().task.done, 1);
}
#[test]
fn maximum_chapter_step_counts_real_text_and_reports_elapsed_time() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = &a.books[0].id;
    s.apply(Command::SaveChapter {
        id: a.chapters[0].id.clone(),
        body: "x".repeat(10 * 1024 * 1024),
        expected_revision: 0,
    })
    .unwrap();
    let t = s.start_context_task(b, "maximum-body").unwrap();
    let before = std::time::Instant::now();
    let result = s.step_next_task().unwrap().unwrap();
    eprintln!(
        "isolated 10MiB chapter step: {:?} (debug Linux, no latency guarantee)",
        before.elapsed()
    );
    assert_eq!(result.status, "completed");
    let detail = s.get_context_task(b, &t.id).unwrap();
    assert_eq!(detail.items[0].char_count, Some(10 * 1024 * 1024));
    assert_eq!(s.get_chapter(&a.chapters[0].id).unwrap().revision, 1);
}
#[test]
fn final_check_rejects_missing_earlier_result_without_scanning_on_every_step() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = &a.books[0].id;
    for title in ["第二章", "第三章"] {
        s.apply(Command::CreateChapter {
            volume_id: a.volumes[0].id.clone(),
            title: title.into(),
        })
        .unwrap();
    }
    let t = s.start_context_task(b, "missing-result").unwrap();
    assert_eq!(s.step_next_task().unwrap().unwrap().done, 1);
    assert_eq!(s.step_next_task().unwrap().unwrap().done, 2);
    let observer = rusqlite::Connection::open(f.db()).unwrap();
    observer
        .execute(
            "DELETE FROM context_task_items WHERE task_id=?1 AND ordinal=0",
            [&t.id],
        )
        .unwrap();
    assert!(s.step_next_task().is_err());
    assert_ne!(s.list_context_tasks(b).unwrap()[0].status, "completed");
}
