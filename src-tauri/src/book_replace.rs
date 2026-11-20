//! Desktop-only trusted, bounded candidates. No external agent route.
use crate::{Service, WorkflowError, WorkflowResult};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
#[derive(Clone, Debug, Serialize)]
pub struct ReplaceChapter {
    pub id: String,
    pub title: String,
    pub revision: i64,
    pub matches: u64,
    pub before_snippet: String,
    pub after_snippet: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct ReplacePreview {
    pub id: String,
    pub book_id: String,
    pub query: String,
    pub replacement: String,
    pub expires_at: i64,
    pub chapters: Vec<ReplaceChapter>,
    pub total_matches: u64,
}
#[derive(Debug, Serialize)]
pub struct ReplaceDetail {
    pub chapter_id: String,
    pub before: String,
    pub after: String,
}
#[derive(Debug, Serialize)]
pub struct ReplaceOutcome {
    pub chapters: usize,
    pub matches: u64,
}
#[derive(Debug, PartialEq)]
struct Entry {
    id: String,
    volume: String,
    title: String,
    revision: i64,
    order: i64,
    volume_order: i64,
}
struct Change {
    id: String,
    before: String,
    after: String,
    revision: i64,
}
pub(crate) struct ReplaceCandidate {
    preview: ReplacePreview,
    epoch: String,
    created: Instant,
    manifest: Vec<Entry>,
    changes: Vec<Change>,
}
fn fail(code: &str, msg: &str) -> WorkflowError {
    WorkflowError {
        code: code.into(),
        message: msg.into(),
    }
}
fn db(_: rusqlite::Error) -> WorkflowError {
    fail("database_error", "书库操作失败，未完成替换；请重新预览")
}
fn manifest(conn: &Connection, book: &str) -> WorkflowResult<Vec<Entry>> {
    let active = conn
        .query_row(
            "SELECT deleted_at IS NULL FROM books WHERE id=?1",
            [book],
            |r| r.get::<_, bool>(0),
        )
        .optional()
        .map_err(db)?;
    if active != Some(true) {
        return Err(fail("not_found", "作品不存在或已回收"));
    }
    let(count,bytes):(i64,i64)=conn.query_row("SELECT count(*),coalesce(sum(length(CAST(c.body AS BLOB))),0) FROM chapters c JOIN volumes v ON c.volume_id=v.id WHERE v.book_id=?1 AND v.deleted_at IS NULL AND c.deleted_at IS NULL",[book],|r|Ok((r.get(0)?,r.get(1)?))).map_err(db)?;
    if count > 100000 || bytes > 64 * 1024 * 1024 {
        return Err(fail(
            "limit_exceeded",
            "作品超过100000章或64MiB正文扫描限制，请缩小书库",
        ));
    }
    let mut stmt=conn.prepare("SELECT c.id,c.volume_id,c.title,c.revision,c.sort_order,v.sort_order FROM chapters c JOIN volumes v ON c.volume_id=v.id WHERE v.book_id=?1 AND v.deleted_at IS NULL AND c.deleted_at IS NULL ORDER BY c.id").map_err(db)?;
    let rows = stmt
        .query_map([book], |r| {
            Ok(Entry {
                id: r.get(0)?,
                volume: r.get(1)?,
                title: r.get(2)?,
                revision: r.get(3)?,
                order: r.get(4)?,
                volume_order: r.get(5)?,
            })
        })
        .map_err(db)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db)
}
fn replacement_len(
    before: usize,
    matches: usize,
    query: usize,
    replacement: usize,
) -> Option<usize> {
    before
        .checked_sub(matches.checked_mul(query)?)?
        .checked_add(matches.checked_mul(replacement)?)
}
fn snippet(text: &str, start: usize) -> String {
    let prefix = text[..start].chars().rev().take(40).collect::<Vec<_>>();
    prefix
        .into_iter()
        .rev()
        .chain(text[start..].chars())
        .take(200)
        .collect()
}
impl Service {
    pub fn preview_book_replace(
        &mut self,
        book_id: &str,
        query: &str,
        replacement: &str,
    ) -> WorkflowResult<ReplacePreview> {
        self.replace_candidate = None;
        if query.is_empty() || query.len() > 256 || replacement.len() > 1024 || query == replacement
        {
            return Err(fail(
                "invalid_input",
                "搜索词须1至256字节，替换词最多1024字节且须不同",
            ));
        }
        let list = manifest(&self.conn, book_id)?;
        let epoch = self
            .epoch()
            .map_err(|_| fail("database_error", "书库状态不可用"))?;
        let mut changes = Vec::new();
        let mut chapters = Vec::new();
        let mut bytes = 0usize;
        let mut total = 0u64;
        for row in &list {
            let before: String = self
                .conn
                .query_row("SELECT body FROM chapters WHERE id=?1", [&row.id], |r| {
                    r.get(0)
                })
                .map_err(db)?;
            let matches = before.matches(query).count() as u64;
            if matches == 0 {
                continue;
            }
            total += matches;
            let new_len = replacement_len(
                before.len(),
                matches as usize,
                query.len(),
                replacement.len(),
            )
            .ok_or_else(|| fail("limit_exceeded", "替换结果过大"))?;
            bytes = bytes.saturating_add(before.len()).saturating_add(new_len);
            if total > 100000
                || changes.len() >= 500
                || bytes > 32 * 1024 * 1024
                || new_len > 10 * 1024 * 1024
            {
                return Err(fail(
                    "limit_exceeded",
                    "替换超过500章、100000处、32MiB候选或单章10MiB限制，未生成候选",
                ));
            }
            let after = before.replace(query, replacement);
            chapters.push(ReplaceChapter {
                id: row.id.clone(),
                title: row.title.clone(),
                revision: row.revision,
                matches,
                before_snippet: snippet(&before, before.find(query).unwrap()),
                after_snippet: snippet(&after, before.find(query).unwrap()),
            });
            changes.push(Change {
                id: row.id.clone(),
                before,
                after,
                revision: row.revision,
            });
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| fail("clock_error", "系统时间不可用"))?
            .as_secs() as i64;
        let preview = ReplacePreview {
            id: uuid::Uuid::new_v4().to_string(),
            book_id: book_id.into(),
            query: query.into(),
            replacement: replacement.into(),
            expires_at: now + 600,
            chapters,
            total_matches: total,
        };
        self.replace_candidate = Some(ReplaceCandidate {
            preview: preview.clone(),
            epoch,
            created: Instant::now(),
            manifest: list,
            changes,
        });
        Ok(preview)
    }
    pub fn get_replace_detail(&self, id: &str, chapter_id: &str) -> WorkflowResult<ReplaceDetail> {
        let c = self
            .replace_candidate
            .as_ref()
            .filter(|c| c.preview.id == id)
            .ok_or_else(|| fail("not_found", "替换候选不存在，请重新预览"))?;
        if c.created.elapsed().as_secs() >= 600 {
            return Err(fail("expired", "预览已过期，请重新预览"));
        }
        if c.epoch
            != self
                .epoch()
                .map_err(|_| fail("database_error", "书库状态不可用"))?
        {
            return Err(fail("revision_conflict", "书库已恢复，请重新预览"));
        }
        let row = c
            .changes
            .iter()
            .find(|r| r.id == chapter_id)
            .ok_or_else(|| fail("not_found", "章节不在此候选中"))?;
        Ok(ReplaceDetail {
            chapter_id: row.id.clone(),
            before: row.before.clone(),
            after: row.after.clone(),
        })
    }
    pub fn discard_book_replace(&mut self, id: &str) -> WorkflowResult<()> {
        if self
            .replace_candidate
            .as_ref()
            .is_some_and(|c| c.preview.id == id)
        {
            self.replace_candidate = None;
        }
        Ok(())
    }
    pub fn commit_book_replace(&mut self, id: &str) -> WorkflowResult<ReplaceOutcome> {
        if !self
            .replace_candidate
            .as_ref()
            .is_some_and(|c| c.preview.id == id)
        {
            return Err(fail("not_found", "替换候选不存在或已处理，请重新预览"));
        }
        let c = self.replace_candidate.take().unwrap();
        if c.created.elapsed().as_secs() >= 600 {
            return Err(fail("expired", "预览已过期，请重新预览"));
        }
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(db)?;
        let epoch: String = tx
            .query_row(
                "SELECT value FROM workflow_meta WHERE key='epoch'",
                [],
                |r| r.get(0),
            )
            .map_err(db)?;
        if c.epoch != epoch || c.manifest != manifest(&tx, &c.preview.book_id)? {
            return Err(fail(
                "revision_conflict",
                "作品已变化，整次替换未执行，请重新预览",
            ));
        }
        for row in &c.changes {
            crate::workflow::save_in_transaction(
                &tx,
                &row.id,
                &row.after,
                row.revision,
                "book_replace",
            )
            .map_err(|e| {
                if e.code == "database_error" {
                    fail("database_error", "替换失败，全部修改已回滚")
                } else {
                    e
                }
            })?;
        }
        tx.commit().map_err(db)?;
        Ok(ReplaceOutcome {
            chapters: c.changes.len(),
            matches: c.preview.total_matches,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Command;
    fn setup() -> (Service, String, String, String) {
        let mut s = Service::open(":memory:").unwrap();
        let a = s
            .apply(Command::CreateBook {
                title: "书".into(),
                author: "".into(),
            })
            .unwrap();
        let b = a.books[0].id.clone();
        let v = a.volumes[0].id.clone();
        let c = a.chapters[0].id.clone();
        s.apply(Command::SaveChapter {
            id: c.clone(),
            body: "甲".into(),
            expected_revision: 0,
        })
        .unwrap();
        (s, b, v, c)
    }
    #[test]
    fn arithmetic_overflow_rejected_before_allocating() {
        assert!(replacement_len(usize::MAX, 1, 1, usize::MAX).is_none());
        assert_eq!(replacement_len(10, 2, 2, 3), Some(12));
    }
    #[test]
    fn expiry_and_wrong_id_do_not_mutate() {
        let (mut s, b, _, c) = setup();
        let p = s.preview_book_replace(&b, "甲", "乙").unwrap();
        assert!(s.commit_book_replace("wrong").is_err());
        assert_eq!(s.get_replace_detail(&p.id, &c).unwrap().after, "乙");
        s.replace_candidate.as_mut().unwrap().created =
            Instant::now() - std::time::Duration::from_secs(601);
        assert!(s.get_replace_detail(&p.id, &c).is_err());
        assert!(s.commit_book_replace(&p.id).is_err());
        assert_eq!(s.get_chapter(&c).unwrap().body, "甲");
    }
    #[test]
    fn mid_transaction_failure_rolls_back_all_and_redacts() {
        let (mut s, b, v, c) = setup();
        s.conn.execute("INSERT INTO chapters(id,volume_id,title,body,revision,sort_order) VALUES('zz',?1,'二','甲',0,2)",[v]).unwrap();
        let p = s.preview_book_replace(&b, "甲", "乙").unwrap();
        let before: i64 = s
            .conn
            .query_row("SELECT count(*) FROM chapter_versions", [], |r| r.get(0))
            .unwrap();
        s.conn.execute_batch("CREATE TRIGGER deny_replace BEFORE UPDATE ON chapters WHEN NEW.id='zz' BEGIN SELECT RAISE(ABORT,'secret manuscript'); END;").unwrap();
        let audit_before = s.audit_log().unwrap().len();
        let err = s.commit_book_replace(&p.id).unwrap_err();
        assert!(!err.message.contains("secret"));
        assert_eq!(s.get_chapter(&c).unwrap().body, "甲");
        assert_eq!(s.get_chapter("zz").unwrap().body, "甲");
        let after: i64 = s
            .conn
            .query_row("SELECT count(*) FROM chapter_versions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(before, after);
        assert_eq!(s.get_chapter(&c).unwrap().revision, 1);
        assert_eq!(s.get_chapter("zz").unwrap().revision, 0);
        assert_eq!(s.audit_log().unwrap().len(), audit_before);
        assert!(s.commit_book_replace(&p.id).is_err());
    }
    #[test]
    fn expansion_and_candidate_and_scan_caps() {
        let (mut s, b, _, c) = setup();
        s.conn
            .execute(
                "UPDATE chapters SET body=?1 WHERE id=?2",
                rusqlite::params!["甲".repeat(100000), c],
            )
            .unwrap();
        assert_eq!(
            s.preview_book_replace(&b, "甲", &"乙".repeat(341))
                .unwrap_err()
                .code,
            "limit_exceeded"
        );
        let v = s.get_chapter(&c).unwrap().volume_id;
        let tx = s.conn.transaction().unwrap();
        tx.execute("DELETE FROM chapters", []).unwrap();
        for n in 0..501 {
            tx.execute("INSERT INTO chapters(id,volume_id,title,body,revision,sort_order) VALUES(?1,?2,'章','甲',0,0)",rusqlite::params![format!("c{n}"),v]).unwrap();
        }
        tx.commit().unwrap();
        assert!(s.preview_book_replace(&b, "甲", "乙").is_err());
    }
    #[test]
    fn candidate_and_whole_book_bytes_are_bounded() {
        let (mut s, b, v, c) = setup();
        let body = format!("甲{}", "x".repeat(9 * 1024 * 1024));
        s.conn
            .execute(
                "UPDATE chapters SET body=?1 WHERE id=?2",
                rusqlite::params![body, c],
            )
            .unwrap();
        s.conn.execute("INSERT INTO chapters(id,volume_id,title,body,revision,sort_order) VALUES('two',?1,'二',?2,0,1)",rusqlite::params![v,body]).unwrap();
        assert_eq!(
            s.preview_book_replace(&b, "甲", "乙").unwrap_err().code,
            "limit_exceeded"
        );
        for n in 0..6 {
            s.conn.execute("INSERT INTO chapters(id,volume_id,title,body,revision,sort_order) VALUES(?1,?2,'章',?3,0,1)",rusqlite::params![format!("extra{n}"),v,body]).unwrap();
        }
        assert_eq!(
            s.preview_book_replace(&b, "absent", "new")
                .unwrap_err()
                .code,
            "limit_exceeded"
        );
        assert_eq!(s.get_chapter(&c).unwrap().revision, 1);
    }
    #[test]
    fn epoch_change_and_deleted_parent_reject() {
        let (mut s, b, v, c) = setup();
        let p = s.preview_book_replace(&b, "甲", "乙").unwrap();
        s.conn
            .execute("UPDATE workflow_meta SET value='new' WHERE key='epoch'", [])
            .unwrap();
        assert!(s.get_replace_detail(&p.id, &c).is_err());
        assert!(s.commit_book_replace(&p.id).is_err());
        let p = s.preview_book_replace(&b, "甲", "乙").unwrap();
        s.apply(Command::TrashVolume { id: v }).unwrap();
        assert!(s.commit_book_replace(&p.id).is_err());
    }
}
