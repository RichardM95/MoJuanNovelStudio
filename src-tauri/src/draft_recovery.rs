//! Trusted desktop, non-canonical chapter draft checkpoints. The caller holds
//! the original Service/writer lock during both canonical checks and sidecar I/O.
use crate::{Service, WorkflowError, WorkflowResult};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::{fs, io::ErrorKind, path::Path, time::Duration};

const MAX_BODY: usize = 10 * 1024 * 1024;
const MAX_TOTAL_BYTES: i64 = 64 * 1024 * 1024;
const MAX_RECORDS: i64 = 1024;
const MAX_SAFE: i64 = 9_007_199_254_740_991;
const APPLICATION_ID: i64 = 0x4d4a4452; // MJDR
const MAX_FILE_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftRecoveryScope {
    pub epoch: String,
    pub book_id: String,
    pub chapter_id: String,
}

#[derive(Clone, Serialize)]
pub struct DraftRecoverySession {
    pub session_id: String,
    pub epoch: String,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointRecoveryDraft {
    pub epoch: String,
    pub book_id: String,
    pub chapter_id: String,
    pub session_id: String,
    pub stream_id: String,
    pub sequence: i64,
    pub expected_token: Option<String>,
    pub base_revision: i64,
    pub base_body: String,
    pub draft_body: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DismissRecoveryDraft {
    pub epoch: String,
    pub book_id: String,
    pub chapter_id: String,
    pub session_id: String,
    pub sequence: i64,
    pub token: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryDraftState {
    MatchesBase,
    Stale,
    AlreadySaved,
}

#[derive(Serialize)]
pub struct RecoveryDraftReceipt {
    pub token: String,
    pub state: RecoveryDraftState,
}

#[derive(Serialize)]
pub struct RecoveryDraftRecord {
    pub token: String,
    pub base_revision: i64,
    pub base_body: String,
    pub draft_body: String,
    pub state: RecoveryDraftState,
}

#[derive(Serialize)]
pub struct RecoveryDraftRead {
    pub epoch: String,
    pub book_id: String,
    pub chapter_id: String,
    pub current_revision: i64,
    pub current_body: String,
    pub record: Option<RecoveryDraftRecord>,
}

struct Stored {
    session: String,
    stream: String,
    sequence: i64,
    token: String,
    base_revision: i64,
    base_body: String,
    draft_body: String,
    dismissed: bool,
}

fn error(code: &str, message: &str) -> WorkflowError {
    WorkflowError {
        code: code.into(),
        message: message.into(),
    }
}
fn io_error(_: impl std::fmt::Display) -> WorkflowError {
    error("draft_storage", "草稿副本存储不可用，未确认本次保存")
}
fn stale_intent() -> WorkflowError {
    error("draft_conflict", "草稿副本已变化，请重新核对后操作")
}
fn invalid() -> WorkflowError {
    error("invalid_input", "草稿副本参数无效或正文超过 10 MiB")
}
fn valid_id(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}
fn valid_revision(value: i64) -> bool {
    (0..=MAX_SAFE).contains(&value)
}

// Fail closed for unsupported/non-file paths; never initialize a nonempty
// unrelated database. New SQLite rows are committed before acknowledging.
fn sidecar(path: &Path, write: bool) -> WorkflowResult<Option<Connection>> {
    let empty = match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() && meta.len() <= MAX_FILE_BYTES => meta.len() == 0,
        Ok(_) => return Err(io_error("not a bounded regular file")),
        Err(e) if e.kind() == ErrorKind::NotFound && !write => return Ok(None),
        Err(e) if e.kind() == ErrorKind::NotFound => {
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            options.open(path).map_err(io_error)?;
            true
        }
        Err(e) => return Err(io_error(e)),
    };
    let flags = if write {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    } else {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    };
    let mut disk = Connection::open_with_flags(path, flags | OpenFlags::SQLITE_OPEN_NOFOLLOW)
        .map_err(io_error)?;
    disk.busy_timeout(Duration::from_millis(250))
        .map_err(io_error)?;
    disk.execute_batch("PRAGMA trusted_schema=OFF;")
        .map_err(io_error)?;
    if write && empty {
        disk.execute_batch("PRAGMA page_size=4096;")
            .map_err(io_error)?;
        let tx = disk
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(io_error)?;
        tx.execute_batch(
            "CREATE TABLE draft_recovery (
            epoch TEXT NOT NULL, book_id TEXT NOT NULL, chapter_id TEXT NOT NULL,
            session TEXT NOT NULL, stream TEXT NOT NULL, sequence INTEGER NOT NULL,
            token TEXT NOT NULL, base_revision INTEGER NOT NULL,
            base_body TEXT NOT NULL, draft_body TEXT NOT NULL,
            dismissed INTEGER NOT NULL CHECK (dismissed IN (0,1)),
            payload_bytes INTEGER NOT NULL CHECK (payload_bytes = octet_length(base_body) + octet_length(draft_body)),
            PRIMARY KEY (epoch,book_id,chapter_id)
        ) STRICT;
        CREATE INDEX draft_recovery_payload_budget ON draft_recovery(payload_bytes);",
        )
        .map_err(io_error)?;
        tx.pragma_update(None, "application_id", APPLICATION_ID)
            .map_err(io_error)?;
        tx.pragma_update(None, "user_version", 1)
            .map_err(io_error)?;
        tx.commit().map_err(io_error)?;
    }
    let app: i64 = disk
        .pragma_query_value(None, "application_id", |row| row.get(0))
        .map_err(io_error)?;
    let version: i64 = disk
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(io_error)?;
    let page: i64 = disk
        .pragma_query_value(None, "page_size", |row| row.get(0))
        .map_err(io_error)?;
    if app != APPLICATION_ID || version != 1 || page != 4096 {
        return Err(io_error("invalid sidecar"));
    }
    if write {
        disk.execute_batch(
            "PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; PRAGMA max_page_count=32768;",
        )
        .map_err(io_error)?;
    }
    Ok(Some(disk))
}

fn stored(connection: &Connection, scope: &DraftRecoveryScope) -> WorkflowResult<Option<Stored>> {
    let row = connection.query_row(
        "SELECT session,stream,sequence,token,base_revision,
                CASE WHEN length(CAST(base_body AS BLOB)) <= ?4 THEN base_body ELSE NULL END,
                CASE WHEN length(CAST(draft_body AS BLOB)) <= ?4 THEN draft_body ELSE NULL END,dismissed
         FROM draft_recovery WHERE epoch=?1 AND book_id=?2 AND chapter_id=?3",
        params![scope.epoch,scope.book_id,scope.chapter_id, MAX_BODY],
        |r| Ok(Stored {session:r.get(0)?,stream:r.get(1)?,sequence:r.get(2)?,token:r.get(3)?,
            base_revision:r.get(4)?,base_body:r.get(5)?,draft_body:r.get(6)?,dismissed:r.get(7)?}),
    ).optional().map_err(io_error)?;
    if row.as_ref().is_some_and(|r| {
        !valid_revision(r.base_revision)
            || !valid_id(&r.token)
            || !(1..=MAX_SAFE).contains(&r.sequence)
            || !valid_id(&r.session)
            || !valid_id(&r.stream)
    }) {
        return Err(io_error("invalid stored metadata"));
    }
    Ok(row)
}

fn classify(current: &(i64, String), base_rev: i64, base: &str, draft: &str) -> RecoveryDraftState {
    use RecoveryDraftState::*;
    if current.0 == base_rev && current.1 == base {
        if current.1 == draft {
            AlreadySaved
        } else {
            MatchesBase
        }
    } else if base_rev.checked_add(1) == Some(current.0) && current.1 == draft {
        AlreadySaved
    } else {
        Stale
    }
}

fn quota(db: &Connection, old: Option<&Stored>, added: usize) -> WorkflowResult<()> {
    let (records, bytes): (i64, i64) = db
        .query_row(
            "SELECT count(*),coalesce(sum(payload_bytes),0)
         FROM draft_recovery",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(io_error)?;
    let previous = old.map_or(0, |row| (row.base_body.len() + row.draft_body.len()) as i64);
    if records + i64::from(old.is_none()) > MAX_RECORDS
        || bytes - previous + (added as i64) > MAX_TOTAL_BYTES
    {
        return Err(error(
            "draft_quota",
            "草稿副本空间已达上限，没有覆盖已有副本",
        ));
    }
    Ok(())
}

fn scope(epoch: String, book_id: String, chapter_id: String) -> DraftRecoveryScope {
    DraftRecoveryScope {
        epoch,
        book_id,
        chapter_id,
    }
}

impl Service {
    fn recovery_path(&self) -> WorkflowResult<&Path> {
        self.draft_recovery_path
            .as_deref()
            .ok_or_else(|| error("draft_storage", "当前书库没有持久草稿副本存储"))
    }

    fn scoped_current(&self, scope: &DraftRecoveryScope) -> WorkflowResult<(i64, String)> {
        if !valid_id(&scope.epoch) || !valid_id(&scope.book_id) || !valid_id(&scope.chapter_id) {
            return Err(invalid());
        }
        if self.epoch()? != scope.epoch {
            return Err(error("draft_epoch", "书库已变化，未使用旧书库草稿"));
        }
        let current: (i64,String) = self.conn.query_row(
            "SELECT c.revision,c.body FROM chapters c JOIN volumes v ON c.volume_id=v.id JOIN books b ON b.id=v.book_id
             WHERE c.id=?1 AND b.id=?2 AND c.deleted_at IS NULL AND v.deleted_at IS NULL AND b.deleted_at IS NULL",
            params![scope.chapter_id, scope.book_id], |r| Ok((r.get(0)?,r.get(1)?)),
        ).optional()?.ok_or_else(||error("draft_scope", "章节不在当前可访问的作品中"))?;
        if !valid_revision(current.0) || current.1.len() > MAX_BODY {
            return Err(invalid());
        }
        Ok(current)
    }

    fn validate_intent(
        &self,
        scope: &DraftRecoveryScope,
        session: &str,
        sequence: i64,
    ) -> WorkflowResult<()> {
        if !valid_id(session) || !(1..=MAX_SAFE).contains(&sequence) {
            return Err(invalid());
        }
        if !self
            .draft_recovery_session
            .as_ref()
            .is_some_and(|s| s.session_id == session && s.epoch == scope.epoch)
        {
            return Err(error("draft_session", "本次草稿副本会话已失效"));
        }
        Ok(())
    }

    pub fn start_draft_recovery_session(&mut self) -> WorkflowResult<DraftRecoverySession> {
        self.recovery_path()?;
        let session = DraftRecoverySession {
            session_id: uuid::Uuid::new_v4().to_string(),
            epoch: self.epoch()?,
        };
        self.draft_recovery_session = Some(session.clone());
        Ok(session)
    }

    pub fn read_recovery_draft(
        &self,
        scope: &DraftRecoveryScope,
    ) -> WorkflowResult<RecoveryDraftRead> {
        let current = self.scoped_current(scope)?;
        let record = sidecar(self.recovery_path()?, false)?
            .map(|disk| stored(&disk, scope))
            .transpose()?
            .flatten()
            .filter(|r| !r.dismissed)
            .map(|r| {
                let state = classify(&current, r.base_revision, &r.base_body, &r.draft_body);
                RecoveryDraftRecord {
                    token: r.token,
                    base_revision: r.base_revision,
                    base_body: r.base_body,
                    draft_body: r.draft_body,
                    state,
                }
            });
        Ok(RecoveryDraftRead {
            epoch: scope.epoch.clone(),
            book_id: scope.book_id.clone(),
            chapter_id: scope.chapter_id.clone(),
            current_revision: current.0,
            current_body: current.1,
            record,
        })
    }

    pub fn checkpoint_recovery_draft(
        &mut self,
        req: CheckpointRecoveryDraft,
    ) -> WorkflowResult<RecoveryDraftReceipt> {
        if !valid_id(&req.stream_id)
            || req.expected_token.as_ref().is_some_and(|s| !valid_id(s))
            || !valid_revision(req.base_revision)
            || req.base_body.len() > MAX_BODY
            || req.draft_body.len() > MAX_BODY
        {
            return Err(invalid());
        }
        let target = scope(req.epoch, req.book_id, req.chapter_id);
        self.validate_intent(&target, &req.session_id, req.sequence)?;
        let current = self.scoped_current(&target)?;
        let anchored = if req.base_revision == current.0 {
            req.base_body == current.1
        } else if req.base_revision < current.0 {
            self.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM chapter_versions WHERE chapter_id=?1 AND revision=?2 AND CAST(body AS BLOB)=?3)",
                params![target.chapter_id,req.base_revision,req.base_body.as_bytes()], |r| r.get::<_,bool>(0),
            )?
        } else {
            false
        };
        if !anchored {
            return Err(error("draft_anchor", "草稿依据版本或原始正文不匹配"));
        }

        let mut disk =
            sidecar(self.recovery_path()?, true)?.ok_or_else(|| io_error("sidecar missing"))?;
        let tx = disk
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(io_error)?;
        let old = stored(&tx, &target)?;
        if req.expected_token.as_ref().is_some_and(|token| {
            !old.as_ref()
                .is_some_and(|r| !r.dismissed && r.token == *token)
        }) {
            return Err(stale_intent());
        }
        if let Some(row) = &old {
            let same_lease = row.session == req.session_id;
            if same_lease && row.sequence >= req.sequence {
                return Err(stale_intent());
            }
            if !row.dismissed && !(same_lease && row.stream == req.stream_id) {
                let state = classify(&current, row.base_revision, &row.base_body, &row.draft_body);
                let adopting = state == RecoveryDraftState::MatchesBase
                    && req.expected_token.is_some()
                    && req.base_revision == current.0
                    && req.base_body == current.1;
                if state != RecoveryDraftState::AlreadySaved && !adopting {
                    return Err(stale_intent());
                }
            }
        }
        let bytes = req.base_body.len() + req.draft_body.len();
        quota(&tx, old.as_ref(), bytes)?;
        let token = uuid::Uuid::new_v4().to_string();
        tx.execute("INSERT INTO draft_recovery VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,0,?11)
            ON CONFLICT(epoch,book_id,chapter_id) DO UPDATE SET
            session=excluded.session,stream=excluded.stream,sequence=excluded.sequence,token=excluded.token,
            base_revision=excluded.base_revision,base_body=excluded.base_body,draft_body=excluded.draft_body,
            dismissed=0,payload_bytes=excluded.payload_bytes",
            params![target.epoch,target.book_id,target.chapter_id,req.session_id,req.stream_id,req.sequence,
                token,req.base_revision,req.base_body,req.draft_body,bytes],
        ).map_err(io_error)?;
        tx.commit().map_err(io_error)?;
        Ok(RecoveryDraftReceipt {
            token,
            state: classify(&current, req.base_revision, &req.base_body, &req.draft_body),
        })
    }

    pub fn dismiss_recovery_draft(&mut self, req: DismissRecoveryDraft) -> WorkflowResult<()> {
        if !valid_id(&req.token) {
            return Err(invalid());
        }
        let target = scope(req.epoch, req.book_id, req.chapter_id);
        self.validate_intent(&target, &req.session_id, req.sequence)?;
        self.scoped_current(&target)?;
        let path = self.recovery_path()?;
        if !path.exists() {
            return Err(stale_intent());
        }
        let mut disk = sidecar(path, true)?.ok_or_else(stale_intent)?;
        let tx = disk
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(io_error)?;
        let changed = tx
            .execute(
                "UPDATE draft_recovery SET session=?4,sequence=?5,dismissed=1
            WHERE epoch=?1 AND book_id=?2 AND chapter_id=?3 AND token=?6 AND dismissed=0
              AND (session<>?4 OR sequence<?5)",
                params![
                    target.epoch,
                    target.book_id,
                    target.chapter_id,
                    req.session_id,
                    req.sequence,
                    req.token
                ],
            )
            .map_err(io_error)?;
        if changed != 1 {
            return Err(stale_intent());
        }
        tx.commit().map_err(io_error)?;
        Ok(())
    }
}
