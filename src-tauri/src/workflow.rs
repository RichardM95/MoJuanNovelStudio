//! Review-only editing workflow. Authorization belongs to the transport; this
//! module enforces revision, deletion-chain, transaction and idempotency rules.
use crate::{Book, Chapter, LibraryState, Service, Volume};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
const BODY_LIMIT: usize = 10 * 1024 * 1024;
const OBJECT_LIMIT: usize = 100_000;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowError {
    pub code: String,
    pub message: String,
}
pub type WorkflowResult<T> = std::result::Result<T, WorkflowError>;
impl std::fmt::Display for WorkflowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for WorkflowError {}
impl From<rusqlite::Error> for WorkflowError {
    fn from(e: rusqlite::Error) -> Self {
        failure("database_error", &e.to_string())
    }
}
fn failure(code: &str, message: &str) -> WorkflowError {
    WorkflowError {
        code: code.into(),
        message: message.into(),
    }
}
fn uid() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn now() -> WorkflowResult<i64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|n| n.as_secs() as i64)
        .map_err(|e| failure("clock_error", &e.to_string()))
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProposalInput {
    pub client_id: String,
    pub idempotency_key: String,
    pub chapter_id: String,
    pub base_revision: i64,
    pub new_body: String,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Proposal {
    pub id: String,
    pub client_id: String,
    pub chapter_id: String,
    pub base_revision: i64,
    pub old_body: String,
    pub new_body: String,
    pub reason: String,
    pub source: String,
    pub status: String,
    pub created_at: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ChapterVersion {
    pub id: String,
    pub chapter_id: String,
    pub revision: i64,
    pub body: String,
    pub source: String,
    pub created_at: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChapterMetadata {
    pub id: String,
    pub volume_id: String,
    pub title: String,
    pub revision: i64,
    pub sort_order: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BookOutline {
    pub book: Book,
    pub volumes: Vec<Volume>,
    pub chapters: Vec<ChapterMetadata>,
}

pub(crate) fn migrate(conn: &Connection) -> WorkflowResult<()> {
    let migration = conn.unchecked_transaction()?;
    let conn = &migration;
    conn.execute_batch("CREATE TABLE IF NOT EXISTS workflow_meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
 CREATE TABLE IF NOT EXISTS chapter_versions(id TEXT PRIMARY KEY,chapter_id TEXT NOT NULL REFERENCES chapters(id) ON DELETE CASCADE,revision INTEGER NOT NULL,body TEXT NOT NULL,source TEXT NOT NULL,created_at INTEGER NOT NULL,UNIQUE(chapter_id,revision));
 CREATE TABLE IF NOT EXISTS edit_proposals(id TEXT PRIMARY KEY,client_id TEXT NOT NULL,idempotency_key TEXT NOT NULL,payload TEXT NOT NULL,chapter_id TEXT NOT NULL REFERENCES chapters(id) ON DELETE CASCADE,base_revision INTEGER NOT NULL,old_body TEXT NOT NULL,new_body TEXT NOT NULL,reason TEXT NOT NULL,source TEXT NOT NULL,status TEXT NOT NULL CHECK(status IN ('pending','accepted','rejected','cancelled')),created_at INTEGER NOT NULL,UNIQUE(client_id,idempotency_key));
 CREATE TABLE IF NOT EXISTS workflow_audit(id TEXT PRIMARY KEY,chapter_id TEXT REFERENCES chapters(id) ON DELETE CASCADE,action TEXT NOT NULL,source TEXT NOT NULL,revision INTEGER,created_at INTEGER NOT NULL);
 CREATE INDEX IF NOT EXISTS proposal_chapter ON edit_proposals(chapter_id);")?;
    let proposal_schema: String = conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name='edit_proposals'",
        [],
        |r| r.get(0),
    )?;
    if !proposal_schema.contains("'cancelled'") {
        // SQLite cannot alter a CHECK constraint. Rebuild only this table in
        // the migration transaction, preserving every immutable proposal/key.
        conn.execute_batch("CREATE TABLE edit_proposals_next(id TEXT PRIMARY KEY,client_id TEXT NOT NULL,idempotency_key TEXT NOT NULL,payload TEXT NOT NULL,chapter_id TEXT NOT NULL REFERENCES chapters(id) ON DELETE CASCADE,base_revision INTEGER NOT NULL,old_body TEXT NOT NULL,new_body TEXT NOT NULL,reason TEXT NOT NULL,source TEXT NOT NULL,status TEXT NOT NULL CHECK(status IN ('pending','accepted','rejected','cancelled')),created_at INTEGER NOT NULL,UNIQUE(client_id,idempotency_key));
        INSERT INTO edit_proposals_next SELECT * FROM edit_proposals;
        DROP TABLE edit_proposals;
        ALTER TABLE edit_proposals_next RENAME TO edit_proposals;
        CREATE INDEX proposal_chapter ON edit_proposals(chapter_id);")?;
    }
    conn.execute(
        "INSERT OR IGNORE INTO workflow_meta VALUES('epoch',?1)",
        [uid()],
    )?;
    migration.commit()?;
    Ok(())
}
pub(crate) fn chapter(conn: &Connection, id: &str) -> WorkflowResult<Chapter> {
    let row=conn.query_row("SELECT c.id,c.volume_id,c.title,c.body,c.revision,c.sort_order,c.deleted_at,v.deleted_at,b.deleted_at FROM chapters c JOIN volumes v ON v.id=c.volume_id JOIN books b ON b.id=v.book_id WHERE c.id=?1",[id],|r|Ok((Chapter{id:r.get(0)?,volume_id:r.get(1)?,title:r.get(2)?,body:r.get(3)?,revision:r.get(4)?,sort_order:r.get(5)?,deleted_at:r.get(6)?},r.get::<_,Option<i64>>(7)?,r.get::<_,Option<i64>>(8)?))).optional()?.ok_or_else(||failure("not_found","章节不存在"))?;
    if row.0.deleted_at.is_some() || row.1.is_some() || row.2.is_some() {
        return Err(failure("deleted", "章节或所属卷、作品在回收站"));
    }
    Ok(row.0)
}
fn check_revision(actual: i64, expected: i64) -> WorkflowResult<()> {
    if expected < 0 || expected == i64::MAX {
        return Err(failure("invalid_input", "revision 无效或溢出"));
    }
    if actual != expected {
        return Err(failure("revision_conflict", "正文已更新，请重新读取章节"));
    }
    Ok(())
}
fn audit(
    tx: &Transaction,
    chapter_id: Option<&str>,
    action: &str,
    source: &str,
    revision: Option<i64>,
) -> WorkflowResult<()> {
    tx.execute(
        "INSERT INTO workflow_audit VALUES(?1,?2,?3,?4,?5,?6)",
        params![uid(), chapter_id, action, source, revision, now()?],
    )?;
    Ok(())
}
fn insert_version(tx: &Transaction, v: &ChapterVersion) -> WorkflowResult<()> {
    tx.execute(
        "INSERT INTO chapter_versions VALUES(?1,?2,?3,?4,?5,?6)",
        params![
            v.id,
            v.chapter_id,
            v.revision,
            v.body,
            v.source,
            v.created_at
        ],
    )?;
    Ok(())
}
/// Caller owns the transaction. A failure in history/audit rolls back the body.
pub(crate) fn save_in_transaction(
    tx: &Transaction,
    id: &str,
    body: &str,
    expected: i64,
    source: &str,
) -> WorkflowResult<()> {
    if body.len() > BODY_LIMIT {
        return Err(failure("invalid_input", "正文超过10MiB"));
    }
    let old = chapter(tx, id)?;
    check_revision(old.revision, expected)?;
    let count: i64 = tx.query_row("SELECT count(*) FROM chapter_versions", [], |r| r.get(0))?;
    if count >= OBJECT_LIMIT as i64 - 1 {
        return Err(failure("limit_exceeded", "历史记录过多"));
    }
    tx.execute("INSERT OR IGNORE INTO chapter_versions(id,chapter_id,revision,body,source,created_at) VALUES(?1,?2,?3,?4,'baseline',?5)",params![uid(),id,old.revision,old.body,now()?])?;
    if tx.execute(
        "UPDATE chapters SET body=?1,revision=revision+1 WHERE id=?2 AND revision=?3",
        params![body, id, expected],
    )? != 1
    {
        return Err(failure("revision_conflict", "正文已更新"));
    }
    insert_version(
        tx,
        &ChapterVersion {
            id: uid(),
            chapter_id: id.into(),
            revision: expected + 1,
            body: body.into(),
            source: source.into(),
            created_at: now()?,
        },
    )?;
    audit(tx, Some(id), "save", source, Some(expected + 1))?;
    Ok(())
}
fn proposal_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Proposal> {
    Ok(Proposal {
        id: r.get(0)?,
        client_id: r.get(1)?,
        chapter_id: r.get(2)?,
        base_revision: r.get(3)?,
        old_body: r.get(4)?,
        new_body: r.get(5)?,
        reason: r.get(6)?,
        source: r.get(7)?,
        status: r.get(8)?,
        created_at: r.get(9)?,
    })
}
const PROPOSAL_COLUMNS: &str =
    "id,client_id,chapter_id,base_revision,old_body,new_body,reason,source,status,created_at";
fn proposal(conn: &Connection, id: &str) -> WorkflowResult<Proposal> {
    conn.query_row(
        &format!("SELECT {PROPOSAL_COLUMNS} FROM edit_proposals WHERE id=?1"),
        [id],
        proposal_row,
    )
    .optional()?
    .ok_or_else(|| failure("not_found", "提案不存在或已因整库恢复失效"))
}
fn owned_proposal(conn: &Connection, id: &str, client_id: &str) -> WorkflowResult<Proposal> {
    if let Some(p) = conn
        .query_row(
            &format!("SELECT {PROPOSAL_COLUMNS} FROM edit_proposals WHERE id=?1 AND client_id=?2"),
            params![id, client_id],
            proposal_row,
        )
        .optional()?
    {
        return Ok(p);
    }
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM edit_proposals WHERE id=?1)",
        [id],
        |r| r.get(0),
    )?;
    Err(if exists {
        failure("forbidden", "不能读取其他客户端的提案")
    } else {
        failure("not_found", "提案不存在或已失效")
    })
}
fn versions(conn: &Connection, id: Option<&str>) -> WorkflowResult<Vec<ChapterVersion>> {
    let sql = if id.is_some() {
        "SELECT id,chapter_id,revision,body,source,created_at FROM chapter_versions WHERE chapter_id=?1 ORDER BY revision,id"
    } else {
        "SELECT id,chapter_id,revision,body,source,created_at FROM chapter_versions ORDER BY chapter_id,revision,id"
    };
    let mut stmt = conn.prepare(sql)?;
    let map = |r: &rusqlite::Row<'_>| {
        Ok(ChapterVersion {
            id: r.get(0)?,
            chapter_id: r.get(1)?,
            revision: r.get(2)?,
            body: r.get(3)?,
            source: r.get(4)?,
            created_at: r.get(5)?,
        })
    };
    let result = if let Some(id) = id {
        stmt.query_map([id], map)?
            .collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        stmt.query_map([], map)?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    Ok(result)
}
pub(crate) fn all_versions(conn: &Connection) -> crate::Result<Vec<ChapterVersion>> {
    versions(conn, None).map_err(|e| e.to_string())
}
pub(crate) fn validate_history(
    state: &LibraryState,
    history: &[ChapterVersion],
) -> crate::Result<()> {
    if history.len() > OBJECT_LIMIT {
        return Err("历史记录过多".into());
    }
    let chapters: std::collections::HashMap<_, _> =
        state.chapters.iter().map(|c| (c.id.as_str(), c)).collect();
    let mut ids = HashSet::new();
    let mut revisions = HashSet::new();
    for v in history {
        let c = chapters
            .get(v.chapter_id.as_str())
            .ok_or("历史引用不存在的章节")?;
        if v.id.is_empty()
            || v.id.len() > 128
            || !ids.insert(&v.id)
            || !revisions.insert((&v.chapter_id, v.revision))
            || v.revision < 0
            || v.revision > c.revision
            || v.body.len() > BODY_LIMIT
            || v.source.is_empty()
            || v.source.len() > 256
            || v.created_at < 0
            || v.revision == c.revision && v.body != c.body
        {
            return Err("历史记录字段、版本或正文无效".into());
        }
    }
    Ok(())
}
pub(crate) fn replace_workflow(tx: &Transaction, history: &[ChapterVersion]) -> crate::Result<()> {
    tx.execute_batch(
        "DELETE FROM edit_proposals; DELETE FROM chapter_versions; DELETE FROM workflow_audit;",
    )
    .map_err(|e| e.to_string())?;
    for v in history {
        insert_version(tx, v).map_err(|e| e.to_string())?;
    }
    tx.execute(
        "UPDATE workflow_meta SET value=?1 WHERE key='epoch'",
        [uid()],
    )
    .map_err(|e| e.to_string())?;
    audit(tx, None, "restore_library", "human", None).map_err(|e| e.to_string())?;
    Ok(())
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditEvent {
    pub id: String,
    pub action: String,
    pub chapter_id: Option<String>,
    pub revision: Option<i64>,
    pub source: String,
    pub created_at: i64,
}
impl Service {
    /// Most recent 500 events, newest first. Metadata only; GUI must not expose
    /// this global view to clients scoped to one book.
    pub fn audit_log(&self) -> WorkflowResult<Vec<AuditEvent>> {
        let rows=self.conn.prepare("SELECT id,action,chapter_id,revision,source,created_at FROM workflow_audit ORDER BY created_at DESC,rowid DESC LIMIT 500")?.query_map([],|r|Ok(AuditEvent{id:r.get(0)?,action:r.get(1)?,chapter_id:r.get(2)?,revision:r.get(3)?,source:r.get(4)?,created_at:r.get(5)?}))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn epoch(&self) -> WorkflowResult<String> {
        Ok(self.conn.query_row(
            "SELECT value FROM workflow_meta WHERE key='epoch'",
            [],
            |r| r.get(0),
        )?)
    }
    pub fn get_chapter(&self, id: &str) -> WorkflowResult<Chapter> {
        chapter(&self.conn, id)
    }
    pub fn get_outline(&self, id: &str) -> WorkflowResult<BookOutline> {
        let book = self
            .conn
            .query_row(
                "SELECT id,title,author,deleted_at FROM books WHERE id=?1",
                [id],
                |r| {
                    Ok(Book {
                        id: r.get(0)?,
                        title: r.get(1)?,
                        author: r.get(2)?,
                        deleted_at: r.get(3)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| failure("not_found", "作品不存在"))?;
        if book.deleted_at.is_some() {
            return Err(failure("deleted", "作品在回收站"));
        }
        let volumes=self.conn.prepare("SELECT id,book_id,title,sort_order,deleted_at FROM volumes WHERE book_id=?1 AND deleted_at IS NULL ORDER BY sort_order,id")?.query_map([id],|r|Ok(Volume{id:r.get(0)?,book_id:r.get(1)?,title:r.get(2)?,sort_order:r.get(3)?,deleted_at:r.get(4)?}))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let chapters=self.conn.prepare("SELECT c.id,c.volume_id,c.title,c.revision,c.sort_order FROM chapters c JOIN volumes v ON c.volume_id=v.id WHERE v.book_id=?1 AND v.deleted_at IS NULL AND c.deleted_at IS NULL ORDER BY v.sort_order,v.id,c.sort_order,c.id")?.query_map([id],|r|Ok(ChapterMetadata{id:r.get(0)?,volume_id:r.get(1)?,title:r.get(2)?,revision:r.get(3)?,sort_order:r.get(4)?}))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(BookOutline {
            book,
            volumes,
            chapters,
        })
    }
    pub fn propose_edit(&mut self, input: ProposalInput) -> WorkflowResult<Proposal> {
        if input.client_id.trim().is_empty()
            || input.client_id.len() > 128
            || input.idempotency_key.trim().is_empty()
            || input.idempotency_key.len() > 128
            || input.chapter_id.len() > 128
            || input.new_body.len() > BODY_LIMIT
            || input.reason.len() > 4096
            || input.base_revision < 0
            || input.base_revision == i64::MAX
        {
            return Err(failure("invalid_input", "提案字段超限或无效"));
        }
        let payload =
            serde_json::to_string(&input).map_err(|e| failure("invalid_input", &e.to_string()))?;
        let tx = self.conn.transaction()?;
        let current = chapter(&tx, &input.chapter_id)?;
        crate::constrained_writing::legacy_guard(&tx,&input.chapter_id)?;
        if let Some((id, old_payload)) = tx
            .query_row(
                "SELECT id,payload FROM edit_proposals WHERE client_id=?1 AND idempotency_key=?2",
                params![input.client_id, input.idempotency_key],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?
        {
            if old_payload != payload {
                return Err(failure(
                    "idempotency_conflict",
                    "同一幂等键不能用于不同载荷",
                ));
            }
            return proposal(&tx, &id);
        }
        check_revision(current.revision, input.base_revision)?;
        let count: i64 = tx.query_row("SELECT count(*) FROM edit_proposals", [], |r| r.get(0))?;
        if count >= 1000 {
            return Err(failure(
                "limit_exceeded",
                "全部状态的提案总数已达1000；不会自动删除任何内容",
            ));
        }
        let (pending,chapter_pending):(i64,i64)=tx.query_row("SELECT count(*),coalesce(sum(CASE WHEN chapter_id=?1 THEN 1 ELSE 0 END),0) FROM edit_proposals WHERE status='pending'",[&input.chapter_id],|r|Ok((r.get(0)?,r.get(1)?)))?;
        let strict:i64=tx.query_row("SELECT count(*) FROM writing_proposals WHERE decision='pending' AND live_epoch IS NOT NULL",[],|r|r.get(0))?;
        if pending + strict >= 100 || chapter_pending >= 20 {
            return Err(failure(
                "limit_exceeded",
                "待审提案已达全库100条或本章20条上限",
            ));
        }
        let stored_bytes:i64=tx.query_row("SELECT coalesce(sum(length(CAST(old_body AS BLOB))+length(CAST(new_body AS BLOB))),0) FROM edit_proposals",[],|r|r.get(0))?;
        if stored_bytes + current.body.len() as i64 + input.new_body.len() as i64 > 32 * 1024 * 1024
        {
            return Err(failure(
                "limit_exceeded",
                "全部提案正文合计超过32MiB；拒绝或撤回不释放此配额，不影响正常写作",
            ));
        }

        let p = Proposal {
            id: uid(),
            client_id: input.client_id.clone(),
            chapter_id: input.chapter_id,
            base_revision: input.base_revision,
            old_body: current.body,
            new_body: input.new_body,
            reason: input.reason,
            source: input.client_id,
            status: "pending".into(),
            created_at: now()?,
        };
        tx.execute(
            "INSERT INTO edit_proposals VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                p.id,
                p.client_id,
                input.idempotency_key,
                payload,
                p.chapter_id,
                p.base_revision,
                p.old_body,
                p.new_body,
                p.reason,
                p.source,
                p.status,
                p.created_at
            ],
        )?;
        audit(
            &tx,
            Some(&p.chapter_id),
            "propose",
            &p.source,
            Some(p.base_revision),
        )?;
        tx.commit()?;
        Ok(p)
    }
    pub fn list_proposals(&self, book_id: &str) -> WorkflowResult<Vec<Proposal>> {
        // Validate the book without fetching its chapter bodies.
        let deleted = self
            .conn
            .query_row("SELECT deleted_at FROM books WHERE id=?1", [book_id], |r| {
                r.get::<_, Option<i64>>(0)
            })
            .optional()?
            .ok_or_else(|| failure("not_found", "作品不存在"))?;
        if deleted.is_some() {
            return Err(failure("deleted", "作品在回收站"));
        }
        let sql=format!("SELECT {} FROM edit_proposals WHERE chapter_id IN (SELECT c.id FROM chapters c JOIN volumes v ON v.id=c.volume_id WHERE v.book_id=?1 AND v.deleted_at IS NULL AND c.deleted_at IS NULL) ORDER BY created_at,id",PROPOSAL_COLUMNS);
        let result = self
            .conn
            .prepare(&sql)?
            .query_map([book_id], proposal_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(result)
    }
    /// Fetch exactly one proposal after checking client ownership. Book-scope
    /// authorization must still be enforced by the calling transport.
    pub fn get_own_proposal(&self, id: &str, client_id: &str) -> WorkflowResult<Proposal> {
        owned_proposal(&self.conn, id, client_id)
    }
    /// A client may withdraw only its own still-pending proposal. This is not
    /// a human rejection or an approval, and never changes chapter text.
    pub fn cancel_own_proposal(&mut self, id: &str, client_id: &str) -> WorkflowResult<Proposal> {
        let tx = self.conn.transaction()?;
        let mut p = owned_proposal(&tx, id, client_id)?;
        if p.status != "pending" {
            return Err(failure("proposal_decided", "只能撤回待审提案"));
        }
        if tx.execute("UPDATE edit_proposals SET status='cancelled' WHERE id=?1 AND client_id=?2 AND status='pending'",params![id,client_id])?!=1{return Err(failure("proposal_decided","提案已处理"))}
        audit(
            &tx,
            Some(&p.chapter_id),
            "cancel_proposal",
            &p.source,
            Some(p.base_revision),
        )?;
        tx.commit()?;
        p.status = "cancelled".into();
        Ok(p)
    }
    pub fn decide_proposal(
        &mut self,
        id: &str,
        approve: bool,
        expected_revision: i64,
    ) -> WorkflowResult<LibraryState> {
        let tx = self.conn.transaction()?;
        let p = proposal(&tx, id)?;
        if p.status != "pending" {
            return Err(failure("proposal_decided", "提案已处理"));
        }
        let current = chapter(&tx, &p.chapter_id)?;
        if approve {
            crate::constrained_writing::legacy_guard(&tx,&p.chapter_id)?;
            check_revision(current.revision, expected_revision)?;
            check_revision(current.revision, p.base_revision)?;
            save_in_transaction(
                &tx,
                &p.chapter_id,
                &p.new_body,
                expected_revision,
                &format!("proposal:{}", p.id),
            )?;
        }
        let status = if approve { "accepted" } else { "rejected" };
        tx.execute(
            "UPDATE edit_proposals SET status=?1 WHERE id=?2 AND status='pending'",
            params![status, id],
        )?;
        audit(
            &tx,
            Some(&p.chapter_id),
            if approve {
                "accept_proposal"
            } else {
                "reject_proposal"
            },
            "human",
            Some(current.revision + if approve { 1 } else { 0 }),
        )?;
        tx.commit()?;
        self.load().map_err(|e| failure("database_error", &e))
    }
    pub fn chapter_history(&self, chapter_id: &str) -> WorkflowResult<Vec<ChapterVersion>> {
        chapter(&self.conn, chapter_id)?;
        versions(&self.conn, Some(chapter_id))
    }
    pub fn restore_chapter_version(
        &mut self,
        version_id: &str,
        expected_revision: i64,
    ) -> WorkflowResult<LibraryState> {
        let tx = self.conn.transaction()?;
        let v = tx
            .query_row(
                "SELECT chapter_id,body FROM chapter_versions WHERE id=?1",
                [version_id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?
            .ok_or_else(|| failure("not_found", "历史版本不存在"))?;
        save_in_transaction(
            &tx,
            &v.0,
            &v.1,
            expected_revision,
            &format!("history:{version_id}"),
        )?;
        tx.commit()?;
        self.load().map_err(|e| failure("database_error", &e))
    }
}
