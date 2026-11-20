//! Local process evidence for the actual Service save, not a Tauri reply test.
use novel_core::{Command, LibraryState, Service};
use rusqlite::{params, Connection};
use std::{path::PathBuf, process};

const TEST: &str = "interrupted_and_unacknowledged_saves_reopen_unambiguously";
const WORKER: &str = "MOJUAN_SAVE_RECOVERY_TEST_WORKER";
const BEFORE: &str = "before-commit.sqlite";
const AFTER: &str = "after-commit.sqlite";
const ABORTED: i32 = 74;
const COMMITTED: i32 = 75;
const FAULT: &str = "fixture fault after body and history, before commit";
const ORIGINAL: &str = "台阶浸着雨。🌧️\r\n我把灯留在门外。";
const NEW_DRAFT: &str = "灯仍亮着。🏮\r\n门槛上添了一行湿脚印。\n";
const STALE_DRAFT: &str = "过期客户端的不同正文，不得覆盖。";

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        let dir =
            std::env::temp_dir().join(format!("mojuan-save-recovery-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        Self(dir)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn save(
    store: &mut Service,
    target: &str,
    body: &str,
    expected_revision: i64,
) -> Result<LibraryState, String> {
    store.apply(Command::SaveChapter {
        id: target.into(),
        body: body.into(),
        expected_revision,
    })
}

fn worker(copy: &str) -> ! {
    assert!(matches!(copy, BEFORE | AFTER));
    let dir = PathBuf::from(std::env::var_os("MOJUAN_SAVE_RECOVERY_TEST_DIR").unwrap());
    let target = std::env::var("MOJUAN_SAVE_RECOVERY_TEST_CHAPTER").unwrap();
    let path = dir.join(copy);
    assert!(path.is_file());
    let mut store = Service::open(&path).unwrap();
    let result = save(&mut store, &target, NEW_DRAFT, 1);
    let diagnostic = if copy == BEFORE {
        assert_eq!(result.unwrap_err(), format!("database_error: {FAULT}"));
        ABORTED
    } else {
        result.unwrap(); // The transaction and Service's post-commit readback succeeded.
        COMMITTED
    };
    // Exit without ordinary Service/SQLite destructors or any application
    // reply. The exit code is a test-only out-of-band diagnostic for the parent.
    process::exit(diagnostic);
}

#[test]
fn interrupted_and_unacknowledged_saves_reopen_unambiguously() {
    if let Ok(copy) = std::env::var(WORKER) {
        worker(&copy);
    }
    let dir = TempDir::new();
    let seed_path = dir.0.join("seed.sqlite");
    let mut seed = Service::open(&seed_path).unwrap();
    let created = seed
        .apply(Command::CreateBook {
            title: "千章保存隔离样本".into(),
            author: "虚构测试".into(),
        })
        .unwrap();
    let target = &created.chapters[0].id;
    save(&mut seed, target, ORIGINAL, 0).unwrap();
    drop(seed);

    // Fixture setup only: fill ordinary chapter rows in one isolated batch.
    // The target's original and both attempted updates use the real save path.
    let mut sqlite = Connection::open(&seed_path).unwrap();
    sqlite.execute_batch("PRAGMA foreign_keys=ON").unwrap();
    let tx = sqlite.transaction().unwrap();
    {
        let mut insert = tx
            .prepare(
                "INSERT INTO chapters(id,volume_id,title,body,revision,sort_order)
             VALUES(?1,?2,?3,?4,0,?5)",
            )
            .unwrap();
        for i in 1..1000 {
            insert
                .execute(params![
                    format!("save-fixture-{i:04}"),
                    created.volumes[0].id,
                    format!("占位章 {i:04}"),
                    format!("虚构占位 {i:04}：风停时留下一页。🌿\r\n"),
                    i,
                ])
                .unwrap();
        }
    }
    tx.commit().unwrap();
    drop(sqlite);

    let seed = Service::open(&seed_path).unwrap();
    let baseline = seed.load().unwrap();
    assert_eq!(baseline.chapters.len(), 1000);
    let history = seed.chapter_history(target).unwrap();
    let audit = seed.audit_log().unwrap();
    assert_eq!(
        history
            .iter()
            .map(|v| (v.revision, v.body.as_bytes(), v.source.as_str()))
            .collect::<Vec<_>>(),
        [
            (0, b"".as_slice(), "baseline"),
            (1, ORIGINAL.as_bytes(), "gui")
        ],
    );
    for copy in [BEFORE, AFTER] {
        seed.recovery_snapshot(dir.0.join(copy)).unwrap();
    }
    drop(seed);

    let before = dir.0.join(BEFORE);
    let after = dir.0.join(AFTER);
    for copy in [&before, &after] {
        let restored = Service::open(copy).unwrap();
        assert_eq!(restored.load().unwrap(), baseline);
        assert_eq!(restored.chapter_history(target).unwrap(), history);
        assert_eq!(restored.audit_log().unwrap(), audit);
    }

    // Fault exists only in the disposable pre-commit copy. Its condition
    // proves the actual new body and history were visible inside the save
    // transaction when the late audit write was aborted, not merely queued.
    let faulted = Connection::open(&before).unwrap();
    let target_sql = target.replace('\'', "''");
    let new_draft_sql = NEW_DRAFT.replace('\'', "''");
    faulted.execute_batch(&format!(
        "CREATE TRIGGER fault_before_save_commit BEFORE INSERT ON workflow_audit
         WHEN NEW.chapter_id='{target_sql}' AND NEW.action='save' AND NEW.source='gui' AND NEW.revision=2 AND EXISTS (
            SELECT 1 FROM chapters c JOIN chapter_versions h ON h.chapter_id=c.id AND h.revision=c.revision
            WHERE c.id=NEW.chapter_id AND c.revision=2 AND CAST(c.body AS BLOB)=CAST('{new_draft_sql}' AS BLOB)
            AND CAST(h.body AS BLOB)=CAST(c.body AS BLOB) AND h.source='gui'
         ) BEGIN SELECT RAISE(ABORT, '{FAULT}'); END;"
    )).unwrap();
    drop(faulted);

    for (copy, expected) in [(BEFORE, ABORTED), (AFTER, COMMITTED)] {
        let output = process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST])
            .env(WORKER, copy)
            .env("MOJUAN_SAVE_RECOVERY_TEST_DIR", &dir.0)
            .env("MOJUAN_SAVE_RECOVERY_TEST_CHAPTER", target)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(expected),
            "{copy}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    let interrupted = Service::open(&before).unwrap();
    let chapter = interrupted.get_chapter(target).unwrap();
    assert_eq!(
        (chapter.revision, chapter.body.as_bytes()),
        (1, ORIGINAL.as_bytes())
    );
    assert_eq!(interrupted.load().unwrap(), baseline);
    assert_eq!(interrupted.chapter_history(target).unwrap(), history);
    assert_eq!(interrupted.audit_log().unwrap(), audit);
    drop(interrupted);

    let mut committed = Service::open(&after).unwrap();
    let chapter = committed.get_chapter(target).unwrap();
    assert_eq!(
        (chapter.revision, chapter.body.as_bytes()),
        (2, NEW_DRAFT.as_bytes())
    );
    let mut expected = baseline;
    let expected_chapter = expected
        .chapters
        .iter_mut()
        .find(|c| &c.id == target)
        .unwrap();
    expected_chapter.revision = 2;
    expected_chapter.body = NEW_DRAFT.into();
    let saved_history = committed.chapter_history(target).unwrap();
    assert_eq!(saved_history.len(), 3);
    assert_eq!(saved_history[..2], history);
    let version = &saved_history[2];
    assert_eq!(
        (
            version.chapter_id.as_str(),
            version.revision,
            version.body.as_bytes(),
            version.source.as_str()
        ),
        (target.as_str(), 2, NEW_DRAFT.as_bytes(), "gui"),
    );
    let saved_audit = committed.audit_log().unwrap();
    assert_eq!(saved_audit.len(), audit.len() + 1);
    assert_eq!(saved_audit[1..], audit);
    let event = &saved_audit[0];
    assert_eq!(
        (
            event.chapter_id.as_deref(),
            event.revision,
            event.action.as_str(),
            event.source.as_str()
        ),
        (Some(target.as_str()), Some(2), "save", "gui"),
    );
    assert_eq!(committed.load().unwrap(), expected);

    // Same stale revision: neither repeating the same request nor sending a
    // different stale draft may duplicate history or clobber the commit.
    for stale in [NEW_DRAFT, STALE_DRAFT] {
        let conflict = save(&mut committed, target, stale, 1).unwrap_err();
        assert!(conflict.starts_with("revision_conflict:"), "{conflict}");
    }
    drop(committed);
    let reopened = Service::open(&after).unwrap();
    assert_eq!(reopened.load().unwrap(), expected);
    assert_eq!(reopened.chapter_history(target).unwrap(), saved_history);
    assert_eq!(reopened.audit_log().unwrap(), saved_audit);
}
