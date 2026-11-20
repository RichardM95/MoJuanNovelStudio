use novel_core::{prepare_restore, Command, Service};
use rusqlite::{Connection, OpenFlags};
use std::path::PathBuf;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("sqlite-maintenance-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn bundled_runtime_contains_official_wal_reset_fix() {
    let c = Connection::open_in_memory().unwrap();
    let version: String = c
        .query_row("select sqlite_version()", [], |r| r.get(0))
        .unwrap();
    let source: String = c
        .query_row("select sqlite_source_id()", [], |r| r.get(0))
        .unwrap();
    eprintln!("SQLite runtime={version}; source_id={source}");
    // Maintenance floor intentionally above the first WAL-reset fix (3.51.3).
    // This is an engine identity guard, not a reproduction of the rare race.
    assert!(
        rusqlite::version_number() >= 3_054_001,
        "fixed maintenance floor: {version}"
    );
    assert_eq!(version, rusqlite::version());
}

#[test]
fn online_backup_includes_committed_wal_not_just_main_file() {
    let f = Fixture::new();
    let path = f.0.join("live.sqlite");
    let mut s = Service::open(&path).unwrap();
    let a = s
        .apply(Command::CreateBook {
            title: "WAL虚构书".into(),
            author: "隔离测试".into(),
        })
        .unwrap();
    let id = &a.chapters[0].id;
    s.apply(Command::SaveChapter {
        id: id.clone(),
        body: "已入主库".into(),
        expected_revision: 0,
    })
    .unwrap();
    // Test-only second connection establishes an old read snapshot. Production
    // does not introduce a second writer/checkpointer by this test.
    let control = Connection::open(&path).unwrap();
    control
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); BEGIN;")
        .unwrap();
    let old: String = control
        .query_row("SELECT body FROM chapters WHERE id=?1", [id], |r| r.get(0))
        .unwrap();
    assert_eq!(old, "已入主库");
    s.apply(Command::SaveChapter {
        id: id.clone(),
        body: "仅在WAL的新稿\r\n🌙".into(),
        expected_revision: 1,
    })
    .unwrap();
    let expected = s.backup().unwrap();
    let history = s.chapter_history(id).unwrap();
    assert!(
        std::fs::metadata(format!("{}-wal", path.display()))
            .unwrap()
            .len()
            > 32
    );
    let bare = f.0.join("bare-main.sqlite");
    std::fs::copy(&path, &bare).unwrap();
    let bare_conn = Connection::open_with_flags(&bare, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let bare_body: String = bare_conn
        .query_row("SELECT body FROM chapters WHERE id=?1", [id], |r| r.get(0))
        .unwrap();
    assert_eq!(
        bare_body, "已入主库",
        "control proves current data is not in copied main file"
    );
    drop(bare_conn);
    let backup = f.0.join("consistent.sqlite");
    s.export_sqlite_backup(&backup, |_| Ok(())).unwrap();
    let check = Connection::open_with_flags(&backup, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(
        check
            .query_row("PRAGMA journal_mode", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "delete"
    );
    assert_eq!(
        check
            .query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    drop(check);
    assert_eq!(s.backup().unwrap(), expected);
    let mut dest = Service::open(f.0.join("restored.sqlite")).unwrap();
    let guard = dest.restore_guard().unwrap();
    dest.commit_prepared_restore(
        prepare_restore(&backup, |_| Ok(())).unwrap(),
        &guard,
        &f.0.join("before.sqlite"),
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(dest.get_chapter(id).unwrap().body, "仅在WAL的新稿\r\n🌙");
    assert_eq!(dest.chapter_history(id).unwrap(), history);
    drop(dest);
    drop(control);
    drop(s);
    let reopened = Service::open(f.0.join("restored.sqlite")).unwrap();
    assert_eq!(reopened.chapter_history(id).unwrap(), history);
}

#[test]
fn managed_same_path_service_lock_survives_engine_upgrade() {
    let f = Fixture::new();
    let path = f.0.join("live.sqlite");
    let s = Service::open(&path).unwrap();
    assert!(Service::open(&path)
        .err()
        .unwrap()
        .contains("writer_locked"));
    let other = path.clone();
    assert!(
        std::thread::spawn(move || Service::open(other).err().unwrap())
            .join()
            .unwrap()
            .contains("writer_locked")
    );
    drop(s);
    let _next = Service::open(path).unwrap();
}

#[test]
fn unsigned_sql_compatibility_preserves_checked_bounds() {
    let c = Connection::open_in_memory().unwrap();
    let max = i64::MAX as u64;
    assert_eq!(
        c.query_row("SELECT ?1", [max], |r| r.get::<_, u64>(0))
            .unwrap(),
        max
    );
    assert!(c
        .query_row("SELECT ?1", [u64::MAX], |r| r.get::<_, i64>(0))
        .is_err());
    assert!(c
        .query_row("SELECT -1", [], |r| r.get::<_, u64>(0))
        .is_err());
    assert_eq!(
        c.query_row("SELECT ?1", [42usize], |r| r.get::<_, usize>(0))
            .unwrap(),
        42
    );
}
