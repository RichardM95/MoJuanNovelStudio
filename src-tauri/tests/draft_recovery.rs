use novel_core::{
    prepare_restore, CheckpointRecoveryDraft, Command, DismissRecoveryDraft, DraftRecoveryScope,
    DraftRecoverySession, RecoveryDraftState, Service,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

const BASE: &str = "第一段🌙\r\n原样\r末行";
const A: &str = "正式稿甲🌊\r\n原样";
const B: &str = "未保存稿乙🏮\r\n不同";

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("mojuan-draft-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self, file: &str) -> PathBuf {
        self.0.join(file)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn seed(path: &Path) -> (Service, DraftRecoveryScope) {
    let mut store = Service::open(path).unwrap();
    let data = store
        .apply(Command::CreateBook {
            title: "虚构稿".into(),
            author: "测试".into(),
        })
        .unwrap();
    let chapter_id = data.chapters[0].id.clone();
    let scope = DraftRecoveryScope {
        epoch: store.epoch().unwrap(),
        book_id: data.books[0].id.clone(),
        chapter_id,
    };
    save(&mut store, &scope, BASE, 0);
    (store, scope)
}
fn save(store: &mut Service, scope: &DraftRecoveryScope, text: &str, expected_revision: i64) {
    store
        .apply(Command::SaveChapter {
            id: scope.chapter_id.clone(),
            body: text.into(),
            expected_revision,
        })
        .unwrap();
}
fn input(
    scope: &DraftRecoveryScope,
    session: &DraftRecoverySession,
    sequence: i64,
    stream: &str,
    draft_body: &str,
) -> CheckpointRecoveryDraft {
    CheckpointRecoveryDraft {
        epoch: scope.epoch.clone(),
        book_id: scope.book_id.clone(),
        chapter_id: scope.chapter_id.clone(),
        session_id: session.session_id.clone(),
        stream_id: stream.into(),
        sequence,
        expected_token: None,
        base_revision: 1,
        base_body: BASE.into(),
        draft_body: draft_body.into(),
    }
}
fn dismiss(
    scope: &DraftRecoveryScope,
    session: &DraftRecoverySession,
    sequence: i64,
    token: String,
) -> DismissRecoveryDraft {
    DismissRecoveryDraft {
        epoch: scope.epoch.clone(),
        book_id: scope.book_id.clone(),
        chapter_id: scope.chapter_id.clone(),
        session_id: session.session_id.clone(),
        sequence,
        token,
    }
}

#[test]
fn reopen_keeps_exact_checkpoint_without_touching_canonical_or_snapshots() {
    let fixture = Fixture::new();
    let live = fixture.path("live.sqlite");
    let (mut writer, scope) = seed(&live);
    let saved = writer.get_chapter(&scope.chapter_id).unwrap();
    let history = writer.chapter_history(&scope.chapter_id).unwrap();
    let audit = writer.audit_log().unwrap();
    assert!(writer.read_recovery_draft(&scope).unwrap().record.is_none());
    assert!(!fixture.path("live.sqlite.draft-recovery.sqlite").exists());
    let lease = writer.start_draft_recovery_session().unwrap();
    assert_eq!(lease.epoch, scope.epoch);
    let receipt = writer
        .checkpoint_recovery_draft(input(&scope, &lease, 1, "first-editor", B))
        .unwrap();
    assert_eq!(receipt.state, RecoveryDraftState::MatchesBase);
    let json = writer.backup().unwrap();
    assert!(!json.contains("未保存稿乙"));
    let snapshot = fixture.path("snapshot.sqlite");
    writer.recovery_snapshot(&snapshot).unwrap();
    let copy = Service::open(&snapshot).unwrap();
    assert!(copy.read_recovery_draft(&scope).unwrap().record.is_none());
    let observer = rusqlite::Connection::open(&snapshot).unwrap();
    let draft_tables: i64 = observer
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name LIKE '%draft_recovery%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(draft_tables, 0);
    drop(observer);
    drop(copy);
    drop(writer);
    let writer = Service::open(&live).unwrap();
    let read = writer.read_recovery_draft(&scope).unwrap();
    assert_eq!(read.current_revision, 1);
    assert_eq!(read.current_body.as_bytes(), BASE.as_bytes());
    let record = read.record.unwrap();
    assert_eq!(record.token, receipt.token);
    assert_eq!(record.state, RecoveryDraftState::MatchesBase);
    assert_eq!(
        (
            record.base_revision,
            record.base_body.as_bytes(),
            record.draft_body.as_bytes()
        ),
        (1, BASE.as_bytes(), B.as_bytes())
    );
    assert_eq!(writer.get_chapter(&scope.chapter_id).unwrap(), saved);
    assert_eq!(writer.chapter_history(&scope.chapter_id).unwrap(), history);
    assert_eq!(writer.audit_log().unwrap(), audit);
}

#[test]
fn exact_committed_copy_is_distinct_from_late_unsaved_draft_and_old_cleanup() {
    let fixture = Fixture::new();
    let (mut writer, scope) = seed(&fixture.path("live.sqlite"));
    let lease = writer.start_draft_recovery_session().unwrap();
    let a = writer
        .checkpoint_recovery_draft(input(&scope, &lease, 1, "editing", A))
        .unwrap();
    save(&mut writer, &scope, A, 1); // Canonical transaction succeeded; no checkpoint cleanup sent.
    assert_eq!(
        writer
            .read_recovery_draft(&scope)
            .unwrap()
            .record
            .unwrap()
            .state,
        RecoveryDraftState::AlreadySaved
    );
    let b = writer
        .checkpoint_recovery_draft(input(&scope, &lease, 3, "editing", B))
        .unwrap();
    assert_eq!(b.state, RecoveryDraftState::Stale);
    assert!(writer
        .checkpoint_recovery_draft(input(&scope, &lease, 2, "editing", A))
        .is_err());
    assert!(writer
        .dismiss_recovery_draft(dismiss(&scope, &lease, 4, a.token))
        .is_err());
    let mut forged = input(&scope, &lease, 5, "editing", A);
    forged.base_body = BASE.replace("\r\n", "\n");
    assert!(writer.checkpoint_recovery_draft(forged).is_err());
    let mut cross_stream = input(&scope, &lease, 6, "different", A);
    cross_stream.expected_token = Some(b.token.clone());
    assert!(writer.checkpoint_recovery_draft(cross_stream).is_err());
    let found = writer.read_recovery_draft(&scope).unwrap();
    assert_eq!(
        (found.current_revision, found.current_body.as_str()),
        (2, A)
    );
    let found = found.record.unwrap();
    assert_eq!(
        (found.token, found.draft_body.as_str(), found.state),
        (b.token, B, RecoveryDraftState::Stale)
    );
    assert_eq!(writer.chapter_history(&scope.chapter_id).unwrap().len(), 3);
}

#[test]
fn new_lease_requires_exact_adoption_and_dismissal_fences_delayed_checkpoint() {
    let fixture = Fixture::new();
    let live = fixture.path("live.sqlite");
    let (mut writer, scope) = seed(&live);
    let old = writer.start_draft_recovery_session().unwrap();
    let first = writer
        .checkpoint_recovery_draft(input(&scope, &old, 10, "old-editor", A))
        .unwrap();
    drop(writer);
    let mut writer = Service::open(&live).unwrap();
    let fresh = writer.start_draft_recovery_session().unwrap();
    assert!(writer
        .checkpoint_recovery_draft(input(&scope, &old, 11, "old-editor", B))
        .is_err());
    let mut candidate = input(&scope, &fresh, 1, "new-editor", B);
    assert!(writer.checkpoint_recovery_draft(candidate.clone()).is_err());
    candidate.expected_token = Some(first.token.clone());
    let newest = writer.checkpoint_recovery_draft(candidate).unwrap();
    assert!(writer
        .dismiss_recovery_draft(dismiss(&scope, &fresh, 2, first.token))
        .is_err());
    writer
        .dismiss_recovery_draft(dismiss(&scope, &fresh, 3, newest.token))
        .unwrap();
    assert!(writer
        .checkpoint_recovery_draft(input(&scope, &fresh, 2, "new-editor", A))
        .is_err());
    assert!(writer.read_recovery_draft(&scope).unwrap().record.is_none());
    let raw =
        rusqlite::Connection::open(fixture.path("live.sqlite.draft-recovery.sqlite")).unwrap();
    let retained: String = raw
        .query_row("SELECT draft_body FROM draft_recovery", [], |r| r.get(0))
        .unwrap();
    assert_eq!(retained, B);
    drop(raw);
    writer
        .checkpoint_recovery_draft(input(&scope, &fresh, 4, "fresh-editor", A))
        .unwrap();
    assert_eq!(
        writer
            .read_recovery_draft(&scope)
            .unwrap()
            .record
            .unwrap()
            .draft_body,
        A
    );
}

#[test]
fn changed_raw_base_wrong_book_and_restored_epoch_cannot_offer_stale_copy_as_current() {
    let fixture = Fixture::new();
    let live = fixture.path("live.sqlite");
    let (mut writer, scope) = seed(&live);
    let lease = writer.start_draft_recovery_session().unwrap();
    let checkpoint = writer
        .checkpoint_recovery_draft(input(&scope, &lease, 1, "editing", B))
        .unwrap();
    let snapshot = fixture.path("original.sqlite");
    writer.recovery_snapshot(&snapshot).unwrap();
    let wrong_book = DraftRecoveryScope {
        book_id: "other-book".into(),
        ..scope.clone()
    };
    assert!(writer.read_recovery_draft(&wrong_book).is_err());
    let control = rusqlite::Connection::open(&live).unwrap();
    let other = BASE.replace("\r\n", "\n");
    control
        .execute(
            "UPDATE chapters SET body=?1 WHERE id=?2",
            rusqlite::params![other, scope.chapter_id],
        )
        .unwrap();
    drop(control);
    assert_eq!(
        writer
            .read_recovery_draft(&scope)
            .unwrap()
            .record
            .unwrap()
            .state,
        RecoveryDraftState::Stale
    );
    assert!(writer
        .checkpoint_recovery_draft(input(&scope, &lease, 2, "editing", A))
        .is_err());
    let prepared = prepare_restore(&snapshot, |_| Ok(())).unwrap();
    let guard = writer.restore_guard().unwrap();
    writer
        .commit_prepared_restore(prepared, &guard, &fixture.path("before.sqlite"), |_| Ok(()))
        .unwrap();
    assert!(writer.read_recovery_draft(&scope).is_err());
    let new_scope = DraftRecoveryScope {
        epoch: writer.epoch().unwrap(),
        ..scope.clone()
    };
    assert_eq!(
        writer.read_recovery_draft(&new_scope).unwrap().current_body,
        BASE
    );
    assert!(writer
        .read_recovery_draft(&new_scope)
        .unwrap()
        .record
        .is_none());
    assert!(writer
        .checkpoint_recovery_draft(input(&new_scope, &lease, 3, "editing", A))
        .is_err());
    assert!(writer
        .dismiss_recovery_draft(dismiss(&new_scope, &lease, 4, checkpoint.token))
        .is_err());
    let raw =
        rusqlite::Connection::open(fixture.path("live.sqlite.draft-recovery.sqlite")).unwrap();
    assert_eq!(
        raw.query_row("SELECT count(*) FROM draft_recovery", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn oversized_inputs_and_an_unrelated_sidecar_fail_without_creating_or_clobbering_data() {
    let fixture = Fixture::new();
    let (mut writer, scope) = seed(&fixture.path("live.sqlite"));
    let lease = writer.start_draft_recovery_session().unwrap();
    let path = fixture.path("live.sqlite.draft-recovery.sqlite");
    let oversize = "x".repeat(10 * 1024 * 1024 + 1);
    assert_eq!(
        writer
            .checkpoint_recovery_draft(input(&scope, &lease, 1, "writer", &oversize))
            .err()
            .unwrap()
            .code,
        "invalid_input",
    );
    assert!(!path.exists());
    let unrelated = rusqlite::Connection::open(&path).unwrap();
    unrelated
        .execute_batch("CREATE TABLE keep_me(value TEXT); INSERT INTO keep_me VALUES('unrelated');")
        .unwrap();
    drop(unrelated);
    let bytes = fs::read(&path).unwrap();
    assert_eq!(
        writer.read_recovery_draft(&scope).err().unwrap().code,
        "draft_storage"
    );
    assert_eq!(
        writer
            .checkpoint_recovery_draft(input(&scope, &lease, 2, "writer", A))
            .err()
            .unwrap()
            .code,
        "draft_storage",
    );
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(writer.get_chapter(&scope.chapter_id).unwrap().body, BASE);
    assert_eq!(writer.chapter_history(&scope.chapter_id).unwrap().len(), 2);
}

#[test]
fn archived_slots_count_toward_quota_without_eviction() {
    let fixture = Fixture::new();
    let (mut writer, scope) = seed(&fixture.path("live.sqlite"));
    let lease = writer.start_draft_recovery_session().unwrap();
    let first = writer
        .checkpoint_recovery_draft(input(&scope, &lease, 1, "first", A))
        .unwrap();
    let filled =
        rusqlite::Connection::open(fixture.path("live.sqlite.draft-recovery.sqlite")).unwrap();
    // Deterministically fill the record budget with tiny, retired previous-epoch rows.
    // Exercise the boundary without allocating 64 MiB or exhausting disk.
    filled
        .execute_batch(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i<1023)
        INSERT INTO draft_recovery SELECT 'past-epoch','archived','chapter-'||i,
        'past-session','past-stream',1,'token-'||i,0,'','x',1,1 FROM n;",
        )
        .unwrap();
    drop(filled);
    let library = writer.load().unwrap();
    let new = writer
        .apply(Command::CreateChapter {
            volume_id: library.volumes[0].id.clone(),
            title: "新空章".into(),
        })
        .unwrap();
    let new_id = new
        .chapters
        .iter()
        .find(|c| c.id != scope.chapter_id)
        .unwrap()
        .id
        .clone();
    let new_scope = DraftRecoveryScope {
        chapter_id: new_id,
        ..scope.clone()
    };
    let mut request = input(&new_scope, &lease, 2, "new", B);
    request.base_revision = 0;
    request.base_body.clear();
    assert_eq!(
        writer
            .checkpoint_recovery_draft(request)
            .err()
            .unwrap()
            .code,
        "draft_quota"
    );
    assert!(writer
        .read_recovery_draft(&new_scope)
        .unwrap()
        .record
        .is_none());
    assert_eq!(
        writer
            .read_recovery_draft(&scope)
            .unwrap()
            .record
            .unwrap()
            .token,
        first.token
    );
    writer
        .checkpoint_recovery_draft(input(&scope, &lease, 3, "first", B))
        .unwrap();
    let raw =
        rusqlite::Connection::open(fixture.path("live.sqlite.draft-recovery.sqlite")).unwrap();
    assert_eq!(
        raw.query_row("SELECT count(*) FROM draft_recovery", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1024
    );
}
