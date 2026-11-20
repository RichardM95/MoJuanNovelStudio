//! Scoped, bounded version metadata and single-snapshot reads. Naming never
//! changes manuscript revisions or creates a new body snapshot.
use crate::{ChapterVersion, LibraryState, Service, WorkflowError, WorkflowResult};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
const MAX_LABEL_REVISION: i64 = 9_007_199_254_740_991;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VersionSummary {
    pub id: String,
    pub chapter_id: String,
    pub revision: i64,
    pub source: String,
    pub created_at: i64,
    pub label: Option<String>,
    pub label_revision: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VersionPage {
    pub items: Vec<VersionSummary>,
    pub next_cursor: Option<String>,
    pub epoch: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VersionDetail {
    pub version: ChapterVersion,
    pub label: Option<String>,
    pub label_revision: i64,
    pub epoch: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    book_id: String,
    chapter_id: String,
    marked_only: bool,
    epoch: String,
    before_revision: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct VersionLabelRecord {
    pub version_id: String,
    pub label: Option<String>,
    pub label_revision: i64,
    pub updated_at: i64,
}
fn fail(code: &str, message: &str) -> WorkflowError {
    WorkflowError {
        code: code.into(),
        message: message.into(),
    }
}
fn current_epoch(conn: &Connection) -> WorkflowResult<String> {
    Ok(conn.query_row(
        "SELECT value FROM workflow_meta WHERE key='epoch'",
        [],
        |r| r.get(0),
    )?)
}
fn require_epoch(conn: &Connection, expected: &str) -> WorkflowResult<()> {
    if current_epoch(conn)? != expected {
        return Err(fail("epoch_conflict", "资料库已恢复，请刷新版本列表后重试"));
    }
    Ok(())
}
fn scope(conn: &Connection, book: &str, chapter: &str) -> WorkflowResult<i64> {
    let (revision,c,v,b):(i64,Option<i64>,Option<i64>,Option<i64>)=conn.query_row("SELECT c.revision,c.deleted_at,v.deleted_at,b.deleted_at FROM chapters c JOIN volumes v ON v.id=c.volume_id JOIN books b ON b.id=v.book_id WHERE c.id=?1 AND b.id=?2",params![chapter,book],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?.ok_or_else(||fail("not_found","章节不存在或不属于该作品"))?;
    if c.is_some() || v.is_some() || b.is_some() {
        return Err(fail("deleted", "作品、卷或章节在回收站，不能浏览版本"));
    }
    Ok(revision)
}
fn normalized(label: Option<String>) -> WorkflowResult<Option<String>> {
    if let Some(label) = label {
        if label.chars().any(char::is_control) {
            return Err(fail("invalid_input", "版本名称不能包含换行或控制字符"));
        }
        let label = label.trim();
        if label.is_empty() {
            return Ok(None);
        }
        if label.len() > 480 || label.chars().count() > 120 {
            return Err(fail(
                "invalid_input",
                "版本名称最多120个Unicode字符且480UTF8字节",
            ));
        }
        return Ok(Some(label.into()));
    }
    Ok(None)
}
fn summary_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<VersionSummary> {
    Ok(VersionSummary {
        id: r.get(0)?,
        chapter_id: r.get(1)?,
        revision: r.get(2)?,
        source: r.get(3)?,
        created_at: r.get(4)?,
        label: r.get(5)?,
        label_revision: r.get(6)?,
    })
}
const SUMMARY_SELECT:&str="SELECT h.id,h.chapter_id,h.revision,h.source,h.created_at,l.label,coalesce(l.label_revision,0) FROM chapter_versions h INDEXED BY chapter_version_metadata LEFT JOIN chapter_version_labels l ON l.version_id=h.id";
fn list_query(with_cursor: bool) -> String {
    let range = if with_cursor {
        "h.revision<?2"
    } else {
        "?2 IS NULL"
    };
    format!("{SUMMARY_SELECT} WHERE h.chapter_id=?1 AND {range} AND (?3=0 OR l.label IS NOT NULL) ORDER BY h.revision DESC,h.id DESC LIMIT ?4")
}
fn summary(conn: &Connection, chapter: &str, id: &str) -> WorkflowResult<VersionSummary> {
    conn.query_row(
        &format!("{SUMMARY_SELECT} WHERE h.chapter_id=?1 AND h.id=?2"),
        params![chapter, id],
        summary_row,
    )
    .optional()?
    .ok_or_else(|| fail("not_found", "版本不存在或不属于该章节"))
}
pub(crate) fn migrate(conn: &Connection) -> WorkflowResult<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS chapter_version_labels(version_id TEXT PRIMARY KEY REFERENCES chapter_versions(id) ON DELETE CASCADE,label TEXT,label_revision INTEGER NOT NULL CHECK(label_revision BETWEEN 1 AND 9007199254740991),updated_at INTEGER NOT NULL);CREATE INDEX IF NOT EXISTS chapter_version_metadata ON chapter_versions(chapter_id,revision DESC,id DESC,source,created_at);")?;
    Ok(())
}
pub(crate) fn all_labels(conn: &Connection) -> crate::Result<Vec<VersionLabelRecord>> {
    conn.prepare("SELECT version_id,label,label_revision,updated_at FROM chapter_version_labels ORDER BY version_id").map_err(|e|e.to_string())?.query_map([],|r|Ok(VersionLabelRecord{version_id:r.get(0)?,label:r.get(1)?,label_revision:r.get(2)?,updated_at:r.get(3)?})).map_err(|e|e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e|e.to_string())
}
pub(crate) fn validate_labels(
    history: &[ChapterVersion],
    labels: &[VersionLabelRecord],
) -> crate::Result<()> {
    let versions: HashSet<&str> = history.iter().map(|v| v.id.as_str()).collect();
    let mut ids = HashSet::new();
    if labels.len() > history.len() {
        return Err("版本名称记录超过历史版本数".into());
    }
    for l in labels {
        if !versions.contains(l.version_id.as_str())
            || !ids.insert(&l.version_id)
            || !(1..=MAX_LABEL_REVISION).contains(&l.label_revision)
            || l.updated_at < 0
        {
            return Err("版本名称引用、重复、计数或时间无效".into());
        }
        if normalized(l.label.clone()).map_err(|e| e.to_string())? != l.label {
            return Err("导入的版本名称必须规范化且非空（清空请使用null）".into());
        }
    }
    Ok(())
}
pub(crate) fn replace_labels(tx: &Transaction, labels: &[VersionLabelRecord]) -> crate::Result<()> {
    tx.execute("DELETE FROM chapter_version_labels", [])
        .map_err(|e| e.to_string())?;
    for l in labels {
        tx.execute(
            "INSERT INTO chapter_version_labels VALUES(?1,?2,?3,?4)",
            params![l.version_id, l.label, l.label_revision, l.updated_at],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}
impl Service {
    pub fn list_chapter_versions(
        &self,
        book: &str,
        chapter: &str,
        cursor: Option<&str>,
        limit: u32,
        marked_only: bool,
    ) -> WorkflowResult<VersionPage> {
        if !(1..=100).contains(&limit) {
            return Err(fail("invalid_input", "版本列表每页须1..100条"));
        }
        let tx = self.conn.unchecked_transaction()?;
        scope(&tx, book, chapter)?;
        let epoch = current_epoch(&tx)?;
        let before = if let Some(text) = cursor {
            if text.len() > 2048 {
                return Err(fail("invalid_cursor", "版本游标过长"));
            }
            let c: Cursor =
                serde_json::from_str(text).map_err(|_| fail("invalid_cursor", "版本游标无效"))?;
            if c.book_id != book
                || c.chapter_id != chapter
                || c.marked_only != marked_only
                || c.before_revision < 0
            {
                return Err(fail("invalid_cursor", "版本游标作用域或筛选不匹配"));
            }
            if c.epoch != epoch {
                return Err(fail("stale_cursor", "资料库已恢复，请重新加载第一页"));
            }
            let found: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM chapter_versions WHERE chapter_id=?1 AND revision=?2)",
                params![chapter, c.before_revision],
                |r| r.get(0),
            )?;
            if !found {
                return Err(fail("invalid_cursor", "版本游标基准不存在"));
            }
            Some(c.before_revision)
        } else {
            None
        };
        let query = list_query(before.is_some());
        let mut items = tx
            .prepare(&query)?
            .query_map(
                params![chapter, before, marked_only, i64::from(limit) + 1],
                summary_row,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let more = items.len() > limit as usize;
        items.truncate(limit as usize);
        let next_cursor = if more {
            let last = items
                .last()
                .ok_or_else(|| fail("database_error", "版本分页状态异常"))?;
            let text = serde_json::to_string(&Cursor {
                book_id: book.into(),
                chapter_id: chapter.into(),
                marked_only,
                epoch: epoch.clone(),
                before_revision: last.revision,
            })
            .map_err(|_| fail("database_error", "版本游标编码失败"))?;
            if text.len() > 2048 {
                return Err(fail("invalid_cursor", "版本游标超过长度限制"));
            }
            Some(text)
        } else {
            None
        };
        Ok(VersionPage {
            items,
            next_cursor,
            epoch,
        })
    }
    pub fn get_chapter_version(
        &self,
        book: &str,
        chapter: &str,
        id: &str,
    ) -> WorkflowResult<VersionDetail> {
        let tx = self.conn.unchecked_transaction()?;
        scope(&tx, book, chapter)?;
        let metadata = summary(&tx, chapter, id)?;
        let version=tx.query_row("SELECT id,chapter_id,revision,body,source,created_at FROM chapter_versions WHERE id=?1 AND chapter_id=?2",params![id,chapter],|r|Ok(ChapterVersion{id:r.get(0)?,chapter_id:r.get(1)?,revision:r.get(2)?,body:r.get(3)?,source:r.get(4)?,created_at:r.get(5)?}))?;
        Ok(VersionDetail {
            version,
            label: metadata.label,
            label_revision: metadata.label_revision,
            epoch: current_epoch(&tx)?,
        })
    }
    pub fn set_chapter_version_label(
        &mut self,
        book: &str,
        chapter: &str,
        id: &str,
        label: Option<String>,
        expected_label_revision: i64,
        expected_epoch: &str,
    ) -> WorkflowResult<VersionSummary> {
        if !(0..=MAX_LABEL_REVISION).contains(&expected_label_revision) {
            return Err(fail("invalid_input", "版本名称并发计数超出安全整数范围"));
        }
        let label = normalized(label)?;
        let tx = self.conn.transaction()?;
        scope(&tx, book, chapter)?;
        require_epoch(&tx, expected_epoch)?;
        let mut metadata = summary(&tx, chapter, id)?;
        if metadata.label_revision != expected_label_revision {
            return Err(fail(
                "revision_conflict",
                "版本名称已被修改，请保留草稿并刷新",
            ));
        }
        if metadata.label == label {
            return Ok(metadata);
        }
        let next = metadata
            .label_revision
            .checked_add(1)
            .filter(|n| *n <= MAX_LABEL_REVISION)
            .ok_or_else(|| fail("limit_exceeded", "版本名称计数已到安全上限，未覆盖名称"))?;
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| fail("clock_error", "无法读取名称保存时间"))?
            .as_secs() as i64;
        if expected_label_revision==0{tx.execute("INSERT INTO chapter_version_labels VALUES(?1,?2,?3,?4)",params![id,label,next,time])?;}else if tx.execute("UPDATE chapter_version_labels SET label=?1,label_revision=?2,updated_at=?3 WHERE version_id=?4 AND label_revision=?5",params![label,next,time,id,expected_label_revision])?!=1{return Err(fail("revision_conflict","版本名称已被修改"))}
        tx.execute(
            "INSERT INTO workflow_audit VALUES(?1,?2,?3,'human',?4,?5)",
            params![
                uuid::Uuid::new_v4().to_string(),
                chapter,
                if label.is_some() {
                    "label_version"
                } else {
                    "clear_version_label"
                },
                metadata.revision,
                time
            ],
        )?;
        metadata.label = label;
        metadata.label_revision = next;
        tx.commit()?;
        Ok(metadata)
    }
    pub fn restore_chapter_version_scoped(
        &mut self,
        book: &str,
        chapter: &str,
        id: &str,
        expected_revision: i64,
        expected_epoch: &str,
    ) -> WorkflowResult<LibraryState> {
        let tx = self.conn.transaction()?;
        let current = scope(&tx, book, chapter)?;
        require_epoch(&tx, expected_epoch)?;
        if expected_revision < 0 || expected_revision == i64::MAX {
            return Err(fail("invalid_input", "章节revision无效或溢出"));
        }
        if current != expected_revision {
            return Err(fail("revision_conflict", "当前正文已更新，未恢复旧版本"));
        }
        let body = tx
            .query_row(
                "SELECT body FROM chapter_versions WHERE id=?1 AND chapter_id=?2",
                params![id, chapter],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| fail("not_found", "版本不存在或不属于该章节"))?;
        crate::workflow::save_in_transaction(
            &tx,
            chapter,
            &body,
            expected_revision,
            &format!("history:{id}"),
        )?;
        tx.commit()?;
        self.load().map_err(|e| fail("database_error", &e))
    }
}

#[cfg(test)]
mod query_plan_tests {
    use super::*;
    #[test]
    fn metadata_cursor_seeks_revision_range_using_covering_index() {
        let service = Service::open(":memory:").unwrap();
        let sql = format!("EXPLAIN QUERY PLAN {}", list_query(true));
        let plans = service
            .conn
            .prepare(&sql)
            .unwrap()
            .query_map(params!["fixture", 100, false, 51], |r| {
                r.get::<_, String>(3)
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        eprintln!("history metadata range plan: {plans:?}");
        assert!(plans
            .iter()
            .any(|s| s.contains("COVERING INDEX chapter_version_metadata")
                && s.contains("revision<?")));
    }
}
