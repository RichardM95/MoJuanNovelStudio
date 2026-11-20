//! Trusted local export only: a bounded frozen byte snapshot, never an agent route.
use crate::{Service, WorkflowError, WorkflowResult};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
const LIMIT: usize = 32 * 1024 * 1024;
const ORDERED_VOLUMES_SQL: &str = "SELECT id FROM volumes WHERE book_id=?1 AND deleted_at IS NULL AND (?2!='volume' OR id=?3) AND (?2!='chapter' OR id=(SELECT volume_id FROM chapters WHERE id=?3 AND deleted_at IS NULL)) ORDER BY sort_order,id LIMIT 100001";
const ORDERED_CHAPTERS_SQL: &str = "SELECT id FROM chapters WHERE volume_id=?1 AND deleted_at IS NULL AND (?2!='chapter' OR id=?3) ORDER BY sort_order,id LIMIT 100001";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportOptions {
    pub scope: String,
    pub target_id: Option<String>,
    pub include_metadata: bool,
    pub include_headings: bool,
    pub newline: String,
    pub bom: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct ExportPreview {
    pub id: String,
    pub bytes: usize,
    pub volumes: usize,
    pub chapters: usize,
    pub text: String,
    pub truncated: bool,
    pub expires_at: i64,
    pub bom: bool,
    pub newline: String,
}
pub(crate) struct ExportCandidate {
    id: String,
    bytes: Vec<u8>,
    created: Instant,
}
fn fail(message: &str) -> WorkflowError {
    WorkflowError {
        code: "export_error".into(),
        message: message.into(),
    }
}
fn db(_: rusqlite::Error) -> WorkflowError {
    fail("无法读取导出内容，未生成文件")
}
fn append(out: &mut String, text: &str, crlf: bool) -> WorkflowResult<()> {
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' || c == '\n' {
            if c == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            if crlf {
                out.push('\r');
            }
            out.push('\n');
        } else {
            out.push(c);
        }
        if out.len() > LIMIT {
            return Err(fail("导出超过32MiB，请改为单卷或单章"));
        }
    }
    Ok(())
}
impl Service {
    pub fn prepare_txt_export(
        &mut self,
        book_id: &str,
        o: ExportOptions,
    ) -> WorkflowResult<ExportPreview> {
        self.export_candidate = None;
        if !matches!(o.scope.as_str(), "book" | "volume" | "chapter")
            || !matches!(o.newline.as_str(), "lf" | "crlf")
            || (o.scope == "book" && o.target_id.is_some())
            || (o.scope != "book"
                && o.target_id
                    .as_ref()
                    .is_none_or(|s| s.is_empty() || s.len() > 128))
        {
            return Err(fail("导出选项无效"));
        }
        let tx = self.conn.transaction().map_err(db)?;
        let book: Option<(String, String)> = tx
            .query_row(
                "SELECT title,author FROM books WHERE id=?1 AND deleted_at IS NULL",
                [book_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db)?;
        let (title, author) = book.ok_or_else(|| fail("作品不存在或已回收"))?;
        if title.len() > 4096 || author.len() > 4096 {
            return Err(fail("作品信息超过限制"));
        }
        let mut out = String::new();
        let crlf = o.newline == "crlf";
        if o.bom {
            out.push('\u{feff}');
        }
        if o.include_metadata {
            append(&mut out, &format!("{title}\n作者：{author}\n\n"), crlf)?;
        }
        let mut vs = tx.prepare(ORDERED_VOLUMES_SQL).map_err(db)?;
        let mut rows = vs
            .query(params![book_id, o.scope, o.target_id])
            .map_err(db)?;
        let (mut volumes, mut chapters) = (0, 0);
        while let Some(r) = rows.next().map_err(db)? {
            volumes += 1;
            if volumes + chapters > 100000 {
                return Err(fail("导出节点过多"));
            }
            let vid: String = r.get(0).map_err(db)?;
            let vt: String = tx
                .query_row("SELECT title FROM volumes WHERE id=?1", [&vid], |r| {
                    r.get(0)
                })
                .map_err(db)?;
            if vt.len() > 4096 {
                return Err(fail("卷名超过限制"));
            }
            if o.include_headings {
                append(&mut out, &format!("第{volumes}卷 {vt}\n\n"), crlf)?;
            }
            let mut cs = tx.prepare(ORDERED_CHAPTERS_SQL).map_err(db)?;
            let mut cr = cs.query(params![vid, o.scope, o.target_id]).map_err(db)?;
            while let Some(c) = cr.next().map_err(db)? {
                chapters += 1;
                if volumes + chapters > 100000 {
                    return Err(fail("导出节点过多"));
                }
                let cid: String = c.get(0).map_err(db)?;
                // Keep payloads out of SQLite's ORDER BY sorter. Read exactly one
                // length-gated body by primary key, within this same snapshot.
                let (ct, body): (String, Option<String>) = tx.query_row(
                    "SELECT title,CASE WHEN length(CAST(body AS BLOB))<=10485760 THEN body ELSE NULL END FROM chapters WHERE id=?1",
                    [&cid],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                ).map_err(db)?;
                if ct.len() > 4096 {
                    return Err(fail("章名超过限制"));
                }
                let body = body.ok_or_else(|| fail("单章超过10MiB，无法导出"))?;
                if o.include_headings {
                    append(&mut out, &format!("第{chapters}章 {ct}\n\n"), crlf)?;
                }
                append(&mut out, &body, crlf)?;
                append(&mut out, "\n\n", crlf)?;
            }
        }
        if (o.scope == "volume" && volumes != 1) || (o.scope == "chapter" && chapters != 1) {
            return Err(fail("所选卷章不存在、已回收或不属于当前作品"));
        }
        drop(rows);
        drop(vs);
        tx.commit().map_err(db)?;
        let shown = if o.bom {
            out.strip_prefix('\u{feff}').unwrap_or(&out)
        } else {
            &out
        };
        let text: String = shown.chars().take(4000).collect();
        let truncated = shown.chars().nth(4000).is_some();
        let id = uuid::Uuid::new_v4().to_string();
        let expires_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| fail("系统时间不可用"))?
            .as_secs() as i64
            + 600;
        let p = ExportPreview {
            id: id.clone(),
            bytes: out.len(),
            volumes,
            chapters,
            text,
            truncated,
            expires_at,
            bom: o.bom,
            newline: o.newline,
        };
        self.export_candidate = Some(ExportCandidate {
            id,
            bytes: out.into_bytes(),
            created: Instant::now(),
        });
        Ok(p)
    }
    pub fn take_txt_export(&mut self, id: &str) -> WorkflowResult<Vec<u8>> {
        let c = self
            .export_candidate
            .as_ref()
            .ok_or_else(|| fail("请重新生成导出预览"))?;
        if c.id != id {
            return Err(fail("导出预览已失效"));
        }
        let c = self.export_candidate.take().unwrap();
        if c.created.elapsed().as_secs() >= 600 {
            return Err(fail("预览已过期，请重新预览"));
        }
        Ok(c.bytes)
    }
    pub fn discard_txt_export(&mut self, id: &str) {
        if self.export_candidate.as_ref().is_some_and(|c| c.id == id) {
            self.export_candidate = None;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordered_cursor_cannot_read_or_sort_chapter_payloads() {
        use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
        let mut s = Service::open(":memory:").unwrap();
        // The actual production cursor must compile even when SQLite is forbidden
        // to read body/title. This rules out either payload entering a temp sorter.
        s.conn
            .authorizer(Some(|ctx: AuthContext<'_>| match ctx.action {
                AuthAction::Read {
                    table_name: "chapters",
                    column_name: "body" | "title",
                } => Authorization::Deny,
                _ => Authorization::Allow,
            })).unwrap();
        let cursor = s.conn.prepare(ORDERED_CHAPTERS_SQL);
        assert!(
            cursor.is_ok(),
            "ordered cursor must not access chapter payload: {:?}",
            cursor.err()
        );
    }

    #[test]
    fn ordered_volume_cursor_cannot_read_or_sort_titles() {
        use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
        let mut s = Service::open(":memory:").unwrap();
        s.conn
            .authorizer(Some(|ctx: AuthContext<'_>| match ctx.action {
                AuthAction::Read {
                    table_name: "volumes",
                    column_name: "title",
                } => Authorization::Deny,
                _ => Authorization::Allow,
            })).unwrap();
        let cursor = s.conn.prepare(ORDERED_VOLUMES_SQL);
        assert!(
            cursor.is_ok(),
            "ordered cursor must contain only IDs: {:?}",
            cursor.err()
        );
    }

    #[test]
    fn output_budget_stops_before_loading_later_chapter_body() {
        let mut s = Service::open(":memory:").unwrap();
        let p = s.prepare_txt_import(b"first", "book.txt").unwrap();
        let id = s.commit_txt_import(&p.id).unwrap().book_id;
        let vid: String = s
            .conn
            .query_row("SELECT id FROM volumes WHERE book_id=?1", [&id], |r| {
                r.get(0)
            })
            .unwrap();
        s.conn.execute("DELETE FROM chapters", []).unwrap();
        // Four individually legal chapters exceed the output budget. The next
        // invalid UTF-8 TEXT would fail Rust decoding if it were ever loaded.
        for i in 0..4 {
            s.conn.execute("INSERT INTO chapters(id,volume_id,title,body,revision,sort_order,deleted_at) VALUES(?1,?2,'',CAST(zeroblob(10485760) AS TEXT),0,?3,NULL)", params![format!("c{i}"), vid, i]).unwrap();
        }
        s.conn.execute("INSERT INTO chapters(id,volume_id,title,body,revision,sort_order,deleted_at) VALUES('tail',?1,'',CAST(X'80' AS TEXT),0,4,NULL)", [&vid]).unwrap();
        let error = s
            .prepare_txt_export(
                &id,
                ExportOptions {
                    scope: "book".into(),
                    target_id: None,
                    include_metadata: false,
                    include_headings: false,
                    newline: "lf".into(),
                    bom: false,
                },
            )
            .unwrap_err();
        assert!(error.message.contains("32MiB"), "wrong failure: {error:?}");
        assert!(s.export_candidate.is_none());
    }

    #[test]
    fn expired_candidate_is_consumed() {
        let mut s = Service::open(":memory:").unwrap();
        s.export_candidate = Some(ExportCandidate {
            id: "x".into(),
            bytes: vec![],
            created: Instant::now() - std::time::Duration::from_secs(600),
        });
        assert!(s.take_txt_export("x").is_err());
        assert!(s.export_candidate.is_none());
    }
    #[test]
    fn oversized_database_chapter_is_rejected_before_loading_body() {
        let mut s = Service::open(":memory:").unwrap();
        let p = s.prepare_txt_import(b"text", "book.txt").unwrap();
        let id = s.commit_txt_import(&p.id).unwrap().book_id;
        s.conn
            .execute(
                "UPDATE chapters SET body=CAST(zeroblob(10485761) AS TEXT)",
                [],
            )
            .unwrap();
        let options = ExportOptions {
            scope: "book".into(),
            target_id: None,
            include_metadata: false,
            include_headings: false,
            newline: "lf".into(),
            bom: false,
        };
        assert!(s
            .prepare_txt_export(&id, options)
            .unwrap_err()
            .message
            .contains("10MiB"));
        assert!(s.export_candidate.is_none());
    }
}
