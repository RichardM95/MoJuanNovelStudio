//! Trusted desktop-only split candidates; no external agent entry point.
use crate::{Service, WorkflowError, WorkflowResult};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
const BODY_LIMIT: usize = 10 * 1024 * 1024;
fn fail(msg: &str) -> WorkflowError {
    WorkflowError {
        code: "split_error".into(),
        message: msg.into(),
    }
}
fn db(_: rusqlite::Error) -> WorkflowError {
    fail("拆章操作失败，未修改书库；请重新预览")
}
fn uid() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn now() -> WorkflowResult<i64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| fail("系统时间不可用"))?
        .as_secs() as i64)
}
fn epoch(c: &Connection) -> WorkflowResult<String> {
    c.query_row(
        "SELECT value FROM workflow_meta WHERE key='epoch'",
        [],
        |r| r.get(0),
    )
    .map_err(db)
}
#[derive(Clone, Debug, Serialize)]
pub struct SplitPreview {
    pub id: String,
    pub original_title: String,
    pub new_title: String,
    pub before: String,
    pub after: String,
    pub before_bytes: usize,
    pub after_bytes: usize,
    pub before_truncated: bool,
    pub after_truncated: bool,
    pub expires_at: i64,
}
#[derive(Clone, Debug, Serialize)]
pub struct SplitOutcome {
    pub book_id: String,
    pub original_chapter_id: String,
    pub new_chapter_id: String,
}
#[derive(Debug, PartialEq)]
struct Entry {
    id: String,
    title: String,
    revision: i64,
    order: i64,
}
#[derive(Debug, PartialEq)]
struct Scope {
    volume: String,
    volume_title: String,
    volume_order: i64,
    title: String,
    body: String,
    revision: i64,
    entries: Vec<Entry>,
}
pub(crate) struct SplitCandidate {
    preview: SplitPreview,
    book: String,
    chapter: String,
    epoch: String,
    scope: Scope,
    left: String,
    right: String,
    created: Instant,
}
fn scope(c: &Connection, book: &str, chapter: &str) -> WorkflowResult<Scope> {
    let row:Option<(String,String,i64,String,Option<String>,i64)>=c.query_row("SELECT v.id,v.title,v.sort_order,c.title,CASE WHEN length(CAST(c.body AS BLOB))<=10485760 THEN c.body ELSE NULL END,c.revision FROM chapters c JOIN volumes v ON v.id=c.volume_id JOIN books b ON b.id=v.book_id WHERE c.id=?1 AND b.id=?2 AND c.deleted_at IS NULL AND v.deleted_at IS NULL AND b.deleted_at IS NULL",params![chapter,book],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional().map_err(db)?;
    let (volume, volume_title, volume_order, title, body, revision) =
        row.ok_or_else(|| fail("章节不存在、已回收或不属于该作品"))?;
    let body = body.ok_or_else(|| fail("单章超过10MiB，不能拆分"))?;
    let mut st=c.prepare("SELECT id FROM chapters WHERE volume_id=?1 AND deleted_at IS NULL ORDER BY sort_order,id LIMIT 100001").map_err(db)?;
    let mut rows = st.query([&volume]).map_err(db)?;
    let mut entries = Vec::new();
    let mut bytes = 0;
    while let Some(r) = rows.next().map_err(db)? {
        let id: String = r.get(0).map_err(db)?;
        let e = c
            .query_row(
                "SELECT id,title,revision,sort_order FROM chapters WHERE id=?1",
                [id],
                |r| {
                    Ok(Entry {
                        id: r.get(0)?,
                        title: r.get(1)?,
                        revision: r.get(2)?,
                        order: r.get(3)?,
                    })
                },
            )
            .map_err(db)?;
        bytes += e.id.len() + e.title.len();
        if entries.len() >= 100000 || bytes > 16 * 1024 * 1024 {
            return Err(fail("当前卷目录超过100000章或16MiB元数据，无法拆分"));
        }
        entries.push(e);
    }
    Ok(Scope {
        volume,
        volume_title,
        volume_order,
        title,
        body,
        revision,
        entries,
    })
}
fn byte_offset(body: &str, offset: u32) -> WorkflowResult<usize> {
    let mut units = 0;
    for (i, ch) in body.char_indices() {
        if units == offset as usize {
            if i == 0 || (&body.as_bytes()[i - 1..=i] == b"\r\n") {
                return Err(fail("请将光标放在正文内部，不能拆开CRLF换行"));
            }
            return Ok(i);
        }
        units += ch.len_utf16();
        if units > offset as usize {
            return Err(fail("不能拆开Unicode代理对"));
        }
    }
    Err(fail("请将光标放在正文内部"))
}
impl Service {
    pub fn prepare_chapter_split(
        &mut self,
        book: &str,
        chapter: &str,
        expected_revision: i64,
        utf16_offset: u32,
        new_title: &str,
    ) -> WorkflowResult<SplitPreview> {
        self.split_candidate = None;
        if new_title.trim().is_empty()
            || new_title.len() > 4096
            || expected_revision < 0
            || expected_revision >= 9_007_199_254_740_991
        {
            return Err(fail("新章名为空或超过4096字节，或版本无效"));
        }
        let tx = self.conn.transaction().map_err(db)?;
        let s = scope(&tx, book, chapter)?;
        if s.revision != expected_revision {
            return Err(fail("正文版本已变更，请重新读取"));
        }
        let ep = epoch(&tx)?;
        let at = byte_offset(&s.body, utf16_offset)?;
        let left = s.body[..at].to_string();
        let right = s.body[at..].to_string();
        let preview = SplitPreview {
            id: uid(),
            original_title: s.title.clone(),
            new_title: new_title.into(),
            before: left.chars().take(2000).collect(),
            after: right.chars().take(2000).collect(),
            before_bytes: left.len(),
            after_bytes: right.len(),
            before_truncated: left.chars().nth(2000).is_some(),
            after_truncated: right.chars().nth(2000).is_some(),
            expires_at: now()? + 600,
        };
        tx.commit().map_err(db)?;
        self.split_candidate = Some(SplitCandidate {
            preview: preview.clone(),
            book: book.into(),
            chapter: chapter.into(),
            epoch: ep,
            scope: s,
            left,
            right,
            created: Instant::now(),
        });
        Ok(preview)
    }
    fn split_candidate_checked(&self, id: &str) -> WorkflowResult<&SplitCandidate> {
        let c = self
            .split_candidate
            .as_ref()
            .ok_or_else(|| fail("请重新生成拆章预览"))?;
        if c.preview.id != id || c.created.elapsed().as_secs() >= 600 {
            return Err(fail("拆章预览已失效或过期"));
        }
        Ok(c)
    }
    pub fn get_split_detail(&self, id: &str, side: &str) -> WorkflowResult<String> {
        let c = self.split_candidate_checked(id)?;
        match side {
            "before" => Ok(c.left.clone()),
            "after" => Ok(c.right.clone()),
            _ => Err(fail("预览位置无效")),
        }
    }
    pub fn discard_chapter_split(&mut self, id: &str) {
        if self
            .split_candidate
            .as_ref()
            .is_some_and(|c| c.preview.id == id)
        {
            self.split_candidate = None;
        }
    }
    pub fn commit_chapter_split(&mut self, id: &str) -> WorkflowResult<SplitOutcome> {
        // Wrong ID never consumes a newer preview. Matching attempts are one-shot even on failure.
        if !self
            .split_candidate
            .as_ref()
            .is_some_and(|c| c.preview.id == id)
        {
            return Err(fail("请重新生成拆章预览"));
        }
        let c = self.split_candidate.take().unwrap();
        if c.created.elapsed().as_secs() >= 600 {
            return Err(fail("拆章预览已过期"));
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        if epoch(&tx)? != c.epoch || scope(&tx, &c.book, &c.chapter)? != c.scope {
            return Err(fail("正文、目录或书库已改变，请重新预览"));
        }
        let count:i64=tx.query_row("SELECT (SELECT count(*) FROM books)+(SELECT count(*) FROM volumes)+(SELECT count(*) FROM chapters)",[],|r|r.get(0)).map_err(db)?;
        let history: i64 = tx
            .query_row("SELECT count(*) FROM chapter_versions", [], |r| r.get(0))
            .map_err(db)?;
        let baseline: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM chapter_versions WHERE chapter_id=?1 AND revision=?2)",
                params![c.chapter, c.scope.revision],
                |r| r.get(0),
            )
            .map_err(db)?;
        if count >= 100000 || history + 2 + if baseline { 0 } else { 1 } > 100000 {
            return Err(fail("书库或历史达到100000条限制，未拆分"));
        }
        if c.left.len() > BODY_LIMIT || c.right.len() > BODY_LIMIT {
            return Err(fail("章节超过10MiB"));
        }
        crate::workflow::save_in_transaction(
            &tx,
            &c.chapter,
            &c.left,
            c.scope.revision,
            "chapter_split",
        )
        .map_err(|_| fail("保存拆前历史或正文失败，未拆分"))?;
        let new = uid();
        let timestamp = now()?;
        tx.execute("INSERT INTO chapters(id,volume_id,title,body,revision,sort_order,deleted_at) VALUES(?1,?2,?3,?4,0,0,NULL)",params![new,c.scope.volume,c.preview.new_title,c.right]).map_err(db)?;
        tx.execute("INSERT INTO chapter_versions(id,chapter_id,revision,body,source,created_at) VALUES(?1,?2,0,?3,'chapter_split',?4)",params![uid(),new,c.right,timestamp]).map_err(db)?;
        tx.execute("INSERT INTO workflow_audit(id,chapter_id,action,source,revision,created_at) VALUES(?1,?2,'create','chapter_split',0,?3)",params![uid(),new,timestamp]).map_err(db)?;
        // Recycled chapters retain their order, and core validation requires
        // uniqueness across both active and recycled rows. Reserve those slots.
        let reserved = tx
            .prepare(
                "SELECT sort_order FROM chapters WHERE volume_id=?1 AND deleted_at IS NOT NULL",
            )
            .map_err(db)?
            .query_map([&c.scope.volume], |r| r.get::<_, i64>(0))
            .map_err(db)?
            .collect::<rusqlite::Result<std::collections::HashSet<_>>>()
            .map_err(db)?;
        let mut order = 0i64;
        let mut next_order = || {
            while reserved.contains(&order) {
                order += 1;
            }
            let assigned = order;
            order += 1;
            assigned
        };
        for e in &c.scope.entries {
            tx.execute(
                "UPDATE chapters SET sort_order=?1 WHERE id=?2",
                params![next_order(), e.id],
            )
            .map_err(db)?;
            if e.id == c.chapter {
                tx.execute(
                    "UPDATE chapters SET sort_order=?1 WHERE id=?2",
                    params![next_order(), new],
                )
                .map_err(db)?;
            }
        }
        tx.commit().map_err(db)?;
        Ok(SplitOutcome {
            book_id: c.book,
            original_chapter_id: c.chapter,
            new_chapter_id: new,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Command;
    fn setup() -> (Service, String, String) {
        let mut s = Service::open(":memory:").unwrap();
        let a = s
            .apply(Command::CreateBook {
                title: "书".into(),
                author: "".into(),
            })
            .unwrap();
        let id = a.chapters[0].id.clone();
        s.apply(Command::SaveChapter {
            id: id.clone(),
            body: "前\r\n后🙂".into(),
            expected_revision: 0,
        })
        .unwrap();
        (s, a.books[0].id.clone(), id)
    }
    #[test]
    fn failure_rolls_back_body_tree_history_audit() {
        let (mut s, b, c) = setup();
        let p = s.prepare_chapter_split(&b, &c, 1, 3, "后").unwrap();
        let before = s.backup().unwrap();
        let audit = s.audit_log().unwrap();
        s.conn.execute_batch("CREATE TRIGGER split_fail BEFORE INSERT ON workflow_audit WHEN NEW.action='create' AND NEW.source='chapter_split' BEGIN SELECT RAISE(ABORT,'fail'); END;").unwrap();
        assert!(s.commit_chapter_split(&p.id).is_err());
        assert_eq!(before, s.backup().unwrap());
        assert_eq!(audit, s.audit_log().unwrap());
        assert!(s.commit_chapter_split(&p.id).is_err());
    }
    #[test]
    fn limits_expiry_and_wrong_id() {
        let (mut s, b, c) = setup();
        let p = s.prepare_chapter_split(&b, &c, 1, 3, "后").unwrap();
        assert!(s.commit_chapter_split("wrong").is_err());
        s.split_candidate.as_mut().unwrap().created =
            Instant::now() - std::time::Duration::from_secs(601);
        assert!(s.commit_chapter_split(&p.id).is_err());
        assert!(s.split_candidate.is_none());
        s.conn
            .execute(
                "UPDATE chapters SET body=?1 WHERE id=?2",
                params!["x".repeat(BODY_LIMIT + 1), c],
            )
            .unwrap();
        assert!(s.prepare_chapter_split(&b, &c, 1, 3, "后").is_err());
    }
    #[test]
    fn history_budget_includes_new_initial_snapshot() {
        let (mut s, b, c) = setup();
        s.conn.execute_batch("WITH RECURSIVE seq(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM seq WHERE n<99997) INSERT INTO chapter_versions(id,chapter_id,revision,body,source,created_at) SELECT 'filler-'||n,(SELECT id FROM chapters LIMIT 1),n+10,'','test',0 FROM seq;").unwrap();
        let p = s.prepare_chapter_split(&b, &c, 1, 3, "后").unwrap();
        let before = s.load().unwrap();
        assert!(s.commit_chapter_split(&p.id).is_err());
        assert_eq!(s.load().unwrap(), before);
        assert_eq!(
            s.conn
                .query_row("SELECT count(*) FROM chapter_versions", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            99999
        );
    }
}
