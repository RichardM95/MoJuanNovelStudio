//! Review-only requests against a saved chapter. The immutable input and frozen
//! position are separate from the one-way decision, which also serves as audit.
use crate::foreshadow::{
    self, active_book, active_card, body, check_epoch, fail, identity, invalid, position,
    quote_matches, time, valid_revision, StoredOccurrence,
};
use crate::{
    CreateForeshadowOccurrence, ForeshadowOccurrence, ForeshadowOccurrenceView, ForeshadowPosition,
    ForeshadowRole, ForeshadowSourceStatus, Service, WorkflowResult,
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

const PER_CARD_PENDING: usize = 20;
const PENDING: usize = 100;
const COUNT: usize = 1000;
const BYTES: usize = 32 * 1024 * 1024;
const ROW: usize = 64 * 1024;
const BODY: u32 = 10 * 1024 * 1024;
const SCHEMA: &str = "CREATE TABLE foreshadow_proposals(id TEXT PRIMARY KEY NOT NULL,book_id TEXT NOT NULL REFERENCES books(id) ON DELETE CASCADE,material_id TEXT NOT NULL REFERENCES materials(id) ON DELETE CASCADE,client_id TEXT NOT NULL,originating_epoch TEXT NOT NULL,request_key TEXT NOT NULL,status TEXT NOT NULL,decided_at INTEGER,accepted_occurrence_id TEXT,record TEXT NOT NULL,payload TEXT NOT NULL,byte_count INTEGER NOT NULL,UNIQUE(book_id,client_id,originating_epoch,request_key),UNIQUE(accepted_occurrence_id))";
const ALIASES: &str = "id,book_id,material_id,client_id,originating_epoch,request_key,status,decided_at,accepted_occurrence_id,byte_count,CASE WHEN typeof(record)='text' AND length(CAST(record AS BLOB))<=65536 THEN record END,CASE WHEN typeof(payload)='text' AND length(CAST(payload AS BLOB))<=65536 THEN payload END";
const LIVE_PENDING: &str = "p.status='pending' AND p.originating_epoch=?2";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForeshadowProposalInput {
    pub material_id: String,
    pub material_revision: i64,
    pub chapter_id: String,
    pub chapter_revision: i64,
    pub start: u32,
    pub end: u32,
    pub quote: String,
    pub role: ForeshadowRole,
    pub note: String,
    pub reason: String,
    pub idempotency_key: String,
}
impl ForeshadowProposalInput {
    fn valid(&self) -> bool {
        [&self.material_id, &self.chapter_id, &self.idempotency_key]
            .into_iter()
            .all(|s| identity(s))
            && valid_revision(self.material_revision)
            && valid_revision(self.chapter_revision)
            && self.start < self.end
            && self.end <= BODY
            && !self.quote.is_empty()
            && self.quote.len() <= 4096
            && self.quote.chars().count() == (self.end - self.start) as usize
            && self.note.len() <= 1024
            && self.reason.len() <= 2048
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForeshadowProposalStatus {
    Pending,
    Accepted,
    Rejected,
    Cancelled,
}
impl ForeshadowProposalStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::Cancelled => "cancelled",
        }
    }
    fn parse(s: &str) -> WorkflowResult<Self> {
        match s {
            "pending" => Ok(Self::Pending),
            "accepted" => Ok(Self::Accepted),
            "rejected" => Ok(Self::Rejected),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(invalid("伏笔建议状态无效")),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForeshadowProposalAction {
    Accept,
    Reject,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecideForeshadowProposal {
    pub book_id: String,
    pub material_id: String,
    pub proposal_id: String,
    pub expected_epoch: String,
    pub expected_material_revision: i64,
    pub action: ForeshadowProposalAction,
}

/// The only response sent through the agent boundary. No client identity,
/// request key, quote, rationale, or private audit/storage fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ForeshadowProposalReceipt {
    pub id: String,
    pub book_id: String,
    pub material_id: String,
    pub material_revision: i64,
    pub chapter_id: String,
    pub chapter_revision: i64,
    pub status: ForeshadowProposalStatus,
    pub created_at: i64,
    pub decided_at: Option<i64>,
    pub accepted_occurrence_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForeshadowProposalSummary {
    pub receipt: ForeshadowProposalReceipt,
    pub material_title: String,
    pub archived: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct ForeshadowProposalPage {
    pub items: Vec<ForeshadowProposalSummary>,
    pub next_after_id: Option<String>,
    pub pending_count: usize,
}
#[derive(Debug, Clone, Serialize)]
pub struct ForeshadowProposal {
    pub receipt: ForeshadowProposalReceipt,
    pub material_title: String,
    pub source: ForeshadowPosition,
    pub source_status: ForeshadowSourceStatus,
    pub card_current: bool,
    pub archived: bool,
    pub start: u32,
    pub end: u32,
    pub quote: String,
    pub role: ForeshadowRole,
    pub note: String,
    pub reason: String,
    pub accepted_occurrence: Option<ForeshadowOccurrenceView>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    id: String,
    book_id: String,
    client_id: String,
    originating_epoch: String,
    created_at: i64,
    source: ForeshadowPosition,
}

fn required_option<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    Option::deserialize(d)
}

/// A terminal decision is never rewritten, even on replay or whole-library
/// import. Status identifies the actor class: author vs original connection.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Decision {
    status: ForeshadowProposalStatus,
    #[serde(deserialize_with = "required_option")]
    decided_at: Option<i64>,
    #[serde(deserialize_with = "required_option")]
    accepted_occurrence_id: Option<String>,
}
impl Decision {
    fn pending() -> Self {
        Self {
            status: ForeshadowProposalStatus::Pending,
            decided_at: None,
            accepted_occurrence_id: None,
        }
    }
    fn valid(&self, created_at: i64) -> bool {
        (self.status == ForeshadowProposalStatus::Pending) == self.decided_at.is_none()
            && (self.status == ForeshadowProposalStatus::Accepted)
                == self.accepted_occurrence_id.is_some()
            && self.decided_at.is_none_or(|t| t >= created_at)
            && self
                .accepted_occurrence_id
                .as_ref()
                .is_none_or(|id| identity(id))
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StoredProposal {
    record: Record,
    payload: ForeshadowProposalInput,
    decision: Decision,
}
impl StoredProposal {
    fn receipt(&self) -> ForeshadowProposalReceipt {
        let p = &self.payload;
        let r = &self.record;
        ForeshadowProposalReceipt {
            id: r.id.clone(),
            book_id: r.book_id.clone(),
            material_id: p.material_id.clone(),
            material_revision: p.material_revision,
            chapter_id: p.chapter_id.clone(),
            chapter_revision: p.chapter_revision,
            status: self.decision.status,
            created_at: r.created_at,
            decided_at: self.decision.decided_at,
            accepted_occurrence_id: self.decision.accepted_occurrence_id.clone(),
        }
    }

    fn occurrence_projection(&self, id: &str) -> ForeshadowOccurrence {
        let p = &self.payload;
        ForeshadowOccurrence {
            id: id.into(),
            book_id: self.record.book_id.clone(),
            material_id: p.material_id.clone(),
            source: self.record.source.clone(),
            start: p.start,
            end: p.end,
            quote: p.quote.clone(),
            role: p.role,
            note: p.note.clone(),
            revision: 0,
            deleted_at: None,
            created_at: self.record.created_at,
        }
    }

    fn request_key(&self) -> String {
        format!("{OCCURRENCE_KEY_PREFIX}{}", self.record.id)
    }

    fn match_projection(&self, o: &ForeshadowOccurrence) -> WorkflowResult<()> {
        let Some(expected) = &self.decision.accepted_occurrence_id else {
            return Err(invalid("伏笔建议缺少采纳关联"));
        };
        let mut projection = self.occurrence_projection(expected);
        // Lifecycle may change. The occurrence has its own creation time,
        // captured in the formal record or its frozen copies, not in the input.
        projection.revision = o.revision;
        projection.deleted_at = o.deleted_at;
        projection.created_at = o.created_at;
        if projection != *o || self.decision.decided_at.is_none_or(|t| t < o.created_at) {
            return Err(invalid("采纳建议与正式出现的冻结证据不一致"));
        }
        Ok(())
    }

    fn match_accepted(&self, row: &StoredOccurrence) -> WorkflowResult<()> {
        if row.request_epoch != self.record.originating_epoch
            || row.request_key != self.request_key()
        {
            return Err(invalid("采纳建议与正式出现的保留来源键不一致"));
        }
        self.match_projection(&row.occurrence)
    }
}

fn absent() -> crate::WorkflowError {
    fail("not_found", "当前授权范围内没有此伏笔建议或卡片")
}
fn cap() -> crate::WorkflowError {
    fail(
        "limit_exceeded",
        "伏笔建议达到单条64KiB、每卡20条待审、全库100条待审、1000条记录或32MiB存储上限",
    )
}
fn status_conflict() -> crate::WorkflowError {
    fail("invalid_state", "建议已作出其他决定")
}

pub(crate) fn schema_generation(c: &Connection) -> WorkflowResult<i64> {
    let generation: i64 = c.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let proposal: Option<(String, Option<String>)> = c
        .query_row(
            "SELECT type,sql FROM sqlite_schema WHERE name='foreshadow_proposals'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let has_occurrence = foreshadow::table_present(c)?;
    match (generation, has_occurrence, proposal) {
        (0, false, None) | (1, true, None) => Ok(generation),
        (2, true, Some((t, Some(sql)))) if t == "table" && sql == SCHEMA => Ok(generation),
        _ => Err(invalid("SQLite schema代际与伏笔出现或建议表不匹配")),
    }
}

pub(crate) fn migrate(c: &Connection) -> WorkflowResult<()> {
    let tx = rusqlite::Transaction::new_unchecked(c, TransactionBehavior::Immediate)?;
    let generation = schema_generation(&tx)?;
    if generation == 0 {
        foreshadow::create_schema(&tx)?;
    }
    if generation < 2 {
        tx.execute_batch(SCHEMA)?;
        tx.pragma_update(None, "user_version", 2)?;
    }
    tx.commit()?;
    Ok(())
}

fn encode(row: &StoredProposal) -> WorkflowResult<(String, String)> {
    let r = &row.record;
    let p = &row.payload;
    let s = &r.source;
    if ![
        &r.id,
        &r.book_id,
        &r.client_id,
        &r.originating_epoch,
        &s.volume_id,
    ]
    .into_iter()
    .all(|s| identity(s))
        || approval_from_key(&row.request_key()).is_none()
        || !p.valid()
        || r.created_at < 0
        || !row.decision.valid(r.created_at)
        || s.chapter_id != p.chapter_id
        || s.chapter_revision != p.chapter_revision
        || s.chapter_sort_order < 0
        || s.volume_sort_order < 0
        || [&s.chapter_title, &s.volume_title]
            .into_iter()
            .any(|t| t.trim().is_empty() || t.len() > 4096)
    {
        return Err(invalid("伏笔建议身份、版本、载荷或决定无效"));
    }
    let coding = |_| invalid("伏笔建议无法编码");
    let record = serde_json::to_string(r).map_err(coding)?;
    let payload = serde_json::to_string(p).map_err(coding)?;
    if record.len() + payload.len() > ROW {
        return Err(cap());
    }
    Ok((record, payload))
}
fn row_from_sql(r: &rusqlite::Row<'_>) -> WorkflowResult<StoredProposal> {
    let record = r.get::<_, Option<String>>(10)?.ok_or_else(cap)?;
    let payload = r.get::<_, Option<String>>(11)?.ok_or_else(cap)?;
    let size = record.len() + payload.len();
    if size > ROW {
        return Err(cap());
    }
    let row = StoredProposal {
        record: serde_json::from_str(&record).map_err(|_| invalid("伏笔建议记录无法解码"))?,
        payload: serde_json::from_str(&payload).map_err(|_| invalid("伏笔建议载荷无法解码"))?,
        decision: Decision {
            status: ForeshadowProposalStatus::parse(&r.get::<_, String>(6)?)?,
            decided_at: r.get(7)?,
            accepted_occurrence_id: r.get(8)?,
        },
    };
    if r.get::<_, String>(0)? != row.record.id
        || r.get::<_, String>(1)? != row.record.book_id
        || r.get::<_, String>(2)? != row.payload.material_id
        || r.get::<_, String>(3)? != row.record.client_id
        || r.get::<_, String>(4)? != row.record.originating_epoch
        || r.get::<_, String>(5)? != row.payload.idempotency_key
        || r.get::<_, i64>(9)? != size as i64
        || encode(&row)? != (record, payload)
    {
        return Err(invalid("伏笔建议索引、载荷或byte_count不一致"));
    }
    Ok(row)
}

fn store(c: &Connection, row: &StoredProposal, record: &str, payload: &str) -> WorkflowResult<()> {
    let r = &row.record;
    let p = &row.payload;
    let d = &row.decision;
    c.execute("INSERT INTO foreshadow_proposals(id,book_id,material_id,client_id,originating_epoch,request_key,status,decided_at,accepted_occurrence_id,record,payload,byte_count) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        params![r.id,r.book_id,p.material_id,r.client_id,r.originating_epoch,p.idempotency_key,d.status.as_str(),d.decided_at,d.accepted_occurrence_id,record,payload,record.len()+payload.len()])?;
    Ok(())
}

fn set_decision(
    c: &Connection,
    row: &mut StoredProposal,
    status: ForeshadowProposalStatus,
    occurrence: Option<&ForeshadowOccurrence>,
) -> WorkflowResult<()> {
    // The immutable JSON columns never grow. The admission count also
    // reserves per-row terminal audit capacity (one bounded ID, integer time,
    // and fixed enum), so rejection/cancellation cannot hit a payload quota.
    let decided_at = time()?
        .max(row.record.created_at)
        .max(occurrence.map_or(0, |o| o.created_at));
    let accepted_occurrence_id = occurrence.map(|o| o.id.clone());
    if c.execute("UPDATE foreshadow_proposals SET status=?1,decided_at=?2,accepted_occurrence_id=?3 WHERE id=?4 AND originating_epoch=?5 AND status='pending'",
        params![status.as_str(),decided_at,accepted_occurrence_id,row.record.id,row.record.originating_epoch])? != 1
    {
        return Err(status_conflict());
    }
    row.decision = Decision {
        status,
        decided_at: Some(decided_at),
        accepted_occurrence_id,
    };
    Ok(())
}

fn quota(c: &Connection, added: usize, adding_card: Option<(&str, &str)>) -> WorkflowResult<()> {
    let (n, bytes): (usize, usize) = c.query_row(
        "SELECT count(*),coalesce(sum(length(CAST(record AS BLOB))+length(CAST(payload AS BLOB))),0) FROM foreshadow_proposals", [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if n + usize::from(adding_card.is_some()) > COUNT || bytes.saturating_add(added) > BYTES {
        return Err(cap());
    }
    if let Some((card, epoch)) = adding_card {
        let (n, per_card): (usize, usize) = c.query_row(
            "SELECT count(*),coalesce(sum(material_id=?2),0) FROM foreshadow_proposals WHERE originating_epoch=?1 AND status='pending'",
            params![epoch,card], |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if n >= PENDING || per_card >= PER_CARD_PENDING {
            return Err(cap());
        }
    }
    Ok(())
}

fn load_one(
    c: &Connection,
    predicate: &str,
    p: impl rusqlite::Params,
) -> WorkflowResult<Option<StoredProposal>> {
    let mut q = c.prepare(&format!(
        "SELECT {ALIASES} FROM foreshadow_proposals WHERE {predicate}"
    ))?;
    let mut rows = q.query(p)?;
    rows.next()?.map(row_from_sql).transpose()
}

fn check_ids(ids: &[&str]) -> WorkflowResult<()> {
    if ids.iter().all(|s| identity(s)) {
        Ok(())
    } else {
        Err(invalid("伏笔建议标识无效"))
    }
}

struct Card {
    title: String,
    revision: i64,
    available: bool,
}
fn card(c: &Connection, book: &str, id: &str) -> WorkflowResult<Card> {
    c.query_row("SELECT m.title,m.revision,m.deleted_at IS NULL AND m.content_review_required=0
        AND (m.volume_id IS NULL OR EXISTS(SELECT 1 FROM volumes v WHERE v.id=m.volume_id AND v.book_id=m.book_id AND v.deleted_at IS NULL))
        AND (m.chapter_id IS NULL OR EXISTS(SELECT 1 FROM chapters c JOIN volumes v ON v.id=c.volume_id WHERE c.id=m.chapter_id AND v.book_id=m.book_id AND c.deleted_at IS NULL AND v.deleted_at IS NULL))
        FROM materials m WHERE m.book_id=?1 AND m.id=?2 AND m.kind='foreshadow' ",
        params![book,id],|r| Ok(Card {title:r.get(0)?,revision:r.get(1)?,available:r.get(2)?})).optional()?.ok_or_else(absent)
}
fn fresh_card(c: &Connection, book: &str, material: &str, expected: i64) -> WorkflowResult<()> {
    active_card(c, book, material)?;
    let m = card(c, book, material)?;
    if !m.available || m.revision != expected {
        return Err(fail(
            "revision_conflict",
            "伏笔卡版本已变或需要作者复核，请重新提交建议",
        ));
    }
    Ok(())
}

fn detail(c: &Connection, row: StoredProposal, epoch: &str) -> WorkflowResult<ForeshadowProposal> {
    let receipt = row.receipt();
    let r = &row.record;
    let material = card(c, &r.book_id, &row.payload.material_id)?;
    let source_status = match position(c, &r.book_id, &row.payload.chapter_id)? {
        None => ForeshadowSourceStatus::Unavailable,
        Some(s) if s == r.source => ForeshadowSourceStatus::Current,
        Some(_) => ForeshadowSourceStatus::Stale,
    };
    let accepted_occurrence = match receipt.accepted_occurrence_id.as_deref() {
        Some(id) => foreshadow::find_row(c, id)?
            .map(|o| {
                row.match_accepted(&o)?;
                foreshadow::view(c, o.occurrence)
            })
            .transpose()?,
        None => None,
    };
    let archived = row.record.originating_epoch != epoch;
    if !archived && receipt.accepted_occurrence_id.is_some() && accepted_occurrence.is_none() {
        return Err(invalid("现行建议的正式出现关联已缺失"));
    }
    let p = row.payload;
    Ok(ForeshadowProposal {
        receipt,
        material_title: material.title,
        source: row.record.source,
        source_status,
        card_current: material.available && material.revision == p.material_revision,
        archived,
        start: p.start,
        end: p.end,
        quote: p.quote,
        role: p.role,
        note: p.note,
        reason: p.reason,
        accepted_occurrence,
    })
}

impl Service {
    pub fn propose_foreshadow_occurrence(
        &mut self,
        book: &str,
        client: &str,
        epoch: &str,
        input: ForeshadowProposalInput,
    ) -> WorkflowResult<ForeshadowProposalReceipt> {
        check_ids(&[book, client, epoch])?;
        if !input.valid() {
            return Err(invalid("建议版本、Unicode标量区间、引文或附言无效（UTF-8字节上限：引文4096 / 备注1024 / 理由2048）"));
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        check_epoch(&tx, epoch)?;
        active_book(&tx, book)?;
        if let Some(row) = load_one(
            &tx,
            "book_id=?1 AND client_id=?2 AND originating_epoch=?3 AND request_key=?4",
            params![book, client, epoch, input.idempotency_key],
        )? {
            if row.payload != input {
                return Err(fail("idempotency_conflict", "同一请求键的建议载荷不能改变"));
            }
            return Ok(row.receipt());
        }
        fresh_card(&tx, book, &input.material_id, input.material_revision)?;
        let source = position(&tx, book, &input.chapter_id)?.ok_or_else(absent)?;
        if source.chapter_revision != input.chapter_revision {
            return Err(fail(
                "revision_conflict",
                "已保存的来源正文版本已变，请重新提交建议",
            ));
        }
        if !quote_matches(
            &body(&tx, &input.chapter_id)?,
            input.start,
            input.end,
            &input.quote,
        ) {
            return Err(invalid("引文必须与已保存正文的Unicode标量区间完全相同"));
        }
        let row = StoredProposal {
            record: Record {
                id: uuid::Uuid::new_v4().to_string(),
                book_id: book.into(),
                client_id: client.into(),
                originating_epoch: epoch.into(),
                created_at: time()?,
                source,
            },
            payload: input,
            decision: Decision::pending(),
        };
        let (record, payload) = encode(&row)?;
        quota(
            &tx,
            record.len() + payload.len(),
            Some((&row.payload.material_id, epoch)),
        )?;
        store(&tx, &row, &record, &payload)?;
        tx.commit()?;
        Ok(row.receipt())
    }

    pub fn get_own_foreshadow_proposal(
        &self,
        book: &str,
        client: &str,
        epoch: &str,
        id: &str,
    ) -> WorkflowResult<ForeshadowProposalReceipt> {
        Ok(own(&self.conn, book, client, epoch, id)?.receipt())
    }
    pub fn cancel_own_foreshadow_proposal(
        &mut self,
        book: &str,
        client: &str,
        epoch: &str,
        id: &str,
    ) -> WorkflowResult<ForeshadowProposalReceipt> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut row = own(&tx, book, client, epoch, id)?;
        match row.decision.status {
            ForeshadowProposalStatus::Pending => {
                set_decision(&tx, &mut row, ForeshadowProposalStatus::Cancelled, None)?
            }
            ForeshadowProposalStatus::Cancelled => (),
            _ => return Err(status_conflict()),
        }
        tx.commit()?;
        Ok(row.receipt())
    }

    pub fn list_foreshadow_proposals(
        &self,
        book: &str,
        material: Option<&str>,
        pending_only: bool,
        after: Option<&str>,
        limit: Option<u32>,
        epoch: &str,
    ) -> WorkflowResult<ForeshadowProposalPage> {
        let n = limit.unwrap_or(10) as usize;
        check_ids(&[book, epoch])?;
        if !(1..=20).contains(&n)
            || after.is_some_and(|id| !identity(id))
            || material.is_some_and(|id| !identity(id))
        {
            return Err(invalid("伏笔建议分页限制为1–20，标识必须有效"));
        }
        let c = &self.conn;
        check_epoch(c, epoch)?;
        active_book(c, book)?;
        if let Some(id) = material {
            card(c, book, id)?;
        }
        let pending_count: usize = c.query_row(&format!("SELECT count(*) FROM foreshadow_proposals p WHERE p.book_id=?1 AND {LIVE_PENDING} AND (?3 IS NULL OR p.material_id=?3)"),
            params![book,epoch,material], |r| r.get(0))?;
        let mut q = c.prepare(&format!("SELECT {ALIASES} FROM foreshadow_proposals p WHERE p.book_id=?1 AND (?3 IS NULL OR p.material_id=?3) AND (NOT ?4 OR ({LIVE_PENDING})) AND p.id>?5 ORDER BY p.id LIMIT ?6"))?;
        let mut rows = q.query(params![
            book,
            epoch,
            material,
            pending_only,
            after.unwrap_or(""),
            n + 1
        ])?;
        let mut items: Vec<ForeshadowProposalSummary> = Vec::with_capacity(n);
        let mut next_after_id = None;
        while let Some(r) = rows.next()? {
            if items.len() == n {
                next_after_id = items.last().map(|item| item.receipt.id.clone());
                break;
            }
            let row = row_from_sql(r)?;
            let material_title = card(c, book, &row.payload.material_id)?.title;
            items.push(ForeshadowProposalSummary {
                receipt: row.receipt(),
                material_title,
                archived: row.record.originating_epoch != epoch,
            });
        }
        Ok(ForeshadowProposalPage {
            items,
            next_after_id,
            pending_count,
        })
    }

    pub fn get_foreshadow_proposal(
        &self,
        book: &str,
        material: &str,
        id: &str,
        epoch: &str,
    ) -> WorkflowResult<ForeshadowProposal> {
        let row = trusted(&self.conn, book, material, id, epoch)?;
        detail(&self.conn, row, epoch)
    }

    pub fn decide_foreshadow_proposal(
        &mut self,
        i: DecideForeshadowProposal,
    ) -> WorkflowResult<ForeshadowProposal> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut row = trusted(
            &tx,
            &i.book_id,
            &i.material_id,
            &i.proposal_id,
            &i.expected_epoch,
        )?;
        if row.record.originating_epoch != i.expected_epoch {
            return Err(fail(
                "invalid_state",
                "备份中的建议已归档，不可执行；如需登记请重新提交",
            ));
        }
        if row.payload.material_revision != i.expected_material_revision {
            return Err(fail("revision_conflict", "作者确认的卡片版本与建议不符"));
        }
        let target = match i.action {
            ForeshadowProposalAction::Accept => ForeshadowProposalStatus::Accepted,
            ForeshadowProposalAction::Reject => ForeshadowProposalStatus::Rejected,
        };
        if row.decision.status == target {
            return detail(&tx, row, &i.expected_epoch);
        }
        if row.decision.status != ForeshadowProposalStatus::Pending {
            return Err(status_conflict());
        }
        let occurrence = if i.action == ForeshadowProposalAction::Accept {
            fresh_card(
                &tx,
                &i.book_id,
                &i.material_id,
                i.expected_material_revision,
            )?;
            let p = &row.payload;
            if position(&tx, &i.book_id, &p.chapter_id)?.as_ref() != Some(&row.record.source) {
                return Err(fail(
                    "revision_conflict",
                    "来源正文或卷章位置已变，请重新提交建议",
                ));
            }
            let created = foreshadow::create_in_transaction(
                &tx,
                CreateForeshadowOccurrence {
                    book_id: i.book_id,
                    material_id: i.material_id,
                    chapter_id: p.chapter_id.clone(),
                    chapter_revision: p.chapter_revision,
                    start: p.start,
                    end: p.end,
                    quote: p.quote.clone(),
                    role: p.role,
                    note: p.note.clone(),
                    epoch: i.expected_epoch.clone(),
                    request_key: row.request_key(),
                },
                false,
            )?;
            Some(created.occurrence)
        } else {
            None
        };
        set_decision(&tx, &mut row, target, occurrence.as_ref())?;
        let result = detail(&tx, row, &i.expected_epoch)?;
        tx.commit()?;
        Ok(result)
    }
}

fn own(
    c: &Connection,
    book: &str,
    client: &str,
    epoch: &str,
    id: &str,
) -> WorkflowResult<StoredProposal> {
    check_ids(&[book, client, epoch, id])?;
    check_epoch(c, epoch)?;
    active_book(c, book)?;
    load_one(
        c,
        "book_id=?1 AND client_id=?2 AND originating_epoch=?3 AND id=?4",
        params![book, client, epoch, id],
    )?
    .ok_or_else(absent)
}
fn trusted(
    c: &Connection,
    book: &str,
    material: &str,
    id: &str,
    epoch: &str,
) -> WorkflowResult<StoredProposal> {
    check_ids(&[book, material, epoch, id])?;
    check_epoch(c, epoch)?;
    active_book(c, book)?;
    load_one(
        c,
        "book_id=?1 AND material_id=?2 AND id=?3",
        params![book, material, id],
    )?
    .ok_or_else(absent)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProposalBackup {
    pub(crate) epoch: Option<String>,
    pub(crate) rows: Vec<StoredProposal>,
}

/// Gate on types and raw byte lengths before materializing untrusted SQLite values.
pub(crate) fn preflight(c: &Connection) -> WorkflowResult<()> {
    quota(c, 0, None)?;
    let invalid_count: usize = c.query_row(
        "SELECT count(*) FROM foreshadow_proposals WHERE typeof(record)!='text' OR typeof(payload)!='text'
        OR typeof(byte_count)!='integer' OR byte_count>65536
        OR byte_count!=length(CAST(record AS BLOB))+length(CAST(payload AS BLOB))
        OR typeof(status)!='text' OR status NOT IN ('pending','accepted','rejected','cancelled')
        OR typeof(decided_at) NOT IN ('integer','null')
        OR typeof(accepted_occurrence_id) NOT IN ('text','null') OR length(CAST(accepted_occurrence_id AS BLOB))>128
        OR typeof(id)!='text' OR length(CAST(id AS BLOB))>128
        OR typeof(book_id)!='text' OR length(CAST(book_id AS BLOB))>128
        OR typeof(material_id)!='text' OR length(CAST(material_id AS BLOB))>128
        OR typeof(client_id)!='text' OR length(CAST(client_id AS BLOB))>128
        OR typeof(originating_epoch)!='text' OR length(CAST(originating_epoch AS BLOB))>128
        OR typeof(request_key)!='text' OR length(CAST(request_key AS BLOB))>128", [], |r| r.get(0))?;
    if invalid_count != 0 {
        return Err(invalid("伏笔建议SQLite索引或储存大小无效"));
    }
    Ok(())
}

pub(crate) fn all(c: &Connection) -> WorkflowResult<Vec<StoredProposal>> {
    preflight(c)?;
    let mut q = c.prepare(&format!(
        "SELECT {ALIASES} FROM foreshadow_proposals ORDER BY id"
    ))?;
    let mut sql = q.query([])?;
    let mut rows = Vec::new();
    while let Some(row) = sql.next()? {
        rows.push(row_from_sql(row)?);
    }
    Ok(rows)
}
pub(crate) fn insert_all(c: &Connection, rows: &[StoredProposal]) -> WorkflowResult<()> {
    for row in rows {
        let (record, payload) = encode(row)?;
        store(c, row, &record, &payload)?;
    }
    Ok(())
}
pub(crate) fn backup_epoch(c: &Connection) -> WorkflowResult<String> {
    let epoch: Option<String> = c.query_row(
        "SELECT CASE WHEN typeof(value)='text' AND length(CAST(value AS BLOB)) BETWEEN 1 AND 128 THEN value END FROM workflow_meta WHERE key='epoch'",
        [], |r| r.get(0))?;
    epoch
        .filter(|s| identity(s))
        .ok_or_else(|| invalid("备份伏笔建议的书库代际无效"))
}

pub(crate) fn validate(
    state: &crate::LibraryState,
    materials: &[crate::Material],
    occurrences: &[StoredOccurrence],
    copies: &HashMap<&str, &ForeshadowOccurrence>,
    rows: &[StoredProposal],
    source_epoch: Option<&str>,
    mut check_quotes: impl FnMut(&crate::Chapter, &[&ForeshadowOccurrence]) -> WorkflowResult<()>,
) -> WorkflowResult<()> {
    if rows.len() > COUNT {
        return Err(cap());
    }
    if source_epoch.is_some_and(|e| !identity(e)) {
        return Err(invalid("备份伏笔建议的书库代际无效"));
    }
    let books: HashSet<_> = state.books.iter().map(|b| b.id.as_str()).collect();
    let chapters: HashMap<_, _> = state.chapters.iter().map(|c| (c.id.as_str(), c)).collect();
    let volumes: HashMap<_, _> = state
        .volumes
        .iter()
        .map(|v| (v.id.as_str(), v.book_id.as_str()))
        .collect();
    let cards: HashMap<_, _> = materials.iter().map(|m| (m.id.as_str(), m)).collect();
    let occurrences: HashMap<_, _> = occurrences
        .iter()
        .map(|o| (o.occurrence.id.as_str(), o))
        .collect();
    let mut proposals_by_id = HashMap::new();
    let mut keys = HashSet::new();
    let mut accepted_ids = HashSet::new();
    let mut consumed_card_revisions = HashSet::new();
    let mut pending_epochs = HashMap::<&str, usize>::new();
    let mut pending_cards = HashMap::<(&str, &str), usize>::new();
    let mut quotes = HashMap::<&str, Vec<ForeshadowOccurrence>>::new();
    let mut bytes = 0;
    for row in rows {
        let r = &row.record;
        let p = &row.payload;
        let (record, payload) = encode(row)?;
        bytes += record.len() + payload.len();
        if bytes > BYTES {
            return Err(cap());
        }
        let card = cards.get(p.material_id.as_str()).ok_or_else(absent)?;
        let chapter = chapters.get(p.chapter_id.as_str());
        if proposals_by_id.insert(r.id.as_str(), row).is_some()
            || !keys.insert((
                &r.book_id,
                &r.client_id,
                &r.originating_epoch,
                &p.idempotency_key,
            ))
            || !books.contains(r.book_id.as_str())
            || card.book_id != r.book_id
            || card.kind != "foreshadow"
            || p.material_revision > card.revision
            || chapter.is_some_and(|c| {
                p.chapter_revision > c.revision
                    || volumes.get(c.volume_id.as_str()).copied() != Some(r.book_id.as_str())
            })
            || volumes
                .get(r.source.volume_id.as_str())
                .is_some_and(|id| *id != r.book_id)
        {
            return Err(invalid("备份伏笔建议身份、卡片、作品、来源或版本不一致"));
        }
        if row.decision.status == ForeshadowProposalStatus::Pending {
            let n = pending_epochs.entry(&r.originating_epoch).or_default();
            let m = pending_cards
                .entry((&r.originating_epoch, &p.material_id))
                .or_default();
            *n += 1;
            *m += 1;
            if *n > PENDING || *m > PER_CARD_PENDING {
                return Err(cap());
            }
        }
        if let Some(id) = &row.decision.accepted_occurrence_id {
            if !accepted_ids.insert(id) || p.material_revision >= card.revision {
                return Err(invalid("备份采纳关联重复或伏笔卡版本不一致"));
            }
            // Acceptance advances this card's revision exactly once. Two
            // recorded acceptances in the same origin cannot consume it twice.
            if !consumed_card_revisions.insert((
                &r.originating_epoch,
                &r.book_id,
                &p.material_id,
                p.material_revision,
            )) {
                return Err(invalid("备份中同一卡片版本在同一书库代际被重复采纳"));
            }
            if let Some(found) = occurrences.get(id.as_str()) {
                row.match_accepted(found)?;
            } else if source_epoch == Some(r.originating_epoch.as_str()) {
                // Existing owned current cards have no independent hard-delete
                // operation for occurrences. A recycled occurrence still exists.
                return Err(invalid("备份现行采纳建议指向不存在的正式出现"));
            } else if let Some(copy) = copies.get(id.as_str()) {
                row.match_projection(copy)?;
            }
        }
        if chapter.is_some_and(|c| p.chapter_revision == c.revision) {
            quotes
                .entry(&p.chapter_id)
                .or_default()
                .push(row.occurrence_projection(&r.id));
        }
    }
    // Check from the formal side too. Deleting the entire audit row is not
    // equivalent to a manual write. Pre-feature manual keys could never
    // include this marker, so genuinely old inputs still validate with no rows.
    for occurrence in occurrences.values() {
        if let Some(id) = approval_from_key(&occurrence.request_key) {
            let audit = proposals_by_id
                .get(id)
                .ok_or_else(|| invalid("正式出现缺少对应的伏笔建议采纳审计"))?;
            audit.match_accepted(occurrence)?;
        }
    }
    for (id, rows) in quotes {
        check_quotes(chapters[id], &rows.iter().collect::<Vec<_>>())?;
    }
    Ok(())
}

/// Old and new public manual keys reject control characters. Only this
/// crate's human-approved path may use the internal approval namespace.
const OCCURRENCE_KEY_PREFIX: &str = "\u{1f}foreshadow-proposal:";
pub(crate) fn approval_from_key(key: &str) -> Option<&str> {
    let id = key.strip_prefix(OCCURRENCE_KEY_PREFIX)?;
    let uuid = uuid::Uuid::parse_str(id).ok()?;
    (uuid.hyphenated().to_string() == id).then_some(id)
}
