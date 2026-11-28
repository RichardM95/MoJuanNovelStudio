use crate::{FanqieChapter, Service, WorkflowError, WorkflowResult};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use uuid::Uuid;

fn problem(message: &str) -> WorkflowError {
    WorkflowError {
        code: "fanqie_draft".into(),
        message: message.into(),
    }
}

pub(crate) fn migrate(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS fanqie_bindings(
        epoch TEXT, book TEXT, volume TEXT, target TEXT NOT NULL, PRIMARY KEY(epoch,book,volume));
        CREATE TABLE IF NOT EXISTS fanqie_jobs(
        id TEXT PRIMARY KEY, epoch TEXT, book TEXT, request_key TEXT, owner TEXT,
        active INTEGER NOT NULL, items TEXT NOT NULL, UNIQUE(epoch,book,request_key));
        CREATE INDEX IF NOT EXISTS fanqie_pending ON fanqie_jobs(epoch,book,active);",
    )
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Binding {
    pub volume_id: String,
    pub list_url: String,
    pub book: String,
    pub volume: String,
    pub first_number: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JobRequest {
    chapter_ids: Vec<String>,
    idempotency_key: String,
}

#[derive(Serialize, Deserialize)]
struct Item {
    chapter: FanqieChapter,
    target: Binding,
    number: u32,
    state: String,
    detail: String,
    claim: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Receipt {
    pub job_id: String,
    pub claim: String,
    pub state: String,
    pub detail: String,
}

fn decode<T: serde::de::DeserializeOwned>(raw: &str) -> WorkflowResult<T> {
    serde_json::from_str(raw).map_err(|_| problem("任务记录无法读取"))
}
fn serialize(value: &impl Serialize) -> WorkflowResult<String> {
    serde_json::to_string(value).map_err(|_| problem("任务记录无法保存"))
}
fn summary(job: &str, items: &[Item]) -> Value {
    json!({"job_id":job,"chapters":items.iter().map(|i|
        json!({"chapter_id":i.chapter.chapter_id,"title":i.chapter.title,"revision":i.chapter.revision,
            "number":i.number,"state":i.state,"detail":i.detail})).collect::<Vec<_>>()})
}
impl Service {
    pub(crate) fn bind_fanqie(&self, book: &str, target: Binding) -> WorkflowResult<Value> {
        let dir = self.fanqie_directory(book)?;
        if !dir.chapters.iter().any(|c| c.volume_id == target.volume_id)
            || !target
                .list_url
                .starts_with("https://fanqienovel.com/main/writer/")
            || target.list_url.len() > 2048
            || target.first_number == 0
            || target.first_number > 999_999_999
            || [&target.book, &target.volume]
                .iter()
                .any(|s| s.trim().is_empty() || s.len() > 512 || s.trim() != s.as_str())
        {
            return Err(problem("请核对番茄作品、卷、草稿箱地址和首章编号"));
        }
        self.conn.execute(
            "INSERT INTO fanqie_bindings VALUES(?1,?2,?3,?4)
            ON CONFLICT(epoch,book,volume) DO UPDATE SET target=excluded.target",
            params![dir.epoch, book, target.volume_id, serialize(&target)?],
        )?;
        Ok(json!({"bound":true}))
    }
    fn fanqie_binding(&self, epoch: &str, book: &str, volume: &str) -> WorkflowResult<Binding> {
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT target FROM fanqie_bindings WHERE epoch=?1 AND book=?2 AND volume=?3",
                params![epoch, book, volume],
                |r| r.get(0),
            )
            .optional()?;
        decode(&raw.ok_or_else(|| problem("请先在扩展中绑定这部作品的目标卷"))?)
    }
    pub(crate) fn create_fanqie_job(
        &self,
        book: &str,
        owner: &str,
        req: JobRequest,
    ) -> WorkflowResult<Value> {
        let ids = &req.chapter_ids;
        if ids.is_empty()
            || ids.len() > 50
            || ids.iter().collect::<HashSet<_>>().len() != ids.len()
            || req.idempotency_key.trim().is_empty()
            || req.idempotency_key.len() > 128
        {
            return Err(problem("请指定 1 至 50 个不同章节和有效请求键"));
        }
        let dir = self.fanqie_directory(book)?;
        let existing: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT id,items FROM fanqie_jobs
            WHERE epoch=?1 AND book=?2 AND request_key=?3",
                params![dir.epoch, book, req.idempotency_key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((job, raw)) = existing {
            let items: Vec<Item> = decode(&raw)?;
            if !items.iter().map(|i| &i.chapter.chapter_id).eq(ids.iter()) {
                return Err(problem("同一请求键指定了不同章节"));
            }
            self.conn.execute(
                "UPDATE fanqie_jobs SET owner=?1 WHERE id=?2",
                params![owner, job],
            )?;
            return Ok(summary(&job, &items));
        }
        let mut items = Vec::new();
        let mut targets = HashSet::new();
        for id in ids {
            let index = dir
                .chapters
                .iter()
                .position(|c| &c.chapter_id == id)
                .ok_or_else(|| problem("指定章节不属于当前作品"))?;
            let chapter = &dir.chapters[index];
            let target = self.fanqie_binding(&dir.epoch, book, &chapter.volume_id)?;
            let offset = dir.chapters[..index]
                .iter()
                .rev()
                .take_while(|c| c.volume_id == chapter.volume_id)
                .count() as u32;
            let number = target.first_number + offset;
            if number > 999_999_999
                || !targets.insert((target.book.clone(), target.volume.clone(), number))
            {
                return Err(problem("目标章号重复或超出范围，请核对绑定"));
            }
            items.push(Item {
                chapter: chapter.clone(),
                target,
                number,
                state: "pending".into(),
                detail: String::new(),
                claim: None,
            });
        }
        let job = Uuid::new_v4().to_string();
        self.conn.execute(
            "INSERT INTO fanqie_jobs VALUES(?1,?2,?3,?4,?5,1,?6)",
            params![
                job,
                dir.epoch,
                book,
                req.idempotency_key,
                owner,
                serialize(&items)?
            ],
        )?;
        Ok(summary(&job, &items))
    }
    fn job_items(&self, book: &str, id: &str) -> WorkflowResult<Vec<Item>> {
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT items FROM fanqie_jobs WHERE epoch=?1 AND book=?2 AND id=?3",
                params![self.epoch()?, book, id],
                |r| r.get(0),
            )
            .optional()?;
        decode(&raw.ok_or_else(|| problem("任务不存在"))?)
    }
    pub(crate) fn fanqie_status(&self, book: &str, id: &str) -> WorkflowResult<Value> {
        Ok(summary(id, &self.job_items(book, id)?))
    }
    fn save_items(&self, id: &str, items: &[Item]) -> WorkflowResult<()> {
        let active = items
            .iter()
            .any(|i| i.state == "pending" || i.state == "checking")
            && !items.iter().any(|i| i.state == "review");
        self.conn.execute(
            "UPDATE fanqie_jobs SET active=?1,items=?2 WHERE id=?3",
            params![active, serialize(&items)?, id],
        )?;
        Ok(())
    }
    pub(crate) fn claim_fanqie(
        &self,
        book: &str,
        authorized: &HashSet<String>,
    ) -> WorkflowResult<Value> {
        let epoch = self.epoch()?;
        let mut stmt=self.conn.prepare("SELECT id,owner,items FROM fanqie_jobs WHERE epoch=?1 AND book=?2 AND active=1 ORDER BY rowid")?;
        let mut rows = stmt.query(params![epoch, book])?;
        let mut next = None;
        while let Some(r) = rows.next()? {
            if authorized.contains(&r.get::<_, String>(1)?) {
                next = Some((r.get::<_, String>(0)?, r.get::<_, String>(2)?));
                break;
            }
        }
        drop(rows);
        drop(stmt);
        let Some((job, raw)) = next else {
            return Ok(Value::Null);
        };
        let mut items: Vec<Item> = decode(&raw)?;
        let i = items
            .iter_mut()
            .find(|i| matches!(i.state.as_str(), "pending" | "checking"))
            .ok_or_else(|| problem("任务状态无效"))?;
        let packet = (|| {
            if self.fanqie_binding(&epoch, book, &i.chapter.volume_id)? != i.target {
                return Err(problem("目标绑定已改变，请重新指定章节"));
            }
            let packet = self.fanqie_packet(book, &epoch, &i.chapter)?;
            let claim = Uuid::new_v4().to_string();
            let mut result = json!({"job_id":job,"claim":claim,"verify_only":i.state=="checking","target":i.target,
                "number":i.number.to_string(),"title":packet.chapter.title,"body":packet.body});
            if serialize(&result)?.len() > 990_000 {
                result.as_object_mut().unwrap().remove("body");
                result["body_bytes"] = json!(packet.body.len());
            }
            i.claim = Some(claim);
            i.state = "checking".into();
            Ok(result)
        })();
        if let Err(e) = &packet {
            i.state = "review".into();
            i.detail = e.message.clone();
        }
        self.save_items(&job, &items)?;
        packet
    }
    pub(crate) fn validate_fanqie(
        &self,
        book: &str,
        id: &str,
        claim: &str,
        authorized: &HashSet<String>,
        cursor: Option<usize>,
    ) -> WorkflowResult<Value> {
        let items = self.job_items(book, id)?;
        let item = items
            .iter()
            .find(|i| i.state == "checking" && i.claim.as_deref() == Some(claim))
            .ok_or_else(|| problem("存稿任务已失效"))?;
        let owner: String =
            self.conn
                .query_row("SELECT owner FROM fanqie_jobs WHERE id=?1", [id], |r| {
                    r.get(0)
                })?;
        let epoch = self.epoch()?;
        if !authorized.contains(&owner)
            || self.fanqie_binding(&epoch, book, &item.chapter.volume_id)? != item.target
        {
            return Err(problem("授权或目标绑定已改变"));
        }
        let body = self.fanqie_packet(book, &epoch, &item.chapter)?.body;
        if let Some(start) = cursor {
            if start >= body.len() || !body.is_char_boundary(start) {
                return Err(problem("正文分段位置无效"));
            }
            let mut end = (start + 131072).min(body.len());
            while !body.is_char_boundary(end) {
                end -= 1;
            }
            return Ok(json!({"text":&body[start..end],"next":end}));
        }
        Ok(json!({"valid":true}))
    }
    pub(crate) fn finish_fanqie(&self, book: &str, r: Receipt) -> WorkflowResult<Value> {
        if !matches!(r.state.as_str(), "verified" | "exists" | "review") || r.detail.len() > 1024 {
            return Err(problem("存稿结果无效"));
        }
        let mut items = self.job_items(book, &r.job_id)?;
        let item = items
            .iter_mut()
            .find(|i| i.state == "checking" && i.claim.as_deref() == Some(&r.claim))
            .ok_or_else(|| problem("存稿回执已过期"))?;
        item.state = r.state;
        item.detail = r.detail;
        item.claim = None;
        self.save_items(&r.job_id, &items)?;
        Ok(summary(&r.job_id, &items))
    }
}
