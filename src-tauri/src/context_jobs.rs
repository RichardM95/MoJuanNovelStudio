//! Durable, local metadata checks. No model calls, draft edits, or external tools.
//! The runner shares the single Service mutex and releases it after every unit.
use crate::{
    Book, BookOutline, ChapterMetadata, MaterialSummary, Service, Volume, WorkflowError,
    WorkflowResult,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
const MANIFEST_LIMIT: usize = 4 * 1024 * 1024;
const STORE_LIMIT: i64 = 64 * 1024 * 1024;
// Space reserved for terminal statuses/errors, never for more work payloads.
const TERMINAL_RESERVE: i64 = 16 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Task {
    pub id: String,
    pub book_id: String,
    pub kind: String,
    pub status: String,
    pub done: u32,
    pub total: u32,
    pub created_at: i64,
    pub updated_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub reason_code: Option<String>,
    pub error_message: Option<String>,
    pub retry_of: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskItem {
    pub ordinal: u32,
    pub kind: String,
    pub item_id: String,
    pub title: String,
    pub revision: i64,
    pub char_count: Option<u64>,
    pub material_kind: Option<String>,
    pub source_status: Option<String>,
    pub payoff_status: Option<String>,
    pub tracking_status: Option<String>,
    pub needs_review: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskDetail {
    pub task: Task,
    pub items: Vec<TaskItem>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Dependency {
    chapter: ChapterMetadata,
    volume: Volume,
    chapter_deleted: Option<i64>,
    book_deleted: Option<i64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Manifest {
    outline: BookOutline,
    materials: Vec<MaterialSummary>,
    dependencies: BTreeMap<String, Option<Dependency>>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PlannedItem {
    Chapter {
        chapter: ChapterMetadata,
        volume: Volume,
    },
    Material {
        material: MaterialSummary,
        dependencies: BTreeMap<String, Option<Dependency>>,
    },
}
fn plan(manifest: &Manifest) -> WorkflowResult<Vec<String>> {
    let mut out = Vec::new();
    for chapter in &manifest.outline.chapters {
        let volume = manifest
            .outline
            .volumes
            .iter()
            .find(|v| v.id == chapter.volume_id)
            .ok_or_else(|| err("database_error", "章节计划缺少父卷metadata"))?;
        out.push(json(&PlannedItem::Chapter {
            chapter: chapter.clone(),
            volume: volume.clone(),
        })?);
    }
    for material in &manifest.materials {
        let mut dependencies = BTreeMap::new();
        for id in [
            material.source_chapter_id.as_deref(),
            material.payoff_chapter_id.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            dependencies.insert(
                id.into(),
                manifest
                    .dependencies
                    .get(id)
                    .cloned()
                    .ok_or_else(|| err("database_error", "资料计划缺少来源metadata"))?,
            );
        }
        out.push(json(&PlannedItem::Material {
            material: material.clone(),
            dependencies,
        })?);
    }
    Ok(out)
}
fn err(code: &str, message: &str) -> WorkflowError {
    WorkflowError {
        code: code.into(),
        message: message.into(),
    }
}
fn now() -> WorkflowResult<i64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .map_err(|_| err("clock_error", "无法读取任务时间"))
}
fn json<T: Serialize>(value: &T) -> WorkflowResult<String> {
    serde_json::to_string(value).map_err(|_| err("database_error", "任务元数据编码失败"))
}
fn decode<T: serde::de::DeserializeOwned>(value: &str) -> WorkflowResult<T> {
    serde_json::from_str(value).map_err(|_| err("database_error", "任务元数据损坏，未继续执行"))
}
fn active_book(conn: &Connection, id: &str) -> WorkflowResult<Book> {
    let b = conn
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
        .ok_or_else(|| err("not_found", "作品不存在"))?;
    if b.deleted_at.is_some() {
        return Err(err("deleted", "作品在回收站，不能读取或执行检查"));
    }
    Ok(b)
}
fn epoch(conn: &Connection) -> WorkflowResult<String> {
    Ok(conn.query_row(
        "SELECT value FROM workflow_meta WHERE key='epoch'",
        [],
        |r| r.get(0),
    )?)
}
fn dependency(conn: &Connection, id: &str) -> WorkflowResult<Option<Dependency>> {
    Ok(conn.query_row("SELECT c.id,c.volume_id,c.title,c.revision,c.sort_order,c.deleted_at,v.id,v.book_id,v.title,v.sort_order,v.deleted_at,b.deleted_at FROM chapters c JOIN volumes v ON v.id=c.volume_id JOIN books b ON b.id=v.book_id WHERE c.id=?1",[id],|r|Ok(Dependency{chapter:ChapterMetadata{id:r.get(0)?,volume_id:r.get(1)?,title:r.get(2)?,revision:r.get(3)?,sort_order:r.get(4)?},chapter_deleted:r.get(5)?,volume:Volume{id:r.get(6)?,book_id:r.get(7)?,title:r.get(8)?,sort_order:r.get(9)?,deleted_at:r.get(10)?},book_deleted:r.get(11)?})).optional()?)
}
fn charge<T: Serialize>(value: &T, budget: &mut usize) -> WorkflowResult<()> {
    *budget += json(value)?.len();
    if *budget > MANIFEST_LIMIT {
        return Err(err("limit_exceeded", "检查输入metadata快照最多4MiB"));
    }
    Ok(())
}
fn collect_metadata<T: Serialize>(
    rows: impl Iterator<Item = rusqlite::Result<T>>,
    budget: &mut usize,
    max: Option<usize>,
) -> WorkflowResult<Vec<T>> {
    let mut out = Vec::new();
    for row in rows {
        if max.is_some_and(|max| out.len() >= max) {
            return Err(err("limit_exceeded", "检查最多5000条活动章节和资料"));
        }
        let row = row?;
        charge(&row, budget)?;
        out.push(row);
    }
    Ok(out)
}
fn capture(conn: &Connection, book_id: &str) -> WorkflowResult<Manifest> {
    let book = active_book(conn, book_id)?;
    let mut budget = 0;
    charge(&book, &mut budget)?;
    let mut stmt=conn.prepare("SELECT id,book_id,title,sort_order,deleted_at FROM volumes WHERE book_id=?1 AND deleted_at IS NULL ORDER BY sort_order,id")?;
    let volumes = collect_metadata(
        stmt.query_map([book_id], |r| {
            Ok(Volume {
                id: r.get(0)?,
                book_id: r.get(1)?,
                title: r.get(2)?,
                sort_order: r.get(3)?,
                deleted_at: r.get(4)?,
            })
        })?,
        &mut budget,
        None,
    )?;
    let mut stmt=conn.prepare("SELECT c.id,c.volume_id,c.title,c.revision,c.sort_order FROM chapters c JOIN volumes v ON v.id=c.volume_id WHERE v.book_id=?1 AND v.deleted_at IS NULL AND c.deleted_at IS NULL ORDER BY v.sort_order,v.id,c.sort_order,c.id")?;
    let chapters = collect_metadata(
        stmt.query_map([book_id], |r| {
            Ok(ChapterMetadata {
                id: r.get(0)?,
                volume_id: r.get(1)?,
                title: r.get(2)?,
                revision: r.get(3)?,
                sort_order: r.get(4)?,
            })
        })?,
        &mut budget,
        Some(5000),
    )?;
    let materials = crate::materials::metadata_in(conn, book_id, false, None)?;
    if chapters.len() + materials.len() > 5000 {
        return Err(err("limit_exceeded", "检查最多5000条活动章节和资料"));
    }
    for m in &materials {
        charge(m, &mut budget)?;
    }
    let mut dependencies = BTreeMap::new();
    for m in &materials {
        for id in [
            m.source_chapter_id.as_deref(),
            m.payoff_chapter_id.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if !dependencies.contains_key(id) {
                let dep = dependency(conn, id)?;
                charge(&dep, &mut budget)?;
                charge(&id, &mut budget)?;
                dependencies.insert(id.into(), dep);
            }
        }
    }
    Ok(Manifest {
        outline: BookOutline {
            book,
            volumes,
            chapters,
        },
        materials,
        dependencies,
    })
}
struct LimitedWriter {
    bytes: Vec<u8>,
    overflow: bool,
}
impl std::io::Write for LimitedWriter {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        if self.bytes.len() + data.len() > MANIFEST_LIMIT {
            self.overflow = true;
            return Err(std::io::Error::other("manifest_limit"));
        }
        self.bytes.extend_from_slice(data);
        Ok(data.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn encode_manifest(m: &Manifest) -> WorkflowResult<String> {
    let mut writer = LimitedWriter {
        bytes: Vec::new(),
        overflow: false,
    };
    if serde_json::to_writer(&mut writer, m).is_err() {
        return Err(err(
            if writer.overflow {
                "limit_exceeded"
            } else {
                "database_error"
            },
            "检查输入metadata快照最多4MiB",
        ));
    }
    String::from_utf8(writer.bytes).map_err(|_| err("database_error", "任务metadata编码无效"))
}
fn matches_manifest(conn: &Connection, book: &str, expected: &Manifest) -> WorkflowResult<bool> {
    match capture(conn, book) {
        Ok(actual) => Ok(actual == *expected),
        Err(e) if matches!(e.code.as_str(), "not_found" | "deleted" | "limit_exceeded") => {
            Ok(false)
        }
        Err(e) => Err(e),
    }
}
fn raw_task(conn: &Connection, book: &str, id: &str) -> WorkflowResult<Task> {
    let record = conn
        .query_row(
            "SELECT record FROM context_tasks WHERE id=?1 AND book_id=?2",
            params![id, book],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| err("not_found", "任务不存在或不属于该作品"))?;
    decode(&record)
}
fn stored(conn: &Connection) -> WorkflowResult<i64> {
    Ok(conn.query_row(
        "SELECT coalesce(sum(storage_bytes),0) FROM context_tasks",
        [],
        |r| r.get(0),
    )?)
}
fn projected(conn: &Connection, t: &Task, extra: i64) -> WorkflowResult<(i64, i64, String)> {
    let (size, record, status): (i64, String, String) = conn.query_row(
        "SELECT storage_bytes,record,status FROM context_tasks WHERE id=?1",
        [&t.id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let encoded = json(t)?;
    let next = size + extra + encoded.len() as i64 - record.len() as i64 + t.status.len() as i64
        - status.len() as i64;
    Ok((stored(conn)? - size + next, next, encoded))
}
fn persist(tx: &Transaction, t: &Task, extra: i64) -> WorkflowResult<()> {
    let (total, size, record) = projected(tx, t, extra)?;
    if total > STORE_LIMIT {
        return Err(err("limit_exceeded", "任务存储已满，未更改原稿"));
    }
    tx.execute(
        "UPDATE context_tasks SET status=?1,record=?2,storage_bytes=?3 WHERE id=?4",
        params![t.status, record, size, t.id],
    )?;
    Ok(())
}
fn stop(t: &mut Task, status: &str, code: &str, message: &str) -> WorkflowResult<()> {
    t.status = status.into();
    t.reason_code = Some(code.into());
    t.error_message = Some(message.into());
    t.updated_at = now()?;
    t.finished_at = Some(t.updated_at);
    Ok(())
}
fn capacities(conn: &Connection) -> WorkflowResult<()> {
    let(total,active):(i64,i64)=conn.query_row("SELECT count(*),coalesce(sum(CASE WHEN status IN ('queued','running') THEN 1 ELSE 0 END),0) FROM context_tasks",[],|r|Ok((r.get(0)?,r.get(1)?)))?;
    if total >= 100 || active >= 8 {
        return Err(err(
            "limit_exceeded",
            "最多100个保留任务、8个排队或运行任务；不会自动清理旧记录",
        ));
    }
    Ok(())
}
fn key_payload(operation: &str, book: &str, parent: Option<&str>) -> WorkflowResult<String> {
    json(&(operation, book, parent))
}
fn replay(conn: &Connection, key: &str, payload: &str) -> WorkflowResult<Option<Task>> {
    if key.trim().is_empty() || key.len() > 128 {
        return Err(err("invalid_input", "任务幂等键须非空且不超过128字节"));
    }
    if let Some((saved, record)) = conn
        .query_row(
            "SELECT request_payload,record FROM context_tasks WHERE request_key=?1",
            [key],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?
    {
        if saved != payload {
            return Err(err(
                "idempotency_conflict",
                "同一任务幂等键不能用于不同操作或目标",
            ));
        }
        return Ok(Some(decode(&record)?));
    }
    Ok(None)
}
fn enqueue(
    tx: &Transaction,
    book: &str,
    key: &str,
    payload: &str,
    parent: Option<&str>,
) -> WorkflowResult<Task> {
    capacities(tx)?;
    let manifest = capture(tx, book)?;
    let encoded = encode_manifest(&manifest)?;
    let plans = plan(&manifest)?;
    let input_epoch = epoch(tx)?;
    let time = now()?;
    let t = Task {
        id: uuid::Uuid::new_v4().to_string(),
        book_id: book.into(),
        kind: "context_check".into(),
        status: "queued".into(),
        done: 0,
        total: (manifest.outline.chapters.len() + manifest.materials.len()) as u32,
        created_at: time,
        updated_at: time,
        started_at: None,
        finished_at: None,
        reason_code: None,
        error_message: None,
        retry_of: parent.map(str::to_string),
    };
    let record = json(&t)?;
    let plan_bytes: i64 = plans
        .iter()
        .map(|p| p.len() as i64 + t.id.len() as i64 + 8)
        .sum();
    let size = (t.id.len()
        + book.len()
        + t.status.len()
        + record.len()
        + encoded.len()
        + input_epoch.len()
        + key.len()
        + payload.len()
        + 24) as i64
        + plan_bytes;
    if stored(tx)? + size > STORE_LIMIT - TERMINAL_RESERVE {
        return Err(err(
            "limit_exceeded",
            "任务metadata存储额度64MiB已满，不会删除旧报告",
        ));
    }
    tx.execute(
        "INSERT INTO context_tasks VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![
            t.id,
            book,
            t.status,
            record,
            encoded,
            input_epoch,
            key,
            payload,
            time,
            size
        ],
    )?;
    for (ordinal, record) in plans.iter().enumerate() {
        tx.execute(
            "INSERT INTO context_task_plan VALUES(?1,?2,?3)",
            params![t.id, ordinal as i64, record],
        )?;
    }
    Ok(t)
}
pub(crate) fn migrate(conn: &Connection) -> WorkflowResult<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS context_tasks(id TEXT PRIMARY KEY,book_id TEXT NOT NULL REFERENCES books(id) ON DELETE CASCADE,status TEXT NOT NULL CHECK(status IN ('queued','running','completed','cancelled','failed','interrupted')),record TEXT NOT NULL,manifest TEXT NOT NULL,input_epoch TEXT NOT NULL,request_key TEXT NOT NULL UNIQUE,request_payload TEXT NOT NULL,created_at INTEGER NOT NULL,storage_bytes INTEGER NOT NULL);CREATE TABLE IF NOT EXISTS context_task_items(task_id TEXT NOT NULL REFERENCES context_tasks(id) ON DELETE CASCADE,ordinal INTEGER NOT NULL,record TEXT NOT NULL,PRIMARY KEY(task_id,ordinal));CREATE TABLE IF NOT EXISTS context_task_plan(task_id TEXT NOT NULL REFERENCES context_tasks(id) ON DELETE CASCADE,ordinal INTEGER NOT NULL,record TEXT NOT NULL,PRIMARY KEY(task_id,ordinal));CREATE INDEX IF NOT EXISTS context_tasks_status ON context_tasks(status,created_at);")?;
    let rows = tx
        .prepare("SELECT record FROM context_tasks WHERE status IN ('queued','running')")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for record in rows {
        let mut t: Task = decode(&record)?;
        stop(
            &mut t,
            "interrupted",
            "restart",
            "应用已重启；任务未自动继续",
        )?;
        persist(&tx, &t, 0)?;
    }
    tx.commit()?;
    Ok(())
}
pub(crate) fn clear_for_restore(tx: &Transaction) -> WorkflowResult<()> {
    tx.execute("DELETE FROM context_tasks", [])?;
    Ok(())
}
fn item(
    conn: &Connection,
    book: &str,
    planned: &PlannedItem,
    ordinal: u32,
) -> WorkflowResult<Option<TaskItem>> {
    match planned {
        PlannedItem::Chapter { chapter, volume } => {
            let Some(current) = dependency(conn, &chapter.id)? else {
                return Ok(None);
            };
            if current.chapter != *chapter
                || current.volume.book_id != book
                || current.chapter_deleted.is_some()
                || current.volume.deleted_at.is_some()
                || current.book_deleted.is_some()
                || volume != &current.volume
            {
                return Ok(None);
            }
            let body: String = conn.query_row(
                "SELECT body FROM chapters WHERE id=?1 AND revision=?2",
                params![chapter.id, chapter.revision],
                |r| r.get(0),
            )?;
            Ok(Some(TaskItem {
                ordinal,
                kind: "chapter".into(),
                item_id: chapter.id.clone(),
                title: chapter.title.clone(),
                revision: chapter.revision,
                char_count: Some(body.chars().filter(|c| !c.is_whitespace()).count() as u64),
                material_kind: None,
                source_status: None,
                payoff_status: None,
                tracking_status: None,
                needs_review: false,
            }))
        }
        PlannedItem::Material {
            material: expected,
            dependencies,
        } => {
            let current = match crate::materials::metadata_in(conn, book, false, Some(&expected.id))
            {
                Ok(v) => v.into_iter().next(),
                Err(e) if matches!(e.code.as_str(), "not_found" | "deleted") => None,
                Err(e) => return Err(e),
            };
            if current.as_ref() != Some(expected) {
                return Ok(None);
            }
            for (id, expected) in dependencies {
                if &dependency(conn, id)? != expected {
                    return Ok(None);
                }
            }
            let needs_review = expected.content_review_required || matches!(expected.source_status.as_str(), "stale" | "unavailable")
                || matches!(expected.payoff_status.as_str(), "stale" | "unavailable")
                || expected.kind == "foreshadow"
                    && matches!(
                        expected.tracking_status.as_deref(),
                        Some("planned" | "planted")
                    );
            Ok(Some(TaskItem {
                ordinal,
                kind: "material".into(),
                item_id: expected.id.clone(),
                title: expected.title.clone(),
                revision: expected.revision,
                char_count: None,
                material_kind: Some(expected.kind.clone()),
                source_status: Some(expected.source_status.clone()),
                payoff_status: Some(expected.payoff_status.clone()),
                tracking_status: expected.tracking_status.clone(),
                needs_review,
            }))
        }
    }
}
impl Service {
    pub fn start_context_task(&mut self, book: &str, key: &str) -> WorkflowResult<Task> {
        let tx = self.conn.transaction()?;
        active_book(&tx, book)?;
        let payload = key_payload("start", book, None)?;
        if let Some(t) = replay(&tx, key, &payload)? {
            return Ok(t);
        }
        let t = enqueue(&tx, book, key, &payload, None)?;
        tx.commit()?;
        Ok(t)
    }
    pub fn list_context_tasks(&self, book: &str) -> WorkflowResult<Vec<Task>> {
        active_book(&self.conn, book)?;
        self.conn.prepare("SELECT record FROM context_tasks WHERE book_id=?1 ORDER BY created_at DESC,rowid DESC")?.query_map([book],|r|r.get::<_,String>(0))?.map(|r|decode(&r?)).collect()
    }
    pub fn get_context_task(&self, book: &str, id: &str) -> WorkflowResult<TaskDetail> {
        active_book(&self.conn, book)?;
        let task = raw_task(&self.conn, book, id)?;
        let items = self
            .conn
            .prepare("SELECT record FROM context_task_items WHERE task_id=?1 ORDER BY ordinal")?
            .query_map([id], |r| r.get::<_, String>(0))?
            .map(|r| decode(&r?))
            .collect::<WorkflowResult<Vec<TaskItem>>>()?;
        if items.len() != task.done as usize {
            return Err(err("database_error", "任务报告条数与已提交进度不一致"));
        }
        Ok(TaskDetail { task, items })
    }
    pub fn cancel_context_task(&mut self, book: &str, id: &str) -> WorkflowResult<Task> {
        let tx = self.conn.transaction()?;
        active_book(&tx, book)?;
        let mut t = raw_task(&tx, book, id)?;
        if matches!(t.status.as_str(), "cancelled" | "completed" | "failed") {
            return Ok(t);
        }
        if !matches!(t.status.as_str(), "queued" | "running" | "interrupted") {
            return Err(err("invalid_state", "仅可取消未完成任务"));
        }
        stop(
            &mut t,
            "cancelled",
            "cancelled_by_user",
            "用户已取消；原稿未更改",
        )?;
        persist(&tx, &t, 0)?;
        tx.commit()?;
        Ok(t)
    }
    pub fn resume_context_task(&mut self, book: &str, id: &str) -> WorkflowResult<Task> {
        let tx = self.conn.transaction()?;
        active_book(&tx, book)?;
        let mut t = raw_task(&tx, book, id)?;
        if matches!(t.status.as_str(), "queued" | "running") {
            return Ok(t);
        }
        if t.status != "interrupted" || t.reason_code.as_deref() != Some("restart") {
            return Err(err("invalid_state", "此任务不能继续，请重新检查生成新任务"));
        }
        let (saved, manifest): (String, String) = tx.query_row(
            "SELECT input_epoch,manifest FROM context_tasks WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if saved != epoch(&tx)? || !matches_manifest(&tx, book, &decode(&manifest)?)? {
            stop(
                &mut t,
                "interrupted",
                "input_changed",
                "输入基准已变化，请重新检查；旧结果不会复用",
            )?;
            persist(&tx, &t, 0)?;
            tx.commit()?;
            return Err(err("revision_conflict", "输入已变化，不能继续旧检查"));
        }
        let active: i64 = tx.query_row(
            "SELECT count(*) FROM context_tasks WHERE status IN ('queued','running')",
            [],
            |r| r.get(0),
        )?;
        if active >= 8 {
            return Err(err("limit_exceeded", "最多8个活动任务"));
        }
        t.status = "queued".into();
        t.updated_at = now()?;
        t.finished_at = None;
        t.reason_code = None;
        t.error_message = None;
        persist(&tx, &t, 0)?;
        tx.commit()?;
        Ok(t)
    }
    pub fn retry_context_task(&mut self, book: &str, id: &str, key: &str) -> WorkflowResult<Task> {
        let tx = self.conn.transaction()?;
        active_book(&tx, book)?;
        let payload = key_payload("retry", book, Some(id))?;
        if let Some(t) = replay(&tx, key, &payload)? {
            return Ok(t);
        }
        let old = raw_task(&tx, book, id)?;
        if matches!(old.status.as_str(), "queued" | "running") {
            return Err(err("invalid_state", "请先取消活动任务再重试"));
        }
        let t = enqueue(&tx, book, key, &payload, Some(id))?;
        tx.commit()?;
        Ok(t)
    }
    pub fn step_next_task(&mut self) -> WorkflowResult<Option<Task>> {
        let tx = self.conn.transaction()?;
        let Some(record)=tx.query_row("SELECT record FROM context_tasks WHERE status IN ('queued','running') ORDER BY CASE status WHEN 'running' THEN 0 ELSE 1 END,created_at,rowid LIMIT 1",[],|r|r.get::<_,String>(0)).optional()?else{return Ok(None)};
        let mut t: Task = decode(&record)?;
        let input_epoch: String = tx.query_row(
            "SELECT input_epoch FROM context_tasks WHERE id=?1",
            [&t.id],
            |r| r.get(0),
        )?;
        let previous: bool = if t.done == 0 {
            true
        } else {
            tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM context_task_items WHERE task_id=?1 AND ordinal=?2)",
                params![t.id, t.done - 1],
                |r| r.get(0),
            )?
        };
        let duplicate: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM context_task_items WHERE task_id=?1 AND ordinal=?2)",
            params![t.id, t.done],
            |r| r.get(0),
        )?;
        if t.done > t.total || !previous || duplicate {
            return Err(err("database_error", "任务游标与结果不一致，未继续"));
        }
        if input_epoch != epoch(&tx)? {
            stop(
                &mut t,
                "interrupted",
                "input_changed",
                "资料库已恢复或输入时代已变化，请重新检查",
            )?;
            persist(&tx, &t, 0)?;
            tx.commit()?;
            return Ok(Some(t));
        }
        t.status = "running".into();
        t.updated_at = now()?;
        if t.started_at.is_none() {
            t.started_at = Some(t.updated_at)
        }
        let result = if t.done < t.total {
            let encoded: String = tx.query_row(
                "SELECT record FROM context_task_plan WHERE task_id=?1 AND ordinal=?2",
                params![t.id, t.done],
                |r| r.get(0),
            )?;
            item(&tx, &t.book_id, &decode(&encoded)?, t.done)?
        } else {
            None
        };
        if t.done < t.total && result.is_none() {
            stop(
                &mut t,
                "interrupted",
                "input_changed",
                "作品输入已变化，旧检查未完成；请重新检查",
            )?;
            persist(&tx, &t, 0)?;
            tx.commit()?;
            return Ok(Some(t));
        }
        let encoded = if let Some(ref result) = result {
            t.done += 1;
            Some(json(result)?)
        } else {
            None
        };
        if t.done == t.total {
            let encoded: String = tx.query_row(
                "SELECT manifest FROM context_tasks WHERE id=?1",
                [&t.id],
                |r| r.get(0),
            )?;
            let frozen: Manifest = decode(&encoded)?;
            let committed: i64 = tx.query_row(
                "SELECT count(*) FROM context_task_items WHERE task_id=?1",
                [&t.id],
                |r| r.get(0),
            )?;
            let planned: i64 = tx.query_row(
                "SELECT count(*) FROM context_task_plan WHERE task_id=?1",
                [&t.id],
                |r| r.get(0),
            )?;
            let pending_result = i64::from(result.is_some());
            if committed + pending_result != i64::from(t.done)
                || planned != i64::from(t.total)
                || frozen.outline.chapters.len() + frozen.materials.len() != t.total as usize
            {
                return Err(err(
                    "database_error",
                    "最终任务条数、计划或游标不一致，未发布完成报告",
                ));
            }
            if matches_manifest(&tx, &t.book_id, &frozen)? {
                t.status = "completed".into();
                t.finished_at = Some(now()?);
            } else {
                stop(
                    &mut t,
                    "interrupted",
                    "input_changed",
                    "最终输入版本复核不一致，未发布完成报告",
                )?;
            }
        }
        let extra = encoded
            .as_ref()
            .map_or(0, |s| s.len() as i64 + t.id.len() as i64 + 8);
        if projected(&tx, &t, extra)?.0 > STORE_LIMIT - TERMINAL_RESERVE {
            if encoded.is_some() {
                t.done -= 1;
            }
            stop(
                &mut t,
                "failed",
                "storage_limit",
                "任务报告存储额度已满，未继续处理；原稿未更改",
            )?;
            persist(&tx, &t, 0)?;
            tx.commit()?;
            return Ok(Some(t));
        }
        if let Some(encoded) = encoded {
            tx.execute(
                "INSERT INTO context_task_items VALUES(?1,?2,?3)",
                params![t.id, t.done - 1, encoded],
            )?;
        }
        persist(&tx, &t, extra)?;
        tx.commit()?;
        Ok(Some(t))
    }
}
