//! Trusted author policies, immutable evidence and book-scoped review-only agent proposals.
use crate::foreshadow::{active_frozen, validate_frozen_occurrence, validate_quotes};
use crate::{ForeshadowOccurrence, ForeshadowPosition};
use crate::{Book, Chapter, Material, Service, Volume, WorkflowError, WorkflowResult};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
const PACK: usize = 4 * 1024 * 1024;
const BODY: usize = 1024 * 1024;
const MAX_CONTEXT_CHAPTERS: usize = 8;
const CONTEXT_DIRECTORY_BYTES: i64 = 16 * 1024 * 1024;
const PRIOR_SCOPE: &str = "FROM chapters c JOIN volumes v ON v.id=c.volume_id
    WHERE v.book_id=?1 AND c.deleted_at IS NULL AND v.deleted_at IS NULL
    AND (v.sort_order,v.id,c.sort_order,c.id) < (?2,?3,?4,?5)";
const TOTAL: i64 = 64 * 1024 * 1024;
const REVIEW_BYTES: usize = 64 * 1024;
const REVIEW_FIELD_BYTES: usize = ",\"author_review\":".len();
// "cancelled" adds at most two chars; an i64 needs at most 20 chars
// instead of four for each of the two nulls. The live-pending quota already
// reserves 256 bytes for completing each decision.
const TERMINAL_METADATA_BYTES: usize = 2 + 2 * (20 - 4);
const MAX_PROPOSAL_BYTES: usize =
    2 * BODY + REVIEW_BYTES + REVIEW_FIELD_BYTES + TERMINAL_METADATA_BYTES;
const MAXREV: i64 = 9_007_199_254_740_000;
fn fail(s: &str) -> WorkflowError {
    WorkflowError {
        code: "writing_constraint".into(),
        message: s.into(),
    }
}
fn db(_: rusqlite::Error) -> WorkflowError {
    fail("写作约束数据操作失败，未确认写入")
}
fn now() -> WorkflowResult<i64> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| fail("系统时间无效"))?
        .as_secs() as i64)
}
fn uid() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn json<T: Serialize>(v: &T) -> WorkflowResult<String> {
    serde_json::to_string(v).map_err(|_| fail("依据编码失败"))
}
fn parse<T: serde::de::DeserializeOwned>(s: &str) -> WorkflowResult<T> {
    serde_json::from_str(s).map_err(|_| fail("依据格式无效"))
}
fn epoch(c: &Connection) -> WorkflowResult<String> {
    c.query_row(
        "SELECT value FROM workflow_meta WHERE key='epoch'",
        [],
        |r| r.get(0),
    )
    .map_err(db)
}
fn identity(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control)
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WritingPolicy {
    pub chapter_id: String,
    pub book_id: String,
    pub revision: i64,
    pub mode: String,
    pub goal: String,
    pub required_material_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_chapter_ids: Vec<String>,
    pub updated_at: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SourceState {
    pub sort_order: i64,
    pub id: String,
    pub revision: i64,
    pub title: String,
    pub volume: Volume,
}
fn legacy_handle_version() -> u8 {
    1
}
fn frozen_materials<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Material>, D::Error> {
    let values = Vec::<serde_json::Value>::deserialize(d)?;
    if values.len() > 32 {
        return Err(serde::de::Error::custom("too many frozen cards"));
    }
    values
        .into_iter()
        .map(|v| {
            for key in [
                "deleted_at",
                "content_review_required",
                "source_chapter_id",
                "source_revision",
                "source_status",
                "payoff_chapter_id",
                "payoff_revision",
                "payoff_status",
            ] {
                if v.get(key).is_none() {
                    return Err(serde::de::Error::custom("missing frozen provenance"));
                }
            }
            serde_json::from_value(v).map_err(serde::de::Error::custom)
        })
        .collect()
}
// Unlike generic legacy directory rows, frozen context rows require every
// field, including explicit nullable deletion markers.
fn explicit_deletion<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
    Option::deserialize(d)
}
fn frozen_context_chapter<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Chapter, D::Error> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Row {
        id: String,
        volume_id: String,
        title: String,
        body: String,
        revision: i64,
        sort_order: i64,
        #[serde(deserialize_with = "explicit_deletion")]
        deleted_at: Option<i64>,
    }
    let Row {
        id,
        volume_id,
        title,
        body,
        revision,
        sort_order,
        deleted_at,
    } = Row::deserialize(d)?;
    Ok(Chapter {
        id,
        volume_id,
        title,
        body,
        revision,
        sort_order,
        deleted_at,
    })
}
fn frozen_context_volume<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Volume, D::Error> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Row {
        id: String,
        book_id: String,
        title: String,
        sort_order: i64,
        #[serde(deserialize_with = "explicit_deletion")]
        deleted_at: Option<i64>,
    }
    let Row {
        id,
        book_id,
        title,
        sort_order,
        deleted_at,
    } = Row::deserialize(d)?;
    Ok(Volume {
        id,
        book_id,
        title,
        sort_order,
        deleted_at,
    })
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FrozenContextChapter {
    #[serde(deserialize_with = "frozen_context_chapter")]
    pub chapter: Chapter,
    #[serde(deserialize_with = "frozen_context_volume")]
    pub volume: Volume,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WritingContextDirectory {
    pub epoch: String,
    pub book_id: String,
    pub chapter_id: String,
    pub volumes: Vec<Volume>,
    pub chapters: Vec<crate::ChapterMetadata>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WritingPredecessorPosition {
    Before,
    After,
    NotVisible,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WritingPredecessorObservation {
    pub chapter_id: String,
    pub position: WritingPredecessorPosition,
    pub current: Option<SourceState>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WritingPredecessorDiagnosis {
    pub epoch: String,
    pub book_id: String,
    pub chapter_id: String,
    pub pack_id: String,
    pub target: SourceState,
    pub entries: Vec<WritingPredecessorObservation>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WritingPack {
    #[serde(default = "legacy_handle_version")]
    pub handle_version: u8,
    pub id: String,
    pub book_id: String,
    pub chapter_id: String,
    pub originating_epoch: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub policy: WritingPolicy,
    pub book: Book,
    pub volume: Volume,
    pub chapter: Chapter,
    #[serde(deserialize_with = "frozen_materials")]
    pub materials: Vec<Material>,
    pub sources: Vec<SourceState>,
    pub related_volumes: Vec<Volume>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_chapters: Vec<FrozenContextChapter>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub foreshadow_occurrences: Vec<ForeshadowOccurrence>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WritingCitation {
    pub entry_id: String,
    pub field_key: String,
    pub start: u32,
    pub end: u32,
    pub explanation: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WritingConflict {
    pub entry_ids: Vec<String>,
    pub explanation: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ConstrainedEditInput {
    pub pack_id: String,
    pub new_body: String,
    pub citations: Vec<WritingCitation>,
    pub conflicts: Vec<WritingConflict>,
    pub idempotency_key: String,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthorReviewVerdict {
    Unreviewed,
    Conforms,
    Question,
    Conflict,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AuthorReviewItem {
    pub material_id: String,
    pub verdict: AuthorReviewVerdict,
    pub note: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AuthorReview {
    pub proposal_id: String,
    pub pack_id: String,
    pub book_id: String,
    pub chapter_id: String,
    pub originating_epoch: String,
    pub policy_revision: i64,
    pub chapter_revision: i64,
    pub revision: i64,
    pub items: Vec<AuthorReviewItem>,
}
impl AuthorReview {
    fn all_conform(&self) -> bool {
        self.items
            .iter()
            .all(|i| i.verdict == AuthorReviewVerdict::Conforms)
    }
}
fn present_author_review<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<AuthorReview>, D::Error> {
    // An absent field is legacy evidence; an explicit value must be a real review.
    AuthorReview::deserialize(d).map(Some)
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ConstrainedProposal {
    pub id: String,
    pub book_id: String,
    pub chapter_id: String,
    pub client_id: String,
    pub input: ConstrainedEditInput,
    pub base_revision: i64,
    pub decision: String,
    pub created_at: i64,
    pub decided_at: Option<i64>,
    pub accepted_revision: Option<i64>,
    pub live_epoch: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_author_review"
    )]
    pub author_review: Option<AuthorReview>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WritingEntry {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub revision: i64,
    pub fields: Vec<String>,
    pub metadata: serde_json::Value,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WritingPackManifest {
    pub id: String,
    pub book_id: String,
    pub chapter_id: String,
    pub policy_revision: i64,
    pub chapter_revision: i64,
    pub goal: String,
    pub expires_at: i64,
    pub archived: bool,
    pub entries: Vec<WritingEntry>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WritingChunk {
    pub text: String,
    pub next_cursor: Option<usize>,
    pub total_bytes: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WritingProposalSummary {
    pub id: String,
    pub pack_id: String,
    pub decision: String,
    pub archived: bool,
    pub created_at: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WritingPackSummary {
    pub id: String,
    pub policy_revision: i64,
    pub chapter_revision: i64,
    pub expires_at: i64,
    pub archived: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WritingState {
    pub epoch: String,
    pub policy: Option<WritingPolicy>,
    pub packs: Vec<WritingPackSummary>,
    pub proposals: Vec<WritingProposalSummary>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WritingEvidence {
    pub policies: Vec<WritingPolicy>,
    pub packs: Vec<WritingPack>,
    pub proposals: Vec<ConstrainedProposal>,
}
pub(crate) fn migrate(c: &Connection) -> WorkflowResult<()> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS writing_policies(chapter_id TEXT PRIMARY KEY,book_id TEXT NOT NULL REFERENCES books(id) ON DELETE CASCADE,revision INTEGER NOT NULL,mode TEXT NOT NULL,record TEXT NOT NULL);
 CREATE TABLE IF NOT EXISTS writing_packs(id TEXT PRIMARY KEY,book_id TEXT NOT NULL REFERENCES books(id) ON DELETE CASCADE,chapter_id TEXT NOT NULL,payload TEXT NOT NULL,byte_count INTEGER NOT NULL,manifest TEXT NOT NULL,originating_epoch TEXT NOT NULL);
 CREATE TABLE IF NOT EXISTS writing_proposals(id TEXT PRIMARY KEY,book_id TEXT NOT NULL REFERENCES books(id) ON DELETE CASCADE,chapter_id TEXT NOT NULL,pack_id TEXT NOT NULL REFERENCES writing_packs(id),client_id TEXT NOT NULL,idempotency_key TEXT NOT NULL,decision TEXT NOT NULL,live_epoch TEXT,payload TEXT NOT NULL,byte_count INTEGER NOT NULL,created_at INTEGER NOT NULL,UNIQUE(client_id,idempotency_key));
 CREATE INDEX IF NOT EXISTS writing_pack_chapter ON writing_packs(chapter_id,id);
 CREATE INDEX IF NOT EXISTS writing_proposal_chapter ON writing_proposals(chapter_id,id);").map_err(db)
}
pub(crate) fn protected(c: &Connection) -> WorkflowResult<u64> {
    c.query_row(
        "SELECT count(*) FROM writing_policies WHERE mode IN ('active','paused')",
        [],
        |r| r.get(0),
    )
    .map_err(db)
}
pub(crate) fn legacy_guard(c: &Connection, id: &str) -> WorkflowResult<()> {
    if c.query_row(
        "SELECT count(*) FROM writing_policies WHERE chapter_id=?1 AND mode IN ('active','paused')",
        [id],
        |r| r.get::<_, i64>(0),
    )
    .map_err(db)?
        > 0
    {
        Err(fail("本章启用写作约束，普通无依据提案不可提交或采纳"))
    } else {
        Ok(())
    }
}
pub(crate) fn restore_guard(c: &Connection, ack: bool) -> WorkflowResult<()> {
    if !ack && protected(c)? > 0 {
        Err(fail("恢复会替换当前写作约束，请明确确认后重试"))
    } else {
        Ok(())
    }
}
fn policy(c: &Connection, id: &str) -> WorkflowResult<Option<WritingPolicy>> {
    let row:Option<(String,String,i64,String)>=c.query_row("SELECT CASE WHEN length(CAST(record AS BLOB))<=65536 THEN record ELSE NULL END,book_id,revision,mode FROM writing_policies WHERE chapter_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(db)?;
    row.map(|(raw, book, rev, mode)| {
        let p: WritingPolicy = parse(&raw)?;
        if p.chapter_id != id || p.book_id != book || p.revision != rev || p.mode != mode {
            return Err(fail("配置行与证据身份不一致"));
        }
        policy_shape(&p)?;
        Ok(p)
    })
    .transpose()
}
fn active_scope(c: &Connection, book: &str, id: &str) -> WorkflowResult<()> {
    let found:i64=c.query_row("SELECT count(*) FROM chapters c JOIN volumes v ON v.id=c.volume_id JOIN books b ON b.id=v.book_id WHERE c.id=?1 AND b.id=?2 AND c.deleted_at IS NULL AND v.deleted_at IS NULL AND b.deleted_at IS NULL",params![id,book],|r|r.get(0)).map_err(db)?;
    if found != 1 {
        return Err(fail("章节不属于活动作品"));
    }
    Ok(())
}
fn body_size(c: &Connection, book: &str, id: &str) -> WorkflowResult<usize> {
    let size: i64 = c.query_row("SELECT length(CAST(c.body AS BLOB)) FROM chapters c JOIN volumes v ON v.id=c.volume_id JOIN books b ON b.id=v.book_id WHERE c.id=?1 AND b.id=?2 AND c.deleted_at IS NULL AND v.deleted_at IS NULL AND b.deleted_at IS NULL",params![id,book],|r|r.get(0)).optional().map_err(db)?.ok_or_else(||fail("章节不属于活动作品"))?;
    usize::try_from(size).map_err(|_| fail("章节正文大小无效"))
}
fn active(c: &Connection, book: &str, id: &str) -> WorkflowResult<(Book, Volume, Chapter)> {
    let size = body_size(c, book, id)?;
    if size > BODY {
        return Err(fail("约束写作目标正文超过1MiB，不会截断"));
    }
    let ch = crate::workflow::chapter(c, id)?;
    let v = c
        .query_row(
            "SELECT id,book_id,title,sort_order,deleted_at FROM volumes WHERE id=?1",
            [&ch.volume_id],
            |r| {
                Ok(Volume {
                    id: r.get(0)?,
                    book_id: r.get(1)?,
                    title: r.get(2)?,
                    sort_order: r.get(3)?,
                    deleted_at: r.get(4)?,
                })
            },
        )
        .map_err(db)?;
    let b = c
        .query_row(
            "SELECT id,title,author,deleted_at FROM books WHERE id=?1",
            [book],
            |r| {
                Ok(Book {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    author: r.get(2)?,
                    deleted_at: r.get(3)?,
                })
            },
        )
        .map_err(db)?;
    Ok((b, v, ch))
}
fn optional_source(c: &Connection, book: &str, id: &str) -> WorkflowResult<Option<SourceState>> {
    c.query_row("SELECT c.id,c.revision,c.title,v.id,v.book_id,v.title,v.sort_order,v.deleted_at,c.sort_order FROM chapters c JOIN volumes v ON v.id=c.volume_id JOIN books b ON b.id=v.book_id WHERE c.id=?1 AND b.id=?2 AND c.deleted_at IS NULL AND v.deleted_at IS NULL AND b.deleted_at IS NULL",params![id,book],|r|Ok(SourceState{sort_order:r.get(8)?,id:r.get(0)?,revision:r.get(1)?,title:r.get(2)?,volume:Volume{id:r.get(3)?,book_id:r.get(4)?,title:r.get(5)?,sort_order:r.get(6)?,deleted_at:r.get(7)?}})).optional().map_err(db)
}
fn source(c: &Connection, book: &str, id: &str) -> WorkflowResult<SourceState> {
    optional_source(c, book, id)?.ok_or_else(|| fail("依据来源不可用"))
}
fn directory_key<'a>(v: &'a Volume, sort_order: i64, id: &'a str) -> (i64, &'a str, i64, &'a str) {
    (v.sort_order, &v.id, sort_order, id)
}
fn context_source(
    c: &Connection,
    p: &WritingPolicy,
    target: &SourceState,
    id: &str,
) -> WorkflowResult<SourceState> {
    let prior = source(c, &p.book_id, id).map_err(|_| fail("所选前文章节不可用，请调整配置"))?;
    if directory_key(&prior.volume, prior.sort_order, &prior.id)
        >= directory_key(&target.volume, target.sort_order, &target.id)
    {
        return Err(fail("所选前文章节必须在目标章之前，请调整配置"));
    }
    Ok(prior)
}
fn context_snapshot(
    c: &Connection,
    p: &WritingPolicy,
    mut bytes: usize,
) -> WorkflowResult<Vec<FrozenContextChapter>> {
    if p.context_chapter_ids.is_empty() {
        return Ok(vec![]);
    }
    let target = source(c, &p.book_id, &p.chapter_id)?;
    let mut rows = vec![];
    for id in &p.context_chapter_ids {
        let prior = context_source(c, p, &target, id)?;
        let size = body_size(c, &p.book_id, id)?;
        if size > BODY {
            return Err(fail("所选前文章节超过1MiB，不会截断"));
        }
        bytes = bytes.checked_add(size).ok_or_else(|| fail("依据过大"))?;
        if bytes > PACK {
            return Err(fail("完整依据包超过4MiB，不会截断"));
        }
        rows.push(prior);
    }
    rows.sort_by(|a, b| {
        directory_key(&a.volume, a.sort_order, &a.id).cmp(&directory_key(
            &b.volume,
            b.sort_order,
            &b.id,
        ))
    });
    rows.into_iter()
        .map(|s| {
            Ok(FrozenContextChapter {
                chapter: crate::workflow::chapter(c, &s.id)?,
                volume: s.volume,
            })
        })
        .collect()
}
fn snapshot(
    c: &Connection,
    p: &WritingPolicy,
) -> WorkflowResult<(
    Book,
    Volume,
    Chapter,
    Vec<Material>,
    Vec<SourceState>,
    Vec<Volume>,
    Vec<FrozenContextChapter>,
    Vec<ForeshadowOccurrence>,
)> {
    let (b, v, ch) = active(c, &p.book_id, &p.chapter_id)?;
    let mut mats = vec![];
    let mut src = vec![];
    let mut used = HashSet::new();
    let mut bytes = ch.body.len();
    let mut volumes = vec![];
    let mut occurrences = vec![];
    for id in &p.required_material_ids {
        let n:Option<i64>=c.query_row("SELECT length(CAST(fields AS BLOB))+length(CAST(title AS BLOB)) FROM materials WHERE id=?1 AND book_id=?2",params![id,p.book_id],|r|r.get(0)).optional().map_err(db)?;
        bytes = bytes
            .checked_add(n.ok_or_else(|| fail("必需资料不存在"))? as usize)
            .ok_or_else(|| fail("依据过大"))?;
        if bytes > PACK {
            return Err(fail("必需依据超过4MiB，不会截断"));
        }
        let m = crate::materials::get_material_in(c, id, false)?;
        if m.book_id != p.book_id
            || m.content_review_required
            || ["stale", "unavailable"].contains(&m.source_status.as_str())
            || ["stale", "unavailable"].contains(&m.payoff_status.as_str())
        {
            return Err(fail("必需资料或来源待复核/不可用，请作者先处理"));
        }
        for sid in [
            m.source_chapter_id.as_ref(),
            m.payoff_chapter_id.as_ref(),
            m.chapter_id.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if used.insert(sid.clone()) {
                src.push(source(c, &p.book_id, sid)?);
            }
        }
        if let Some(vid) = &m.volume_id {
            if !volumes.iter().any(|v: &Volume| &v.id == vid) {
                let v=c.query_row("SELECT id,book_id,title,sort_order,deleted_at FROM volumes WHERE id=?1 AND book_id=?2 AND deleted_at IS NULL",params![vid,p.book_id],|r|Ok(Volume{id:r.get(0)?,book_id:r.get(1)?,title:r.get(2)?,sort_order:r.get(3)?,deleted_at:r.get(4)?})).map_err(db)?;
                volumes.push(v);
            }
        }
        if m.kind == "foreshadow" {
            for occurrence in active_frozen(c, &p.book_id, &m.id)? {
                bytes = bytes.checked_add(json(&occurrence)?.len()).ok_or_else(|| fail("依据过大"))?;
                if bytes > PACK { return Err(fail("伏笔出现证据超过4MiB包上限，不会截断")); }
                occurrences.push(occurrence);
            }
        }
        mats.push(m);
    }
    occurrences.sort_by(|a,b| a.id.cmp(&b.id));
    src.sort_by(|a, b| a.id.cmp(&b.id));
    volumes.sort_by(|a, b| a.id.cmp(&b.id));
    let contexts = context_snapshot(c, p, bytes)?;
    Ok((b, v, ch, mats, src, volumes, contexts, occurrences))
}
fn policy_shape(p: &WritingPolicy) -> WorkflowResult<()> {
    let unique: HashSet<_> = p.required_material_ids.iter().collect();
    let unique_context: HashSet<_> = p.context_chapter_ids.iter().collect();
    if !identity(&p.chapter_id)
        || !identity(&p.book_id)
        || p.revision < 0
        || p.revision >= MAXREV
        || !["active", "paused", "disabled"].contains(&p.mode.as_str())
        || p.goal.trim().is_empty()
        || p.goal.len() > 4096
        || p.required_material_ids.is_empty()
        || p.required_material_ids.len() > 32
        || unique.len() != p.required_material_ids.len()
        || p.required_material_ids.iter().any(|x| !identity(x))
        || p.context_chapter_ids.len() > MAX_CONTEXT_CHAPTERS
        || unique_context.len() != p.context_chapter_ids.len()
        || p.context_chapter_ids
            .iter()
            .any(|id| !identity(id) || id == &p.chapter_id)
        || p.updated_at < 0
    {
        return Err(fail("写作约束配置字段无效或超限"));
    }
    Ok(())
}
fn store_policy(c: &Connection, p: &WritingPolicy) -> WorkflowResult<()> {
    if json(p)?.len() > 65536 {
        return Err(fail("配置编码超64KiB"));
    }
    c.execute("INSERT INTO writing_policies VALUES(?1,?2,?3,?4,?5) ON CONFLICT(chapter_id) DO UPDATE SET book_id=excluded.book_id,revision=excluded.revision,mode=excluded.mode,record=excluded.record",params![p.chapter_id,p.book_id,p.revision,p.mode,json(p)?]).map_err(db)?;
    Ok(())
}
fn get_pack(c: &Connection, book: &str, id: &str) -> WorkflowResult<WritingPack> {
    let (raw,chapter):(String,String)=c.query_row("SELECT CASE WHEN length(CAST(payload AS BLOB))<=4194304 THEN payload ELSE NULL END,chapter_id FROM writing_packs WHERE id=?1 AND book_id=?2",params![id,book],|r|Ok((r.get(0)?,r.get(1)?))).map_err(db)?;
    let p: WritingPack = parse(&raw)?;
    if p.id != id || p.book_id != book || p.chapter_id != chapter {
        return Err(fail("依据包行与载荷身份不一致"));
    }
    policy_shape(&p.policy)?;
    check_handles(&p)?;
    validate_context(&p)?;
    validate_foreshadow(&p)?;
    Ok(p)
}
fn get_proposal(c: &Connection, book: &str, id: &str) -> WorkflowResult<ConstrainedProposal> {
    let (raw,chapter):(String,String)=c.query_row("SELECT CASE WHEN length(CAST(payload AS BLOB))<=?3 THEN payload ELSE NULL END,chapter_id FROM writing_proposals WHERE id=?1 AND book_id=?2",params![id,book,MAX_PROPOSAL_BYTES as i64],|r|Ok((r.get(0)?,r.get(1)?))).map_err(db)?;
    let p: ConstrainedProposal = parse(&raw)?;
    if p.id != id || p.book_id != book || p.chapter_id != chapter {
        return Err(fail("严格提案行与载荷身份不一致"));
    }
    Ok(p)
}
fn proposal_bytes(p: &ConstrainedProposal) -> WorkflowResult<String> {
    let s = json(p)?;
    let extra = if let Some(review) = &p.author_review {
        review_bytes(review)? + REVIEW_FIELD_BYTES
    } else {
        0
    };
    // Hold the original 2MiB admission allowance for the proposal itself.
    // For a terminal record, exempt only actual growth of these three
    // decision fields compared to an otherwise identical pending record.
    let admission_size = match p.decision.as_str() {
        "accepted" | "rejected" | "cancelled" => {
            let encoded_i64 = |value: Option<i64>| value.map_or(4, |v| v.to_string().len());
            s.len()
                - p.decision.len()
                - encoded_i64(p.decided_at)
                - encoded_i64(p.accepted_revision)
                + "pending".len()
                + 8
        }
        _ => s.len(),
    };
    if admission_size > 2 * BODY + extra {
        return Err(fail("严格提案编码过大"));
    }
    Ok(s)
}
fn store_proposal(c: &Connection, p: &ConstrainedProposal) -> WorkflowResult<()> {
    let s = proposal_bytes(p)?;
    c.execute("INSERT INTO writing_proposals VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11) ON CONFLICT(id) DO UPDATE SET decision=excluded.decision,live_epoch=excluded.live_epoch,payload=excluded.payload,byte_count=excluded.byte_count",params![p.id,p.book_id,p.chapter_id,p.input.pack_id,p.client_id,p.input.idempotency_key,p.decision,p.live_epoch,s,s.len()as i64,p.created_at]).map_err(db)?;
    Ok(())
}
fn budget(c: &Connection, extra: usize) -> WorkflowResult<()> {
    let n:i64=c.query_row("SELECT coalesce((SELECT sum(byte_count) FROM writing_packs),0)+coalesce((SELECT sum(byte_count) FROM writing_proposals),0)+256*(SELECT count(*) FROM writing_proposals WHERE decision='pending' AND live_epoch IS NOT NULL)",[],|r|r.get(0)).map_err(db)?;
    if n < 0 || n.saturating_add(extra as i64) > TOTAL {
        return Err(fail("写作依据证据达到64MiB上限，不会自动清理"));
    }
    Ok(())
}
fn check_handles(p: &WritingPack) -> WorkflowResult<()> {
    if ![1, 2, 3, 4].contains(&p.handle_version)
        || (p.handle_version < 4 && !p.foreshadow_occurrences.is_empty())
        || (p.handle_version < 3
            && (!p.context_chapters.is_empty() || !p.policy.context_chapter_ids.is_empty()))
        || (p.handle_version == 1 && p.materials.iter().any(|m| m.id == "$chapter"))
    {
        return Err(fail(
            "旧依据句柄歧义或版本无效，不能作为可追溯证据读取或恢复",
        ));
    }
    Ok(())
}
fn chapter_handle(p: &WritingPack) -> String {
    if p.handle_version == 1 {
        "$chapter".into()
    } else {
        format!("chapter:{}", p.chapter_id)
    }
}
fn material_handle(p: &WritingPack, id: &str) -> String {
    if p.handle_version == 1 {
        id.into()
    } else {
        format!("material:{id}")
    }
}
fn manifest(p: &WritingPack, e: &str) -> WritingPackManifest {
    let mut entries = vec![WritingEntry {
        id: chapter_handle(p),
        title: p.chapter.title.clone(),
        kind: "chapter".into(),
        revision: p.chapter.revision,
        fields: vec!["body".into(), "$title".into()],
        metadata: serde_json::json!({"book_id":p.book_id,"volume_id":p.volume.id,"sort_order":p.chapter.sort_order}),
    }];
    entries.extend(p.materials.iter().map(|m| {
        WritingEntry {
            id: material_handle(p,&m.id),
            title: m.title.clone(),
            kind: m.kind.clone(),
            revision: m.revision,
            metadata:serde_json::json!({"book_id":m.book_id,"volume_id":m.volume_id,"chapter_id":m.chapter_id,"source_chapter_id":m.source_chapter_id,"source_revision":m.source_revision,"source_status":m.source_status,"payoff_chapter_id":m.payoff_chapter_id,"payoff_revision":m.payoff_revision,"payoff_status":m.payoff_status,"content_review_required":m.content_review_required,"story_order":m.story_order,"story_time":m.story_time,"tracking_status":m.tracking_status}),
            fields: std::iter::once("$title".into())
                .chain(m.fields.keys().cloned())
                .collect(),
        }
    }));
    if p.handle_version >= 2 {
        for (entry, material) in entries.iter_mut().skip(1).zip(p.materials.iter()) {
            entry.metadata["material_id"] = serde_json::json!(material.id);
            let occurrences: Vec<_> = p.foreshadow_occurrences.iter()
                .filter(|o| o.material_id == material.id).collect();
            if !occurrences.is_empty() {
                entry.fields.extend(occurrences.iter().map(|o| format!("occurrence:{}", o.id)));
                entry.metadata["foreshadow_occurrences"] = serde_json::json!(
                    occurrences.iter().map(|o| (&o.id, o.role)).collect::<Vec<_>>()
                );
            }
        }
    }
    entries.extend(p.context_chapters.iter().map(|frozen| {
        let c = &frozen.chapter;
        let v = &frozen.volume;
        WritingEntry {
            id: format!("context:{}", c.id),
            title: c.title.clone(),
            kind: "context_chapter".into(),
            revision: c.revision,
            fields: vec!["body".into(), "$title".into()],
            metadata: serde_json::json!({
                "book_id": p.book_id,
                "chapter_id": c.id,
                "volume_id": v.id,
                "volume_title": v.title,
                "volume_sort_order": v.sort_order,
                "sort_order": c.sort_order,
            }),
        }
    }));
    WritingPackManifest {
        id: p.id.clone(),
        book_id: p.book_id.clone(),
        chapter_id: p.chapter_id.clone(),
        policy_revision: p.policy.revision,
        chapter_revision: p.chapter.revision,
        goal: p.policy.goal.clone(),
        expires_at: p.expires_at,
        archived: p.originating_epoch != e,
        entries,
    }
}
fn field<'a>(p: &'a WritingPack, id: &str, key: &str) -> Option<&'a str> {
    if id == chapter_handle(p) {
        return match key {
            "body" => Some(&p.chapter.body),
            "$title" => Some(&p.chapter.title),
            _ => None,
        };
    }
    if p.handle_version >= 3 {
        if let Some(frozen) = id
            .strip_prefix("context:")
            .and_then(|id| p.context_chapters.iter().find(|row| row.chapter.id == id))
        {
            return match key {
                "body" => Some(&frozen.chapter.body),
                "$title" => Some(&frozen.chapter.title),
                _ => None,
            };
        }
    }
    let m = p
        .materials
        .iter()
        .find(|m| material_handle(p, &m.id) == id)?;
    if key == "$title" {
        Some(&m.title)
    } else if let Some(occurrence_id) = key.strip_prefix("occurrence:") {
        p.foreshadow_occurrences.iter().find(|o| o.id == occurrence_id && o.material_id == m.id)
            .map(|o| o.quote.as_str())
    } else {
        m.fields.get(key).map(String::as_str)
    }
}
fn citations(p: &WritingPack, i: &ConstrainedEditInput) -> WorkflowResult<()> {
    if i.pack_id != p.id
        || i.new_body.len() > BODY
        || i.citations.len() > 128
        || i.conflicts.len() > 32
        || !identity(&i.idempotency_key)
        || json(&(&i.citations, &i.conflicts))?.len() > 64 * 1024
    {
        return Err(fail("严格提案正文、引用或附注超限"));
    }
    let mut covered = HashSet::new();
    for q in &i.citations {
        let text =
            field(p, &q.entry_id, &q.field_key).ok_or_else(|| fail("引用条目或字段不存在"))?;
        if q.start >= q.end
            || q.end as usize > text.chars().count()
            || q.explanation.trim().is_empty()
            || q.explanation.len() > 4096
        {
            return Err(fail("引用范围或说明无效"));
        }
        covered.insert(&q.entry_id);
    }
    if p.policy
        .required_material_ids
        .iter()
        .any(|id| !covered.contains(&material_handle(p, id)))
    {
        return Err(fail("每张必需资料必须有有效引用，不能静默遗漏"));
    }
    for q in &i.conflicts {
        if q.explanation.trim().is_empty()
            || q.explanation.len() > 4096
            || q.entry_ids.is_empty()
            || q.entry_ids.len() > 32
            || q.entry_ids
                .iter()
                .any(|id| !p.policy.required_material_ids.contains(id))
        {
            return Err(fail("冲突条目无效"));
        }
    }
    Ok(())
}
fn live(c: &Connection, p: &WritingPack) -> WorkflowResult<()> {
    let current = policy(c, &p.chapter_id)?.ok_or_else(|| fail("约束配置不存在"))?;
    if current != p.policy
        || current.mode != "active"
        || epoch(c)? != p.originating_epoch
        || now()? >= p.expires_at
    {
        return Err(fail("约束包已过期、归档或配置改变，请重新生成"));
    }
    let (b, v, ch, m, s, vs, contexts, occurrences) = snapshot(c, &current)?;
    if b != p.book
        || v != p.volume
        || ch != p.chapter
        || m != p.materials
        || s != p.sources
        || vs != p.related_volumes
        || contexts != p.context_chapters
        || occurrences != p.foreshadow_occurrences
    {
        return Err(fail("章节、前文、必需资料或来源已改变，旧建议不可采纳"));
    }
    Ok(())
}
fn review_bytes(review: &AuthorReview) -> WorkflowResult<usize> {
    let n = json(review)?.len();
    if n > REVIEW_BYTES {
        return Err(fail("逐项作者审阅编码超64KiB"));
    }
    Ok(n)
}
fn review_items(items: &[AuthorReviewItem]) -> WorkflowResult<HashSet<&str>> {
    if !(1..=32).contains(&items.len()) {
        return Err(fail("逐项作者审阅须包含1至32份必需资料"));
    }
    let mut ids = HashSet::new();
    for item in items {
        if !identity(&item.material_id) || !ids.insert(item.material_id.as_str()) {
            return Err(fail("逐项作者审阅资料身份无效或重复"));
        }
        if item.note.len() > 4096 {
            return Err(fail("每份作者审阅备注最多4096 UTF-8字节"));
        }
    }
    Ok(ids)
}
fn validate_review(
    p: &ConstrainedProposal,
    pack: &WritingPack,
    review: &AuthorReview,
) -> WorkflowResult<()> {
    let ids = review_items(&review.items)?;
    if review.proposal_id != p.id
        || review.pack_id != p.input.pack_id
        || review.pack_id != pack.id
        || review.book_id != p.book_id
        || review.book_id != pack.book_id
        || review.chapter_id != p.chapter_id
        || review.chapter_id != pack.chapter_id
        || review.policy_revision != pack.policy.revision
        || review.chapter_revision != p.base_revision
        || review.chapter_revision != pack.chapter.revision
        || review.originating_epoch != pack.originating_epoch
        || p.live_epoch
            .as_ref()
            .is_some_and(|e| e != &review.originating_epoch)
        || !(1..MAXREV).contains(&review.revision)
        || ids.len() != pack.policy.required_material_ids.len()
        || pack
            .policy
            .required_material_ids
            .iter()
            .any(|id| !ids.contains(id.as_str()))
        || (p.decision == "accepted" && !review.all_conform())
    {
        return Err(fail("逐项作者审阅与当前提案、冻结依据或版本不匹配"));
    }
    review_bytes(review)?;
    Ok(())
}
fn pending_proposal(
    c: &Connection,
    book: &str,
    id: &str,
    e: &str,
) -> WorkflowResult<ConstrainedProposal> {
    let p = get_proposal(c, book, id)?;
    if p.decision != "pending" || p.live_epoch.as_deref() != Some(e) || epoch(c)? != e {
        return Err(fail("提案已处理、归档或书库已恢复"));
    }
    Ok(p)
}
fn finish_decision(
    c: &Connection,
    p: &mut ConstrainedProposal,
    decision: &str,
) -> WorkflowResult<()> {
    p.decision = decision.into();
    p.decided_at = Some(now()?);
    store_proposal(c, p)?;
    budget(c, 0)
}
fn cancel_pending(c: &Connection, chapter: &str) -> WorkflowResult<()> {
    c.execute(
        "UPDATE edit_proposals SET status='cancelled' WHERE chapter_id=?1 AND status='pending'",
        [chapter],
    )
    .map_err(db)?;
    let ids: Vec<(String, String)> = {
        let mut q=c.prepare("SELECT book_id,id FROM writing_proposals WHERE chapter_id=?1 AND decision='pending' AND live_epoch IS NOT NULL").map_err(db)?;
        let x = q
            .query_map([chapter], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(db)?
            .collect::<rusqlite::Result<_>>()
            .map_err(db)?;
        x
    };
    for (b, id) in ids {
        let mut p = get_proposal(c, &b, &id)?;
        p.decision = "cancelled".into();
        p.decided_at = Some(now()?);
        store_proposal(c, &p)?;
    }
    budget(c, 0)
}
impl Service {
    pub fn configure_writing_policy(
        &mut self,
        book: &str,
        chapter: &str,
        expected: Option<i64>,
        mode: &str,
        goal: &str,
        ids: Vec<String>,
        e: &str,
    ) -> WorkflowResult<WritingPolicy> {
        self.configure_policy(book, chapter, expected, mode, goal, ids, None, e)
    }
    /// Trusted author action. An empty context list explicitly clears the selection.
    pub fn configure_writing_policy_with_context(
        &mut self,
        book: &str,
        chapter: &str,
        expected: Option<i64>,
        mode: &str,
        goal: &str,
        ids: Vec<String>,
        context_chapter_ids: Vec<String>,
        e: &str,
    ) -> WorkflowResult<WritingPolicy> {
        self.configure_policy(
            book,
            chapter,
            expected,
            mode,
            goal,
            ids,
            Some(context_chapter_ids),
            e,
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn configure_policy(
        &mut self,
        book: &str,
        chapter: &str,
        expected: Option<i64>,
        mode: &str,
        goal: &str,
        ids: Vec<String>,
        contexts: Option<Vec<String>>,
        e: &str,
    ) -> WorkflowResult<WritingPolicy> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        if epoch(&tx)? != e {
            return Err(fail("书库已恢复"));
        }
        active_scope(&tx, book, chapter)?;
        let old = policy(&tx, chapter)?;
        if old.as_ref().map(|p| p.revision) != expected {
            return Err(fail("约束配置已改变"));
        }
        let p = WritingPolicy {
            chapter_id: chapter.into(),
            book_id: book.into(),
            revision: match expected {
                None => 0,
                Some(n) => n.checked_add(1).ok_or_else(|| fail("配置版本超限"))?,
            },
            mode: mode.into(),
            goal: goal.into(),
            required_material_ids: ids,
            context_chapter_ids: contexts.unwrap_or_else(|| {
                old.as_ref()
                    .map_or_else(Vec::new, |p| p.context_chapter_ids.clone())
            }),
            updated_at: now()?,
        };
        policy_shape(&p)?;
        // Pausing/disabling must remain possible when an old dependency was deleted.
        if mode == "active" {
            for id in &p.required_material_ids {
                let m = crate::materials::get_material_in(&tx, id, false)?;
                if m.book_id != book {
                    return Err(fail("必需资料跨作品"));
                }
            }
        }
        if !p.context_chapter_ids.is_empty() {
            let target = source(&tx, book, chapter)?;
            for id in &p.context_chapter_ids {
                // Existing inactive/invalid selections may stay on a paused/disabled
                // policy, but an invalid selection cannot be newly introduced.
                if mode != "active"
                    && old
                        .as_ref()
                        .is_some_and(|p| p.context_chapter_ids.contains(id))
                {
                    continue;
                }
                context_source(&tx, &p, &target, id)?;
            }
        }
        store_policy(&tx, &p)?;
        cancel_pending(&tx, chapter)?;
        tx.commit().map_err(db)?;
        Ok(p)
    }
    /// Read only the exact frozen predecessor metadata. Existing pack validation
    /// may read up to the stored 4 MiB payload; no current body is loaded.
    pub fn diagnose_writing_predecessors(
        &self,
        book: &str,
        chapter: &str,
        pack_id: &str,
        expected_epoch: &str,
    ) -> WorkflowResult<WritingPredecessorDiagnosis> {
        let tx = self.conn.unchecked_transaction().map_err(db)?;
        let epoch = epoch(&tx)?;
        if epoch != expected_epoch {
            return Err(fail("书库已变化，未核对冻结前文"));
        }
        let pack = get_pack(&tx, book, pack_id)?;
        if pack.originating_epoch != epoch
            || pack.chapter_id != chapter
            || pack.chapter.id != chapter
            || pack.policy.chapter_id != chapter
            || pack.book.id != book
            || pack.policy.book_id != book
            || pack.volume.book_id != book
            || pack.chapter.volume_id != pack.volume.id
        {
            return Err(fail("冻结依据已归档或不属于此章，未核对目录"));
        }
        let target = source(&tx, book, chapter)?;
        let target_key = directory_key(&target.volume, target.sort_order, &target.id);
        let mut entries = Vec::with_capacity(pack.context_chapters.len());
        for frozen in &pack.context_chapters {
            let chapter_id = frozen.chapter.id.clone();
            let current = optional_source(&tx, book, &chapter_id)?;
            let position = match &current {
                None => WritingPredecessorPosition::NotVisible,
                Some(prior) => match directory_key(&prior.volume, prior.sort_order, &prior.id)
                    .cmp(&target_key)
                {
                    std::cmp::Ordering::Less => WritingPredecessorPosition::Before,
                    std::cmp::Ordering::Greater => WritingPredecessorPosition::After,
                    std::cmp::Ordering::Equal => return Err(fail("前文与目标目录位置相同，未核对")),
                },
            };
            entries.push(WritingPredecessorObservation { chapter_id, position, current });
        }
        let result = WritingPredecessorDiagnosis {
            epoch, book_id: book.into(), chapter_id: chapter.into(), pack_id: pack_id.into(),
            target, entries,
        };
        if json(&result)?.len() > 64 * 1024 {
            return Err(fail("冻结前文目录核对超过64KiB"));
        }
        Ok(result)
    }

    /// Trusted metadata read in the actual directory order; never fetches bodies.
    pub fn get_writing_context_directory(
        &self,
        book: &str,
        chapter: &str,
    ) -> WorkflowResult<WritingContextDirectory> {
        let tx = self.conn.unchecked_transaction().map_err(db)?;
        let epoch = epoch(&tx)?;
        let target = source(&tx, book, chapter)?;
        let args = (
            book,
            target.volume.sort_order,
            target.volume.id.as_str(),
            target.sort_order,
            target.id.as_str(),
        );
        // Preflight row/metadata size before fetching any directory titles.
        // Parent metadata is conservatively counted once per eligible row.
        let (count, bytes): (i64, i64) = tx
            .query_row(
                &format!(
                    "SELECT count(*), coalesce(sum(length(CAST(c.id AS BLOB)) +
                length(CAST(c.title AS BLOB)) + length(CAST(v.id AS BLOB)) +
                length(CAST(v.title AS BLOB)) + 128), 0) {PRIOR_SCOPE}"
                ),
                args,
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(db)?;
        if count >= 100_000 || bytes > CONTEXT_DIRECTORY_BYTES {
            return Err(fail("可选前文目录超过16MiB或节点上限，未读取正文"));
        }
        let mut query = tx
            .prepare(&format!(
                "SELECT c.id,c.title,c.revision,c.sort_order,v.id,v.title,v.sort_order
            {PRIOR_SCOPE} ORDER BY v.sort_order,v.id,c.sort_order,c.id",
            ))
            .map_err(db)?;
        let mut rows = query.query(args).map_err(db)?;
        let mut chapters = Vec::new();
        let mut volumes = Vec::<Volume>::new();
        while let Some(r) = rows.next().map_err(db)? {
            let volume_id: String = r.get(4).map_err(db)?;
            if volumes.last().is_none_or(|v| v.id != volume_id) {
                volumes.push(Volume {
                    id: volume_id.clone(),
                    book_id: book.into(),
                    title: r.get(5).map_err(db)?,
                    sort_order: r.get(6).map_err(db)?,
                    deleted_at: None,
                });
            }
            chapters.push(crate::ChapterMetadata {
                id: r.get(0).map_err(db)?,
                volume_id,
                title: r.get(1).map_err(db)?,
                revision: r.get(2).map_err(db)?,
                sort_order: r.get(3).map_err(db)?,
            });
        }
        Ok(WritingContextDirectory {
            epoch,
            book_id: book.into(),
            chapter_id: chapter.into(),
            volumes,
            chapters,
        })
    }
    pub fn get_writing_state(&self, book: &str, chapter: &str) -> WorkflowResult<WritingState> {
        active_scope(&self.conn, book, chapter)?;
        let e = epoch(&self.conn)?;
        let p = policy(&self.conn, chapter)?;
        let ids: Vec<String> = {
            let mut q=self.conn.prepare("SELECT id FROM writing_packs WHERE book_id=?1 AND chapter_id=?2 ORDER BY id LIMIT 1001").map_err(db)?;
            let x = q
                .query_map(params![book, chapter], |r| r.get(0))
                .map_err(db)?
                .collect::<rusqlite::Result<_>>()
                .map_err(db)?;
            x
        };
        let mut packs = vec![];
        for id in ids {
            let m = read_manifest(&self.conn, book, &id, &e)?;
            packs.push(WritingPackSummary {
                id: m.id,
                policy_revision: m.policy_revision,
                chapter_revision: m.chapter_revision,
                expires_at: m.expires_at,
                archived: m.archived,
            });
        }
        let props: Vec<WritingProposalSummary> = {
            let mut q=self.conn.prepare("SELECT id,pack_id,decision,live_epoch IS NULL,created_at FROM writing_proposals WHERE book_id=?1 AND chapter_id=?2 ORDER BY id LIMIT 1001").map_err(db)?;
            let x = q
                .query_map(params![book, chapter], |r| {
                    Ok(WritingProposalSummary {
                        id: r.get(0)?,
                        pack_id: r.get(1)?,
                        decision: r.get(2)?,
                        archived: r.get(3)?,
                        created_at: r.get(4)?,
                    })
                })
                .map_err(db)?
                .collect::<rusqlite::Result<_>>()
                .map_err(db)?;
            x
        };
        Ok(WritingState {
            epoch: e,
            policy: p,
            packs,
            proposals: props,
        })
    }
    pub fn prepare_writing_pack(
        &mut self,
        book: &str,
        chapter: &str,
        pr: i64,
        cr: i64,
        e: &str,
    ) -> WorkflowResult<WritingPackManifest> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        let p = policy(&tx, chapter)?.ok_or_else(|| fail("请先配置写作约束"))?;
        if p.book_id != book || p.mode != "active" || p.revision != pr || epoch(&tx)? != e {
            return Err(fail("约束配置失效或暂停"));
        }
        let (b, v, ch, m, s, vs, contexts, occurrences) = snapshot(&tx, &p)?;
        if ch.revision != cr {
            return Err(fail("正文已改变"));
        }
        let time = now()?;
        let pack = WritingPack {
            handle_version: 4,
            id: uid(),
            book_id: book.into(),
            chapter_id: chapter.into(),
            originating_epoch: e.into(),
            created_at: time,
            expires_at: time + 1800,
            policy: p,
            book: b,
            volume: v,
            chapter: ch,
            materials: m,
            sources: s,
            related_volumes: vs,
            context_chapters: contexts,
            foreshadow_occurrences: occurrences,
        };
        validate_foreshadow(&pack)?;
        let encoded = json(&pack)?;
        if encoded.len() > PACK {
            return Err(fail("完整依据包超过4MiB，不会截断"));
        }
        let count: i64 = tx
            .query_row("SELECT count(*) FROM writing_packs", [], |r| r.get(0))
            .map_err(db)?;
        if count >= 1000 {
            return Err(fail("依据包已达1000个上限"));
        }
        budget(&tx, encoded.len())?;
        store_pack(&tx, &pack)?;
        let result = manifest(&pack, e);
        tx.commit().map_err(db)?;
        Ok(result)
    }
    pub fn get_writing_pack(&self, book: &str, id: &str) -> WorkflowResult<WritingPack> {
        get_pack(&self.conn, book, id)
    }
    pub fn get_writing_pack_manifest(
        &self,
        book: &str,
        id: &str,
    ) -> WorkflowResult<WritingPackManifest> {
        read_manifest(&self.conn, book, id, &epoch(&self.conn)?)
    }
    pub fn read_writing_pack_entry(
        &self,
        book: &str,
        id: &str,
        entry: &str,
        key: &str,
        cursor: usize,
    ) -> WorkflowResult<WritingChunk> {
        let p = get_pack(&self.conn, book, id)?;
        let text = field(&p, entry, key).ok_or_else(|| fail("包内字段不存在"))?;
        if cursor > text.len() || !text.is_char_boundary(cursor) {
            return Err(fail("分块游标不是UTF-8边界"));
        }
        let mut end = (cursor + 32768).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        Ok(WritingChunk {
            text: text[cursor..end].into(),
            next_cursor: (end < text.len()).then_some(end),
            total_bytes: text.len(),
        })
    }
    pub fn propose_constrained_edit(
        &mut self,
        book: &str,
        client: &str,
        i: ConstrainedEditInput,
    ) -> WorkflowResult<ConstrainedProposal> {
        if !identity(client) {
            return Err(fail("客户端身份无效"));
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        let prior: Option<String> = tx
            .query_row(
                "SELECT id FROM writing_proposals WHERE client_id=?1 AND idempotency_key=?2",
                params![client, i.idempotency_key],
                |r| r.get(0),
            )
            .optional()
            .map_err(db)?;
        if let Some(id) = prior {
            let p = get_proposal(&tx, book, &id)?;
            if p.input != i {
                return Err(fail("同一幂等键不能用于不同载荷"));
            }
            return Ok(p);
        }
        let pack = get_pack(&tx, book, &i.pack_id)?;
        live(&tx, &pack)?;
        citations(&pack, &i)?;
        let(count,pending,chapter_pending):(i64,i64,i64)=tx.query_row("SELECT count(*),coalesce(sum(decision='pending' AND live_epoch IS NOT NULL),0),coalesce(sum(decision='pending' AND live_epoch IS NOT NULL AND chapter_id=?1),0) FROM writing_proposals",[&pack.chapter_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(db)?;
        let legacy: i64 = tx
            .query_row(
                "SELECT count(*) FROM edit_proposals WHERE status='pending'",
                [],
                |r| r.get(0),
            )
            .map_err(db)?;
        if count >= 1000 || pending + legacy >= 100 || chapter_pending >= 20 {
            return Err(fail("严格提案或待审数量达到上限"));
        }
        let p = ConstrainedProposal {
            id: uid(),
            book_id: book.into(),
            chapter_id: pack.chapter_id,
            client_id: client.into(),
            input: i,
            base_revision: pack.chapter.revision,
            decision: "pending".into(),
            created_at: now()?,
            decided_at: None,
            accepted_revision: None,
            live_epoch: Some(epoch(&tx)?),
            author_review: None,
        };
        budget(&tx, json(&p)?.len() + 256)?;
        store_proposal(&tx, &p)?;
        tx.commit().map_err(db)?;
        Ok(p)
    }
    pub fn get_constrained_proposal(
        &self,
        book: &str,
        id: &str,
    ) -> WorkflowResult<ConstrainedProposal> {
        let p = get_proposal(&self.conn, book, id)?;
        if let Some(review) = &p.author_review {
            validate_review(&p, &get_pack(&self.conn, book, &p.input.pack_id)?, review)?;
        }
        Ok(p)
    }
    /// Trusted author action. `None` only matches a proposal with no previous review.
    pub fn save_constrained_review(
        &mut self,
        book: &str,
        id: &str,
        expected_revision: Option<i64>,
        e: &str,
        items: Vec<AuthorReviewItem>,
    ) -> WorkflowResult<AuthorReview> {
        review_items(&items)?;
        let revision = expected_revision
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| fail("逐项作者审阅版本超限"))?;
        if expected_revision.is_some_and(|r| r < 1) || !(1..MAXREV).contains(&revision) {
            return Err(fail("逐项作者审阅版本无效或超限"));
        }
        // Reject invalid sets and encodings before acquiring a write transaction.
        let p = pending_proposal(&self.conn, book, id, e)?;
        let pack = get_pack(&self.conn, book, &p.input.pack_id)?;
        let review = AuthorReview {
            proposal_id: p.id.clone(),
            pack_id: pack.id.clone(),
            book_id: p.book_id.clone(),
            chapter_id: p.chapter_id.clone(),
            originating_epoch: e.into(),
            policy_revision: pack.policy.revision,
            chapter_revision: pack.chapter.revision,
            revision,
            items,
        };
        validate_review(&p, &pack, &review)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        let mut p = pending_proposal(&tx, book, id, e)?;
        let pack = get_pack(&tx, book, &p.input.pack_id)?;
        live(&tx, &pack)?;
        validate_review(&p, &pack, &review)?;
        if let Some(previous) = &p.author_review {
            validate_review(&p, &pack, previous)?;
        }
        if p.author_review.as_ref().map(|r| r.revision) != expected_revision {
            return Err(fail("逐项作者审阅已改变，请重新读取核对"));
        }
        p.author_review = Some(review.clone());
        store_proposal(&tx, &p)?;
        budget(&tx, 0)?;
        tx.commit().map_err(db)?;
        Ok(review)
    }
    pub fn cancel_constrained_proposal(
        &mut self,
        book: &str,
        id: &str,
        client: &str,
    ) -> WorkflowResult<ConstrainedProposal> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        let mut p = get_proposal(&tx, book, id)?;
        if p.client_id != client
            || p.decision != "pending"
            || p.live_epoch.as_deref() != Some(&epoch(&tx)?)
        {
            return Err(fail("只能撤回本客户端当前待审严格提案"));
        }
        p.decision = "cancelled".into();
        p.decided_at = Some(now()?);
        store_proposal(&tx, &p)?;
        budget(&tx, 0)?;
        tx.commit().map_err(db)?;
        Ok(p)
    }
    /// Compatibility entry point: legacy callers can reject, but never approve.
    pub fn decide_constrained_proposal(
        &mut self,
        book: &str,
        id: &str,
        approve: bool,
        _pr: i64,
        _cr: i64,
        e: &str,
        _conflicts_ack: bool,
    ) -> WorkflowResult<ConstrainedProposal> {
        if approve {
            return Err(fail("采纳必须提供作者已保存的逐项审阅版本"));
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        let mut p = pending_proposal(&tx, book, id, e)?;
        finish_decision(&tx, &mut p, "rejected")?;
        tx.commit().map_err(db)?;
        Ok(p)
    }
    /// Trusted author action. All checks and manuscript/evidence writes are atomic.
    pub fn accept_reviewed_constrained_proposal(
        &mut self,
        book: &str,
        id: &str,
        pr: i64,
        cr: i64,
        e: &str,
        expected_review_revision: i64,
        conflicts_ack: bool,
    ) -> WorkflowResult<ConstrainedProposal> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        let mut p = pending_proposal(&tx, book, id, e)?;
        let pack = get_pack(&tx, book, &p.input.pack_id)?;
        live(&tx, &pack)?;
        citations(&pack, &p.input)?;
        let review = p
            .author_review
            .as_ref()
            .ok_or_else(|| fail("请先保存逐项作者审阅"))?;
        validate_review(&p, &pack, review)?;
        if review.revision != expected_review_revision
            || pack.policy.revision != pr
            || pack.chapter.revision != cr
        {
            return Err(fail("审核版本已改变，请重新读取核对"));
        }
        if !review.all_conform() {
            return Err(fail("所有必需资料均须由作者明确判断为符合"));
        }
        if !p.input.conflicts.is_empty() && !conflicts_ack {
            return Err(fail("须作者明确确认未解决冲突"));
        }
        crate::workflow::save_in_transaction(
            &tx,
            &p.chapter_id,
            &p.input.new_body,
            cr,
            &format!("strict:{}", p.id),
        )?;
        p.accepted_revision = Some(cr + 1);
        finish_decision(&tx, &mut p, "accepted")?;
        tx.commit().map_err(db)?;
        Ok(p)
    }
}
fn store_pack(c: &Connection, p: &WritingPack) -> WorkflowResult<()> {
    let s = json(p)?;
    let m = json(&manifest(p, &p.originating_epoch))?;
    if s.len() > PACK || m.len() > 256 * 1024 {
        return Err(fail("依据编码超限"));
    }
    c.execute(
        "INSERT INTO writing_packs VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![
            p.id,
            p.book_id,
            p.chapter_id,
            s,
            s.len() as i64,
            m,
            p.originating_epoch
        ],
    )
    .map_err(db)?;
    Ok(())
}
fn read_manifest(
    c: &Connection,
    b: &str,
    id: &str,
    e: &str,
) -> WorkflowResult<WritingPackManifest> {
    let(s,origin,chapter):(String,String,String)=c.query_row("SELECT CASE WHEN length(CAST(manifest AS BLOB))<=262144 THEN manifest ELSE NULL END,originating_epoch,chapter_id FROM writing_packs WHERE id=?1 AND book_id=?2",params![id,b],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(db)?;
    let mut m: WritingPackManifest = parse(&s)?;
    if m.id != id || m.book_id != b || m.chapter_id != chapter {
        return Err(fail("依据索引身份不一致"));
    }
    m.archived = origin != e;
    Ok(m)
}
pub(crate) fn clear(c: &Connection) -> WorkflowResult<()> {
    c.execute_batch(
        "DELETE FROM writing_proposals;DELETE FROM writing_packs;DELETE FROM writing_policies;",
    )
    .map_err(db)
}
pub(crate) fn all(c: &Connection) -> WorkflowResult<WritingEvidence> {
    budget(c, 0)?;
    let mut out = WritingEvidence::default();
    let mut q = c
        .prepare("SELECT chapter_id FROM writing_policies LIMIT 100001")
        .map_err(db)?;
    for id in q.query_map([], |r| r.get::<_, String>(0)).map_err(db)? {
        out.policies
            .push(policy(c, &id.map_err(db)?)?.ok_or_else(|| fail("配置缺失"))?);
    }
    for table in ["writing_packs", "writing_proposals"] {
        let mut q = c
            .prepare(&format!("SELECT book_id,id FROM {table} LIMIT 1001"))
            .map_err(db)?;
        for row in q
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(db)?
        {
            let (b, id) = row.map_err(db)?;
            if table == "writing_packs" {
                out.packs.push(get_pack(c, &b, &id)?);
            } else {
                out.proposals.push(get_proposal(c, &b, &id)?);
            }
        }
    }
    Ok(out)
}
fn validate_context(p: &WritingPack) -> WorkflowResult<()> {
    let ids: HashSet<_> = p.context_chapters.iter().map(|f| &f.chapter.id).collect();
    if p.context_chapters.len() != p.policy.context_chapter_ids.len()
        || ids.len() != p.context_chapters.len()
        || p.policy
            .context_chapter_ids
            .iter()
            .any(|id| !ids.contains(id))
    {
        return Err(fail("冻结前文章节集合与配置不一致"));
    }
    let mut previous = None;
    let target = directory_key(&p.volume, p.chapter.sort_order, &p.chapter.id);
    let mut parents = std::collections::HashMap::<&str, &Volume>::new();
    if !p.context_chapters.is_empty() {
        for v in std::iter::once(&p.volume)
            .chain(&p.related_volumes)
            .chain(p.sources.iter().map(|s| &s.volume))
        {
            if parents.insert(&v.id, v).is_some_and(|prior| prior != v) {
                return Err(fail("冻结父卷依据不一致"));
            }
        }
    }
    for f in &p.context_chapters {
        let ch = &f.chapter;
        let v = &f.volume;
        let order = directory_key(v, ch.sort_order, &ch.id);
        if ch.id == p.chapter.id
            || ch.volume_id != v.id
            || v.book_id != p.book_id
            || ch.deleted_at.is_some()
            || v.deleted_at.is_some()
            || ch.body.len() > BODY
            || order >= target
            || previous.is_some_and(|key| key >= order)
            || parents.insert(&v.id, v).is_some_and(|prior| prior != v)
            || p.sources.iter().any(|s| {
                s.id == ch.id
                    && (s.volume != *v
                        || s.sort_order != ch.sort_order
                        || s.revision != ch.revision
                        || s.title != ch.title)
            })
        {
            return Err(fail("冻结前文章节或目录依据不一致"));
        }
        previous = Some(order);
    }
    Ok(())
}

// Frozen sources are compared only with fellow snapshots, never against
// today's manuscript. All current dependencies are checked separately by live().
fn validate_foreshadow(p: &WritingPack) -> WorkflowResult<()> {
    use std::collections::HashMap;
    if p.foreshadow_occurrences.is_empty() { return Ok(()); }
    fn snapshot_position(id: &str, title: &str, revision: i64, order: i64, volume: &Volume) -> ForeshadowPosition {
        ForeshadowPosition {
            chapter_id:id.into(), chapter_title:title.into(), chapter_revision:revision,
            chapter_sort_order:order, volume_id:volume.id.clone(), volume_title:volume.title.clone(),
            volume_sort_order:volume.sort_order,
        }
    }
    let mut full_bodies = HashMap::<_, &str>::new();
    let mut known = HashMap::new();
    let mut known_volumes = HashMap::new();
    let mut counts = HashMap::new();
    let mut verify = HashMap::<_, Vec<_>>::new();
    for (ch,v) in std::iter::once((&p.chapter,&p.volume)).chain(p.context_chapters.iter().map(|f| (&f.chapter,&f.volume))) {
        full_bodies.insert(ch.id.as_str(),ch.body.as_str());
        known.insert(ch.id.as_str(),snapshot_position(&ch.id,&ch.title,ch.revision,ch.sort_order,v));
    }
    for s in &p.sources {
        let pos = snapshot_position(&s.id,&s.title,s.revision,s.sort_order,&s.volume);
        if known.insert(&s.id,pos.clone()).is_some_and(|existing| existing != pos) {
            return Err(fail("冻结章节来源与伏笔依据上下文不一致"));
        }
    }
    for v in std::iter::once(&p.volume).chain(&p.related_volumes)
        .chain(p.sources.iter().map(|s| &s.volume)).chain(p.context_chapters.iter().map(|f| &f.volume))
    {
        let key=(v.title.as_str(),v.sort_order);
        if known_volumes.insert(v.id.as_str(),key).is_some_and(|old| old!=key) {
            return Err(fail("冻结卷与伏笔依据上下文不一致"));
        }
    }
    let mut previous: Option<&str> = None;
    for o in &p.foreshadow_occurrences {
        let source=&o.source;
        let card = p.materials.iter().find(|m| m.id == o.material_id)
            .ok_or_else(|| fail("冻结伏笔出现记录未归属于必需资料"))?;
        validate_frozen_occurrence(o)?;
        let (n, changes)=counts.entry(&o.material_id).or_insert((0,0i64));
        *n+=1;
        *changes=changes.checked_add(o.revision+1).ok_or_else(|| fail("冻结伏笔版本计数溢出"))?;
        if *n>32 || card.kind!="foreshadow" || card.book_id!=p.book_id
            || o.book_id!=p.book_id || o.deleted_at.is_some() || o.created_at>p.created_at
            || *changes>card.revision || previous.is_some_and(|id| id>=o.id.as_str())
            || known.get(source.chapter_id.as_str()).is_some_and(|pos| pos!=source)
            || known_volumes.get(source.volume_id.as_str()).is_some_and(|v| *v!=(source.volume_title.as_str(),source.volume_sort_order))
        {
            return Err(fail("冻结伏笔出现记录与资料或来源不一致"));
        }
        known.insert(&source.chapter_id,source.clone());
        known_volumes.insert(&source.volume_id,(&source.volume_title,source.volume_sort_order));
        if full_bodies.contains_key(source.chapter_id.as_str()) {
            verify.entry(source.chapter_id.as_str()).or_default().push(o);
        }
        previous=Some(&o.id);
    }
    for (id,rows) in verify {
        validate_quotes(full_bodies[id],&rows)?;
    }
    Ok(())
}

impl WritingEvidence {
    pub(crate) fn has_foreshadow_format(&self) -> bool {
        self.packs.iter().any(|p| p.handle_version >= 4 || !p.foreshadow_occurrences.is_empty())
    }
    pub(crate) fn has_prior_context_format(&self) -> bool {
        self.policies
            .iter()
            .any(|p| !p.context_chapter_ids.is_empty())
            || self.packs.iter().any(|p| {
                p.handle_version >= 3
                    || !p.context_chapters.is_empty()
                    || !p.policy.context_chapter_ids.is_empty()
            })
    }
}
fn frozen_source_valid(
    p: &WritingPack,
    id: Option<&str>,
    revision: Option<i64>,
    status: &str,
) -> bool {
    match id {
        None => revision.is_none() && status == "none",
        Some(id) => {
            status == "current"
                && p.sources
                    .iter()
                    .any(|s| s.id == id && Some(s.revision) == revision)
        }
    }
}
pub(crate) fn validate_evidence(
    state: &crate::LibraryState,
    e: &WritingEvidence,
) -> WorkflowResult<()> {
    if e.policies.len() > 100000 || e.packs.len() > 1000 || e.proposals.len() > 1000 {
        return Err(fail("写作证据记录超限"));
    }
    let books: HashSet<_> = state.books.iter().map(|b| b.id.as_str()).collect();
    let chapters: std::collections::HashMap<_, _> =
        state.chapters.iter().map(|c| (c.id.as_str(), c)).collect();
    let volumes: std::collections::HashMap<_, _> =
        state.volumes.iter().map(|v| (v.id.as_str(), v)).collect();
    let mut ids = HashSet::new();
    let mut size = 0usize;
    for p in &e.policies {
        policy_shape(p)?;
        // Missing, deleted, or later same-book current dependencies may be
        // archived and repaired by the author. They are not live authorization.
        for id in std::iter::once(&p.chapter_id).chain(&p.context_chapter_ids) {
            if let Some(ch) = chapters.get(id.as_str()) {
                if !volumes
                    .get(ch.volume_id.as_str())
                    .is_some_and(|v| v.book_id == p.book_id)
                {
                    return Err(fail("配置目标或当前前文引用跨作品"));
                }
            }
        }
        if !books.contains(p.book_id.as_str()) || !ids.insert(&p.chapter_id) {
            return Err(fail("配置引用或身份无效"));
        }
    }
    ids.clear();
    for p in &e.packs {
        policy_shape(&p.policy)?;
        check_handles(p)?;
        validate_context(p)?;
        validate_foreshadow(p)?;
        if p.foreshadow_occurrences.iter().any(|o| {
            let f = &o.source;
            let wrong_book = |v: &Volume| v.book_id != p.book_id;
            volumes.get(f.volume_id.as_str()).is_some_and(|v| wrong_book(v))
                || chapters.get(f.chapter_id.as_str()).and_then(|ch| volumes.get(ch.volume_id.as_str()))
                    .is_some_and(|v| wrong_book(v))
        }) {
            return Err(fail("冻结伏笔来源标识与另一作品冲突"));
        }
        let n = json(p)?.len();
        size = size.checked_add(n).ok_or_else(|| fail("证据过大"))?;
        if n > PACK
            || !identity(&p.id)
            || !identity(&p.originating_epoch)
            || !ids.insert(&p.id)
            || !books.contains(p.book_id.as_str())
            || p.book.id != p.book_id
            || p.policy.book_id != p.book_id
            || p.policy.chapter_id != p.chapter_id
            || p.chapter.id != p.chapter_id
            || p.chapter.volume_id != p.volume.id
            || p.volume.book_id != p.book_id
            || p.policy.mode != "active"
            || p.book.deleted_at.is_some()
            || p.volume.deleted_at.is_some()
            || p.chapter.deleted_at.is_some()
            || p.chapter.body.len() > BODY
            || p.created_at < 0
            || p.expires_at.checked_sub(p.created_at) != Some(1800)
            || p.materials.len() != p.policy.required_material_ids.len()
            || p.sources.len() > 96
            || p.related_volumes.len() > 32
        {
            return Err(fail("冻结依据格式或引用无效"));
        }
        let source_ids: HashSet<_> = p.sources.iter().map(|s| s.id.as_str()).collect();
        let expected_sources: HashSet<_> = p
            .materials
            .iter()
            .flat_map(|m| {
                [
                    m.source_chapter_id.as_deref(),
                    m.payoff_chapter_id.as_deref(),
                    m.chapter_id.as_deref(),
                ]
            })
            .flatten()
            .collect();
        let volume_ids: HashSet<_> = p.related_volumes.iter().map(|v| v.id.as_str()).collect();
        let expected_volumes: HashSet<_> = p
            .materials
            .iter()
            .filter_map(|m| m.volume_id.as_deref())
            .collect();
        if source_ids.len() != p.sources.len()
            || source_ids != expected_sources
            || volume_ids.len() != p.related_volumes.len()
            || volume_ids != expected_volumes
            || p.related_volumes
                .iter()
                .any(|v| v.deleted_at.is_some() || v.book_id != p.book_id)
            || p.sources.iter().any(|s| {
                s.volume.deleted_at.is_some()
                    || s.volume.book_id != p.book_id
                    || s.id == p.chapter.id
                        && (s.sort_order != p.chapter.sort_order
                            || s.revision != p.chapter.revision
                            || s.title != p.chapter.title
                            || s.volume != p.volume)
            })
        {
            return Err(fail("冻结来源集合不一致"));
        }
        let mut st = crate::LibraryState {
            books: vec![p.book.clone()],
            volumes: vec![p.volume.clone()],
            chapters: vec![p.chapter.clone()],
        };
        for v in &p.related_volumes {
            if !st.volumes.iter().any(|x| x.id == v.id) {
                st.volumes.push(v.clone());
            }
        }
        for src in &p.sources {
            if !st.volumes.iter().any(|x| x.id == src.volume.id) {
                st.volumes.push(src.volume.clone());
            }
            if !st.chapters.iter().any(|x| x.id == src.id) {
                st.chapters.push(Chapter {
                    id: src.id.clone(),
                    volume_id: src.volume.id.clone(),
                    title: src.title.clone(),
                    body: String::new(),
                    revision: src.revision,
                    sort_order: src.sort_order,
                    deleted_at: None,
                });
            }
        }
        for f in &p.context_chapters {
            if !st.volumes.iter().any(|v| v.id == f.volume.id) {
                st.volumes.push(f.volume.clone());
            }
            if let Some(ch) = st.chapters.iter_mut().find(|ch| ch.id == f.chapter.id) {
                // A material source was already checked against this full snapshot.
                *ch = f.chapter.clone();
            } else {
                st.chapters.push(f.chapter.clone());
            }
        }
        let validated = if p.context_chapters.is_empty() {
            crate::core::validate(&st)
        } else {
            crate::core::validate_frozen_directory(&st)
        };
        validated.map_err(|_| fail("冻结书卷章格式无效"))?;
        crate::materials::validate(&st, &p.materials).map_err(|_| fail("冻结资料格式无效"))?;
        if p.materials
            .iter()
            .zip(&p.policy.required_material_ids)
            .any(|(m, id)| {
                &m.id != id
                    || m.deleted_at.is_some()
                    || !frozen_source_valid(
                        p,
                        m.source_chapter_id.as_deref(),
                        m.source_revision,
                        &m.source_status,
                    )
                    || !frozen_source_valid(
                        p,
                        m.payoff_chapter_id.as_deref(),
                        m.payoff_revision,
                        &m.payoff_status,
                    )
                    || m.content_review_required
                    || ["stale", "unavailable"].contains(&m.source_status.as_str())
                    || ["stale", "unavailable"].contains(&m.payoff_status.as_str())
            })
        {
            return Err(fail("冻结必需资料不匹配"));
        }
    }
    ids.clear();
    let mut keys = HashSet::new();
    for p in &e.proposals {
        let n = proposal_bytes(p)?.len();
        size = size.checked_add(n).ok_or_else(|| fail("证据过大"))?;
        let pack = e
            .packs
            .iter()
            .find(|x| x.id == p.input.pack_id)
            .ok_or_else(|| fail("提案包缺失"))?;
        if !identity(&p.id)
            || !identity(&p.client_id)
            || !ids.insert(&p.id)
            || !keys.insert((&p.client_id, &p.input.idempotency_key))
            || p.book_id != pack.book_id
            || p.chapter_id != pack.chapter_id
            || p.base_revision != pack.chapter.revision
            || p.created_at < 0
            || p.created_at < pack.created_at
            || !["pending", "accepted", "rejected", "cancelled"].contains(&p.decision.as_str())
            || p.live_epoch
                .as_ref()
                .is_some_and(|e| e != &pack.originating_epoch)
            || p.decided_at.is_some_and(|t| t < p.created_at)
            || ((p.decision == "pending") != p.decided_at.is_none())
            || ((p.decision == "accepted") != p.accepted_revision.is_some())
            || p.accepted_revision
                .is_some_and(|v| Some(v) != p.base_revision.checked_add(1))
        {
            return Err(fail("严格提案证据无效"));
        }
        citations(pack, &p.input)?;
        if let Some(review) = &p.author_review {
            validate_review(p, pack, review)?;
        }
    }
    if size > TOTAL as usize {
        return Err(fail("证据合计超64MiB"));
    }
    Ok(())
}
pub(crate) fn insert_all(c: &Connection, e: &WritingEvidence) -> WorkflowResult<()> {
    for p in &e.policies {
        store_policy(c, p)?;
    }
    for p in &e.packs {
        store_pack(c, p)?;
    }
    for p in &e.proposals {
        store_proposal(c, p)?;
    }
    budget(c, 0)
}
pub(crate) fn archive(c: &Connection) -> WorkflowResult<()> {
    let mut e = all(c)?;
    for p in &mut e.policies {
        if p.mode == "active" {
            p.mode = "paused".into();
            p.revision = p
                .revision
                .checked_add(1)
                .ok_or_else(|| fail("配置版本超限"))?;
            policy_shape(p)?;
            store_policy(c, p)?;
        }
    }
    for p in &mut e.proposals {
        p.live_epoch = None;
        store_proposal(c, p)?;
    }
    budget(c, 0)
}
pub(crate) fn validate_db(c: &Connection, state: &crate::LibraryState) -> WorkflowResult<Vec<WritingPack>> {
    let e = all(c)?;
    validate_evidence(state, &e)?;
    for p in &e.policies {
        let (b, r, m): (String, i64, String) = c
            .query_row(
                "SELECT book_id,revision,mode FROM writing_policies WHERE chapter_id=?1",
                [&p.chapter_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(db)?;
        if b != p.book_id || r != p.revision || m != p.mode {
            return Err(fail("配置索引不一致"));
        }
    }
    for p in &e.packs {
        let (b,ch,n,sz,m,o):(String,String,i64,i64,String,String)=c.query_row("SELECT book_id,chapter_id,byte_count,length(CAST(payload AS BLOB)),manifest,originating_epoch FROM writing_packs WHERE id=?1",[&p.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).map_err(db)?;
        if b != p.book_id
            || ch != p.chapter_id
            || n != sz
            || o != p.originating_epoch
            || m != json(&manifest(p, &p.originating_epoch))?
        {
            return Err(fail("依据索引不一致"));
        }
    }
    for p in &e.proposals {
        let row:(String,String,String,String,String,String,Option<String>,i64,i64,i64)=c.query_row("SELECT book_id,chapter_id,pack_id,client_id,idempotency_key,decision,live_epoch,byte_count,length(CAST(payload AS BLOB)),created_at FROM writing_proposals WHERE id=?1",[&p.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?))).map_err(db)?;
        if row.0 != p.book_id
            || row.1 != p.chapter_id
            || row.2 != p.input.pack_id
            || row.3 != p.client_id
            || row.4 != p.input.idempotency_key
            || row.5 != p.decision
            || row.6 != p.live_epoch
            || row.7 != row.8
            || row.9 != p.created_at
        {
            return Err(fail("提案索引不一致"));
        }
    }
    Ok(e.packs)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn setup() -> (Service, String, String, String) {
        let mut s = Service::open(":memory:").unwrap();
        let st = s
            .apply(crate::Command::CreateBook {
                title: "fixture".into(),
                author: "".into(),
            })
            .unwrap();
        let b = st.books[0].id.clone();
        let c = st.chapters[0].id.clone();
        let m=s.create_material(serde_json::from_value(json!({"book_id":b,"kind":"setting","title":"规则","fields":{"synopsis":"不能游泳"}})).unwrap()).unwrap();
        (s, b, c, m.id)
    }
    #[test]
    fn stable_ties_are_ordered_by_ids_and_frozen_before_the_target() {
        let (mut s, b, first, m) = setup();
        let v1 = s.get_chapter(&first).unwrap().volume_id;
        let st = s
            .apply(crate::Command::CreateVolume {
                book_id: b.clone(),
                title: "另一卷".into(),
            })
            .unwrap();
        let v2 = st.volumes.iter().find(|v| v.id != v1).unwrap().id.clone();
        for v in [&v1, &v2] {
            while s
                .get_outline(&b)
                .unwrap()
                .chapters
                .iter()
                .filter(|c| &c.volume_id == v)
                .count()
                < 2
            {
                s.apply(crate::Command::CreateChapter {
                    volume_id: v.clone(),
                    title: "同序".into(),
                })
                .unwrap();
            }
        }
        let mut volumes = [v1, v2];
        volumes.sort();
        let chapters = s.get_outline(&b).unwrap().chapters;
        let in_volume = |v: &str| {
            let mut ids: Vec<_> = chapters
                .iter()
                .filter(|c| c.volume_id == v)
                .map(|c| c.id.clone())
                .collect();
            ids.sort();
            ids
        };
        let early = in_volume(&volumes[0]);
        let late = in_volume(&volumes[1]);
        let target = &late[1];
        let ordered = vec![early[0].clone(), early[1].clone(), late[0].clone()];
        s.conn
            .execute_batch("UPDATE chapters SET sort_order=0; UPDATE volumes SET sort_order=0;")
            .unwrap();
        let directory = s.get_writing_context_directory(&b, target).unwrap();
        assert_eq!(
            directory
                .chapters
                .iter()
                .map(|c| c.id.clone())
                .collect::<Vec<_>>(),
            ordered
        );
        let e = s.epoch().unwrap();
        let rev = s.get_chapter(target).unwrap().revision;
        let policy = s
            .configure_writing_policy_with_context(
                &b,
                target,
                None,
                "active",
                "目录顺序",
                vec![m],
                ordered.iter().rev().cloned().collect(),
                &e,
            )
            .unwrap();
        let pack = s
            .prepare_writing_pack(&b, target, policy.revision, rev, &e)
            .unwrap();
        assert_eq!(
            pack.entries
                .iter()
                .filter(|e| e.kind == "context_chapter")
                .map(|e| e.metadata["chapter_id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ordered.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        // A legacy/equal-key active directory can be normalized without changing
        // its historical evidence. Current library restore rules remain strict.
        s.conn
            .execute("UPDATE volumes SET sort_order=1 WHERE id=?1", [&volumes[1]])
            .unwrap();
        for id in [&early[1], &late[1]] {
            s.conn
                .execute("UPDATE chapters SET sort_order=1 WHERE id=?1", [id])
                .unwrap();
        }
        let backup = s.backup().unwrap();
        s.restore_with_writing_ack(&backup, true).unwrap();
        assert_eq!(
            s.get_writing_pack(&b, &pack.id)
                .unwrap()
                .context_chapters
                .len(),
            3
        );
    }

    #[test]
    fn context_selection_projects_no_body_and_caps_prior_directory_metadata() {
        use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
        let (mut s, b, prior, _) = setup();
        let v = s.get_chapter(&prior).unwrap().volume_id;
        let st = s
            .apply(crate::Command::CreateChapter {
                volume_id: v.clone(),
                title: "目标".into(),
            })
            .unwrap();
        let target = st
            .chapters
            .iter()
            .find(|c| c.id != prior)
            .unwrap()
            .id
            .clone();
        s.conn
            .execute(
                "UPDATE chapters SET sort_order=99999 WHERE id=?1",
                [&target],
            )
            .unwrap();
        s.conn
            .authorizer(Some(|ctx: AuthContext<'_>| match ctx.action {
                AuthAction::Read {
                    table_name: "chapters",
                    column_name: "body",
                } => Authorization::Deny,
                _ => Authorization::Allow,
            })).unwrap();
        let directory = s.get_writing_context_directory(&b, &target).unwrap();
        assert_eq!(directory.chapters.len(), 1);
        assert_eq!(directory.chapters[0].id, prior);
        assert!(serde_json::to_value(directory).unwrap()["chapters"][0]
            .get("body")
            .is_none());
        s.conn.execute(
            "WITH RECURSIVE numbers(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM numbers WHERE n<4200)
             INSERT INTO chapters(id,volume_id,title,body,revision,sort_order)
             SELECT 'large-metadata-' || n,?1,printf('%04096d',n),'',0,n FROM numbers",
            [&v],
        ).unwrap();
        assert!(s
            .get_writing_context_directory(&b, &target)
            .unwrap_err()
            .message
            .contains("16MiB"));
        // Earlier candidates are capped, not the rest of an unrelated directory.
        assert!(s
            .get_writing_context_directory(&b, &prior)
            .unwrap()
            .chapters
            .is_empty());
    }

    fn configure(s: &mut Service, b: &str, c: &str, m: &str) {
        s.configure_writing_policy(
            b,
            c,
            None,
            "active",
            "过河",
            vec![m.into()],
            &s.epoch().unwrap(),
        )
        .unwrap();
    }
    fn input(id: &str, m: &str) -> ConstrainedEditInput {
        ConstrainedEditInput {
            pack_id: id.into(),
            new_body: "船".into(),
            citations: vec![WritingCitation {
                entry_id: format!("material:{m}"),
                field_key: "synopsis".into(),
                start: 0,
                end: 1,
                explanation: "遵循".into(),
            }],
            conflicts: vec![],
            idempotency_key: "key".into(),
        }
    }
    #[test]
    fn both_actual_legacy_guards_and_paused_guard() {
        let (mut s, b, c, m) = setup();
        let p = s
            .propose_edit(crate::ProposalInput {
                client_id: "a".into(),
                chapter_id: c.clone(),
                base_revision: 0,
                new_body: "bad".into(),
                reason: "".into(),
                idempotency_key: "old".into(),
            })
            .unwrap();
        configure(&mut s, &b, &c, &m);
        for mode in ["active", "paused"] {
            s.conn
                .execute("UPDATE writing_policies SET mode=?1", [mode])
                .unwrap();
            s.conn
                .execute(
                    "UPDATE edit_proposals SET status='pending' WHERE id=?1",
                    [&p.id],
                )
                .unwrap();
            assert_eq!(
                s.decide_proposal(&p.id, true, 0).unwrap_err().code,
                "writing_constraint"
            );
            assert!(s
                .propose_edit(crate::ProposalInput {
                    client_id: "a".into(),
                    chapter_id: c.clone(),
                    base_revision: 0,
                    new_body: "bad".into(),
                    reason: "".into(),
                    idempotency_key: "new".into()
                })
                .is_err());
        }
        assert_eq!(s.get_chapter(&c).unwrap().body, "");
    }
    #[test]
    fn flags_expiry_size_and_total_budget_refuse_without_mutation() {
        let (mut s, b, c, m) = setup();
        configure(&mut s, &b, &c, &m);
        let e = s.epoch().unwrap();
        s.conn
            .execute(
                "UPDATE materials SET content_review_required=1 WHERE id=?1",
                [&m],
            )
            .unwrap();
        assert!(s.prepare_writing_pack(&b, &c, 0, 0, &e).is_err());
        s.conn
            .execute(
                "UPDATE materials SET content_review_required=0 WHERE id=?1",
                [&m],
            )
            .unwrap();
        let p = s.prepare_writing_pack(&b, &c, 0, 0, &e).unwrap();
        let mut pack = s.get_writing_pack(&b, &p.id).unwrap();
        pack.created_at = 0;
        pack.expires_at = 1800;
        s.conn
            .execute(
                "UPDATE writing_packs SET payload=?1 WHERE id=?2",
                params![json(&pack).unwrap(), pack.id],
            )
            .unwrap();
        assert!(s
            .propose_constrained_edit(&b, "a", input(&pack.id, &m))
            .is_err());
        s.conn.execute("DELETE FROM writing_packs", []).unwrap();
        let p = s.prepare_writing_pack(&b, &c, 0, 0, &e).unwrap();
        let mut i = input(&p.id, &m);
        i.new_body = "x".repeat(BODY + 1);
        assert!(s.propose_constrained_edit(&b, "a", i).is_err());
        s.conn
            .execute("UPDATE writing_packs SET byte_count=?1", [TOTAL])
            .unwrap();
        assert!(s
            .propose_constrained_edit(&b, "a", input(&p.id, &m))
            .is_err());
        assert_eq!(s.get_chapter(&c).unwrap().body, "");
    }
    #[test]
    fn source_revision_change_even_same_target_revision_blocks() {
        let (mut s, b, c, _) = setup();
        let state = s
            .apply(crate::Command::CreateChapter {
                volume_id: s.get_chapter(&c).unwrap().volume_id,
                title: "来源".into(),
            })
            .unwrap();
        let source = state
            .chapters
            .iter()
            .find(|x| x.id != c)
            .unwrap()
            .id
            .clone();
        let m=s.create_material(serde_json::from_value(json!({"book_id":b,"kind":"summary","title":"摘要","fields":{"content":"人物不能游泳"},"source_chapter_id":source,"source_revision":0})).unwrap()).unwrap();
        configure(&mut s, &b, &c, &m.id);
        let p = s
            .prepare_writing_pack(&b, &c, 0, 0, &s.epoch().unwrap())
            .unwrap();
        s.apply(crate::Command::SaveChapter {
            id: source,
            body: "changed".into(),
            expected_revision: 0,
        })
        .unwrap();
        let mut i = input(&p.id, &m.id);
        i.citations[0].field_key = "content".into();
        assert!(s.propose_constrained_edit(&b, "a", i).is_err());
        assert_eq!(s.get_chapter(&c).unwrap().revision, 0);
    }
    #[test]
    fn review_quota_failure_rolls_back_payload_and_revision() {
        let (mut s, b, c, m) = setup();
        configure(&mut s, &b, &c, &m);
        let e = s.epoch().unwrap();
        let pack = s.prepare_writing_pack(&b, &c, 0, 0, &e).unwrap();
        let q = s
            .propose_constrained_edit(&b, "a", input(&pack.id, &m))
            .unwrap();
        let n: i64 = s
            .conn
            .query_row("SELECT byte_count FROM writing_proposals", [], |r| r.get(0))
            .unwrap();
        s.conn
            .execute("UPDATE writing_packs SET byte_count=?1", [TOTAL - n - 256])
            .unwrap();
        assert!(s
            .save_constrained_review(
                &b,
                &q.id,
                None,
                &e,
                vec![AuthorReviewItem {
                    material_id: m,
                    verdict: AuthorReviewVerdict::Conforms,
                    note: "must be counted".into(),
                }]
            )
            .unwrap_err()
            .message
            .contains("64MiB"));
        assert_eq!(s.get_constrained_proposal(&b, &q.id).unwrap(), q);
        assert_eq!(
            s.conn
                .query_row("SELECT byte_count FROM writing_proposals", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            n
        );
        assert_eq!(s.get_chapter(&c).unwrap().revision, 0);
    }
    #[test]
    fn decision_failure_rolls_back_body_history_and_receipt() {
        let (mut s, b, c, m) = setup();
        configure(&mut s, &b, &c, &m);
        let e = s.epoch().unwrap();
        let pack = s.prepare_writing_pack(&b, &c, 0, 0, &e).unwrap();
        let q = s
            .propose_constrained_edit(&b, "a", input(&pack.id, &m))
            .unwrap();
        let r = s
            .save_constrained_review(
                &b,
                &q.id,
                None,
                &e,
                vec![AuthorReviewItem {
                    material_id: m,
                    verdict: AuthorReviewVerdict::Conforms,
                    note: "fixture".into(),
                }],
            )
            .unwrap();
        let history = s.chapter_history(&c).unwrap().len();
        s.conn.execute_batch("CREATE TRIGGER fail_strict_decision BEFORE UPDATE ON writing_proposals BEGIN SELECT RAISE(ABORT,'injected isolated test failure'); END;").unwrap();
        assert!(s
            .accept_reviewed_constrained_proposal(&b, &q.id, 0, 0, &e, r.revision, false)
            .is_err());
        assert_eq!(
            s.get_constrained_proposal(&b, &q.id).unwrap().author_review,
            Some(r)
        );
        assert_eq!(s.get_chapter(&c).unwrap().body, "");
        assert_eq!(s.chapter_history(&c).unwrap().len(), history);
        assert_eq!(
            s.get_constrained_proposal(&b, &q.id).unwrap().decision,
            "pending"
        );
    }
}
/// External receipts deliberately exclude manuscript text and annotations: MCP
/// text envelopes escape JSON a second time. Author review still uses full data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstrainedProposalReceipt {
    pub id: String,
    pub book_id: String,
    pub chapter_id: String,
    pub pack_id: String,
    pub decision: String,
    pub base_revision: i64,
    pub accepted_revision: Option<i64>,
    pub created_at: i64,
    pub decided_at: Option<i64>,
    pub live_epoch: Option<String>,
    pub new_body_bytes: usize,
    pub citation_count: usize,
    pub conflict_count: usize,
}
impl ConstrainedProposal {
    pub fn receipt(&self) -> ConstrainedProposalReceipt {
        ConstrainedProposalReceipt {
            id: self.id.clone(),
            book_id: self.book_id.clone(),
            chapter_id: self.chapter_id.clone(),
            pack_id: self.input.pack_id.clone(),
            decision: self.decision.clone(),
            base_revision: self.base_revision,
            accepted_revision: self.accepted_revision,
            created_at: self.created_at,
            decided_at: self.decided_at,
            live_epoch: self.live_epoch.clone(),
            new_body_bytes: self.input.new_body.len(),
            citation_count: self.input.citations.len(),
            conflict_count: self.input.conflicts.len(),
        }
    }
}
