//! Book-scoped creation notes. Public writes are for the trusted GUI only;
//! external transports expose metadata/single-card reads after book authorization.
use crate::{LibraryState, Service, WorkflowError, WorkflowResult};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
pub(crate) const CARD_BYTES: usize = 256 * 1024;
const ALL_BYTES: i64 = 32 * 1024 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Material {
    pub id: String,
    pub book_id: String,
    pub kind: String,
    pub title: String,
    pub fields: BTreeMap<String, String>,
    pub volume_id: Option<String>,
    pub chapter_id: Option<String>,
    pub revision: i64,
    pub deleted_at: Option<i64>,
    pub updated_at: i64,
    #[serde(default)]
    pub source_chapter_id: Option<String>,
    #[serde(default)]
    pub source_revision: Option<i64>,
    #[serde(default = "default_source_status")]
    pub source_status: String,
    #[serde(default)]
    pub story_order: i64,
    #[serde(default)]
    pub story_time: Option<String>,
    #[serde(default)]
    pub tracking_status: Option<String>,
    #[serde(default)]
    pub payoff_chapter_id: Option<String>,
    #[serde(default)]
    pub payoff_revision: Option<i64>,
    #[serde(default = "default_source_status")]
    pub payoff_status: String,
    #[serde(default)]
    pub content_review_required: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MaterialSummary {
    pub id: String,
    pub book_id: String,
    pub kind: String,
    pub title: String,
    pub volume_id: Option<String>,
    pub chapter_id: Option<String>,
    pub revision: i64,
    pub deleted_at: Option<i64>,
    pub updated_at: i64,
    #[serde(default)]
    pub source_chapter_id: Option<String>,
    #[serde(default)]
    pub source_revision: Option<i64>,
    #[serde(default = "default_source_status")]
    pub source_status: String,
    #[serde(default)]
    pub story_order: i64,
    #[serde(default)]
    pub story_time: Option<String>,
    #[serde(default)]
    pub tracking_status: Option<String>,
    #[serde(default)]
    pub payoff_chapter_id: Option<String>,
    #[serde(default)]
    pub payoff_revision: Option<i64>,
    #[serde(default = "default_source_status")]
    pub payoff_status: String,
    #[serde(default)]
    pub content_review_required: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateMaterialInput {
    pub book_id: String,
    pub kind: String,
    pub title: String,
    pub fields: BTreeMap<String, String>,
    pub volume_id: Option<String>,
    pub chapter_id: Option<String>,
    #[serde(default)]
    pub source_chapter_id: Option<String>,
    #[serde(default)]
    pub source_revision: Option<i64>,
    #[serde(default)]
    pub story_order: i64,
    #[serde(default)]
    pub story_time: Option<String>,
    #[serde(default)]
    pub tracking_status: Option<String>,
    #[serde(default)]
    pub payoff_chapter_id: Option<String>,
    #[serde(default)]
    pub payoff_revision: Option<i64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveMaterialInput {
    pub id: String,
    pub title: String,
    pub fields: BTreeMap<String, String>,
    pub volume_id: Option<String>,
    pub chapter_id: Option<String>,
    pub expected_revision: i64,
    #[serde(default)]
    pub source_chapter_id: Option<String>,
    #[serde(default)]
    pub source_revision: Option<i64>,
    #[serde(default)]
    pub story_order: i64,
    #[serde(default)]
    pub story_time: Option<String>,
    #[serde(default)]
    pub tracking_status: Option<String>,
    #[serde(default)]
    pub payoff_chapter_id: Option<String>,
    #[serde(default)]
    pub payoff_revision: Option<i64>,
}
fn fail(code: &str, message: &str) -> WorkflowError {
    WorkflowError {
        code: code.into(),
        message: message.into(),
    }
}
fn now() -> WorkflowResult<i64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .map_err(|e| fail("clock_error", &e.to_string()))
}
fn bytes(fields: &BTreeMap<String, String>) -> usize {
    fields.values().map(|s| s.len()).sum()
}
pub(crate) fn shape(
    kind: &str,
    title: &str,
    fields: &BTreeMap<String, String>,
    volume: Option<&str>,
    chapter: Option<&str>,
) -> WorkflowResult<usize> {
    if title.trim().is_empty() || title.len() > 4096 {
        return Err(fail("invalid_input", "资料标题为空或过长"));
    }
    let keys: &[&str] = match kind {
        "setting" => &["synopsis", "genre", "world", "style", "rules"],
        "character" => &[
            "identity",
            "personality",
            "appearance",
            "goal",
            "relationships",
            "status",
            "aliases",
        ],
        "outline" | "summary" | "timeline" => &["content"],
        "document" => &["content"],
        "foreshadow" => &["setup", "resolution"],
        _ => return Err(fail("invalid_input", "未知资料类型")),
    };
    if fields.keys().any(|k| {
        !keys.contains(&k.as_str())
            && !(k == "source_files" && matches!(kind, "setting" | "character" | "outline" | "document"))
    }) {
        return Err(fail("invalid_input", "资料包含未知字段"));
    }
    if volume.is_some() && chapter.is_some()
        || kind != "outline" && (volume.is_some() || chapter.is_some())
    {
        return Err(fail("invalid_input", "仅大纲可关联一个卷或章节"));
    }
    for id in [volume, chapter].into_iter().flatten() {
        if id.is_empty() || id.len() > 128 {
            return Err(fail("invalid_input", "关联ID无效"));
        }
    }
    let size = bytes(fields);
    if size > CARD_BYTES {
        return Err(fail("limit_exceeded", "单卡资料字段超过256KiB"));
    }
    Ok(size)
}
fn active_book(conn: &Connection, id: &str) -> WorkflowResult<()> {
    let deleted = conn
        .query_row("SELECT deleted_at FROM books WHERE id=?1", [id], |r| {
            r.get::<_, Option<i64>>(0)
        })
        .optional()?
        .ok_or_else(|| fail("not_found", "所属作品不存在"))?;
    if deleted.is_some() {
        return Err(fail("deleted", "所属作品在回收站，请先恢复作品"));
    }
    Ok(())
}
fn target(
    conn: &Connection,
    book: &str,
    volume: Option<&str>,
    chapter: Option<&str>,
    allow_deleted: bool,
) -> WorkflowResult<()> {
    if let Some(id) = volume {
        let (scope, deleted) = conn
            .query_row(
                "SELECT book_id,deleted_at FROM volumes WHERE id=?1",
                [id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?)),
            )
            .optional()?
            .ok_or_else(|| fail("not_found", "关联卷不存在"))?;
        if scope != book {
            return Err(fail("scope_mismatch", "不能关联其他作品的卷"));
        }
        if !allow_deleted && deleted.is_some() {
            return Err(fail("deleted", "关联卷在回收站，请先恢复卷"));
        }
    }
    if let Some(id) = chapter {
        let (scope,chapter_deleted,volume_deleted)=conn.query_row("SELECT v.book_id,c.deleted_at,v.deleted_at FROM chapters c JOIN volumes v ON v.id=c.volume_id WHERE c.id=?1",[id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Option<i64>>(1)?,r.get::<_,Option<i64>>(2)?))).optional()?.ok_or_else(||fail("not_found","关联章节不存在"))?;
        if scope != book {
            return Err(fail("scope_mismatch", "不能关联其他作品的章节"));
        }
        if !allow_deleted && (chapter_deleted.is_some() || volume_deleted.is_some()) {
            return Err(fail("deleted", "关联章节或卷在回收站，请先恢复父项"));
        }
    }
    Ok(())
}
const COLUMNS: &str =
    "id,book_id,kind,title,fields,volume_id,chapter_id,revision,deleted_at,updated_at,source_chapter_id,source_revision,story_order,story_time,tracking_status,payoff_chapter_id,payoff_revision,content_review_required";
fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Material> {
    let text: String = r.get(4)?;
    let fields = serde_json::from_str(&text).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e))
    })?;
    Ok(Material {
        id: r.get(0)?,
        book_id: r.get(1)?,
        kind: r.get(2)?,
        title: r.get(3)?,
        fields,
        volume_id: r.get(5)?,
        chapter_id: r.get(6)?,
        revision: r.get(7)?,
        deleted_at: r.get(8)?,
        updated_at: r.get(9)?,
        source_chapter_id: r.get(10)?,
        source_revision: r.get(11)?,
        source_status: default_source_status(),
        story_order: r.get(12)?,
        story_time: r.get(13)?,
        tracking_status: r.get(14)?,
        payoff_chapter_id: r.get(15)?,
        payoff_revision: r.get(16)?,
        payoff_status: default_source_status(),
        content_review_required: r.get(17)?,
    })
}
pub(crate) fn raw(conn: &Connection, id: &str) -> WorkflowResult<Material> {
    let mut m = conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM materials WHERE id=?1"),
            [id],
            row,
        )
        .optional()?
        .ok_or_else(|| fail("not_found", "资料不存在"))?;
    m.source_status = source_status(
        conn,
        &m.book_id,
        m.source_chapter_id.as_deref(),
        m.source_revision,
    )?;
    m.payoff_status = source_status(
        conn,
        &m.book_id,
        m.payoff_chapter_id.as_deref(),
        m.payoff_revision,
    )?;
    review_status(&mut m);
    Ok(m)
}
pub(crate) fn review_status(m: &mut Material) {
    if m.content_review_required {
        if m.source_status == "current" {m.source_status="stale".into();}
        if m.payoff_status == "current" {m.payoff_status="stale".into();}
    }
}
fn default_source_status() -> String {
    "none".into()
}
fn source_pair(kind: &str, id: Option<&str>, revision: Option<i64>) -> WorkflowResult<()> {
    match (id, revision) {
        (None, None) if kind != "summary" => Ok(()),
        (Some(id), Some(rev)) if !id.is_empty() && id.len() <= 128 && rev >= 0 => Ok(()),
        _ => Err(fail(
            "invalid_input",
            "摘要必须记录来源章节与revision，来源字段须成对有效",
        )),
    }
}
fn source_status(
    conn: &Connection,
    book: &str,
    id: Option<&str>,
    revision: Option<i64>,
) -> WorkflowResult<String> {
    let (Some(id), Some(base)) = (id, revision) else {
        return Ok(if id.is_none() && revision.is_none() {
            "none"
        } else {
            "unavailable"
        }
        .into());
    };
    let row=conn.query_row("SELECT v.book_id,c.revision,c.deleted_at,v.deleted_at,b.deleted_at FROM chapters c JOIN volumes v ON v.id=c.volume_id JOIN books b ON b.id=v.book_id WHERE c.id=?1",[id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,Option<i64>>(2)?,r.get::<_,Option<i64>>(3)?,r.get::<_,Option<i64>>(4)?))).optional()?;
    Ok(match row {
        Some((scope, current, None, None, None)) if scope == book => {
            if current == base {
                "current"
            } else {
                "stale"
            }
        }
        _ => "unavailable",
    }
    .into())
}
pub(crate) fn require_current_source(
    conn: &Connection,
    book: &str,
    id: Option<&str>,
    revision: Option<i64>,
) -> WorkflowResult<()> {
    source_pair("setting", id, revision)?;
    if id.is_none() {
        return Ok(());
    }
    match source_status(conn, book, id, revision)?.as_str() {
        "current" => Ok(()),
        "stale" => Err(fail(
            "revision_conflict",
            "来源正文已更新，请重新读取并明确复核来源revision",
        )),
        _ => Err(fail("revision_conflict", "来源不存在、跨书或在回收站")),
    }
}
fn revision(actual: i64, expected: i64) -> WorkflowResult<()> {
    if expected < 0 || expected == i64::MAX {
        return Err(fail("invalid_input", "资料revision无效或溢出"));
    }
    if actual != expected {
        return Err(fail(
            "revision_conflict",
            "资料已更新，请保留草稿并重新读取",
        ));
    }
    Ok(())
}
fn quota(
    conn: &Connection,
    book: &str,
    size: usize,
    old_size: usize,
    creating: bool,
) -> WorkflowResult<()> {
    if creating {
        let count: i64 = conn.query_row(
            "SELECT count(*) FROM materials WHERE book_id=?1",
            [book],
            |r| r.get(0),
        )?;
        if count >= 1000 {
            return Err(fail(
                "limit_exceeded",
                "单书资料已达1000条（含回收项），不会自动删除",
            ));
        }
    }
    let total: i64 = conn.query_row(
        "SELECT coalesce(sum(field_bytes),0) FROM materials",
        [],
        |r| r.get(0),
    )?;
    if total - old_size as i64 + size as i64 > ALL_BYTES {
        return Err(fail(
            "limit_exceeded",
            "全库资料字段已达32MiB，回收不释放配额",
        ));
    }
    Ok(())
}
pub(crate) fn migrate(conn: &Connection) -> WorkflowResult<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS materials(id TEXT PRIMARY KEY,book_id TEXT NOT NULL REFERENCES books(id) ON DELETE CASCADE,kind TEXT NOT NULL,title TEXT NOT NULL,fields TEXT NOT NULL,volume_id TEXT REFERENCES volumes(id) ON DELETE SET NULL,chapter_id TEXT REFERENCES chapters(id) ON DELETE SET NULL,revision INTEGER NOT NULL,deleted_at INTEGER,updated_at INTEGER NOT NULL,field_bytes INTEGER NOT NULL,source_chapter_id TEXT,source_revision INTEGER,story_order INTEGER NOT NULL DEFAULT 0,story_time TEXT,tracking_status TEXT,payoff_chapter_id TEXT,payoff_revision INTEGER);CREATE INDEX IF NOT EXISTS materials_book_id ON materials(book_id,id);")?;
    let columns = tx
        .prepare("PRAGMA table_info(materials)")?
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (column, sql) in [
        (
            "source_chapter_id",
            "ALTER TABLE materials ADD COLUMN source_chapter_id TEXT",
        ),
        (
            "source_revision",
            "ALTER TABLE materials ADD COLUMN source_revision INTEGER",
        ),
    ] {
        if !columns.iter().any(|c| c == column) {
            tx.execute_batch(sql)?;
        }
    }
    for (column, sql) in [
        (
            "story_order",
            "ALTER TABLE materials ADD COLUMN story_order INTEGER NOT NULL DEFAULT 0",
        ),
        (
            "story_time",
            "ALTER TABLE materials ADD COLUMN story_time TEXT",
        ),
        (
            "tracking_status",
            "ALTER TABLE materials ADD COLUMN tracking_status TEXT",
        ),
        (
            "payoff_chapter_id",
            "ALTER TABLE materials ADD COLUMN payoff_chapter_id TEXT",
        ),
        (
            "payoff_revision",
            "ALTER TABLE materials ADD COLUMN payoff_revision INTEGER",
        ),
    ] {
        if !columns.iter().any(|c| c == column) {
            tx.execute_batch(sql)?;
        }
    }
    if !columns.iter().any(|c| c == "content_review_required") { tx.execute_batch("ALTER TABLE materials ADD COLUMN content_review_required INTEGER NOT NULL DEFAULT 0 CHECK(content_review_required IN (0,1))")?; }
    crate::material_history::migrate(&tx)?;
    tx.commit()?;
    Ok(())
}
fn insert(tx: &Transaction, m: &Material) -> WorkflowResult<()> {
    let text =
        serde_json::to_string(&m.fields).map_err(|e| fail("invalid_input", &e.to_string()))?;
    tx.execute(
        "INSERT INTO materials(id,book_id,kind,title,fields,volume_id,chapter_id,revision,deleted_at,updated_at,field_bytes,source_chapter_id,source_revision,story_order,story_time,tracking_status,payoff_chapter_id,payoff_revision,content_review_required) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
        params![
            m.id,
            m.book_id,
            m.kind,
            m.title,
            text,
            m.volume_id,
            m.chapter_id,
            m.revision,
            m.deleted_at,
            m.updated_at,
            bytes(&m.fields) as i64,m.source_chapter_id,m.source_revision,m.story_order,m.story_time,m.tracking_status,m.payoff_chapter_id,m.payoff_revision,m.content_review_required
        ],
    )?;
    Ok(())
}
pub(crate) fn all(conn: &Connection) -> crate::Result<Vec<Material>> {
    let mut items = conn
        .prepare(&format!("SELECT {COLUMNS} FROM materials ORDER BY id"))
        .map_err(|e| e.to_string())?
        .query_map([], row)
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    for m in &mut items {
        m.source_status = source_status(
            conn,
            &m.book_id,
            m.source_chapter_id.as_deref(),
            m.source_revision,
        )
        .map_err(|e| e.to_string())?;
        m.payoff_status = source_status(
            conn,
            &m.book_id,
            m.payoff_chapter_id.as_deref(),
            m.payoff_revision,
        )
        .map_err(|e| e.to_string())?;
        review_status(m);
    }
    Ok(items)
}
pub(crate) fn insert_all(tx: &Transaction, items: &[Material]) -> crate::Result<()> {
    for m in items {
        insert(tx, m).map_err(|e| e.to_string())?;
    }
    Ok(())
}
pub(crate) fn validate(state: &LibraryState, items: &[Material]) -> crate::Result<()> {
    let books: HashSet<_> = state.books.iter().map(|b| b.id.as_str()).collect();
    let volumes: HashMap<_, _> = state
        .volumes
        .iter()
        .map(|v| (v.id.as_str(), v.book_id.as_str()))
        .collect();
    let chapters: HashMap<_, _> = state
        .chapters
        .iter()
        .map(|c| (c.id.as_str(), c.volume_id.as_str()))
        .collect();
    let chapter_revisions: HashMap<&str, i64> = state
        .chapters
        .iter()
        .map(|c| (c.id.as_str(), c.revision))
        .collect();
    let mut ids: HashSet<&str> = state
        .books
        .iter()
        .map(|b| b.id.as_str())
        .chain(state.volumes.iter().map(|v| v.id.as_str()))
        .chain(state.chapters.iter().map(|c| c.id.as_str()))
        .collect();
    let mut counts = HashMap::new();
    let mut total = 0usize;
    for m in items {
        if m.id.is_empty()
            || m.id.len() > 128
            || !ids.insert(m.id.as_str())
            || !books.contains(m.book_id.as_str())
            || m.revision < 0
            || m.updated_at < 0
            || m.deleted_at.is_some_and(|d| d < 0)
        {
            return Err("资料ID、作品引用、revision或时间无效".into());
        }
        let size = shape(
            &m.kind,
            &m.title,
            &m.fields,
            m.volume_id.as_deref(),
            m.chapter_id.as_deref(),
        )
        .map_err(|e| e.to_string())?;
        if let Some(v) = m.volume_id.as_deref() {
            if volumes.get(v).copied() != Some(m.book_id.as_str()) {
                return Err("资料卷关联不存在或跨书".into());
            }
        }
        if let Some(c) = m.chapter_id.as_deref() {
            if chapters.get(c).and_then(|v| volumes.get(v)).copied() != Some(m.book_id.as_str()) {
                return Err("资料章节关联不存在或跨书".into());
            }
        }
        tracking_shape(m).map_err(|e| e.to_string())?;
        source_pair(&m.kind, m.source_chapter_id.as_deref(), m.source_revision)
            .map_err(|e| e.to_string())?;
        for (id, base) in [
            (m.source_chapter_id.as_deref(), m.source_revision),
            (m.payoff_chapter_id.as_deref(), m.payoff_revision),
        ] {
            if let (Some(source), Some(revision)) = (id, base) {
                if let Some(volume) = chapters.get(source) {
                    let current = *chapter_revisions.get(source).ok_or("来源不存在")?;
                    if volumes.get(volume).copied() != Some(m.book_id.as_str())
                        || revision > current
                    {
                        return Err("来源跨书或revision超前".into());
                    }
                }
            }
        }
        let count = counts.entry(&m.book_id).or_insert(0usize);
        *count += 1;
        if *count > 1000 {
            return Err("备份单书资料超过1000条".into());
        }
        total += size;
        if total > ALL_BYTES as usize {
            return Err("备份资料字段超过32MiB".into());
        }
    }
    Ok(())
}
/// Called inside the same tree-write transaction, before deleting target rows.
/// Detachment is an observable card edit so stale drafts cannot silently save.
pub(crate) fn detach_removed_targets(tx: &Transaction, state: &LibraryState) -> crate::Result<()> {
    let books: HashSet<&str> = state.books.iter().map(|b| b.id.as_str()).collect();
    let volumes: HashSet<&str> = state.volumes.iter().map(|v| v.id.as_str()).collect();
    let chapters: HashSet<&str> = state.chapters.iter().map(|c| c.id.as_str()).collect();
    let affected=tx.prepare("SELECT id,book_id,volume_id,chapter_id,revision,updated_at FROM materials WHERE volume_id IS NOT NULL OR chapter_id IS NOT NULL").map_err(|e|e.to_string())?.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,Option<String>>(3)?,r.get::<_,i64>(4)?,r.get::<_,i64>(5)?))).map_err(|e|e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e|e.to_string())?;
    for (id, book, volume, chapter, revision, updated) in affected {
        if !books.contains(book.as_str()) {
            continue;
        }
        if volume.as_deref().is_some_and(|v| !volumes.contains(v))
            || chapter.as_deref().is_some_and(|c| !chapters.contains(c))
        {
            let next = revision
                .checked_add(1)
                .ok_or("资料revision溢出，拒绝删除关联目标")?;
            let updated = next_time(updated).map_err(|e| e.to_string())?;
            if tx.execute("UPDATE materials SET volume_id=NULL,chapter_id=NULL,revision=?1,updated_at=?2 WHERE id=?3 AND revision=?4",params![next,updated,id,revision]).map_err(|e|e.to_string())?!=1{return Err("资料版本冲突，未删除关联目标".into())}
        }
    }
    Ok(())
}
fn next_time(old: i64) -> WorkflowResult<i64> {
    Ok(now()?.max(
        old.checked_add(1)
            .ok_or_else(|| fail("invalid_input", "资料更新时间溢出"))?,
    ))
}
impl Service {
    pub fn list_materials(
        &self,
        book_id: &str,
        include_deleted: bool,
    ) -> WorkflowResult<Vec<MaterialSummary>> {
        metadata_in(&self.conn, book_id, include_deleted, None)
    }
    pub fn get_material(&self, id: &str, include_deleted: bool) -> WorkflowResult<Material> {
        get_material_in(&self.conn, id, include_deleted)
    }
    pub fn create_material(&mut self, input: CreateMaterialInput) -> WorkflowResult<Material> {
        let tx = self.conn.transaction()?;
        let material = Self::create_material_in_transaction(&tx, input)?;
        tx.commit()?;
        Ok(material)
    }
    pub(crate) fn create_material_in_transaction(
        tx: &Transaction<'_>,
        input: CreateMaterialInput,
    ) -> WorkflowResult<Material> {
        let size = shape(
            &input.kind,
            &input.title,
            &input.fields,
            input.volume_id.as_deref(),
            input.chapter_id.as_deref(),
        )?;
        active_book(tx, &input.book_id)?;
        target(
            tx,
            &input.book_id,
            input.volume_id.as_deref(),
            input.chapter_id.as_deref(),
            false,
        )?;
        source_pair(
            &input.kind,
            input.source_chapter_id.as_deref(),
            input.source_revision,
        )?;
        require_current_source(
            tx,
            &input.book_id,
            input.source_chapter_id.as_deref(),
            input.source_revision,
        )?;
        quota(tx, &input.book_id, size, 0, true)?;
        let m = Material {
            id: uuid::Uuid::new_v4().to_string(),
            book_id: input.book_id,
            kind: input.kind,
            title: input.title,
            fields: input.fields,
            volume_id: input.volume_id,
            chapter_id: input.chapter_id,
            revision: 0,
            deleted_at: None,
            updated_at: now()?,
            source_chapter_id: input.source_chapter_id,
            source_revision: input.source_revision,
            source_status: default_source_status(),
            story_order: input.story_order,
            story_time: input.story_time,
            tracking_status: input.tracking_status,
            payoff_chapter_id: input.payoff_chapter_id,
            payoff_revision: input.payoff_revision,
            payoff_status: default_source_status(),
            content_review_required: false,
        };
        tracking_shape(&m)?;
        require_current_source(
            tx,
            &m.book_id,
            m.payoff_chapter_id.as_deref(),
            m.payoff_revision,
        )?;
        require_state_sources(tx, &m)?;
        insert(tx, &m)?;
        crate::material_history::snapshot(tx, &m, "create")?;
        let mut m = m;
        m.source_status = source_status(
            tx,
            &m.book_id,
            m.source_chapter_id.as_deref(),
            m.source_revision,
        )?;
        m.payoff_status = source_status(
            tx,
            &m.book_id,
            m.payoff_chapter_id.as_deref(),
            m.payoff_revision,
        )?;
        Ok(m)
    }
    pub fn save_material(&mut self, input: SaveMaterialInput) -> WorkflowResult<Material> {
        let tx = self.conn.transaction()?;
        let m = save_material_in_transaction(&tx, input)?;
        tx.commit()?;
        Ok(m)
    }
    pub fn trash_material(&mut self, id: &str, expected_revision: i64) -> WorkflowResult<Material> {
        self.material_recycle(id, expected_revision, false)
    }
    pub fn restore_material(
        &mut self,
        id: &str,
        expected_revision: i64,
    ) -> WorkflowResult<Material> {
        self.material_recycle(id, expected_revision, true)
    }
    fn material_recycle(
        &mut self,
        id: &str,
        expected: i64,
        restore: bool,
    ) -> WorkflowResult<Material> {
        let tx = self.conn.transaction()?;
        let mut m = raw(&tx, id)?;
        active_book(&tx, &m.book_id)?;
        revision(m.revision, expected)?;
        target(
            &tx,
            &m.book_id,
            m.volume_id.as_deref(),
            m.chapter_id.as_deref(),
            false,
        )?;
        if restore && m.deleted_at.is_none() {
            return Err(fail("invalid_state", "资料不在回收站"));
        }
        if !restore && m.deleted_at.is_some() {
            return Err(fail("deleted", "资料已在回收站"));
        }
        m.updated_at = next_time(m.updated_at)?;
        m.deleted_at = if restore { None } else { Some(m.updated_at) };
        m.revision += 1;
        if tx.execute("UPDATE materials SET deleted_at=?1,revision=?2,updated_at=?3 WHERE id=?4 AND revision=?5",params![m.deleted_at,m.revision,m.updated_at,id,expected])?!=1{return Err(fail("revision_conflict","资料已更新"))}
        tx.commit()?;
        Ok(m)
    }
}

pub(crate) fn get_material_in(conn: &Connection, id: &str, include_deleted: bool) -> WorkflowResult<Material> {
    let m = raw(conn, id)?;
    active_book(conn, &m.book_id)?;
    if !include_deleted && m.deleted_at.is_some() {
        return Err(fail("deleted", "资料在回收站"));
    }
    target(
        conn,
        &m.book_id,
        m.volume_id.as_deref(),
        m.chapter_id.as_deref(),
        include_deleted,
    )?;
    Ok(m)
}
pub(crate) fn get_material_in_transaction(
    tx: &Transaction,
    id: &str,
    include_deleted: bool,
) -> WorkflowResult<Material> {
    get_material_in(tx, id, include_deleted)
}
fn prepare_edit(tx: &Transaction, input: &SaveMaterialInput) -> WorkflowResult<(Material, usize)> {
    let mut m = get_material_in(tx, &input.id, false)?;
    revision(m.revision, input.expected_revision)?;
    let size = shape(
        &m.kind,
        &input.title,
        &input.fields,
        input.volume_id.as_deref(),
        input.chapter_id.as_deref(),
    )?;
    target(
        tx,
        &m.book_id,
        input.volume_id.as_deref(),
        input.chapter_id.as_deref(),
        false,
    )?;
    source_pair(
        &m.kind,
        input.source_chapter_id.as_deref(),
        input.source_revision,
    )?;
    if input.source_chapter_id != m.source_chapter_id || input.source_revision != m.source_revision
    {
        require_current_source(
            tx,
            &m.book_id,
            input.source_chapter_id.as_deref(),
            input.source_revision,
        )?;
    }
    quota(tx, &m.book_id, size, bytes(&m.fields), false)?;
    let previous_state = m.tracking_status.clone();
    if input.payoff_chapter_id != m.payoff_chapter_id || input.payoff_revision != m.payoff_revision
    {
        require_current_source(
            tx,
            &m.book_id,
            input.payoff_chapter_id.as_deref(),
            input.payoff_revision,
        )?;
    }
    m.title = input.title.clone();
    m.fields = input.fields.clone();
    m.volume_id = input.volume_id.clone();
    m.chapter_id = input.chapter_id.clone();
    m.source_chapter_id = input.source_chapter_id.clone();
    m.source_revision = input.source_revision;
    m.source_status = source_status(
        tx,
        &m.book_id,
        m.source_chapter_id.as_deref(),
        m.source_revision,
    )?;
    m.story_order = input.story_order;
    m.story_time = input.story_time.clone();
    m.tracking_status = input.tracking_status.clone();
    m.payoff_chapter_id = input.payoff_chapter_id.clone();
    m.payoff_revision = input.payoff_revision;
    tracking_shape(&m)?;
    if previous_state != m.tracking_status {
        require_state_sources(tx, &m)?;
    }
    m.payoff_status = source_status(
        tx,
        &m.book_id,
        m.payoff_chapter_id.as_deref(),
        m.payoff_revision,
    )?;
    m.revision += 1;
    m.updated_at = next_time(m.updated_at)?;
    review_status(&mut m);
    Ok((m, size))
}
pub(crate) fn validate_material_edit_in_transaction(
    tx: &Transaction,
    input: &SaveMaterialInput,
) -> WorkflowResult<()> {
    prepare_edit(tx, input).map(|_| ())
}
pub(crate) fn save_material_in_transaction(
    tx: &Transaction,
    input: SaveMaterialInput,
) -> WorkflowResult<Material> {
    let old = get_material_in(tx,&input.id,false)?;
    let (mut m, size) = prepare_edit(tx, &input)?;
    let content_changed=old.title!=m.title || old.fields!=m.fields;
    if content_changed { crate::material_history::snapshot(tx,&old,"baseline")?; }
    let text =
        serde_json::to_string(&m.fields).map_err(|e| fail("invalid_input", &e.to_string()))?;
    if tx.execute("UPDATE materials SET title=?1,fields=?2,volume_id=?3,chapter_id=?4,revision=?5,updated_at=?6,field_bytes=?7,source_chapter_id=?8,source_revision=?9,story_order=?10,story_time=?11,tracking_status=?12,payoff_chapter_id=?13,payoff_revision=?14 WHERE id=?15 AND revision=?16 AND deleted_at IS NULL",params![m.title,text,m.volume_id,m.chapter_id,m.revision,m.updated_at,size as i64,m.source_chapter_id,m.source_revision,m.story_order,m.story_time,m.tracking_status,m.payoff_chapter_id,m.payoff_revision,m.id,input.expected_revision])?!=1{return Err(fail("revision_conflict","资料已更新"))}
    if content_changed { crate::material_history::snapshot(tx,&m,"save")?; }
    review_status(&mut m);
    Ok(m)
}
pub(crate) use get_material_in_transaction as get_material_in_tx;
pub(crate) use save_material_in_transaction as save_material_in_tx;
pub(crate) use validate_material_edit_in_transaction as validate_material_edit_in_tx;

fn tracking_shape(m: &Material) -> WorkflowResult<()> {
    if !(0..=1_000_000_000).contains(&m.story_order)
        || m.story_time.as_ref().is_some_and(|s| s.len() > 256)
    {
        return Err(fail(
            "invalid_input",
            "故事顺序须0..1e9，故事时间标签限256字节",
        ));
    }
    source_pair("setting", m.payoff_chapter_id.as_deref(), m.payoff_revision)?;
    let required = match m.kind.as_str() {
        "timeline" => {
            if m.payoff_chapter_id.is_some() || m.payoff_revision.is_some() {
                return Err(fail("invalid_input", "时间线不能带伏笔回收章节"));
            }
            match m.tracking_status.as_deref() {
                Some("planned") => false,
                Some("written") => true,
                _ => return Err(fail("invalid_input", "时间线状态须planned/written")),
            }
        }
        "foreshadow" => match m.tracking_status.as_deref() {
            Some("planned" | "abandoned") => false,
            Some("planted") => true,
            Some("resolved") => {
                if m.payoff_chapter_id.is_none()
                    || m.fields
                        .get("resolution")
                        .is_none_or(|s| s.trim().is_empty())
                {
                    return Err(fail(
                        "invalid_input",
                        "已回收伏笔须回收章节基准及非空回收说明",
                    ));
                }
                true
            }
            _ => {
                return Err(fail(
                    "invalid_input",
                    "伏笔状态须planned/planted/resolved/abandoned",
                ))
            }
        },
        _ => {
            if m.story_order != 0
                || m.story_time.is_some()
                || m.tracking_status.is_some()
                || m.payoff_chapter_id.is_some()
                || m.payoff_revision.is_some()
            {
                return Err(fail(
                    "invalid_input",
                    "普通资料不能携带时间线/伏笔跟踪元数据",
                ));
            }
            false
        }
    };
    if required && (m.source_chapter_id.is_none() || m.source_revision.is_none()) {
        return Err(fail(
            "invalid_input",
            "已写事件或已铺设伏笔须明确发生/铺设章节基准",
        ));
    }
    Ok(())
}
fn require_state_sources(conn: &Connection, m: &Material) -> WorkflowResult<()> {
    if matches!(
        (m.kind.as_str(), m.tracking_status.as_deref()),
        ("timeline", Some("written")) | ("foreshadow", Some("planted" | "resolved"))
    ) {
        require_current_source(
            conn,
            &m.book_id,
            m.source_chapter_id.as_deref(),
            m.source_revision,
        )?;
    }
    if m.kind == "foreshadow" && m.tracking_status.as_deref() == Some("resolved") {
        require_current_source(
            conn,
            &m.book_id,
            m.payoff_chapter_id.as_deref(),
            m.payoff_revision,
        )?;
    }
    Ok(())
}

pub(crate) fn metadata_in(
    conn: &Connection,
    book_id: &str,
    include_deleted: bool,
    id: Option<&str>,
) -> WorkflowResult<Vec<MaterialSummary>> {
    active_book(conn, book_id)?;
    let query="SELECT m.id,m.book_id,m.kind,m.title,m.volume_id,m.chapter_id,m.revision,m.deleted_at,m.updated_at,m.source_chapter_id,m.source_revision,CASE WHEN m.source_chapter_id IS NULL AND m.source_revision IS NULL THEN 'none' WHEN m.source_chapter_id IS NULL OR m.source_revision IS NULL OR sc.id IS NULL OR sv.id IS NULL OR sb.id IS NULL OR sv.book_id!=m.book_id OR sc.deleted_at IS NOT NULL OR sv.deleted_at IS NOT NULL OR sb.deleted_at IS NOT NULL THEN 'unavailable' WHEN sc.revision=m.source_revision AND m.content_review_required=0 THEN 'current' ELSE 'stale' END,m.story_order,m.story_time,m.tracking_status,m.payoff_chapter_id,m.payoff_revision,CASE WHEN m.payoff_chapter_id IS NULL AND m.payoff_revision IS NULL THEN 'none' WHEN m.payoff_chapter_id IS NULL OR m.payoff_revision IS NULL OR pc.id IS NULL OR pv.id IS NULL OR pb.id IS NULL OR pv.book_id!=m.book_id OR pc.deleted_at IS NOT NULL OR pv.deleted_at IS NOT NULL OR pb.deleted_at IS NOT NULL THEN 'unavailable' WHEN pc.revision=m.payoff_revision AND m.content_review_required=0 THEN 'current' ELSE 'stale' END,m.content_review_required FROM materials m LEFT JOIN chapters sc ON sc.id=m.source_chapter_id LEFT JOIN volumes sv ON sv.id=sc.volume_id LEFT JOIN books sb ON sb.id=sv.book_id LEFT JOIN chapters pc ON pc.id=m.payoff_chapter_id LEFT JOIN volumes pv ON pv.id=pc.volume_id LEFT JOIN books pb ON pb.id=pv.book_id WHERE m.book_id=?1 AND (?2 OR (m.deleted_at IS NULL AND (m.volume_id IS NULL OR EXISTS(SELECT 1 FROM volumes v WHERE v.id=m.volume_id AND v.book_id=m.book_id AND v.deleted_at IS NULL)) AND (m.chapter_id IS NULL OR EXISTS(SELECT 1 FROM chapters c JOIN volumes v ON v.id=c.volume_id WHERE c.id=m.chapter_id AND v.book_id=m.book_id AND c.deleted_at IS NULL AND v.deleted_at IS NULL)))) ORDER BY m.id";
    let query = if id.is_some() {
        query.replace("WHERE m.book_id=?1", "WHERE m.id=?3 AND m.book_id=?1")
    } else {
        query.replace("WHERE m.book_id=?1", "WHERE ?3 IS NULL AND m.book_id=?1")
    };
    let rows = conn
        .prepare(&query)?
        .query_map(params![book_id, include_deleted, id], |r| {
            Ok(MaterialSummary {
                id: r.get(0)?,
                book_id: r.get(1)?,
                kind: r.get(2)?,
                title: r.get(3)?,
                volume_id: r.get(4)?,
                chapter_id: r.get(5)?,
                revision: r.get(6)?,
                deleted_at: r.get(7)?,
                updated_at: r.get(8)?,
                source_chapter_id: r.get(9)?,
                source_revision: r.get(10)?,
                source_status: r.get(11)?,
                story_order: r.get(12)?,
                story_time: r.get(13)?,
                tracking_status: r.get(14)?,
                payoff_chapter_id: r.get(15)?,
                payoff_revision: r.get(16)?,
                payoff_status: r.get(17)?,
                content_review_required: r.get(18)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
