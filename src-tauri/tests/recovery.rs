use novel_core::*;
use std::path::PathBuf;
struct Fixture {
    dir: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let dir =
            std::env::temp_dir().join(format!("mojuan-recovery-fixture-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        Self { dir }
    }
    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
fn library(f: &Fixture) -> (Service, String, String) {
    let mut s = Service::open(f.path("source.sqlite")).unwrap();
    let a = s
        .apply(Command::CreateBook {
            title: "快照虚构作品".into(),
            author: "隔离测试".into(),
        })
        .unwrap();
    let c = a.chapters[0].id.clone();
    s.apply(Command::SaveChapter {
        id: c.clone(),
        body: "已保存稿".into(),
        expected_revision: 0,
    })
    .unwrap();
    (s, a.books[0].id.clone(), c)
}
#[test]
fn verified_snapshot_reopens_all_state_history_proposals_epoch_and_audit() {
    let f = Fixture::new();
    let (mut s, b, c) = library(&f);
    s.propose_edit(ProposalInput {
        client_id: "fixture".into(),
        idempotency_key: "snapshot".into(),
        chapter_id: c.clone(),
        base_revision: 1,
        new_body: "待审核稿".into(),
        reason: "仅测试".into(),
    })
    .unwrap();
    let expected = s.load().unwrap();
    let history = s.chapter_history(&c).unwrap();
    let proposals = s.list_proposals(&b).unwrap();
    let audit = s.audit_log().unwrap();
    let epoch = s.epoch().unwrap();
    let path = s.recovery_snapshot(f.path("recovery.sqlite")).unwrap();
    let recovered = Service::open(&path).unwrap();
    assert_eq!(recovered.load().unwrap(), expected);
    assert_eq!(recovered.chapter_history(&c).unwrap(), history);
    assert_eq!(recovered.list_proposals(&b).unwrap(), proposals);
    assert_eq!(recovered.audit_log().unwrap(), audit);
    assert_eq!(recovered.epoch().unwrap(), epoch);
    drop(recovered);
    s.apply(Command::SaveChapter {
        id: c.clone(),
        body: "快照后新稿".into(),
        expected_revision: 1,
    })
    .unwrap();
    let recovered = Service::open(&path).unwrap();
    assert_eq!(recovered.get_chapter(&c).unwrap().body, "已保存稿");
}
#[test]
fn preexisting_target_is_never_overwritten() {
    let f = Fixture::new();
    let (s, _, _) = library(&f);
    let target = f.path("existing.sqlite");
    std::fs::write(&target, b"existing target fixture").unwrap();
    assert!(s.recovery_snapshot(&target).is_err());
    assert_eq!(std::fs::read(target).unwrap(), b"existing target fixture");
}
#[test]
fn interrupted_copy_cleans_only_its_temporary_file_and_keeps_source() {
    let f = Fixture::new();
    let (mut s, _, c) = library(&f);
    s.apply(Command::SaveChapter {
        id: c.clone(),
        body: "x".repeat(1024 * 1024),
        expected_revision: 1,
    })
    .unwrap();
    let expected = s.load().unwrap();
    let history = s.chapter_history(&c).unwrap();
    let sentinel = f.path("unrelated.txt");
    std::fs::write(&sentinel, b"keep fixture").unwrap();
    let before: Vec<_> = std::fs::read_dir(&f.dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    let mut called = false;
    let result = s.recovery_snapshot_with_progress(f.path("cancelled.sqlite"), |progress| {
        called = true;
        assert!(progress.remaining_pages > 0);
        Err("fixture interrupted".into())
    });
    assert!(called && result.is_err());
    assert!(!f.path("cancelled.sqlite").exists());
    let after: Vec<_> = std::fs::read_dir(&f.dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(before.len(), after.len());
    assert_eq!(std::fs::read(sentinel).unwrap(), b"keep fixture");
    assert_eq!(s.load().unwrap(), expected);
    assert_eq!(s.chapter_history(&c).unwrap(), history);
}
#[test]
fn injected_enospc_error_leaves_no_successful_snapshot_or_source_change() {
    let f = Fixture::new();
    let (s, _, c) = library(&f);
    let before = s.get_chapter(&c).unwrap();
    let error = s
        .recovery_snapshot_with_progress(f.path("diskfull.sqlite"), |_| {
            Err(std::io::Error::from_raw_os_error(28).to_string())
        })
        .unwrap_err();
    assert!(!error.is_empty());
    assert!(!f.path("diskfull.sqlite").exists());
    assert_eq!(s.get_chapter(&c).unwrap(), before);
    assert!(!std::fs::read_dir(&f.dir).unwrap().any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .contains(".recovery-")));
}
#[test]
fn invalid_snapshot_schema_is_rejected_before_publication() {
    let f = Fixture::new();
    let (s, _, _) = library(&f);
    let observer = rusqlite::Connection::open(f.path("source.sqlite")).unwrap();
    observer
        .execute_batch("ALTER TABLE chapter_versions RENAME COLUMN body TO fixture_broken_body;")
        .unwrap();
    assert!(s.recovery_snapshot(f.path("invalid.sqlite")).is_err());
    assert!(!f.path("invalid.sqlite").exists());
}
#[test]
fn sqlite_snapshot_retains_history_larger_than_json_export_limit() {
    let f = Fixture::new();
    let (s, _, c) = library(&f);
    let mut conn = rusqlite::Connection::open(f.path("source.sqlite")).unwrap();
    let body = "h".repeat(8 * 1024 * 1024);
    let tx = conn.transaction().unwrap();
    tx.execute("DELETE FROM chapter_versions", []).unwrap();
    tx.execute(
        "UPDATE chapters SET body=?1,revision=8 WHERE id=?2",
        rusqlite::params![body, c],
    )
    .unwrap();
    for revision in 0..9 {
        tx.execute(
            "INSERT INTO chapter_versions VALUES(?1,?2,?3,?4,'fixture',1)",
            rusqlite::params![format!("history-fixture-{revision}"), c, revision, body],
        )
        .unwrap();
    }
    tx.commit().unwrap();
    drop(conn);
    assert!(s.backup().err().is_some_and(|e| e.contains("SQLite")));
    let before = s.get_chapter(&c).unwrap();
    let mut steps = 0;
    let path = s
        .recovery_snapshot_with_progress(f.path("large-recovery.sqlite"), |_| {
            steps += 1;
            Ok(())
        })
        .unwrap();
    assert!(steps > 1);
    let recovered = Service::open(path).unwrap();
    assert_eq!(recovered.get_chapter(&c).unwrap(), before);
    let history = recovered.chapter_history(&c).unwrap();
    assert_eq!(history.len(), 9);
    for (revision, v) in history.iter().enumerate() {
        assert_eq!(v.revision, revision as i64);
        assert_eq!(v.body, body);
    }
}
#[test]
fn target_created_during_copy_is_not_overwritten() {
    let f = Fixture::new();
    let (s, _, c) = library(&f);
    let target = f.path("racing.sqlite");
    let before = s.get_chapter(&c).unwrap();
    let result = s.recovery_snapshot_with_progress(&target, |_| {
        if !target.exists() {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)
                .map_err(|e| e.to_string())?;
            std::io::Write::write_all(&mut file, b"racing target fixture")
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    });
    assert!(result.is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"racing target fixture");
    assert_eq!(s.get_chapter(&c).unwrap(), before);
}
#[test]
fn corrupt_copied_file_is_rejected_before_publication() {
    let f = Fixture::new();
    let (s, _, c) = library(&f);
    let before = s.get_chapter(&c).unwrap();
    let result = s.recovery_snapshot_with_progress(f.path("corrupt.sqlite"), |p| {
        if p.remaining_pages == 0 {
            let temporary = std::fs::read_dir(&f.dir)
                .map_err(|e| e.to_string())?
                .filter_map(|e| e.ok())
                .find(|e| e.file_name().to_string_lossy().starts_with(".recovery-"))
                .ok_or("missing fixture temp")?
                .path();
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .open(temporary)
                .map_err(|e| e.to_string())?;
            std::io::Write::write_all(&mut file, b"broken fixture header")
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    });
    assert!(result.is_err());
    assert!(!f.path("corrupt.sqlite").exists());
    assert_eq!(s.get_chapter(&c).unwrap(), before);
    assert!(!std::fs::read_dir(&f.dir).unwrap().any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".recovery-")));
}
