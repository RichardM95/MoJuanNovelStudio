use novel_core::*;
use std::path::PathBuf;
fn dir() -> PathBuf {
    let p = std::env::temp_dir().join(format!("restore-fixture-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&p).unwrap();
    p
}
fn fixture() -> Service {
    let mut s = Service::open(":memory:").unwrap();
    s.apply(Command::CreateBook {
        title: "虚构作品".into(),
        author: "测试".into(),
    })
    .unwrap();
    s
}
#[test]
fn sqlite_prepared_is_frozen_and_commit_keeps_writer() {
    let d = dir();
    let source = fixture();
    let p = d.join("source.sqlite");
    source.export_sqlite_backup(&p, |_| Ok(())).unwrap();
    let prepared = prepare_restore(&p, |_| Ok(())).unwrap();
    assert_eq!(prepared.summary().books, 1);
    std::fs::remove_file(p).unwrap();
    let mut dest = Service::open(d.join("live.sqlite")).unwrap();
    let guard = dest.restore_guard().unwrap();
    let result = dest
        .commit_prepared_restore(prepared, &guard, &d.join("before.sqlite"), |_| Ok(()))
        .unwrap();
    assert_eq!(result.summary.books, 1);
    assert_eq!(dest.load().unwrap().books[0].title, "虚构作品");
    assert!(Service::open(d.join("live.sqlite")).is_err());
    drop(dest);
    std::fs::remove_dir_all(d).unwrap();
}
#[test]
fn guard_and_cancel_leave_original() {
    let d = dir();
    let source = fixture();
    let p = d.join("source.sqlite");
    source.recovery_snapshot(&p).unwrap();
    let mut dest = fixture();
    let guard = dest.restore_guard().unwrap();
    dest.apply(Command::CreateBook {
        title: "后来".into(),
        author: "".into(),
    })
    .unwrap();
    let before = dest.backup().unwrap();
    assert!(dest
        .commit_prepared_restore(
            prepare_restore(&p, |_| Ok(())).unwrap(),
            &guard,
            &d.join("before.sqlite"),
            |_| Ok(())
        )
        .is_err());
    assert_eq!(before, dest.backup().unwrap());
    let guard = dest.restore_guard().unwrap();
    assert!(dest
        .commit_prepared_restore(
            prepare_restore(&p, |_| Ok(())).unwrap(),
            &guard,
            &d.join("cancel.sqlite"),
            |p| if p.phase == "import" {
                Err("cancelled".into())
            } else {
                Ok(())
            }
        )
        .is_err());
    assert_eq!(before, dest.backup().unwrap());
    std::fs::remove_dir_all(d).unwrap();
}
#[test]
fn rejects_trigger_and_nonempty_wal() {
    let d = dir();
    let p = d.join("source.sqlite");
    fixture().recovery_snapshot(&p).unwrap();
    let c = rusqlite::Connection::open(&p).unwrap();
    c.execute_batch(
        "CREATE TRIGGER malicious AFTER INSERT ON books BEGIN DELETE FROM chapters; END;",
    )
    .unwrap();
    drop(c);
    assert!(prepare_restore(&p, |_| Ok(())).is_err());
    std::fs::write(p.with_extension("sqlite-wal"), b"nonempty").unwrap();
    assert!(prepare_restore(&p, |_| Ok(())).is_err());
    std::fs::remove_dir_all(d).unwrap();
}
#[test]
fn over_json_limit_roundtrip_preserves_history_and_labels() {
    let d = dir();
    let p = d.join("large.sqlite");
    let s = fixture();
    s.recovery_snapshot(&p).unwrap();
    let c = rusqlite::Connection::open(&p).unwrap();
    let chapter: String = c
        .query_row("SELECT id FROM chapters", [], |r| r.get(0))
        .unwrap();
    let body = "虚".repeat(8 * 1024 * 1024 / 3);
    c.execute(
        "UPDATE chapters SET body=?1,revision=9 WHERE id=?2",
        rusqlite::params![body, chapter],
    )
    .unwrap();
    for n in 1..=9 {
        c.execute(
            "INSERT INTO chapter_versions VALUES(?1,?2,?3,?4,'human',1)",
            rusqlite::params![format!("h{n}"), chapter, n, body],
        )
        .unwrap();
    }
    c.execute(
        "INSERT INTO chapter_version_labels VALUES('h1','关键版本',1,1)",
        [],
    )
    .unwrap();
    drop(c);
    assert!(std::fs::metadata(&p).unwrap().len() > 64 * 1024 * 1024);
    let prepared = prepare_restore(&p, |_| Ok(())).unwrap();
    assert_eq!(prepared.summary().history, 9);
    let mut dest = fixture();
    let guard = dest.restore_guard().unwrap();
    dest.commit_prepared_restore(prepared, &guard, &d.join("before.sqlite"), |_| Ok(()))
        .unwrap();
    assert_eq!(dest.chapter_history(&chapter).unwrap().len(), 9);
    let book = dest.load().unwrap().books[0].id.clone();
    let page = dest
        .list_chapter_versions(&book, &chapter, None, 50, true)
        .unwrap();
    assert_eq!(page.items[0].label.as_deref(), Some("关键版本"));
    assert!(dest.backup().is_err());
    drop(dest);
    std::fs::remove_dir_all(d).unwrap();
}
#[test]
fn snapshot_failure_and_mid_import_rollback() {
    let d = dir();
    let p = d.join("source.sqlite");
    fixture().recovery_snapshot(&p).unwrap();
    let mut dest = fixture();
    let before = dest.backup().unwrap();
    let guard = dest.restore_guard().unwrap();
    std::fs::write(d.join("exists.sqlite"), b"keep").unwrap();
    assert!(dest
        .commit_prepared_restore(
            prepare_restore(&p, |_| Ok(())).unwrap(),
            &guard,
            &d.join("exists.sqlite"),
            |_| Ok(())
        )
        .is_err());
    assert_eq!(before, dest.backup().unwrap());
    let guard = dest.restore_guard().unwrap();
    assert!(dest
        .commit_prepared_restore(
            prepare_restore(&p, |_| Ok(())).unwrap(),
            &guard,
            &d.join("before.sqlite"),
            |p| if p.phase == "import" && p.completed >= 2 {
                Err("simulated io/cancel".into())
            } else {
                Ok(())
            }
        )
        .is_err());
    assert_eq!(before, dest.backup().unwrap());
    let snapshot = Service::open(d.join("before.sqlite")).unwrap();
    assert_eq!(snapshot.backup().unwrap(), before);
    drop(snapshot);
    std::fs::remove_dir_all(d).unwrap();
}
#[test]
fn bounded_input_and_json_compatibility() {
    let d = dir();
    let p = d.join("too-big.sqlite");
    let f = std::fs::File::create(&p).unwrap();
    f.set_len(512 * 1024 * 1024 + 1).unwrap();
    assert!(prepare_restore(&p, |_| Ok(())).is_err());
    drop(f);
    let json = d.join("old.json");
    let s = fixture();
    std::fs::write(&json, s.backup().unwrap()).unwrap();
    let p = prepare_restore(&json, |_| Ok(())).unwrap();
    assert_eq!(p.summary().format, "json");
    assert_eq!(p.summary().chapters, 1);
    drop(p);
    assert!(prepare_restore(&json, |_| Err("cancelled".into())).is_err());
    std::fs::remove_dir_all(d).unwrap();
}
#[test]
fn cancelled_control_and_malformed_fields_rejected() {
    use std::sync::{atomic::AtomicBool, Arc};
    let d = dir();
    let p = d.join("source.sqlite");
    fixture().recovery_snapshot(&p).unwrap();
    assert!(prepare_restore_with_cancel(&p, Arc::new(AtomicBool::new(true)), |_| Ok(())).is_err());
    let c = rusqlite::Connection::open(&p).unwrap();
    c.execute_batch("ALTER TABLE books ADD COLUMN foreign_column TEXT")
        .unwrap();
    drop(c);
    assert!(prepare_restore(&p, |_| Ok(())).is_err());
    std::fs::remove_dir_all(d).unwrap();
}
#[test]
fn restore_preserves_materials_clears_runtime_and_rotates_epoch() {
    let d = dir();
    let mut source = fixture();
    let s = source.load().unwrap();
    let chapter = &s.chapters[0];
    let book = &s.books[0].id;
    let m=source.create_material(serde_json::from_value(serde_json::json!({"book_id":book,"kind":"summary","title":"摘要","fields":{"content":"虚构材料正文"},"volume_id":null,"chapter_id":null,"source_chapter_id":chapter.id,"source_revision":0})).unwrap()).unwrap();
    source.start_context_task(book, "fixture-task").unwrap();
    source
        .propose_edit(ProposalInput {
            client_id: "fixture-agent".into(),
            idempotency_key: "fixture-proposal".into(),
            chapter_id: chapter.id.clone(),
            base_revision: 0,
            new_body: "未采纳虚构稿".into(),
            reason: "测试".into(),
        })
        .unwrap();
    let p = d.join("source.sqlite");
    source.recovery_snapshot(&p).unwrap();
    let mut dest = fixture();
    let guard = dest.restore_guard().unwrap();
    dest.commit_prepared_restore(
        prepare_restore(&p, |_| Ok(())).unwrap(),
        &guard,
        &d.join("before.sqlite"),
        |_| Ok(()),
    )
    .unwrap();
    assert_ne!(dest.epoch().unwrap(), guard.epoch);
    assert_eq!(dest.get_material(&m.id, false).unwrap(), m);
    assert!(dest.list_context_tasks(book).unwrap().is_empty());
    assert!(dest.list_proposals(book).unwrap().is_empty());
    assert_eq!(dest.audit_log().unwrap().len(), 1);
    let stale = dest.restore_guard().unwrap();
    let text = dest.backup().unwrap();
    dest.restore(&text).unwrap();
    assert!(dest
        .commit_prepared_restore(
            prepare_restore(&p, |_| Ok(())).unwrap(),
            &stale,
            &d.join("stale.sqlite"),
            |_| Ok(())
        )
        .is_err());
    drop(source);
    drop(dest);
    std::fs::remove_dir_all(d).unwrap();
}
#[test]
fn cancellation_after_copy_before_publish_does_not_publish() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let d = dir();
    let s = fixture();
    let flag = Arc::new(AtomicBool::new(false));
    let inner = flag.clone();
    let mut finished_callbacks = 0;
    let error = s
        .export_sqlite_backup_with_cancel(&d.join("cancel.sqlite"), flag, move |p| {
            if p.completed == p.total.unwrap_or(u64::MAX) {
                finished_callbacks += 1;
                if finished_callbacks == 2 {
                    inner.store(true, Ordering::Relaxed);
                }
            }
            Ok(())
        })
        .unwrap_err();
    assert!(error.contains("cancelled"));
    assert!(!d.join("cancel.sqlite").exists());
    assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0);
    std::fs::remove_dir_all(d).unwrap();
}
#[test]
fn cancelled_import_clears_live_hook_and_normal_save_still_works() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let d = dir();
    let source = fixture();
    let p = d.join("source.sqlite");
    source.recovery_snapshot(&p).unwrap();
    let mut dest = fixture();
    let before = dest.backup().unwrap();
    let guard = dest.restore_guard().unwrap();
    let flag = Arc::new(AtomicBool::new(false));
    let inner = flag.clone();
    let error = dest
        .commit_prepared_restore_with_cancel(
            prepare_restore(&p, |_| Ok(())).unwrap(),
            &guard,
            &d.join("before.sqlite"),
            flag,
            move |p| {
                if p.phase == "import" {
                    inner.store(true, Ordering::Relaxed);
                }
                Ok(())
            },
        )
        .unwrap_err();
    assert_eq!(error, "backup_job_cancelled");
    assert_eq!(dest.backup().unwrap(), before);
    dest.apply(Command::CreateBook {
        title: "取消后仍可保存".into(),
        author: "".into(),
    })
    .unwrap();
    std::fs::remove_dir_all(d).unwrap();
}
