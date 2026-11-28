use novel_core::{workflow::ChapterVersion, BackupJobManager, Command, Service};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
fn setup() -> (PathBuf, Arc<Mutex<Service>>, BackupJobManager) {
    let dir = std::env::temp_dir().join(format!("backup-jobs-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let mut s = Service::open(dir.join("live.db")).unwrap();
    s.apply(Command::CreateBook {
        title: "backup fixture".into(),
        author: "".into(),
    })
    .unwrap();
    let store = Arc::new(Mutex::new(s));
    let manager = BackupJobManager::new(store.clone(), dir.join("recovery"));
    (dir, store, manager)
}
fn wait(manager: &BackupJobManager, id: &str, status: &[&str]) -> novel_core::BackupJob {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let job = manager.status(id).unwrap();
        if status.contains(&job.status.as_str()) {
            return job;
        }
        assert!(Instant::now() < end, "job stalled: {}", job.status);
        thread::sleep(Duration::from_millis(2));
    }
}
fn save_original(store: &Mutex<Service>) -> (String, Vec<ChapterVersion>) {
    let mut service = store.lock().unwrap();
    let id = service.load().unwrap().chapters[0].id.clone();
    service
        .apply(Command::SaveChapter {
            id: id.clone(),
            body: "原稿：渡船在雨中靠岸。".into(),
            expected_revision: 0,
        })
        .unwrap();
    let history = service.chapter_history(&id).unwrap();
    (id, history)
}

fn assert_can_save_and_reopen(
    dir: &Path,
    store: Arc<Mutex<Service>>,
    manager: BackupJobManager,
    id: &str,
    original_history: &[ChapterVersion],
) {
    assert!(!manager.has_pending());
    let history = {
        let mut service = store.lock().unwrap();
        let before = service.get_chapter(id).unwrap();
        assert_eq!(before.revision, 1);
        assert_eq!(before.body.as_bytes(), "原稿：渡船在雨中靠岸。".as_bytes());
        assert_eq!(service.chapter_history(id).unwrap(), original_history);
        let stale = service
            .apply(Command::SaveChapter {
                id: id.into(),
                body: "过期稿不得写入".into(),
                expected_revision: 0,
            })
            .unwrap_err();
        assert!(stale.starts_with("revision_conflict:"));
        assert_eq!(service.get_chapter(id).unwrap(), before);
        assert_eq!(service.chapter_history(id).unwrap(), original_history);

        service
            .apply(Command::SaveChapter {
                id: id.into(),
                body: "新稿：渡船在雨中靠岸，远处亮起一盏灯。".into(),
                expected_revision: 1,
            })
            .unwrap();
        let history = service.chapter_history(id).unwrap();
        assert_eq!(history.len(), 3);
        assert_eq!(&history[..2], original_history);
        assert_eq!(
            history
                .iter()
                .map(|v| (v.revision, v.body.as_str(), v.source.as_str()))
                .collect::<Vec<_>>(),
            [
                (0, "", "baseline"),
                (1, "原稿：渡船在雨中靠岸。", "gui"),
                (2, "新稿：渡船在雨中靠岸，远处亮起一盏灯。", "gui"),
            ]
        );
        history
    };
    manager.shutdown();
    drop(manager);
    drop(store);
    let reopened = Service::open(dir.join("live.db")).unwrap();
    let saved = reopened.get_chapter(id).unwrap();
    assert_eq!(saved.revision, 2);
    assert_eq!(
        saved.body.as_bytes(),
        "新稿：渡船在雨中靠岸，远处亮起一盏灯。".as_bytes()
    );
    assert_eq!(reopened.chapter_history(id).unwrap(), history);
}

#[test]
fn export_prepare_confirm_returns_metadata_without_library_bodies() {
    let (dir, store, manager) = setup();
    let path = dir.join("backup.sqlite");
    let job = manager.start_export(path.clone()).unwrap();
    let done = wait(&manager, &job.id, &["done", "failed"]);
    assert_eq!(done.status, "done");
    assert!(path.exists());
    let prep = manager.start_prepare_restore(path).unwrap();
    let ready = wait(&manager, &prep.id, &["prepared", "failed"]);
    assert_eq!(ready.status, "prepared");
    assert!(ready.preview.as_ref().unwrap().books > 0);
    assert!(manager.has_pending());
    assert!(manager.confirm_restore(&ready.id, "wrong-token").is_err());
    let token = ready.confirmation_token.unwrap();
    manager.confirm_restore(&ready.id, &token).unwrap();
    assert!(manager.confirm_restore(&ready.id, &token).is_err());
    let done = wait(&manager, &ready.id, &["done", "failed"]);
    assert_eq!(done.status, "done");
    assert!(done.result.unwrap().recovery_path.is_some());
    assert!(!manager.has_pending());
    manager.shutdown();
    drop(manager);
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn status_cancel_and_shutdown_do_not_wait_for_service_lock() {
    let (dir, store, manager) = setup();
    assert!(manager.current().unwrap().is_none());
    let guard = store.lock().unwrap();
    let first = manager.start_export(dir.join("cancel.sqlite")).unwrap();
    assert!(manager.start_export(dir.join("extra.sqlite")).is_err());
    assert_eq!(manager.status(&first.id).unwrap().id, first.id);
    assert!(manager.cancel(&first.id).unwrap().cancel_requested);
    manager.shutdown();
    assert_eq!(manager.status(&first.id).unwrap().status, "cancelled");
    drop(guard);
    drop(manager);
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn prepared_guard_refuses_changes_and_cancel_invalidates_token() {
    let (dir, store, manager) = setup();
    let export = manager.start_export(dir.join("fixture.sqlite")).unwrap();
    assert_eq!(
        wait(&manager, &export.id, &["done", "failed"]).status,
        "done"
    );
    let prep = manager
        .start_prepare_restore(dir.join("fixture.sqlite"))
        .unwrap();
    let ready = wait(&manager, &prep.id, &["prepared", "failed"]);
    let token = ready.confirmation_token.unwrap();
    store
        .lock()
        .unwrap()
        .apply(Command::CreateBook {
            title: "new after preview".into(),
            author: "".into(),
        })
        .unwrap();
    manager.confirm_restore(&ready.id, &token).unwrap();
    assert_eq!(
        wait(&manager, &ready.id, &["failed", "done"]).status,
        "failed"
    );
    let again = manager
        .start_prepare_restore(dir.join("fixture.sqlite"))
        .unwrap();
    let ready = wait(&manager, &again.id, &["prepared", "failed"]);
    let token = ready.confirmation_token.unwrap();
    assert_eq!(manager.cancel(&ready.id).unwrap().status, "cancelled");
    assert!(manager.confirm_restore(&ready.id, &token).is_err());
    assert!(!manager.has_pending());
    manager.shutdown();
    drop(manager);
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn stale_restore_preview_fails_without_preventing_save_and_reopen() {
    let (dir, store, manager) = setup();
    let (id, original_history) = save_original(&store);
    let path = dir.join("source.sqlite");
    let export = manager.start_export(path.clone()).unwrap();
    assert_eq!(wait(&manager, &export.id, &["done", "failed"]).status, "done");
    let prep = manager.start_prepare_restore(path).unwrap();
    let ready = wait(&manager, &prep.id, &["prepared", "failed"]);
    assert_eq!(ready.status, "prepared");
    store
        .lock()
        .unwrap()
        .apply(Command::CreateBook {
            title: "Change after preview".into(),
            author: "".into(),
        })
        .unwrap();
    manager
        .confirm_restore(&ready.id, ready.confirmation_token.as_deref().unwrap())
        .unwrap();
    let failed = wait(&manager, &ready.id, &["failed", "done"]);
    assert_eq!(failed.status, "failed");
    assert_eq!(failed.error_code.as_deref(), Some("restore_conflict"));
    assert_eq!(store.lock().unwrap().load().unwrap().books.len(), 2);
    assert!(!dir.join("recovery").exists());
    assert_can_save_and_reopen(&dir, store, manager, &id, &original_history);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cancel_confirmed_job_before_service_acquisition_keeps_live_data() {
    let (dir, store, manager) = setup();
    let (id, original_history) = save_original(&store);
    let export = manager.start_export(dir.join("source.sqlite")).unwrap();
    assert_eq!(
        wait(&manager, &export.id, &["done", "failed"]).status,
        "done"
    );
    let prep = manager
        .start_prepare_restore(dir.join("source.sqlite"))
        .unwrap();
    let ready = wait(&manager, &prep.id, &["prepared", "failed"]);
    let guard = store.lock().unwrap();
    manager
        .confirm_restore(&ready.id, ready.confirmation_token.as_deref().unwrap())
        .unwrap();
    let requested = manager.cancel(&ready.id).unwrap();
    assert!(requested.cancel_requested);
    assert_eq!(requested.status, "running");
    assert_eq!(
        wait(&manager, &ready.id, &["cancelled", "failed", "done"]).status,
        "cancelled"
    );
    drop(guard);
    assert!(!dir.join("recovery").exists());
    assert_can_save_and_reopen(&dir, store, manager, &id, &original_history);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn invalid_prepare_is_safe_failure_and_does_not_hold_slot() {
    let (dir, store, manager) = setup();
    std::fs::write(
        dir.join("bad.sqlite"),
        b"not a valid backup: synthetic fixture",
    )
    .unwrap();
    let failed = manager
        .start_prepare_restore(dir.join("bad.sqlite"))
        .unwrap();
    let failed = wait(&manager, &failed.id, &["failed", "prepared"]);
    assert_eq!(failed.status, "failed");
    assert!(failed.confirmation_token.is_none());
    assert!(!failed.error_message.unwrap().contains("synthetic"));
    assert!(!manager.has_pending());
    let next = manager.start_export(dir.join("good.sqlite")).unwrap();
    assert!(manager.status(&failed.id).is_err());
    assert!(manager.cancel(&failed.id).is_err());
    assert_eq!(wait(&manager, &next.id, &["done", "failed"]).status, "done");
    manager.shutdown();
    drop(manager);
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cancelled_import_does_not_leave_live_connection_interrupt_hook() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let dir = std::env::temp_dir().join(format!("cancel-hook-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let mut s = Service::open(":memory:").unwrap();
    let state = s
        .apply(Command::CreateBook {
            title: "hook isolation fixture".into(),
            author: "".into(),
        })
        .unwrap();
    for i in 0..240 {
        s.apply(Command::CreateChapter {
            volume_id: state.volumes[0].id.clone(),
            title: format!("fixture {i}"),
        })
        .unwrap();
    }
    let path = dir.join("input.sqlite");
    s.export_sqlite_backup(&path, |_| Ok(())).unwrap();
    let prepared = novel_core::prepare_restore(&path, |_| Ok(())).unwrap();
    let expected = s.restore_guard().unwrap();
    let flag = Arc::new(AtomicBool::new(false));
    let callback_flag = flag.clone();
    let result = s.commit_prepared_restore_with_cancel(
        prepared,
        &expected,
        &dir.join("before.sqlite"),
        flag.clone(),
        move |p| {
            if p.phase == "import" && p.completed >= 10 {
                callback_flag.store(true, Ordering::SeqCst);
                return Err("backup_job_cancelled".into());
            }
            Ok(())
        },
    );
    assert_eq!(result.unwrap_err(), "backup_job_cancelled");
    assert!(flag.load(Ordering::SeqCst));
    s.apply(Command::SaveChapter {
        id: state.chapters[0].id.clone(),
        body: "saved after cancelled import".into(),
        expected_revision: 0,
    })
    .unwrap();
    let backup = s.backup().unwrap();
    assert!(backup.contains("saved after cancelled import"));
    assert_eq!(s.get_chapter(&state.chapters[0].id).unwrap().revision, 1);
    drop(s);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn backup_manager_methods_are_not_external_agent_tools() {
    use novel_core::{agent::AgentBroker, agent_stdio};
    use serde_json::{json, Value};
    let (dir, store, manager) = setup();
    let book = store.lock().unwrap().load().unwrap().books[0].id.clone();
    let broker = Arc::new(AgentBroker::new(store.clone()));
    let c = broker.connect().unwrap();
    c.call(json!({"method":"request_access","params":{"client_name":"backup boundary fixture"}}));
    broker.grant_session(c.id(), &book).unwrap();
    let names = [
        "start_backup_export",
        "start_backup_prepare",
        "confirm_backup_restore",
        "current_backup_job",
        "backup_job_status",
        "cancel_backup_job",
        "startBackupExport",
        "startBackupPrepare",
        "confirmBackupRestore",
        "currentBackupJob",
        "backupJobStatus",
        "cancelBackupJob",
    ];
    let mut lines = vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
    ];
    for (i, name) in names.iter().enumerate() {
        assert_eq!(
            c.call(json!({"method":name,"params":{}}))["error"]["code"],
            "method_not_found"
        );
        lines.push(json!({"jsonrpc":"2.0","id":i+3,"method":"tools/call","params":{"name":name,"arguments":{}}}));
    }
    let input = lines
        .into_iter()
        .map(|v| format!("{v}\n"))
        .collect::<String>();
    let mut output = Vec::new();
    agent_stdio::run(std::io::Cursor::new(input), &mut output, true, |_| {
        panic!("backup call crossed transport boundary")
    })
    .unwrap();
    let responses = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str::<Value>(s).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        responses[1]["result"]["tools"].as_array().unwrap().len(),
        26
    );
    for response in &responses[2..] {
        assert_eq!(response["error"]["code"], -32602);
    }
    assert!(manager.current().unwrap().is_none());
    manager.shutdown();
    drop(c);
    drop(broker);
    drop(manager);
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn prepare_reports_size_and_wal_failures_without_raw_paths() {
    let (dir, store, manager) = setup();
    let huge = dir.join("private-name-large.sqlite");
    let file = std::fs::File::create(&huge).unwrap();
    file.set_len(513 * 1024 * 1024).unwrap();
    drop(file);
    let job = manager.start_prepare_restore(huge).unwrap();
    let failed = wait(&manager, &job.id, &["failed", "prepared"]);
    assert_eq!(failed.error_code.as_deref(), Some("limit_exceeded"));
    assert!(failed.error_message.as_deref().unwrap().contains("512MiB"));
    assert!(!failed.error_message.unwrap().contains("private-name"));
    let path = dir.join("private-name-wal.sqlite");
    std::fs::write(&path, b"fixture").unwrap();
    std::fs::write(dir.join("private-name-wal.sqlite-wal"), b"nonempty fixture").unwrap();
    let job = manager.start_prepare_restore(path).unwrap();
    let failed = wait(&manager, &job.id, &["failed", "prepared"]);
    assert_eq!(failed.error_code.as_deref(), Some("not_standalone"));
    assert!(!failed.error_message.unwrap().contains("private-name"));
    manager.shutdown();
    drop(manager);
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]fn protected_writing_policy_requires_manager_ack_and_archives_on_restore(){let(dir,store,manager)=setup();let(b,c)={let mut s=store.lock().unwrap();let state=s.load().unwrap();let b=state.books[0].id.clone();let c=state.chapters[0].id.clone();let m=s.create_material(serde_json::from_value(serde_json::json!({"book_id":b,"kind":"setting","title":"规则","fields":{"synopsis":"船"}})).unwrap()).unwrap();let e=s.epoch().unwrap();s.configure_writing_policy(&b,&c,None,"active","过河",vec![m.id],&e).unwrap();s.prepare_writing_pack(&b,&c,0,0,&e).unwrap();(b,c)};let path=dir.join("writing.sqlite");let j=manager.start_export(path.clone()).unwrap();assert_eq!(wait(&manager,&j.id,&["done","failed"]).status,"done");let j=manager.start_prepare_restore(path.clone()).unwrap();let ready=wait(&manager,&j.id,&["prepared","failed"]);manager.confirm_restore(&ready.id,ready.confirmation_token.as_ref().unwrap()).unwrap();assert_eq!(wait(&manager,&j.id,&["done","failed"]).status,"failed");assert_eq!(store.lock().unwrap().get_writing_state(&b,&c).unwrap().policy.unwrap().mode,"active");let j=manager.start_prepare_restore(path).unwrap();let ready=wait(&manager,&j.id,&["prepared","failed"]);manager.confirm_restore_with_writing_ack(&ready.id,ready.confirmation_token.as_ref().unwrap(),true).unwrap();assert_eq!(wait(&manager,&j.id,&["done","failed"]).status,"done");assert_eq!(store.lock().unwrap().get_writing_state(&b,&c).unwrap().policy.unwrap().mode,"paused");manager.shutdown();drop(manager);drop(store);std::fs::remove_dir_all(dir).unwrap();}
