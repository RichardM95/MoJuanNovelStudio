//! Frozen, bounded UTF-8 import candidates. Only trusted desktop commands expose these methods.
use crate::{Service, WorkflowError, WorkflowResult};
use rusqlite::{params, Connection, Transaction, TransactionBehavior};
use serde::Serialize;
use std::{
    sync::Arc,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
pub(crate) const SOURCE_LIMIT: usize = 16 * 1024 * 1024;
pub(crate) const CHAPTER_LIMIT: usize = 10 * 1024 * 1024;
pub(crate) const VOLUME_COUNT_LIMIT: usize = 200;
pub(crate) const CHAPTER_COUNT_LIMIT: usize = 2000;
#[derive(Clone, Debug, Serialize)]
pub struct ImportSourceHeading {
    pub line: usize,
    pub number: Option<u32>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ImportChapter {
    pub index: u32,
    pub title: String,
    pub bytes: usize,
    pub snippet: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<ImportSourceHeading>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ImportVolume {
    pub title: String,
    pub chapters: Vec<ImportChapter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<ImportSourceHeading>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ImportPreview {
    pub id: String,
    pub source_name: String,
    pub title: String,
    pub author: String,
    pub mode: String,
    pub expires_at: i64,
    pub volumes: Vec<ImportVolume>,
    pub total_chapters: usize,
    pub total_bytes: usize,
}
#[derive(Debug, Serialize)]
pub struct ImportDetail {
    pub index: u32,
    pub body: String,
}
#[derive(Debug, Serialize)]
pub struct ImportOutcome {
    pub book_id: String,
    pub volumes: usize,
    pub chapters: usize,
}
pub(crate) struct Chapter {
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) source: Option<ImportSourceHeading>,
}
pub(crate) struct Volume {
    pub(crate) title: String,
    pub(crate) chapters: Vec<Chapter>,
    pub(crate) source: Option<ImportSourceHeading>,
}
pub(crate) struct ImportCandidate {
    source: Arc<str>,
    preview: ImportPreview,
    volumes: Vec<Volume>,
    epoch: String,
    created: Instant,
    ready: bool,
}
pub(crate) fn fail(code: &str, message: &str) -> WorkflowError {
    WorkflowError {
        code: code.into(),
        message: message.into(),
    }
}
pub(crate) fn db(_: rusqlite::Error) -> WorkflowError {
    fail("database_error", "导入失败，未创建作品；请重新选择文件")
}
pub(crate) fn now() -> WorkflowResult<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .map_err(|_| fail("clock_error", "系统时间不可用"))
}
pub(crate) fn epoch(c: &Connection) -> WorkflowResult<String> {
    c.query_row(
        "SELECT value FROM workflow_meta WHERE key='epoch'",
        [],
        |r| r.get(0),
    )
    .map_err(db)
}
pub(crate) fn uid() -> String {
    uuid::Uuid::new_v4().to_string()
}
pub(crate) fn heading_parts(line: &str) -> Option<(char, &str, &str)> {
    if line.chars().take(201).count() > 200 {
        return None;
    }
    let line = line.trim();
    let rest = line.strip_prefix('第')?;
    let end = rest
        .char_indices()
        .find(|(_, c)| {
            !"0123456789０１２３４５６７８９零〇一二三四五六七八九十百千万两".contains(*c)
        })?
        .0;
    if end == 0 {
        return None;
    }
    let mut tail = rest[end..].chars();
    let kind = tail.next()?;
    if kind != '卷' && kind != '章' {
        return None;
    }
    Some((kind, &rest[..end], tail.as_str()))
}
pub(crate) fn heading_title(kind: char, suffix: &str) -> String {
    let title = suffix.trim();
    if title.is_empty() {
        if kind == '卷' {
            "未命名卷"
        } else {
            "未命名章"
        }
        .into()
    } else {
        title.into()
    }
}
fn heading(line: &str) -> Option<(char, String)> {
    let (kind, _, suffix) = heading_parts(line)?;
    if !suffix.is_empty() && !suffix.chars().next()?.is_whitespace() {
        return None;
    }
    Some((kind, heading_title(kind, suffix)))
}
// Slices include their original LF, CRLF, or CR terminator without allocating copies.
pub(crate) fn lines(text: &str) -> impl Iterator<Item = (&str, &str)> {
    let mut offset = 0;
    std::iter::from_fn(move || {
        if offset >= text.len() {
            return None;
        }
        let start = offset;
        let b = text.as_bytes();
        while offset < b.len() && b[offset] != b'\n' && b[offset] != b'\r' {
            offset += 1
        }
        let end = offset;
        if offset < b.len() {
            if b[offset] == b'\r' && offset + 1 < b.len() && b[offset + 1] == b'\n' {
                offset += 2
            } else {
                offset += 1
            }
        }
        Some((&text[start..end], &text[start..offset]))
    })
}
pub(crate) fn push_volume(volumes: &mut Vec<Volume>, title: String) -> WorkflowResult<()> {
    if volumes.len() >= VOLUME_COUNT_LIMIT {
        return Err(fail("limit_exceeded", "最多导入200卷，未创建作品"));
    }
    volumes.push(Volume {
        title,
        chapters: vec![],
        source: None,
    });
    Ok(())
}
pub(crate) fn push_chapter(
    volumes: &mut Vec<Volume>,
    count: &mut usize,
    title: String,
) -> WorkflowResult<()> {
    if *count >= CHAPTER_COUNT_LIMIT {
        return Err(fail("limit_exceeded", "最多导入2000章，未创建作品"));
    }
    if volumes.is_empty() {
        push_volume(volumes, "正文".into())?
    }
    volumes.last_mut().unwrap().chapters.push(Chapter {
        title,
        body: String::new(),
        source: None,
    });
    *count += 1;
    Ok(())
}
fn parse(text: &str, mode: &str) -> WorkflowResult<Vec<Volume>> {
    if mode == "single" {
        if text.len() > CHAPTER_LIMIT {
            return Err(fail(
                "limit_exceeded",
                "全文单章超过10MiB，请使用有明确章节标题的文件",
            ));
        }
        return Ok(vec![Volume {
            title: "正文".into(),
            source: None,
            chapters: vec![Chapter {
                title: "正文".into(),
                body: text.into(),
                source: None,
            }],
        }]);
    }
    if mode != "headings" {
        return Err(fail("invalid_input", "不支持的分章模式"));
    }
    let has_heading = lines(text).any(|(line, _)| heading(line).is_some());
    let mut volumes = vec![];
    let mut count = 0;
    for (line, raw) in lines(text) {
        if let Some((kind, title)) = heading(line) {
            if kind == '卷' {
                push_volume(&mut volumes, title)?
            } else {
                push_chapter(&mut volumes, &mut count, title)?
            }
            continue;
        }
        if volumes.last().is_none_or(|v| v.chapters.is_empty()) {
            push_chapter(
                &mut volumes,
                &mut count,
                if has_heading { "前言" } else { "正文" }.into(),
            )?
        }
        let body = &mut volumes
            .last_mut()
            .unwrap()
            .chapters
            .last_mut()
            .unwrap()
            .body;
        if body.len().saturating_add(raw.len()) > CHAPTER_LIMIT {
            return Err(fail(
                "limit_exceeded",
                "单章超过10MiB，请检查分章标题；未创建作品",
            ));
        }
        body.push_str(raw);
    }
    if count == 0 {
        push_chapter(&mut volumes, &mut count, "正文".into())?
    }
    Ok(volumes)
}
fn build(
    source: Arc<str>,
    source_name: String,
    title: String,
    author: String,
    mode: &str,
    epoch: String,
) -> WorkflowResult<ImportCandidate> {
    validate_import_metadata(&title, &author)?;
    let volumes = parse(&source, mode)?;
    let (summaries, total_chapters, total_bytes) = summarize_volumes(&volumes);
    let preview = ImportPreview {
        id: uid(),
        source_name,
        title,
        author,
        mode: mode.into(),
        expires_at: now()? + 600,
        volumes: summaries,
        total_chapters,
        total_bytes,
    };
    Ok(ImportCandidate {
        source,
        preview,
        volumes,
        epoch,
        created: Instant::now(),
        ready: true,
    })
}
pub(crate) fn validate_import_metadata(title: &str, author: &str) -> WorkflowResult<()> {
    if title.trim().is_empty() || title.len() > 4096 || author.len() > 4096 {
        return Err(fail(
            "invalid_input",
            "书名不能为空，书名与作者各最多4096字节",
        ));
    }
    Ok(())
}
pub(crate) fn summarize_volumes(volumes: &[Volume]) -> (Vec<ImportVolume>, usize, usize) {
    let mut index = 0u32;
    let mut total_bytes = 0;
    let summaries = volumes
        .iter()
        .map(|v| ImportVolume {
            title: v.title.clone(),
            source: v.source.clone(),
            chapters: v
                .chapters
                .iter()
                .map(|c| {
                    let out = ImportChapter {
                        index,
                        title: c.title.clone(),
                        bytes: c.body.len(),
                        snippet: c.body.chars().take(200).collect(),
                        source: c.source.clone(),
                    };
                    index += 1;
                    total_bytes += c.body.len();
                    out
                })
                .collect(),
        })
        .collect();
    (summaries, index as usize, total_bytes)
}
pub(crate) fn decode_import_text<'a>(data: &'a [u8], format: &str) -> WorkflowResult<&'a str> {
    let data = data.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(data);
    let text = std::str::from_utf8(data).map_err(|_| {
        fail(
            "invalid_encoding",
            &format!("仅支持UTF-8 {format}（可带BOM），请先转换编码；未创建作品"),
        )
    })?;
    if text.contains('\0') {
        return Err(fail(
            "invalid_encoding",
            &format!("文件含NUL字符，不能作为{format}导入"),
        ));
    }
    if text.trim().is_empty() {
        return Err(fail("invalid_input", "文件为空或只有空白，未创建作品"));
    }
    Ok(text)
}
impl Service {
    pub fn prepare_txt_import(
        &mut self,
        data: &[u8],
        source_name: &str,
    ) -> WorkflowResult<ImportPreview> {
        self.prepare_txt_import_mode(data, source_name, "headings")
    }
    pub fn prepare_txt_import_mode(
        &mut self,
        data: &[u8],
        source_name: &str,
        mode: &str,
    ) -> WorkflowResult<ImportPreview> {
        self.import_candidate = None;
        if data.len() > SOURCE_LIMIT || source_name.len() > 4096 {
            return Err(fail(
                "limit_exceeded",
                "TXT文件上限16MiB，文件名最多4096字节",
            ));
        }
        let text = decode_import_text(data, "TXT")?;
        let title = std::path::Path::new(source_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("导入作品")
            .to_string();
        let c = build(
            Arc::from(text),
            source_name.into(),
            title,
            String::new(),
            mode,
            epoch(&self.conn)?,
        )?;
        let p = c.preview.clone();
        self.import_candidate = Some(c);
        Ok(p)
    }
    fn import_candidate(&self, id: &str) -> WorkflowResult<&ImportCandidate> {
        let c = self
            .import_candidate
            .as_ref()
            .filter(|c| c.preview.id == id)
            .ok_or_else(|| fail("not_found", "导入候选不存在，请重新选择文件"))?;
        if c.created.elapsed().as_secs() >= 600 {
            return Err(fail("expired", "导入预览已过期，请重新选择文件"));
        }
        if c.epoch != epoch(&self.conn)? {
            return Err(fail("revision_conflict", "书库已恢复，请重新选择文件"));
        }
        Ok(c)
    }
    pub fn revise_txt_import(
        &mut self,
        id: &str,
        title: &str,
        author: &str,
        mode: &str,
    ) -> WorkflowResult<ImportPreview> {
        let c = self.import_candidate(id)?;
        let source = c.source.clone();
        let name = c.preview.source_name.clone();
        let saved_epoch = c.epoch.clone();
        self.import_candidate.as_mut().unwrap().ready = false;
        let next = build(source, name, title.into(), author.into(), mode, saved_epoch)?;
        let p = next.preview.clone();
        self.import_candidate = Some(next);
        Ok(p)
    }
    pub fn get_txt_import_chapter(&self, id: &str, index: u32) -> WorkflowResult<ImportDetail> {
        let c = self.import_candidate(id)?;
        if !c.ready {
            return Err(fail("not_ready", "配置校验失败，请重新预览"));
        }
        let row = c
            .volumes
            .iter()
            .flat_map(|v| v.chapters.iter())
            .nth(index as usize)
            .ok_or_else(|| fail("not_found", "章节不在当前导入预览中"))?;
        Ok(ImportDetail {
            index,
            body: row.body.clone(),
        })
    }
    pub fn discard_txt_import(&mut self, id: &str) -> WorkflowResult<()> {
        if self
            .import_candidate
            .as_ref()
            .is_some_and(|c| c.preview.id == id)
        {
            self.import_candidate = None
        }
        Ok(())
    }
    pub fn commit_txt_import(&mut self, id: &str) -> WorkflowResult<ImportOutcome> {
        if !self
            .import_candidate
            .as_ref()
            .is_some_and(|c| c.preview.id == id)
        {
            return Err(fail("not_found", "候选不存在或已处理，请先核对书架"));
        }
        let c = self.import_candidate.take().unwrap();
        if !c.ready {
            return Err(fail("not_ready", "配置校验失败，请重新选择文件"));
        }
        if c.created.elapsed().as_secs() >= 600 {
            return Err(fail("expired", "导入预览已过期，请重新选择文件"));
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        if c.epoch != epoch(&tx)? {
            return Err(fail("revision_conflict", "书库已恢复，请重新选择文件"));
        }
        let book = uid();
        insert_import_tree(&tx, &book, &c.preview.title, &c.preview.author, &c.volumes)?;
        tx.commit().map_err(db)?;
        Ok(ImportOutcome {
            book_id: book,
            volumes: c.volumes.len(),
            chapters: c.preview.total_chapters,
        })
    }
}
pub(crate) struct ImportedTree {
    pub(crate) volumes: Vec<String>,
    pub(crate) chapters: Vec<String>,
}
pub(crate) fn insert_import_tree(
    tx: &Transaction<'_>,
    book: &str,
    title: &str,
    author: &str,
    volumes: &[Volume],
) -> WorkflowResult<ImportedTree> {
    let chapter_count = volumes
        .iter()
        .map(|volume| volume.chapters.len())
        .sum::<usize>();
    let existing: i64 = tx.query_row(
        "SELECT (SELECT count(*) FROM books)+(SELECT count(*) FROM volumes)+(SELECT count(*) FROM chapters)",
        [], |r| r.get(0),
    ).map_err(db)?;
    if existing.saturating_add(1 + volumes.len() as i64 + chapter_count as i64) > 100000 {
        return Err(fail(
            "limit_exceeded",
            "导入后书库将超过100000条记录，未创建作品",
        ));
    }
    let history_count: i64 = tx
        .query_row("SELECT count(*) FROM chapter_versions", [], |r| r.get(0))
        .map_err(db)?;
    if history_count.saturating_add(chapter_count as i64) > 100000 {
        return Err(fail(
            "limit_exceeded",
            "导入后历史将超过100000条，未创建作品；原历史保持不变",
        ));
    }
    let created = now()?;
    let mut ids = ImportedTree {
        volumes: Vec::with_capacity(volumes.len()),
        chapters: Vec::with_capacity(chapter_count),
    };
    tx.execute(
        "INSERT INTO books(id,title,author,deleted_at) VALUES(?1,?2,?3,NULL)",
        params![book, title, author],
    )
    .map_err(db)?;
    for (vi, volume) in volumes.iter().enumerate() {
        let vid = uid();
        tx.execute(
            "INSERT INTO volumes(id,book_id,title,sort_order,deleted_at) VALUES(?1,?2,?3,?4,NULL)",
            params![vid, book, volume.title, vi as i64],
        )
        .map_err(db)?;
        for (ci, chapter) in volume.chapters.iter().enumerate() {
            let cid = uid();
            tx.execute(
                "INSERT INTO chapters(id,volume_id,title,body,revision,sort_order,deleted_at) VALUES(?1,?2,?3,?4,0,?5,NULL)",
                params![cid, vid, chapter.title, chapter.body, ci as i64],
            ).map_err(db)?;
            tx.execute(
                "INSERT INTO chapter_versions(id,chapter_id,revision,body,source,created_at) VALUES(?1,?2,0,?3,'txt_import',?4)",
                params![uid(), cid, chapter.body, created],
            ).map_err(db)?;
            tx.execute(
                "INSERT INTO workflow_audit(id,chapter_id,action,source,revision,created_at) VALUES(?1,?2,'import_chapter','txt_import',0,?3)",
                params![uid(), cid, created],
            ).map_err(db)?;
            ids.chapters.push(cid);
        }
        ids.volumes.push(vid);
    }
    Ok(ids)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expired_and_epoch_changed_candidates_never_create_books() {
        let mut s = Service::open(":memory:").unwrap();
        let p = s.prepare_txt_import(b"body", "a.txt").unwrap();
        s.import_candidate.as_mut().unwrap().created =
            Instant::now() - std::time::Duration::from_secs(601);
        assert!(s.get_txt_import_chapter(&p.id, 0).is_err());
        assert!(s.revise_txt_import(&p.id, "x", "", "single").is_err());
        assert!(s.commit_txt_import(&p.id).is_err());
        let p = s.prepare_txt_import(b"body", "a.txt").unwrap();
        s.conn
            .execute("UPDATE workflow_meta SET value='new' WHERE key='epoch'", [])
            .unwrap();
        assert!(s.commit_txt_import(&p.id).is_err());
        assert!(s.load().unwrap().books.is_empty());
    }
    #[test]
    fn second_chapter_failure_rolls_back_new_tree_history_audit() {
        let mut s = Service::open(":memory:").unwrap();
        let old = s
            .apply(crate::Command::CreateBook {
                title: "原稿".into(),
                author: "作者".into(),
            })
            .unwrap();
        let p = s
            .prepare_txt_import("第一章 good\n正文\n第二章 bad\n二".as_bytes(), "a.txt")
            .unwrap();
        let before_history: i64 = s
            .conn
            .query_row("SELECT count(*) FROM chapter_versions", [], |r| r.get(0))
            .unwrap();
        let before_audit = s.audit_log().unwrap().len();
        s.conn.execute_batch("CREATE TRIGGER fail_import BEFORE INSERT ON chapters WHEN NEW.title='bad' BEGIN SELECT RAISE(ABORT,'secret manuscript'); END;").unwrap();
        let e = s.commit_txt_import(&p.id).unwrap_err();
        assert!(!e.message.contains("secret"));
        assert_eq!(s.load().unwrap(), old);
        assert_eq!(s.audit_log().unwrap().len(), before_audit);
        let after: i64 = s
            .conn
            .query_row("SELECT count(*) FROM chapter_versions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(before_history, after);
        assert!(s.commit_txt_import(&p.id).is_err());
    }
    #[test]
    fn all_library_records_are_counted_before_import() {
        let mut s = Service::open(":memory:").unwrap();
        s.conn.execute_batch("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<99998) INSERT INTO books(id,title,author,deleted_at) SELECT printf('old-%d',x),'old','',1 FROM n;").unwrap();
        let p = s.prepare_txt_import(b"body", "a.txt").unwrap();
        assert_eq!(
            s.commit_txt_import(&p.id).unwrap_err().code,
            "limit_exceeded"
        );
        let count: i64 = s
            .conn
            .query_row("SELECT count(*) FROM books", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 99998);
    }
    #[test]
    fn history_budget_rejects_two_at_99999_but_accepts_one() {
        let mut s = Service::open(":memory:").unwrap();
        let old = s
            .apply(crate::Command::CreateBook {
                title: "已有".into(),
                author: "".into(),
            })
            .unwrap();
        let cid = &old.chapters[0].id;
        s.conn.execute("WITH RECURSIVE n(x) AS (VALUES(0) UNION ALL SELECT x+1 FROM n WHERE x<99998) INSERT INTO chapter_versions(id,chapter_id,revision,body,source,created_at) SELECT printf('history-%d',x),?1,x,'old','fixture',0 FROM n",[cid]).unwrap();
        s.conn
            .execute("UPDATE chapters SET revision=99998 WHERE id=?1", [cid])
            .unwrap();
        let before = s.load().unwrap();
        let audit = s.audit_log().unwrap().len();
        let p = s
            .prepare_txt_import("第一章 一\n甲\n第二章 二\n乙".as_bytes(), "two.txt")
            .unwrap();
        assert_eq!(
            s.commit_txt_import(&p.id).unwrap_err().code,
            "limit_exceeded"
        );
        assert_eq!(s.load().unwrap(), before);
        assert_eq!(s.audit_log().unwrap().len(), audit);
        let count: i64 = s
            .conn
            .query_row("SELECT count(*) FROM chapter_versions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 99999);
        assert!(s.commit_txt_import(&p.id).is_err());
        let p = s.prepare_txt_import(b"new", "one.txt").unwrap();
        let outcome = s.commit_txt_import(&p.id).unwrap();
        assert_eq!(outcome.chapters, 1);
        let count: i64 = s
            .conn
            .query_row("SELECT count(*) FROM chapter_versions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 100000);
        assert_eq!(s.load().unwrap().books.len(), 2);
    }
}
