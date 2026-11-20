//! Bounded, literal, book-scoped search. Snippets are untrusted plain text.
use crate::{Service, WorkflowError, WorkflowResult};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchHit {
    pub kind: String,
    pub id: String,
    pub title: String,
    pub snippet: String,
    pub revision: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchResults {
    pub items: Vec<SearchHit>,
    pub truncated: bool,
}
fn fail(code: &str, message: &str) -> WorkflowError {
    WorkflowError {
        code: code.into(),
        message: message.into(),
    }
}
impl Service {
    pub fn search_book(
        &self,
        book_id: &str,
        query: &str,
        limit: u32,
    ) -> WorkflowResult<SearchResults> {
        if query.is_empty() || query.len() > 256 || !(1..=50).contains(&limit) {
            return Err(fail(
                "invalid_input",
                "搜索关键词须1..256字节，结果数须1..50",
            ));
        }
        let deleted = self
            .conn
            .query_row("SELECT deleted_at FROM books WHERE id=?1", [book_id], |r| {
                r.get::<_, Option<i64>>(0)
            })
            .optional()?
            .ok_or_else(|| fail("not_found", "作品不存在"))?;
        if deleted.is_some() {
            return Err(fail("deleted", "作品在回收站"));
        }
        let sql="WITH matches(kind,id,title,text,revision) AS (
       SELECT 'chapter',c.id,c.title,CASE WHEN instr(c.title,?2)>0 THEN c.title ELSE c.body END,c.revision FROM chapters c JOIN volumes v ON v.id=c.volume_id WHERE v.book_id=?1 AND v.deleted_at IS NULL AND c.deleted_at IS NULL AND (instr(c.title,?2)>0 OR instr(c.body,?2)>0)
       UNION ALL
       SELECT 'material',m.id,m.title,CASE WHEN instr(m.title,?2)>0 THEN m.title WHEN instr(coalesce(m.story_time,''),?2)>0 THEN m.story_time ELSE (SELECT CAST(j.value AS TEXT) FROM json_each(m.fields) j WHERE instr(CAST(j.value AS TEXT),?2)>0 ORDER BY j.key LIMIT 1) END,m.revision FROM materials m WHERE m.book_id=?1 AND m.deleted_at IS NULL AND (m.volume_id IS NULL OR EXISTS(SELECT 1 FROM volumes v WHERE v.id=m.volume_id AND v.book_id=m.book_id AND v.deleted_at IS NULL)) AND (m.chapter_id IS NULL OR EXISTS(SELECT 1 FROM chapters c JOIN volumes v ON v.id=c.volume_id WHERE c.id=m.chapter_id AND v.book_id=m.book_id AND c.deleted_at IS NULL AND v.deleted_at IS NULL)) AND (instr(m.title,?2)>0 OR instr(coalesce(m.story_time,''),?2)>0 OR EXISTS(SELECT 1 FROM json_each(m.fields) j WHERE instr(CAST(j.value AS TEXT),?2)>0))
    ) SELECT kind,id,title,substr(text,max(1,instr(text,?2)-80),200),revision FROM matches ORDER BY kind,id LIMIT ?3";
        let mut items = self
            .conn
            .prepare(sql)?
            .query_map(params![book_id, query, i64::from(limit) + 1], |r| {
                Ok(SearchHit {
                    kind: r.get(0)?,
                    id: r.get(1)?,
                    title: r.get(2)?,
                    snippet: r.get(3)?,
                    revision: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let truncated = items.len() > limit as usize;
        items.truncate(limit as usize);
        Ok(SearchResults { items, truncated })
    }
}
