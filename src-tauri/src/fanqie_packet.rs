//! Local desktop chapter preparation. Does not modify the manuscript or contact a provider.
use crate::{Service, WorkflowError, WorkflowResult};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

const BODY_BYTES: usize = 10 * 1024 * 1024;
const MAX_DIRECTORY: usize = 20_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FanqieChapter {
    pub chapter_id: String,
    pub volume_id: String,
    pub volume_title: String,
    pub title: String,
    pub revision: i64,
    pub ordinal: usize,
    pub volume_ordinal: usize,
}

#[derive(Debug, Serialize)]
pub struct FanqieDirectory {
    pub book_id: String,
    pub book_title: String,
    pub epoch: String,
    pub chapters: Vec<FanqieChapter>,
}

#[derive(Debug, Serialize)]
pub struct FanqiePacket {
    #[serde(flatten)]
    pub chapter: FanqieChapter,
    pub body: String,
}

fn problem(message: &str) -> WorkflowError {
    WorkflowError {
        code: "fanqie_packet".into(),
        message: message.into(),
    }
}

fn directory(tx: &Connection, book_id: &str) -> WorkflowResult<FanqieDirectory> {
    let book_title = tx
        .query_row(
            "SELECT title FROM books WHERE id=?1 AND deleted_at IS NULL",
            [book_id],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| problem("作品不存在或已回收"))?;
    let epoch = tx.query_row(
        "SELECT value FROM workflow_meta WHERE key='epoch'",
        [],
        |r| r.get(0),
    )?;
    // Metadata-only ordering; including empty active volumes keeps volume positions stable.
    let mut query = tx.prepare(
        "WITH v AS (
            SELECT id,title, ROW_NUMBER() OVER (ORDER BY sort_order,id) AS ordinal
            FROM volumes WHERE book_id=?1 AND deleted_at IS NULL
         )
         SELECT c.id,v.id,v.title,c.title,c.revision,v.ordinal
         FROM v JOIN chapters c ON c.volume_id=v.id
         WHERE c.deleted_at IS NULL ORDER BY v.ordinal,c.sort_order,c.id LIMIT ?2",
    )?;
    let mut rows = query.query(params![book_id, MAX_DIRECTORY + 1])?;
    let mut chapters = Vec::new();
    while let Some(r) = rows.next()? {
        let ordinal = chapters.len() + 1;
        if ordinal > MAX_DIRECTORY {
            return Err(problem("作品章节数超过章稿队列支持的上限"));
        }
        chapters.push(FanqieChapter {
            chapter_id: r.get(0)?,
            volume_id: r.get(1)?,
            volume_title: r.get(2)?,
            title: r.get(3)?,
            revision: r.get(4)?,
            volume_ordinal: r.get(5)?,
            ordinal,
        });
    }
    Ok(FanqieDirectory {
        book_id: book_id.to_owned(),
        book_title,
        epoch,
        chapters,
    })
}

impl Service {
    pub fn fanqie_directory(&self, book_id: &str) -> WorkflowResult<FanqieDirectory> {
        let tx = self.conn.unchecked_transaction()?;
        let result = directory(&tx, book_id)?;
        tx.commit()?;
        Ok(result)
    }

    pub fn fanqie_packet(
        &self,
        book_id: &str,
        epoch: &str,
        expected: &FanqieChapter,
    ) -> WorkflowResult<FanqiePacket> {
        let tx = self.conn.unchecked_transaction()?;
        let current = directory(&tx, book_id)?;
        if current.epoch != epoch {
            return Err(problem("书库已变化，请重建队列"));
        }
        let chapter = current
            .chapters
            .into_iter()
            .find(|chapter| chapter.chapter_id == expected.chapter_id)
            .filter(|chapter| chapter == expected)
            .ok_or_else(|| problem("章稿或目录已变，请重建队列"))?;
        let body: Option<String> = tx.query_row(
            "SELECT CASE WHEN length(CAST(body AS BLOB))<=?2 THEN body END FROM chapters WHERE id=?1",
            params![chapter.chapter_id, BODY_BYTES],
            |r| r.get(0),
        )?;
        let body = body.ok_or_else(|| problem("单章超过 10 MiB，不能准备章稿"))?;
        if chapter.title.trim().is_empty() || body.trim().is_empty() {
            return Err(problem("章节标题或正文为空，请先编辑并保存"));
        }
        tx.commit()?;
        Ok(FanqiePacket { chapter, body })
    }
}
