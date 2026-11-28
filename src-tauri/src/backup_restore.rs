//! Bounded, canonical-only SQLite import. Candidate schema is never executed in
//! the live connection; published snapshots and imports have different trust.
use crate::{Book, Chapter, ChapterVersion, LibraryState, Service, Volume};
use rusqlite::{params_from_iter, types::Value, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
type Result<T> = std::result::Result<T, String>;
const CAP: u64 = 512 * 1024 * 1024;
const META_CAP: u64 = 32 * 1024 * 1024;
const CANONICAL: &[&str] = &[
    "books",
    "volumes",
    "chapters",
    "materials",
    "foreshadow_occurrences",
    "foreshadow_proposals",
    "chapter_versions",
    "chapter_version_labels",
    "material_content_versions",
    "writing_policies", "writing_packs", "writing_proposals",
];
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupProgress {
    pub phase: String,
    pub completed: u64,
    pub total: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreSummary {
    pub format: String,
    pub books: u64,
    pub volumes: u64,
    pub chapters: u64,
    pub history: u64,
    pub materials: u64,
    #[serde(default)]
    pub material_history: u64,
    pub version_labels: u64,
    pub canonical_bytes: u64,
    #[serde(default)]
    pub writing_policies:u64,
    #[serde(default)]
    pub writing_packs:u64,
    #[serde(default)]
    pub writing_proposals:u64,
    #[serde(default)]
    pub foreshadow_proposals: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RestoreGuard {
    pub epoch: String,
    pub total_changes: u64,
}
#[derive(Debug, Clone, Serialize)]
pub struct RestoreOutcome {
    pub summary: RestoreSummary,
    pub recovery_path: PathBuf,
}
pub struct PreparedRestore {
    dir: PathBuf,
    summary: RestoreSummary,
}
impl PreparedRestore {
    pub fn summary(&self) -> &RestoreSummary {
        &self.summary
    }
    fn path(&self) -> PathBuf {
        self.dir.join("canonical.sqlite")
    }
}
impl Drop for PreparedRestore {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
fn e(x: impl std::fmt::Display) -> String {
    x.to_string()
}
fn tick(
    f: &mut impl FnMut(BackupProgress) -> Result<()>,
    phase: &str,
    n: u64,
    total: Option<u64>,
) -> Result<()> {
    f(BackupProgress {
        phase: phase.into(),
        completed: n,
        total,
    })
}
fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(suffix);
    s.into()
}
fn standalone(path: &Path) -> Result<()> {
    let m = std::fs::metadata(path).map_err(e)?;
    if !m.is_file() || m.len() > CAP {
        return Err("SQLite备份限512MiB，或输入不是普通文件".into());
    }
    if std::fs::metadata(sidecar(path, "-wal"))
        .map(|m| m.len() > 0)
        .unwrap_or(false)
    {
        return Err("仅支持独立SQLite文件：发现非空WAL".into());
    }
    Ok(())
}
fn bounded_cancel(c: &mut Connection, cancel: Arc<AtomicBool>) -> Result<()> {
    let _ = c.execute_batch("PRAGMA cache_size=-2048; PRAGMA mmap_size=0; PRAGMA temp_store=FILE;");
    use rusqlite::limits::Limit;
    c.set_limit(Limit::SQLITE_LIMIT_LENGTH, 11 * 1024 * 1024).map_err(e)?;
    c.set_limit(Limit::SQLITE_LIMIT_SQL_LENGTH, 64 * 1024).map_err(e)?;
    c.set_limit(Limit::SQLITE_LIMIT_ATTACHED, 0).map_err(e)?;
    let start = Instant::now();
    c.progress_handler(
        1000,
        Some(move || cancel.load(Ordering::Relaxed) || start.elapsed() > Duration::from_secs(120)),
    ).map_err(e)
}
fn cols(c: &Connection, t: &str) -> Result<Vec<String>> {
    c.prepare(&format!("PRAGMA table_info(\"{t}\")"))
        .map_err(e)?
        .query_map([], |r| r.get(1))
        .map_err(e)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(e)
}
fn table_signature(
    c: &Connection,
    t: &str,
) -> Result<Vec<(String, String, i64, Option<String>, i64)>> {
    collect(c, &format!("PRAGMA table_info(\"{t}\")"), |r| {
        Ok((r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
    })
}
fn schema(c: &Connection, trusted: &Connection) -> Result<()> {
    let generation = crate::foreshadow_proposals::schema_generation(c).map_err(e)?;
    let writing=["writing_policies","writing_packs","writing_proposals"].iter().map(|t|cols(c,t).map(|x|!x.is_empty())).collect::<Result<Vec<_>>>()?;
    if writing.iter().any(|x|*x)&&!writing.iter().all(|x|*x){return Err("写作依据schema不完整".into());}
    let has_history=!cols(c,"material_content_versions")?.is_empty();
    let has_flag=cols(c,"materials")?.iter().any(|v|v=="content_review_required");
    if has_history!=has_flag{return Err("资料历史schema不完整".into());}
    let mut q = c
        .prepare("SELECT type,name,tbl_name FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'")
        .map_err(e)?;
    for r in q
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(e)?
    {
        let (ty, name, table) = r.map_err(e)?;
        if ty != "table" && ty != "index" {
            return Err("备份含不允许的schema对象".into());
        }
        let exists: i64 = trusted
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE type=?1 AND name=?2 AND tbl_name=?3",
                [&ty, &name, &table],
                |r| r.get(0),
            )
            .map_err(e)?;
        if exists != 1 {
            return Err("备份不是受支持的当前应用schema".into());
        }
    }
    let mut q = trusted
        .prepare("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%'")
        .map_err(e)?;
    for t in q.query_map([], |r| r.get::<_, String>(0)).map_err(e)? {
        let t = t.map_err(e)?;
        // Older snapshots predate the machine-local draft queue and browser bindings.
        if matches!(t.as_str(), "fanqie_jobs" | "fanqie_bindings") && cols(c, &t)?.is_empty() { continue; }
        if (t == "foreshadow_occurrences" && generation < 1) || (t == "foreshadow_proposals" && generation < 2) { continue; }
        if (t=="material_content_versions"||t.starts_with("writing_")) && cols(c,&t)?.is_empty() {continue;}
        let sql: String = c
            .query_row(
                "SELECT sql FROM sqlite_schema WHERE type='table' AND name=?1",
                [&t],
                |r| r.get(0),
            )
            .map_err(e)?;
        if !sql.trim_start().to_uppercase().starts_with("CREATE TABLE ") {
            return Err("不支持虚表或特殊表".into());
        }
        let hidden: i64 = c
            .query_row(
                &format!("SELECT count(*) FROM pragma_table_xinfo('{t}') WHERE hidden!=0"),
                [],
                |r| r.get(0),
            )
            .map_err(e)?;
        let actual=table_signature(c,&t)?;
        let mut expected=table_signature(trusted,&t)?;
        if t=="materials" && !actual.iter().any(|x|x.0=="content_review_required") {expected.retain(|x|x.0!="content_review_required");}
        if hidden != 0 || actual != expected {
            return Err("备份表列不匹配当前schema".into());
        }
    }
    Ok(())
}
fn copy_rows(
    src: &Connection,
    dst: &Connection,
    mut progress: impl FnMut(BackupProgress) -> Result<()>,
) -> Result<u64> {
    let mut bytes = 0u64;
    let mut meta = 0u64;
    let mut n = 0;
    let mut field_bytes = 0u64;
    for t in CANONICAL {
        let source_columns=cols(src,t)?;
        if (*t=="material_content_versions"||*t=="foreshadow_occurrences"||*t=="foreshadow_proposals"||t.starts_with("writing_")) && source_columns.is_empty(){continue;}
        if *t=="foreshadow_occurrences" { crate::foreshadow::preflight(src).map_err(e)?; }
        if *t=="foreshadow_proposals" { crate::foreshadow_proposals::preflight(src).map_err(e)?; }
        let columns = cols(dst, t)?;
        let names = columns
            .iter()
            .map(|s| format!("\"{s}\""))
            .collect::<Vec<_>>()
            .join(",");
        let read_names=columns.iter().map(|col|if *t=="materials" && col=="content_review_required" && !source_columns.contains(col) {"0".into()}else{format!("\"{col}\"")}).collect::<Vec<_>>().join(",");
        let mut read = src
            .prepare(&format!("SELECT {read_names} FROM \"{t}\""))
            .map_err(e)?;
        let mut rows = read.query([]).map_err(e)?;
        let placeholders = (1..=columns.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let mut write = dst
            .prepare(&format!(
                "INSERT INTO \"{t}\" ({names}) VALUES ({placeholders})"
            ))
            .map_err(e)?;
        let mut count = 0;
        while let Some(r) = rows.next().map_err(e)? {
            count += 1;
            if count > 100000 {
                return Err("备份记录数超限".into());
            }
            let values = (0..columns.len())
                .map(|i| r.get::<_, Value>(i))
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(e)?;
            for (i, v) in values.iter().enumerate() {
                let len = match v {
                    Value::Text(s) => s.len() as u64,
                    Value::Blob(_) => return Err("正文或元数据不允许BLOB".into()),
                    _ => 0,
                };
                bytes = bytes.checked_add(len).ok_or("备份容量溢出")?;
                if columns[i] == "fields" {
                    if let Value::Text(text) = v {
                        let fields: std::collections::BTreeMap<String, String> =
                            serde_json::from_str(text).map_err(e)?;
                        let size = fields.values().map(|s| s.len() as u64).sum::<u64>();
                        if size > 256 * 1024 {
                            return Err("资料单卡字段超限".into());
                        }
                        if *t=="materials" {field_bytes += size;}
                        if field_bytes > 32 * 1024 * 1024 {
                            return Err("资料全库字段超限".into());
                        }
                    }
                }
                if is_metadata_column(t, &columns[i]) {
                    meta += len;
                }
                if bytes > CAP || meta > META_CAP {
                    return Err("备份canonical文本512MiB或metadata32MiB超限".into());
                }
            }
            write.execute(params_from_iter(values)).map_err(e)?;
            n += 1;
            tick(&mut progress, "import", n, None)?;
        }
    }
    Ok(bytes)
}
fn collect<T>(
    c: &Connection,
    sql: &str,
    f: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>> {
    c.prepare(sql)
        .map_err(e)?
        .query_map([], f)
        .map_err(e)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(e)
}
fn validate(
    c: &Connection,
    format: &str,
    _bytes: u64,
    proposal_epoch: Option<&str>,
    mut progress: impl FnMut(BackupProgress) -> Result<()>,
) -> Result<RestoreSummary> {
    let validation_started = Instant::now();
    tick(&mut progress, "validate", 0, None)?;
    let (measured, metadata) = canonical_text_bytes(c)?;
    if measured > CAP || metadata > META_CAP {
        return Err("备份canonical文本512MiB或metadata32MiB超限".into());
    }
    let state = LibraryState {
        books: collect(c, "SELECT id,title,author,deleted_at FROM books", |r| {
            Ok(Book {
                id: r.get(0)?,
                title: r.get(1)?,
                author: r.get(2)?,
                deleted_at: r.get(3)?,
            })
        })?,
        volumes: collect(
            c,
            "SELECT id,book_id,title,sort_order,deleted_at FROM volumes",
            |r| {
                Ok(Volume {
                    id: r.get(0)?,
                    book_id: r.get(1)?,
                    title: r.get(2)?,
                    sort_order: r.get(3)?,
                    deleted_at: r.get(4)?,
                })
            },
        )?,
        chapters: collect(
            c,
            "SELECT id,volume_id,title,revision,sort_order,deleted_at FROM chapters",
            |r| {
                Ok(Chapter {
                    id: r.get(0)?,
                    volume_id: r.get(1)?,
                    title: r.get(2)?,
                    body: String::new(),
                    revision: r.get(3)?,
                    sort_order: r.get(4)?,
                    deleted_at: r.get(5)?,
                })
            },
        )?,
    };
    tick(&mut progress, "validate", 0, None)?;
    crate::core::validate(&state)?;
    tick(&mut progress, "validate", 0, None)?;
    if validation_started.elapsed() > Duration::from_secs(120) {
        return Err("验证超时".into());
    }
    let oversized:i64=c.query_row("SELECT count(*) FROM chapters WHERE typeof(body)!='text' OR length(CAST(body AS BLOB))>10485760",[],|r|r.get(0)).map_err(e)?;
    if oversized != 0 {
        return Err("章节正文类型或大小无效".into());
    }
    let chapters: HashMap<_, _> = state.chapters.iter().map(|c| (c.id.as_str(), c)).collect();
    let mut history = 0;
    let mut q = c
        .prepare("SELECT id,chapter_id,revision,body,source,created_at FROM chapter_versions")
        .map_err(e)?;
    let mut rows = q.query([]).map_err(e)?;
    while let Some(r) = rows.next().map_err(e)? {
        let v = ChapterVersion {
            id: r.get(0).map_err(e)?,
            chapter_id: r.get(1).map_err(e)?,
            revision: r.get(2).map_err(e)?,
            body: r.get(3).map_err(e)?,
            source: r.get(4).map_err(e)?,
            created_at: r.get(5).map_err(e)?,
        };
        let ch = chapters
            .get(v.chapter_id.as_str())
            .ok_or("历史关联章节不存在")?;
        if v.id.is_empty()
            || v.id.len() > 128
            || v.revision < 0
            || v.revision > ch.revision
            || v.body.len() > 10 * 1024 * 1024
            || v.source.is_empty()
            || v.source.len() > 256
            || v.created_at < 0
        {
            return Err("历史字段无效".into());
        }
        if v.revision == ch.revision {
            let body: String = c
                .query_row(
                    "SELECT body FROM chapters WHERE id=?1",
                    [&v.chapter_id],
                    |r| r.get(0),
                )
                .map_err(e)?;
            if body != v.body {
                return Err("当前版本历史与正文不一致".into());
            }
        }
        history += 1;
        tick(&mut progress, "validate", history, None)?;
    }
    let materials = crate::materials::all(c)?;
    tick(&mut progress, "validate", history, None)?;
    crate::materials::validate(&state, &materials)?;
    tick(&mut progress, "validate", history, None)?;
    for m in &materials {
        let actual: i64 = c
            .query_row(
                "SELECT field_bytes FROM materials WHERE id=?1",
                [&m.id],
                |r| r.get(0),
            )
            .map_err(e)?;
        let expected = m.fields.values().map(|s| s.len() as i64).sum::<i64>();
        if actual != expected {
            return Err("资料field_bytes与实际字段不一致".into());
        }
    }
    let material_history=crate::material_history::validate_stream(c)?;
    let packs = crate::constrained_writing::validate_db(c,&state).map_err(e)?;
    crate::foreshadow::validate_db(c, &state, &materials, &packs, proposal_epoch).map_err(e)?;
    let invalid_flags:i64=c.query_row("SELECT count(*) FROM materials WHERE content_review_required NOT IN (0,1)",[],|r|r.get(0)).map_err(e)?;
    if invalid_flags!=0{return Err("资料复核标记无效".into());}
    let labels = crate::history_browser::all_labels(c)?;
    // Existing strict label validator needs only version identities, not bodies.
    let versions = collect(
        c,
        "SELECT id,chapter_id,revision,source,created_at FROM chapter_versions",
        |r| {
            Ok(ChapterVersion {
                id: r.get(0)?,
                chapter_id: r.get(1)?,
                revision: r.get(2)?,
                body: String::new(),
                source: r.get(3)?,
                created_at: r.get(4)?,
            })
        },
    )?;
    crate::history_browser::validate_labels(&versions, &labels)?;
    tick(&mut progress, "validate", history, None)?;
    if validation_started.elapsed() > Duration::from_secs(120) {
        return Err("验证超时".into());
    }
    let fk: Option<String> = c
        .prepare("PRAGMA foreign_key_check")
        .map_err(e)?
        .query_map([], |r| r.get(0))
        .map_err(e)?
        .next()
        .transpose()
        .map_err(e)?;
    if fk.is_some() {
        return Err("备份关联完整性失败".into());
    }
    Ok(RestoreSummary {
        format: format.into(),
        books: state.books.len() as u64,
        volumes: state.volumes.len() as u64,
        chapters: state.chapters.len() as u64,
        history,
        materials: materials.len() as u64,
        material_history,
        version_labels: labels.len() as u64,
        canonical_bytes: measured,
        writing_policies:c.query_row("SELECT count(*) FROM writing_policies",[],|r|r.get(0)).map_err(e)?,
        writing_packs:c.query_row("SELECT count(*) FROM writing_packs",[],|r|r.get(0)).map_err(e)?,
        writing_proposals:c.query_row("SELECT count(*) FROM writing_proposals",[],|r|r.get(0)).map_err(e)?,
        foreshadow_proposals: c.query_row("SELECT count(*) FROM foreshadow_proposals", [], |r| r.get(0)).map_err(e)?,
    })
}
pub fn prepare_restore(
    path: &Path,
    progress: impl FnMut(BackupProgress) -> Result<()>,
) -> Result<PreparedRestore> {
    prepare_restore_with_cancel(path, Arc::new(AtomicBool::new(false)), progress)
}
pub fn prepare_restore_with_cancel(
    path: &Path,
    cancel: Arc<AtomicBool>,
    progress: impl FnMut(BackupProgress) -> Result<()>,
) -> Result<PreparedRestore> {
    let result = prepare_restore_inner(path, cancel.clone(), progress);
    if cancel.load(Ordering::Relaxed) {
        return Err("backup_job_cancelled".into());
    }
    result
}
fn prepare_restore_inner(
    path: &Path,
    cancel: Arc<AtomicBool>,
    mut progress: impl FnMut(BackupProgress) -> Result<()>,
) -> Result<PreparedRestore> {
    if cancel.load(Ordering::Relaxed) {
        return Err("cancelled".into());
    }
    standalone(path)?;
    tick(&mut progress, "prepare", 0, None)?;
    let dir = std::env::temp_dir().join(format!("mojuan-restore-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).map_err(e)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).map_err(e)?;
    }
    let mut prepared = PreparedRestore {
        dir,
        summary: RestoreSummary {
            format: String::new(),
            books: 0,
            volumes: 0,
            chapters: 0,
            history: 0,
            materials: 0,
            material_history: 0,
            version_labels: 0,
            canonical_bytes: 0,writing_policies:0,writing_packs:0,writing_proposals:0,foreshadow_proposals:0,
        },
    };
    let mut stage = Service::open(prepared.path())?;
    bounded_cancel(&mut stage.conn, cancel.clone())?;
    let mut prefix = [0u8; 16];
    {
        use std::io::Read;
        let mut f = std::fs::File::open(path).map_err(e)?;
        let _ = f.read(&mut prefix).map_err(e)?;
    }
    let (format, bytes, proposal_epoch) = if &prefix == b"SQLite format 3\0" {
        let mut source = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(e)?;
        bounded_cancel(&mut source, cancel.clone())?;
        source
            .execute_batch("PRAGMA trusted_schema=OFF; PRAGMA query_only=ON; BEGIN;")
            .map_err(e)?;
        schema(&source, &stage.conn)?;
        // Integrity is checked only after schema rejection, with a progress deadline.
        let check: String = source
            .query_row("PRAGMA integrity_check(1)", [], |r| r.get(0))
            .map_err(e)?;
        if check != "ok" {
            return Err("SQLite完整性校验失败".into());
        }
        stage.conn.execute_batch("BEGIN IMMEDIATE").map_err(e)?;
        let bytes = copy_rows(&source, &stage.conn, &mut progress)?;
        if !crate::foreshadow::schema_present(&source).map_err(e)?
            && crate::constrained_writing::all(&stage.conn).map_err(e)?.has_foreshadow_format() {
            return Err("新版依据存在但伏笔出现表缺失".into());
        }
        if !cols(&source,"material_content_versions")?.is_empty(){crate::material_history::validate_stream(&stage.conn)?;}
        crate::material_history::migrate(&stage.conn).map_err(e)?;
        stage.conn.execute_batch("COMMIT").map_err(e)?;
        standalone(path)?;
        let proposal_epoch = if crate::foreshadow_proposals::schema_generation(&source).map_err(e)? == 2 {
            Some(crate::foreshadow_proposals::backup_epoch(&source).map_err(e)?)
        } else { None };
        ("sqlite", bytes, proposal_epoch)
    } else {
        if std::fs::metadata(path).map_err(e)?.len() > 64 * 1024 * 1024 {
            return Err("JSON备份限64MiB；大库请使用SQLite备份".into());
        }
        use std::io::Read;
        let mut text = String::new();
        std::fs::File::open(path)
            .map_err(e)?
            .take(64 * 1024 * 1024 + 1)
            .read_to_string(&mut text)
            .map_err(e)?;
        if text.len() > 64 * 1024 * 1024 {
            return Err("JSON备份限64MiB".into());
        }
        stage.restore(&text)?;
        ("json", text.len() as u64, None) // Service::restore checked the supplied source epoch.
    };
    prepared.summary = validate(&stage.conn, format, bytes, proposal_epoch.as_deref(), &mut progress)?;
    stage
        .conn
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;")
        .map_err(e)?;
    drop(stage);
    Ok(prepared)
}
impl Service {
    pub fn restore_guard(&self) -> Result<RestoreGuard> {
        Ok(RestoreGuard {
            epoch: self.epoch().map_err(e)?,
            total_changes: self.conn.total_changes(),
        })
    }
    pub fn export_sqlite_backup(
        &self,
        path: &Path,
        progress: impl FnMut(BackupProgress) -> Result<()>,
    ) -> Result<PathBuf> {
        self.export_sqlite_backup_with_cancel(path, Arc::new(AtomicBool::new(false)), progress)
    }
    pub fn export_sqlite_backup_with_cancel(
        &self,
        path: &Path,
        cancel: Arc<AtomicBool>,
        mut progress: impl FnMut(BackupProgress) -> Result<()>,
    ) -> Result<PathBuf> {
        let pages: i64 = self
            .conn
            .query_row("PRAGMA page_count", [], |r| r.get(0))
            .map_err(e)?;
        let page_size: i64 = self
            .conn
            .query_row("PRAGMA page_size", [], |r| r.get(0))
            .map_err(e)?;
        if pages.saturating_mul(page_size) > CAP as i64 {
            return Err("SQLite导出首版上限512MiB；原库未更改".into());
        }
        self.recovery_snapshot_with_cancel(path, cancel, |p| {
            tick(
                &mut progress,
                "export",
                (p.total_pages - p.remaining_pages) as u64,
                Some(p.total_pages as u64),
            )
        })
    }
    pub fn commit_prepared_restore(
        &mut self,
        prepared: PreparedRestore,
        expected: &RestoreGuard,
        recovery_path: &Path,
        progress: impl FnMut(BackupProgress) -> Result<()>,
    ) -> Result<RestoreOutcome> {
        self.commit_prepared_restore_with_cancel(
            prepared,
            expected,
            recovery_path,
            Arc::new(AtomicBool::new(false)),
            progress,
        )
    }
    pub fn commit_prepared_restore_with_cancel(
        &mut self,
        prepared: PreparedRestore,
        expected: &RestoreGuard,
        recovery_path: &Path,
        cancel: Arc<AtomicBool>,
        progress: impl FnMut(BackupProgress) -> Result<()>,
    ) -> Result<RestoreOutcome> {
        self.commit_prepared_restore_with_writing_ack(prepared,expected,recovery_path,cancel,false,progress)
    }
    pub fn commit_prepared_restore_with_writing_ack(&mut self,prepared:PreparedRestore,expected:&RestoreGuard,recovery_path:&Path,cancel:Arc<AtomicBool>,writing_ack:bool,progress:impl FnMut(BackupProgress)->Result<()>) -> Result<RestoreOutcome>{
        let result = self.commit_prepared_restore_controlled_inner(
            prepared,
            expected,
            recovery_path,
            cancel.clone(),
            writing_ack,
            progress,
        );
        if result.is_err() && cancel.load(Ordering::Relaxed) {
            return Err("backup_job_cancelled".into());
        }
        result
    }
    fn commit_prepared_restore_controlled_inner(
        &mut self,
        prepared: PreparedRestore,
        expected: &RestoreGuard,
        recovery_path: &Path,
        cancel: Arc<AtomicBool>,
        writing_ack:bool,
        mut progress: impl FnMut(BackupProgress) -> Result<()>,
    ) -> Result<RestoreOutcome> {
        if cancel.load(Ordering::Relaxed) {
            return Err("backup_job_cancelled".into());
        }
        if &self.restore_guard()? != expected {
            return Err("restore_conflict: 预览后书库发生变化，请重新选择备份".into());
        }
        crate::constrained_writing::restore_guard(&self.conn,writing_ack).map_err(e)?;
        self.recovery_snapshot_with_cancel(recovery_path, cancel.clone(), |p| {
            tick(
                &mut progress,
                "recovery",
                (p.total_pages - p.remaining_pages) as u64,
                Some(p.total_pages as u64),
            )
        })?;
        let mut source = Connection::open_with_flags(prepared.path(), OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(e)?;
        bounded_cancel(&mut source, cancel.clone())?;
        let _hook = TemporaryProgressHook::new(&mut self.conn, cancel.clone())?;
        let tx = _hook.0.unchecked_transaction().map_err(e)?;
        crate::context_jobs::clear_for_restore(&tx).map_err(e)?;
        crate::material_workflow::clear_for_restore(&tx).map_err(e)?;
        crate::constrained_writing::clear(&tx).map_err(e)?;
        tx.execute_batch("DELETE FROM edit_proposals; DELETE FROM workflow_audit; DELETE FROM chapter_version_labels; DELETE FROM chapter_versions; DELETE FROM materials; DELETE FROM chapters; DELETE FROM volumes; DELETE FROM books;").map_err(e)?;
        copy_rows(&source, &tx, &mut progress)?;
        tx.execute(
            "UPDATE workflow_meta SET value=?1 WHERE key='epoch'",
            [uuid::Uuid::new_v4().to_string()],
        )
        .map_err(e)?;
        crate::constrained_writing::archive(&tx).map_err(e)?;
        tx.execute("INSERT INTO workflow_audit(id,chapter_id,action,source,revision,created_at) VALUES(?1,NULL,'restore_library','human',NULL,?2)",rusqlite::params![uuid::Uuid::new_v4().to_string(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(e)?.as_secs()as i64]).map_err(e)?;
        tick(&mut progress, "commit", 0, Some(1))?;
        if cancel.load(Ordering::Relaxed) {
            return Err("backup_job_cancelled".into());
        }
        tx.commit().map_err(e)?;
        Ok(RestoreOutcome {
            summary: prepared.summary.clone(),
            recovery_path: recovery_path.to_path_buf(),
        })
    }
}
#[derive(Default)]
pub(crate) struct BoundedJsonWriter(pub(crate) Vec<u8>);
impl std::io::Write for BoundedJsonWriter {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        if self.0.len().saturating_add(b.len()) > 64 * 1024 * 1024 {
            return Err(std::io::Error::other("JSON capacity exceeded"));
        }
        self.0.extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_sqlite_full_on_import_rolls_back_live_database() {
        let dir = std::env::temp_dir().join(format!("restore-full-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let mut s = Service::open(":memory:").unwrap();
        let state = s
            .apply(crate::Command::CreateBook {
                title: "fiction".into(),
                author: String::new(),
            })
            .unwrap();
        s.apply(crate::Command::SaveChapter {
            id: state.chapters[0].id.clone(),
            body: "x".repeat(1024 * 1024),
            expected_revision: 0,
        })
        .unwrap();
        let source = dir.join("source.sqlite");
        s.recovery_snapshot(&source).unwrap();
        let mut target = Service::open(":memory:").unwrap();
        let before = target.backup().unwrap();
        let guard = target.restore_guard().unwrap();
        let pages: i64 = target
            .conn
            .query_row("PRAGMA page_count", [], |r| r.get(0))
            .unwrap();
        target
            .conn
            .execute_batch(&format!("PRAGMA max_page_count={pages}"))
            .unwrap();
        let error = target
            .commit_prepared_restore(
                prepare_restore(&source, |_| Ok(())).unwrap(),
                &guard,
                &dir.join("before.sqlite"),
                |_| Ok(()),
            )
            .unwrap_err();
        assert!(error.to_lowercase().contains("full"), "{error}");
        assert_eq!(target.backup().unwrap(), before);
        drop(target);
        drop(s);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn bounded_json_encoding_stops_before_capacity() {
        use std::io::Write;
        let mut writer = BoundedJsonWriter(vec![b'a'; 64 * 1024 * 1024 - 1]);
        assert!(writer.write_all(b"ab").is_err());
        assert_eq!(writer.0.len(), 64 * 1024 * 1024 - 1);
    }
}

// Historical titles are versioned content, already counted by the history quota.
// Keep all other titles and history IDs/source tags in the metadata budget.
fn is_metadata_column(table: &str, column: &str) -> bool {
    column != "body" && column != "fields"
        && !(table == "material_content_versions" && column == "title")
        && !(table == "foreshadow_occurrences" && column == "record")
        && !(table == "foreshadow_proposals" && ["record", "payload"].contains(&column))
        && !(table.starts_with("writing_") && ["payload","record","manifest"].contains(&column))
}
pub(crate) fn canonical_text_bytes(c: &Connection) -> Result<(u64, u64)> {
    let mut total = 0u64;
    let mut meta = 0u64;
    for table in CANONICAL {
        let columns = cols(c, table)?;
        let terms=columns.iter().map(|column|format!("CASE WHEN typeof(\"{column}\")='text' THEN length(CAST(\"{column}\" AS BLOB)) ELSE 0 END")).collect::<Vec<_>>();
        let metadata = columns
            .iter()
            .zip(&terms)
            .filter(|(column, _)| is_metadata_column(table, column))
            .map(|(_, term)| term.clone())
            .collect::<Vec<_>>()
            .join("+");
        let sql = format!(
            "SELECT coalesce(sum({}),0),coalesce(sum({}),0) FROM \"{table}\"",
            terms.join("+"),
            metadata
        );
        let (t, m): (i64, i64) = c
            .query_row(&sql, [], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(e)?;
        total = total.checked_add(t as u64).ok_or("容量溢出")?;
        meta = meta.checked_add(m as u64).ok_or("容量溢出")?;
    }
    Ok((total, meta))
}
struct TemporaryProgressHook<'a>(&'a mut Connection);
impl<'a> TemporaryProgressHook<'a> {
    fn new(c: &'a mut Connection, cancel: Arc<AtomicBool>) -> Result<Self> {
        // Own cleanup before registration, including a fallible registration.
        let guard = Self(c);
        let start = Instant::now();
        guard.0.progress_handler(
            1000,
            Some(move || {
                cancel.load(Ordering::Relaxed) || start.elapsed() > Duration::from_secs(120)
            }),
        ).map_err(e)?;
        Ok(guard)
    }
}
impl Drop for TemporaryProgressHook<'_> {
    fn drop(&mut self) {
        // Clearing installs a null callback before client-data cleanup; no allocation.
        // Drop must also run on transaction errors and cannot return an error.
        let _ = self.0.progress_handler(0, None::<fn() -> bool>);
    }
}
