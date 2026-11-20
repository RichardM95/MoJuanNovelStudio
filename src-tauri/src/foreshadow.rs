//! Author-recorded chapter excerpts. A source position is frozen evidence, not
//! a pointer that may be silently updated or used to derive story semantics.
use crate::{
    Chapter, LibraryState, Material, MaterialSummary, Service, WorkflowError, WorkflowResult,
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

const QUOTE: usize = 4096;
const NOTE: usize = 1024;
const ROW: usize = 32 * 1024;
const EVIDENCE: usize = 32 * 1024 * 1024;
const COUNT: usize = 10_000;
const PER_CARD: usize = 32;
const BODY: usize = 10 * 1024 * 1024;
const DIRECTORY: usize = 16 * 1024 * 1024;
// MCP encodes the JSON result again as tool text. A raw page under this
// bound fits the 2MiB wire envelope even if every byte were escaped twice.
const PAGE_JSON: usize = 900 * 1024;
const MAX_REV: i64 = 9_007_199_254_740_000;
const SCHEMA: &str = "CREATE TABLE foreshadow_occurrences(id TEXT PRIMARY KEY NOT NULL,book_id TEXT NOT NULL REFERENCES books(id) ON DELETE CASCADE,material_id TEXT NOT NULL REFERENCES materials(id) ON DELETE CASCADE,request_epoch TEXT NOT NULL,request_key TEXT NOT NULL,revision INTEGER NOT NULL,deleted_at INTEGER,record TEXT NOT NULL,byte_count INTEGER NOT NULL,UNIQUE(book_id,request_epoch,request_key))";
const ALIASES: &str = "id,book_id,material_id,request_epoch,request_key,revision,deleted_at,byte_count,CASE WHEN typeof(record)='text' AND length(CAST(record AS BLOB))<=32768 THEN record END";
const ACTIVE_DIRECTORY: &str = "FROM chapters c JOIN volumes v ON v.id=c.volume_id JOIN books b ON b.id=v.book_id WHERE b.id=?1 AND b.deleted_at IS NULL AND v.deleted_at IS NULL AND c.deleted_at IS NULL";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForeshadowRole {
    Plant,
    Hint,
    Payoff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForeshadowSourceStatus {
    Current,
    Stale,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForeshadowPosition {
    pub chapter_id: String,
    pub chapter_title: String,
    pub chapter_revision: i64,
    pub chapter_sort_order: i64,
    pub volume_id: String,
    pub volume_title: String,
    pub volume_sort_order: i64,
}
impl ForeshadowPosition {
    fn key(&self) -> (i64, &str, i64, &str) {
        (
            self.volume_sort_order,
            &self.volume_id,
            self.chapter_sort_order,
            &self.chapter_id,
        )
    }
}

fn explicit_option<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
    Option::deserialize(d)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForeshadowOccurrence {
    pub id: String,
    pub book_id: String,
    pub material_id: String,
    pub source: ForeshadowPosition,
    pub start: u32,
    pub end: u32,
    pub quote: String,
    pub role: ForeshadowRole,
    pub note: String,
    pub revision: i64,
    #[serde(deserialize_with = "explicit_option")]
    pub deleted_at: Option<i64>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StoredOccurrence {
    pub(crate) occurrence: ForeshadowOccurrence,
    pub(crate) request_epoch: String,
    pub(crate) request_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateForeshadowOccurrence {
    pub book_id: String,
    pub material_id: String,
    pub chapter_id: String,
    pub chapter_revision: i64,
    pub start: u32,
    pub end: u32,
    pub quote: String,
    pub role: ForeshadowRole,
    pub note: String,
    pub epoch: String,
    pub request_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ForeshadowOccurrenceView {
    pub occurrence: ForeshadowOccurrence,
    pub source_status: ForeshadowSourceStatus,
    pub current: Option<ForeshadowPosition>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForeshadowSummary {
    pub material: MaterialSummary,
    pub occurrence_count: usize,
    pub review_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForeshadowOccurrencePage {
    pub items: Vec<ForeshadowOccurrenceView>,
    pub next_after_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForeshadowSourceWindow {
    pub position: ForeshadowPosition,
    pub before: String,
    pub at_range: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForeshadowChapterExcerpt {
    pub position: ForeshadowPosition,
    pub body: String,
}

pub(crate) fn fail(code: &str, message: &str) -> WorkflowError {
    WorkflowError {
        code: code.into(),
        message: message.into(),
    }
}
pub(crate) fn invalid(message: &str) -> WorkflowError {
    fail("invalid_input", message)
}
fn absent() -> WorkflowError {
    fail("not_found", "当前作品中没有此活动伏笔、记录或来源章节")
}
fn limit() -> WorkflowError {
    fail(
        "limit_exceeded",
        "伏笔出现记录超过单条32KiB、每卡32条、全库10000条或32MiB上限（含回收）",
    )
}
pub(crate) fn identity(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control)
}
fn title(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 4096
}
pub(crate) fn valid_revision(r: i64) -> bool {
    (0..MAX_REV).contains(&r)
}
pub(crate) fn time() -> WorkflowResult<i64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .map_err(|_| fail("clock_error", "系统时间无效"))
}
pub(crate) fn check_epoch(c: &Connection, epoch: &str) -> WorkflowResult<()> {
    let current: String = c.query_row(
        "SELECT value FROM workflow_meta WHERE key='epoch'",
        [],
        |r| r.get(0),
    )?;
    if epoch != current {
        return Err(fail(
            "epoch_changed",
            "书库已更换，请关闭并重新打开全书伏笔",
        ));
    }
    Ok(())
}
pub(crate) fn active_book(c: &Connection, book: &str) -> WorkflowResult<()> {
    c.query_row(
        "SELECT 1 FROM books WHERE id=?1 AND deleted_at IS NULL",
        [book],
        |_| Ok(()),
    )
    .optional()?
    .ok_or_else(absent)
}
pub(crate) fn active_card(
    c: &Connection,
    book: &str,
    material: &str,
) -> WorkflowResult<(i64, i64)> {
    c.query_row("SELECT m.revision,m.updated_at FROM materials m JOIN books b ON b.id=m.book_id WHERE b.deleted_at IS NULL AND m.deleted_at IS NULL AND m.kind='foreshadow' AND m.book_id=?1 AND m.id=?2",
        params![book, material], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?.ok_or_else(absent)
}
fn bump_card(
    c: &Connection,
    book: &str,
    card: &str,
    prior: (i64, i64),
    timestamp: i64,
) -> WorkflowResult<()> {
    let (revision, updated) = prior;
    let next = revision
        .checked_add(1)
        .filter(|v| valid_revision(*v))
        .ok_or_else(|| invalid("资料版本已达上限"))?;
    let updated = updated
        .checked_add(1)
        .ok_or_else(|| invalid("资料时间已达上限"))?
        .max(timestamp);
    if c.execute(
        "UPDATE materials SET revision=?1,updated_at=?2 WHERE id=?3 AND book_id=?4 AND revision=?5",
        params![next, updated, card, book, revision],
    )? != 1
    {
        return Err(fail("revision_conflict", "伏笔资料已改变"));
    }
    Ok(())
}

pub(crate) fn table_present(c: &Connection) -> WorkflowResult<bool> {
    let object: Option<(String, Option<String>)> = c
        .query_row(
            "SELECT type,sql FROM sqlite_schema WHERE name='foreshadow_occurrences'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match object {
        None => Ok(false),
        Some((kind, Some(sql))) if kind == "table" && sql == SCHEMA => Ok(true),
        _ => Err(invalid("伏笔出现表schema不匹配")),
    }
}
pub(crate) fn schema_present(c: &Connection) -> WorkflowResult<bool> {
    Ok(crate::foreshadow_proposals::schema_generation(c)? >= 1)
}

pub(crate) fn create_schema(c: &Connection) -> WorkflowResult<()> {
    c.execute_batch(SCHEMA)?;
    Ok(())
}

fn position_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ForeshadowPosition> {
    Ok(ForeshadowPosition {
        chapter_id: r.get(0)?,
        chapter_title: r.get(1)?,
        chapter_revision: r.get(2)?,
        chapter_sort_order: r.get(3)?,
        volume_id: r.get(4)?,
        volume_title: r.get(5)?,
        volume_sort_order: r.get(6)?,
    })
}
pub(crate) fn position(
    c: &Connection,
    book: &str,
    chapter: &str,
) -> WorkflowResult<Option<ForeshadowPosition>> {
    Ok(c.query_row(&format!(
        "SELECT c.id,c.title,c.revision,c.sort_order,v.id,v.title,v.sort_order {ACTIVE_DIRECTORY} AND c.id=?2"
    ), params![book, chapter], position_row).optional()?)
}
pub(crate) fn body(c: &Connection, chapter: &str) -> WorkflowResult<String> {
    let out = c.query_row(
        "SELECT CASE WHEN typeof(body)='text' AND length(CAST(body AS BLOB))<=?2 THEN body END FROM chapters WHERE id=?1",
        params![chapter, BODY], |r| r.get::<_, Option<String>>(0))?;
    out.ok_or_else(|| fail("limit_exceeded", "章节正文超10MiB或不是文本，未读取"))
}
pub(crate) fn quote_matches(body: &str, start: u32, end: u32, quote: &str) -> bool {
    if start >= end
        || quote.is_empty()
        || end as usize > BODY
        || quote.chars().count() != (end - start) as usize
    {
        return false;
    }
    let mut chars = body.chars().skip(start as usize);
    quote.chars().all(|q| chars.next() == Some(q))
}
pub(crate) fn valid_occurrence(o: &ForeshadowOccurrence) -> bool {
    let s = &o.source;
    [
        o.id.as_str(),
        &o.book_id,
        &o.material_id,
        &s.chapter_id,
        &s.volume_id,
    ]
    .into_iter()
    .all(identity)
        && title(&s.chapter_title)
        && title(&s.volume_title)
        && s.chapter_sort_order >= 0
        && s.volume_sort_order >= 0
        && valid_revision(s.chapter_revision)
        && valid_revision(o.revision)
        && o.created_at >= 0
        && o.revision % 2 == i64::from(o.deleted_at.is_some())
        && o.deleted_at
            .is_none_or(|d| o.revision > 0 && d >= o.created_at)
        && o.start < o.end
        && (o.end as usize) <= BODY
        && !o.quote.is_empty()
        && o.quote.len() <= QUOTE
        && o.note.len() <= NOTE
        && o.quote.chars().count() == (o.end - o.start) as usize
}
pub(crate) fn validate_frozen_occurrence(value: &ForeshadowOccurrence) -> WorkflowResult<()> {
    if !valid_occurrence(value) {
        return Err(invalid("冻结伏笔出现格式无效"));
    }
    let bytes = serde_json::to_vec(value).map_err(|_| invalid("冻结伏笔出现无法编码"))?;
    if bytes.len() > ROW {
        return Err(limit());
    }
    Ok(())
}

fn encode(row: &StoredOccurrence) -> WorkflowResult<String> {
    if !valid_occurrence(&row.occurrence)
        || !identity(&row.request_epoch)
        || !(identity(&row.request_key)
            || crate::foreshadow_proposals::approval_from_key(&row.request_key).is_some())
    {
        return Err(invalid("伏笔出现记录格式无效"));
    }
    let raw = serde_json::to_string(row).map_err(|_| invalid("伏笔出现记录无法编码"))?;
    if raw.len() > ROW {
        return Err(limit());
    }
    Ok(raw)
}
fn row_from_sql(r: &rusqlite::Row<'_>) -> WorkflowResult<StoredOccurrence> {
    let raw = r.get::<_, Option<String>>(8)?.ok_or_else(limit)?;
    let row: StoredOccurrence =
        serde_json::from_str(&raw).map_err(|_| invalid("伏笔出现证据无法解码"))?;
    let o = &row.occurrence;
    if r.get::<_, String>(0)? != o.id || r.get::<_, String>(1)? != o.book_id
        || r.get::<_, String>(2)? != o.material_id || r.get::<_, String>(3)? != row.request_epoch
        || r.get::<_, String>(4)? != row.request_key || r.get::<_, i64>(5)? != o.revision
        || r.get::<_, Option<i64>>(6)? != o.deleted_at || r.get::<_, i64>(7)? != raw.len() as i64
        // A canonical encoding gives one unambiguous byte count for JSON/SQLite.
        || encode(&row)? != raw
    {
        return Err(invalid("伏笔出现索引、载荷或byte_count不一致"));
    }
    Ok(row)
}
fn store(c: &Connection, row: &StoredOccurrence, raw: &str) -> WorkflowResult<()> {
    let o = &row.occurrence;
    c.execute("INSERT INTO foreshadow_occurrences(id,book_id,material_id,request_epoch,request_key,revision,deleted_at,record,byte_count) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![o.id,o.book_id,o.material_id,row.request_epoch,row.request_key,o.revision,o.deleted_at,raw,raw.len()])?;
    Ok(())
}
pub(crate) fn view(
    c: &Connection,
    occurrence: ForeshadowOccurrence,
) -> WorkflowResult<ForeshadowOccurrenceView> {
    let current = position(c, &occurrence.book_id, &occurrence.source.chapter_id)?;
    let source_status = match &current {
        Some(p) if p == &occurrence.source => ForeshadowSourceStatus::Current,
        Some(_) => ForeshadowSourceStatus::Stale,
        None => ForeshadowSourceStatus::Unavailable,
    };
    Ok(ForeshadowOccurrenceView {
        occurrence,
        source_status,
        current,
    })
}
fn ordered(rows: &mut [ForeshadowOccurrenceView]) {
    rows.sort_by(|a, b| {
        match (&a.current, &b.current) {
            (Some(a), Some(b)) => a.key().cmp(&b.key()),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.occurrence.source.key().cmp(&b.occurrence.source.key()),
        }
        .then(a.occurrence.start.cmp(&b.occurrence.start))
        .then(a.occurrence.id.cmp(&b.occurrence.id))
    });
}
fn get_row(c: &Connection, book: &str, card: &str, id: &str) -> WorkflowResult<StoredOccurrence> {
    let mut q = c.prepare(&format!("SELECT {ALIASES} FROM foreshadow_occurrences WHERE book_id=?1 AND material_id=?2 AND id=?3"))?;
    let mut rows = q.query(params![book, card, id])?;
    row_from_sql(rows.next()?.ok_or_else(absent)?)
}
pub(crate) fn find_row(c: &Connection, id: &str) -> WorkflowResult<Option<StoredOccurrence>> {
    let mut q = c.prepare(&format!(
        "SELECT {ALIASES} FROM foreshadow_occurrences WHERE id=?1"
    ))?;
    let mut rows = q.query([id])?;
    rows.next()?.map(row_from_sql).transpose()
}

fn by_card(
    c: &Connection,
    book: &str,
    card: &str,
    active: bool,
    after: &str,
    limit: usize,
) -> WorkflowResult<Vec<ForeshadowOccurrenceView>> {
    let mut q = c.prepare(&format!(
        "SELECT {ALIASES} FROM foreshadow_occurrences WHERE book_id=?1 AND material_id=?2 AND id>?3 AND (NOT ?4 OR deleted_at IS NULL) ORDER BY id LIMIT ?5"
    ))?;
    let mut r = q.query(params![book, card, after, active, limit])?;
    let mut out = Vec::new();
    while let Some(sql) = r.next()? {
        out.push(view(c, row_from_sql(sql)?.occurrence)?);
    }
    Ok(out)
}
fn quota(c: &Connection, new_bytes: usize, adding: Option<&str>) -> WorkflowResult<()> {
    let (n, bytes): (i64, i64) = c.query_row(
        "SELECT count(*),coalesce(sum(length(CAST(record AS BLOB))),0) FROM foreshadow_occurrences",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if n < 0
        || bytes < 0
        || n as usize + usize::from(adding.is_some()) > COUNT
        || (bytes as usize).saturating_add(new_bytes) > EVIDENCE
    {
        return Err(limit());
    }
    if let Some(id) = adding {
        let n: usize = c.query_row(
            "SELECT count(*) FROM foreshadow_occurrences WHERE material_id=?1",
            [id],
            |r| r.get(0),
        )?;
        if n >= PER_CARD {
            return Err(limit());
        }
    }
    Ok(())
}

fn validate_create(i: &CreateForeshadowOccurrence, manual: bool) -> WorkflowResult<()> {
    let valid_key = if manual {
        identity(&i.request_key)
    } else {
        crate::foreshadow_proposals::approval_from_key(&i.request_key).is_some()
    };
    if ![&i.book_id, &i.material_id, &i.chapter_id, &i.epoch]
        .into_iter()
        .all(|s| identity(s))
        || !valid_key
        || !valid_revision(i.chapter_revision)
        || i.quote.is_empty()
        || i.quote.len() > QUOTE
        || i.note.len() > NOTE
        || i.start >= i.end
        || i.end as usize > BODY
    {
        return Err(invalid(
            "章节版本、区间、引文或备注无效（引文4096字节，备注1024字节）",
        ));
    }
    Ok(())
}

/// Called by trusted manual creation and by trusted proposal acceptance only.
/// The caller owns the transaction and decides when to commit all its effects.
pub(crate) fn create_in_transaction(
    tx: &rusqlite::Transaction<'_>,
    i: CreateForeshadowOccurrence,
    allow_replay: bool,
) -> WorkflowResult<ForeshadowOccurrenceView> {
    validate_create(&i, allow_replay)?;
    check_epoch(tx, &i.epoch)?;
    let parent = active_card(tx, &i.book_id, &i.material_id)?;
    {
        let mut q = tx.prepare(&format!("SELECT {ALIASES} FROM foreshadow_occurrences WHERE book_id=?1 AND request_epoch=?2 AND request_key=?3"))?;
        let mut rows = q.query(params![i.book_id, i.epoch, i.request_key])?;
        if let Some(r) = rows.next()? {
            let o = row_from_sql(r)?.occurrence;
            if !allow_replay
                || o.material_id != i.material_id
                || o.source.chapter_id != i.chapter_id
                || o.source.chapter_revision != i.chapter_revision
                || o.start != i.start
                || o.end != i.end
                || o.quote != i.quote
                || o.role != i.role
                || o.note != i.note
            {
                return Err(fail(
                    "idempotency_conflict",
                    "同一新增键不能用于不同伏笔或片段",
                ));
            }
            // Return the actual record, even if its source has changed or the
            // author already moved it to trash. Never resurrect it on replay.
            return view(tx, o);
        }
    }
    let source = position(tx, &i.book_id, &i.chapter_id)?.ok_or_else(absent)?;
    if source.chapter_revision != i.chapter_revision {
        return Err(fail(
            "revision_conflict",
            "来源正文已变化，请重新读取并选择片段",
        ));
    }
    if !quote_matches(&body(tx, &i.chapter_id)?, i.start, i.end, &i.quote) {
        return Err(invalid("引文必须与已保存正文的Unicode标量区间完全相同"));
    }
    let created_at = time()?;
    let row = StoredOccurrence {
        occurrence: ForeshadowOccurrence {
            id: uuid::Uuid::new_v4().to_string(),
            book_id: i.book_id,
            material_id: i.material_id,
            source,
            start: i.start,
            end: i.end,
            quote: i.quote,
            role: i.role,
            note: i.note,
            revision: 0,
            deleted_at: None,
            created_at,
        },
        request_epoch: i.epoch,
        request_key: i.request_key,
    };
    let raw = encode(&row)?;
    quota(tx, raw.len(), Some(&row.occurrence.material_id))?;
    store(tx, &row, &raw)?;
    bump_card(
        tx,
        &row.occurrence.book_id,
        &row.occurrence.material_id,
        parent,
        created_at,
    )?;
    view(tx, row.occurrence)
}

impl Service {
    pub fn create_foreshadow_occurrence(
        &mut self,
        i: CreateForeshadowOccurrence,
    ) -> WorkflowResult<ForeshadowOccurrenceView> {
        validate_create(&i, true)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let result = create_in_transaction(&tx, i, true)?;
        tx.commit()?;
        Ok(result)
    }
    pub fn set_foreshadow_occurrence_deleted(
        &mut self,
        book: &str,
        card: &str,
        id: &str,
        expected: i64,
        epoch: &str,
        deleted: bool,
    ) -> WorkflowResult<ForeshadowOccurrenceView> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        check_epoch(&tx, epoch)?;
        let parent = active_card(&tx, book, card)?;
        let mut row = get_row(&tx, book, card, id)?;
        let old_len = encode(&row)?.len();
        if row.occurrence.revision != expected {
            return Err(fail("revision_conflict", "出现记录已变化，请重新读取"));
        }
        if row.occurrence.deleted_at.is_some() == deleted {
            return Err(fail("invalid_state", "出现记录已处于目标状态"));
        }
        let now = time()?.max(row.occurrence.created_at);
        row.occurrence.revision = expected
            .checked_add(1)
            .filter(|v| valid_revision(*v))
            .ok_or_else(|| invalid("出现记录版本已达上限"))?;
        row.occurrence.deleted_at = deleted.then_some(now);
        let raw = encode(&row)?;
        quota(&tx, raw.len().saturating_sub(old_len), None)?;
        if tx.execute("UPDATE foreshadow_occurrences SET revision=?1,deleted_at=?2,record=?3,byte_count=?4 WHERE id=?5 AND book_id=?6 AND material_id=?7 AND revision=?8",
            params![row.occurrence.revision,row.occurrence.deleted_at,raw,raw.len(),id,book,card,expected])? != 1 {
            return Err(fail("revision_conflict", "出现记录已变化，请重新读取"));
        }
        bump_card(&tx, book, card, parent, now)?;
        let result = view(&tx, row.occurrence)?;
        tx.commit()?;
        Ok(result)
    }
    pub fn get_foreshadow_occurrence(
        &self,
        book: &str,
        card: &str,
        id: &str,
        epoch: &str,
    ) -> WorkflowResult<ForeshadowOccurrenceView> {
        check_epoch(&self.conn, epoch)?;
        active_card(&self.conn, book, card)?;
        view(&self.conn, get_row(&self.conn, book, card, id)?.occurrence)
    }
    pub fn list_foreshadow_occurrences(
        &self,
        book: &str,
        card: &str,
        epoch: &str,
    ) -> WorkflowResult<Vec<ForeshadowOccurrenceView>> {
        check_epoch(&self.conn, epoch)?;
        active_card(&self.conn, book, card)?;
        let mut out = by_card(&self.conn, book, card, false, "", PER_CARD + 1)?;
        if out.len() > PER_CARD {
            return Err(limit());
        }
        ordered(&mut out);
        Ok(out)
    }
    pub fn list_book_foreshadow(
        &self,
        book: &str,
        epoch: &str,
    ) -> WorkflowResult<Vec<ForeshadowSummary>> {
        check_epoch(&self.conn, epoch)?;
        // SQL projects only bounded directory/status metadata. Excerpts are
        // returned separately, after the author selects one card.
        let mut counts = HashMap::new();
        let mut q = self.conn.prepare(
            "SELECT o.material_id,count(*),sum(CASE WHEN c.id IS NULL OR v.id IS NULL OR v.book_id!=o.book_id OR c.deleted_at IS NOT NULL OR v.deleted_at IS NOT NULL
             OR c.revision!=json_extract(o.record,'$.occurrence.source.chapter_revision')
             OR c.title!=json_extract(o.record,'$.occurrence.source.chapter_title')
             OR c.sort_order!=json_extract(o.record,'$.occurrence.source.chapter_sort_order')
             OR v.id!=json_extract(o.record,'$.occurrence.source.volume_id')
             OR v.title!=json_extract(o.record,'$.occurrence.source.volume_title')
             OR v.sort_order!=json_extract(o.record,'$.occurrence.source.volume_sort_order')
             THEN 1 ELSE 0 END) FROM foreshadow_occurrences o
             LEFT JOIN chapters c ON c.id=json_extract(o.record,'$.occurrence.source.chapter_id')
             LEFT JOIN volumes v ON v.id=c.volume_id
             WHERE o.book_id=?1 AND o.deleted_at IS NULL GROUP BY o.material_id"
        )?;
        for r in q.query_map([book], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, usize>(1)?,
                r.get::<_, usize>(2)?,
            ))
        })? {
            let (id, n, review) = r?;
            counts.insert(id, (n, review));
        }
        Ok(
            crate::materials::metadata_in(&self.conn, book, false, None)?
                .into_iter()
                .filter(|m| m.kind == "foreshadow")
                .map(|material| {
                    let (occurrence_count, review_count) =
                        counts.get(&material.id).copied().unwrap_or_default();
                    ForeshadowSummary {
                        material,
                        occurrence_count,
                        review_count,
                    }
                })
                .collect(),
        )
    }
    pub fn get_foreshadow_directory(
        &self,
        book: &str,
        epoch: &str,
    ) -> WorkflowResult<Vec<ForeshadowPosition>> {
        check_epoch(&self.conn, epoch)?;
        active_book(&self.conn, book)?;
        let (n,size):(usize,usize)=self.conn.query_row(&format!(
            "SELECT count(*),coalesce(sum(length(CAST(c.id AS BLOB))+length(CAST(c.title AS BLOB))+length(CAST(v.id AS BLOB))+length(CAST(v.title AS BLOB))),0) {ACTIVE_DIRECTORY}"
        ),[book],|r|Ok((r.get(0)?,r.get(1)?)))?;
        if n > 100_000 || size > DIRECTORY {
            return Err(fail("limit_exceeded", "作品目录超过16MiB"));
        }
        let mut q=self.conn.prepare(&format!(
            "SELECT c.id,c.title,c.revision,c.sort_order,v.id,v.title,v.sort_order {ACTIVE_DIRECTORY} ORDER BY v.sort_order,v.id,c.sort_order,c.id"
        ))?;
        let mut positions = Vec::with_capacity(n);
        let mut encoded = 2usize;
        for row in q.query_map([book], position_row)? {
            let position = row?;
            encoded += serde_json::to_vec(&position)
                .map_err(|_| invalid("无法编码目录"))?
                .len()
                + 1;
            if encoded > DIRECTORY {
                return Err(fail("limit_exceeded", "作品目录编码超过16MiB"));
            }
            positions.push(position);
        }
        Ok(positions)
    }
    pub fn get_foreshadow_chapter(
        &self,
        book: &str,
        chapter: &str,
        epoch: &str,
    ) -> WorkflowResult<ForeshadowChapterExcerpt> {
        check_epoch(&self.conn, epoch)?;
        let position = position(&self.conn, book, chapter)?.ok_or_else(absent)?;
        let body = body(&self.conn, chapter)?;
        Ok(ForeshadowChapterExcerpt { position, body })
    }
    /// Trusted, read-only author comparison. Returns at most 17,024 UTF-8
    /// body bytes. SQLite may materialize a full field internally, but Rust
    /// only borrows the row and copies the requested excerpt to the response.
    pub fn get_foreshadow_source_window(
        &self,
        book: &str,
        chapter: &str,
        epoch: &str,
        start: u32,
        end: u32,
    ) -> WorkflowResult<ForeshadowSourceWindow> {
        check_epoch(&self.conn, epoch)?;
        if start >= end || (end - start) as usize > QUOTE || end as usize > BODY {
            return Err(invalid("核对片段范围无效"));
        }
        let mut stmt = self.conn.prepare(&format!(
            "SELECT c.id,c.title,c.revision,c.sort_order,v.id,v.title,v.sort_order,
             CASE WHEN typeof(c.body)='text' AND length(CAST(c.body AS BLOB))<=?3 THEN c.body END
             {ACTIVE_DIRECTORY} AND c.id=?2"
        ))?;
        let mut rows = stmt.query(params![book, chapter, BODY])?;
        let row = rows.next()?.ok_or_else(absent)?;
        let position = position_row(row)?;
        let body = row.get_ref(7)?.as_str()
            .map_err(|_| fail("limit_exceeded", "章节正文超10MiB或不是文本，未读取"))?;
        const CONTEXT: usize = 80;
        let start = start as usize;
        let end = end as usize;
        let limits = [start.saturating_sub(CONTEXT), start, end, end + CONTEXT];
        let mut offsets = [body.len(); 4];
        // Use Rust Unicode scalar offsets: SQLite text substr truncates at NUL.
        for (i, (offset, _)) in body.char_indices().take(limits[3] + 1).enumerate() {
            for (j, limit) in limits.iter().enumerate() {
                if i == *limit {
                    offsets[j] = offset;
                }
            }
        }
        Ok(ForeshadowSourceWindow {
            position,
            before: body[offsets[0]..offsets[1]].to_owned(),
            at_range: body[offsets[1]..offsets[2]].to_owned(),
            after: body[offsets[2]..offsets[3]].to_owned(),
        })
    }
    pub fn lookup_foreshadow_request(
        &self,
        book: &str,
        card: &str,
        key: &str,
        epoch: &str,
    ) -> WorkflowResult<Option<ForeshadowOccurrenceView>> {
        check_epoch(&self.conn, epoch)?;
        active_card(&self.conn, book, card)?;
        if !identity(key) {
            return Err(invalid("新增请求标识无效"));
        }
        let mut q=self.conn.prepare(&format!("SELECT {ALIASES} FROM foreshadow_occurrences WHERE book_id=?1 AND material_id=?2 AND request_epoch=?3 AND request_key=?4"))?;
        let mut rows = q.query(params![book, card, epoch, key])?;
        rows.next()?
            .map(|r| view(&self.conn, row_from_sql(r)?.occurrence))
            .transpose()
    }
    pub fn get_book_foreshadow_material(
        &self,
        book: &str,
        id: &str,
        epoch: &str,
    ) -> WorkflowResult<Material> {
        check_epoch(&self.conn, epoch)?;
        active_card(&self.conn, book, id)?;
        self.get_material(id, false)
    }
    pub fn create_book_foreshadow_material(
        &mut self,
        input: crate::CreateMaterialInput,
        epoch: &str,
    ) -> WorkflowResult<Material> {
        check_epoch(&self.conn, epoch)?;
        if input.kind != "foreshadow" {
            return Err(invalid("全书伏笔仅可新增伏笔卡"));
        }
        self.create_material(input)
    }
    pub fn save_book_foreshadow_material(
        &mut self,
        book: &str,
        input: crate::SaveMaterialInput,
        epoch: &str,
    ) -> WorkflowResult<Material> {
        check_epoch(&self.conn, epoch)?;
        active_card(&self.conn, book, &input.id)?;
        self.save_material(input)
    }
    pub(crate) fn foreshadow_agent_page(
        &self,
        book: &str,
        card: &str,
        after: Option<&str>,
        requested_limit: Option<u32>,
    ) -> WorkflowResult<ForeshadowOccurrencePage> {
        let n = requested_limit.unwrap_or(10) as usize;
        if n == 0 || n > 20 || !identity(card) || after.is_some_and(|s| !identity(s)) {
            return Err(invalid("分页限制为1–20，标识必须有效"));
        }
        active_card(&self.conn, book, card)?;
        let mut items = Vec::new();
        let mut more = false;
        let mut bytes = 0;
        for item in by_card(&self.conn, book, card, true, after.unwrap_or(""), n + 1)? {
            let size = serde_json::to_vec(&item)
                .map_err(|_| invalid("无法编码伏笔出现"))?
                .len()
                + 1;
            if items.len() == n || bytes + size > PAGE_JSON {
                more = true;
                break;
            }
            bytes += size;
            items.push(item);
        }
        let next_after_id = if more {
            Some(items.last().ok_or_else(limit)?.occurrence.id.clone())
        } else {
            None
        };
        Ok(ForeshadowOccurrencePage {
            items,
            next_after_id,
        })
    }
}

pub(crate) fn active_frozen(
    c: &Connection,
    book: &str,
    card: &str,
) -> WorkflowResult<Vec<ForeshadowOccurrence>> {
    let rows = by_card(c, book, card, true, "", PER_CARD + 1)?;
    if rows.len() > PER_CARD {
        return Err(limit());
    }
    rows.into_iter()
        .map(|v| {
            if v.source_status != ForeshadowSourceStatus::Current {
                return Err(fail(
                    "revision_conflict",
                    "必需伏笔的出现来源已变化或不可用，请作者先处理",
                ));
            }
            Ok(v.occurrence)
        })
        .collect()
}

pub(crate) fn preflight(c: &Connection) -> WorkflowResult<()> {
    quota(c, 0, None)?;
    let invalid_size: i64 = c.query_row("SELECT count(*) FROM foreshadow_occurrences WHERE typeof(record)!='text' OR typeof(byte_count)!='integer' OR byte_count!=length(CAST(record AS BLOB)) OR byte_count>32768", [], |r| r.get(0))?;
    if invalid_size != 0 {
        return Err(invalid("伏笔出现byte_count或载荷长度无效"));
    }
    Ok(())
}

pub(crate) fn all(c: &Connection) -> WorkflowResult<Vec<StoredOccurrence>> {
    preflight(c)?;
    let mut q = c.prepare(&format!(
        "SELECT {ALIASES} FROM foreshadow_occurrences ORDER BY id"
    ))?;
    let mut rows = q.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(row_from_sql(row)?);
    }
    Ok(out)
}
pub(crate) fn validate(
    state: &LibraryState,
    materials: &[Material],
    rows: &[StoredOccurrence],
    mut check_quotes: impl FnMut(&Chapter, &[&ForeshadowOccurrence]) -> WorkflowResult<()>,
) -> WorkflowResult<()> {
    if rows.len() > COUNT {
        return Err(limit());
    }
    let cards: HashMap<_, _> = materials.iter().map(|m| (m.id.as_str(), m)).collect();
    let chapters: HashMap<_, _> = state
        .chapters
        .iter()
        .map(|ch| (ch.id.as_str(), ch))
        .collect();
    let volumes: HashMap<_, _> = state
        .volumes
        .iter()
        .map(|v| (v.id.as_str(), v.book_id.as_str()))
        .collect();
    let mut keys = HashSet::new();
    let mut ids = HashSet::new();
    let mut counts = HashMap::new();
    let mut size = 0usize;
    let mut verify = HashMap::<_, Vec<_>>::new();
    let mut changes = HashMap::<&str, i64>::new();
    for row in rows {
        let o = &row.occurrence;
        let card = cards.get(o.material_id.as_str()).ok_or_else(absent)?;
        let count = counts.entry(o.material_id.as_str()).or_insert(0usize);
        *count += 1;
        size += encode(row)?.len();
        if *count > PER_CARD || size > EVIDENCE {
            return Err(limit());
        }
        let n_changes = changes.entry(o.material_id.as_str()).or_default();
        *n_changes = n_changes
            .checked_add(o.revision + 1)
            .ok_or_else(|| invalid("资料版本计数溢出"))?;
        if card.book_id != o.book_id
            || card.kind != "foreshadow"
            || card.revision < *n_changes
            || !ids.insert(o.id.as_str())
            || !keys.insert((&o.book_id, &row.request_epoch, &row.request_key))
            || chapters
                .get(o.source.chapter_id.as_str())
                .is_some_and(|ch| {
                    volumes.get(ch.volume_id.as_str()).copied() != Some(o.book_id.as_str())
                        || o.source.chapter_revision > ch.revision
                })
            || volumes
                .get(o.source.volume_id.as_str())
                .is_some_and(|book| *book != o.book_id)
        {
            return Err(invalid("伏笔出现记录身份、作品、资料、来源或版本不一致"));
        }
        // Check equal known revisions only. Older/missing sources remain
        // historical evidence and are not compared to today's manuscript.
        if chapters
            .get(o.source.chapter_id.as_str())
            .is_some_and(|ch| ch.revision == o.source.chapter_revision)
        {
            verify
                .entry(o.source.chapter_id.as_str())
                .or_default()
                .push(o);
        }
    }
    for (id, rows) in verify {
        check_quotes(chapters[id], &rows)?;
    }
    Ok(())
}
pub(crate) fn insert_all(c: &Connection, rows: &[StoredOccurrence]) -> WorkflowResult<()> {
    for row in rows {
        store(c, row, &encode(row)?)?;
    }
    Ok(())
}

pub(crate) fn validate_quotes(body: &str, rows: &[&ForeshadowOccurrence]) -> WorkflowResult<()> {
    let mut indexes: Vec<usize> = rows
        .iter()
        .flat_map(|row| [row.start as usize, row.end as usize])
        .collect();
    indexes.sort_unstable();
    indexes.dedup();
    let mut offsets = HashMap::new();
    let mut wanted = indexes.into_iter().peekable();
    for (scalar, (offset, _)) in body
        .char_indices()
        .chain(std::iter::once((body.len(), '\0')))
        .enumerate()
    {
        if wanted.peek() == Some(&scalar) {
            offsets.insert(scalar, offset);
            wanted.next();
        }
        if wanted.peek().is_none() {
            break;
        }
    }
    if rows.iter().any(|row| {
        let bytes = offsets
            .get(&(row.start as usize))
            .zip(offsets.get(&(row.end as usize)));
        bytes.and_then(|(a, b)| body.get(*a..*b)) != Some(row.quote.as_str())
    }) {
        return Err(invalid("伏笔引文与同版本冻结正文不匹配"));
    }
    Ok(())
}

/// Backup copies may capture different lifecycle revisions, or outlive the
/// current row. Any copies that do exist must describe the same captured event.
pub(crate) fn validate_copies<'a>(
    rows: &'a [StoredOccurrence],
    packs: &'a [crate::WritingPack],
) -> WorkflowResult<HashMap<&'a str, &'a ForeshadowOccurrence>> {
    fn same_immutable(a: &ForeshadowOccurrence, b: &ForeshadowOccurrence) -> bool {
        let ForeshadowOccurrence {
            id,
            book_id,
            material_id,
            source,
            start,
            end,
            quote,
            role,
            note,
            created_at,
            revision: _,
            deleted_at: _,
        } = a;
        id == &b.id
            && book_id == &b.book_id
            && material_id == &b.material_id
            && source == &b.source
            && *start == b.start
            && *end == b.end
            && quote == &b.quote
            && *role == b.role
            && note == &b.note
            && *created_at == b.created_at
    }
    let mut known: HashMap<&str, &ForeshadowOccurrence> = HashMap::new();
    for occurrence in rows
        .iter()
        .map(|row| &row.occurrence)
        .chain(packs.iter().flat_map(|pack| &pack.foreshadow_occurrences))
    {
        let previous = known.entry(&occurrence.id).or_insert(occurrence);
        if !same_immutable(previous, occurrence) {
            return Err(invalid("同一伏笔出现ID的不可变内容不一致"));
        }
    }
    Ok(known)
}

pub(crate) fn validate_db(
    c: &Connection,
    state: &LibraryState,
    materials: &[Material],
    packs: &[crate::WritingPack],
    proposal_epoch: Option<&str>,
) -> WorkflowResult<()> {
    let evidence = all(c)?;
    validate(state, materials, &evidence, |ch, rows| {
        validate_quotes(&body(c, &ch.id)?, rows)
    })?;
    let copies = validate_copies(&evidence, packs)?;
    let proposals = crate::foreshadow_proposals::all(c)?;
    crate::foreshadow_proposals::validate(
        state,
        materials,
        &evidence,
        &copies,
        &proposals,
        proposal_epoch,
        |ch, rows| validate_quotes(&body(c, &ch.id)?, rows),
    )
}
