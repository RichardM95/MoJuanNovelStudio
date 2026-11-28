use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::Path};
pub type Result<T> = std::result::Result<T, String>;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Book {
    pub id: String,
    pub title: String,
    pub author: String,
    pub deleted_at: Option<i64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Volume {
    pub id: String,
    pub book_id: String,
    pub title: String,
    pub sort_order: i64,
    #[serde(default)]
    pub deleted_at: Option<i64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Chapter {
    pub id: String,
    pub volume_id: String,
    pub title: String,
    pub body: String,
    pub revision: i64,
    pub sort_order: i64,
    #[serde(default)]
    pub deleted_at: Option<i64>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct LibraryState {
    pub books: Vec<Book>,
    pub volumes: Vec<Volume>,
    pub chapters: Vec<Chapter>,
}
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    CreateBook {
        title: String,
        author: String,
    },
    UpdateBook {
        id: String,
        title: String,
        author: String,
    },
    TrashBook {
        id: String,
    },
    RestoreBook {
        id: String,
    },
    TrashVolume {
        id: String,
    },
    RestoreVolume {
        id: String,
    },
    TrashChapter {
        id: String,
    },
    RestoreChapter {
        id: String,
    },
    PurgeBook {
        id: String,
    },
    PurgeVolume {
        id: String,
    },
    PurgeChapter {
        id: String,
    },
    CreateVolume {
        book_id: String,
        title: String,
    },
    RenameVolume {
        id: String,
        title: String,
    },
    MoveVolume {
        id: String,
        direction: i32,
    },
    CreateChapter {
        volume_id: String,
        title: String,
    },
    RenameChapter {
        id: String,
        title: String,
    },
    MoveChapter {
        id: String,
        direction: i32,
    },
    TransferChapter {
        id: String,
        volume_id: String,
    },
    PlaceChapter {
        id: String,
        volume_id: String,
        before_id: Option<String>,
    },
    PlaceVolume {
        id: String,
        before_id: Option<String>,
    },
    SaveChapter {
        id: String,
        body: String,
        expected_revision: i64,
    },
}
#[derive(Serialize, Deserialize)]
struct Backup {
    version: u32,
    #[serde(flatten)]
    state: LibraryState,
    #[serde(default)]
    history: Option<Vec<crate::ChapterVersion>>,
    #[serde(default)]
    materials: Option<Vec<crate::Material>>,
    #[serde(default)]
    version_labels: Option<Vec<crate::history_browser::VersionLabelRecord>>,
    #[serde(default)]
    material_content_history: Option<Vec<crate::MaterialContentVersion>>,
    #[serde(default)]
    writing_evidence: Option<crate::WritingEvidence>,
    #[serde(default)]
    foreshadow_occurrences: Option<Vec<crate::foreshadow::StoredOccurrence>>,
    #[serde(default, deserialize_with = "present_proposal_backup")]
    foreshadow_proposals: Option<crate::foreshadow_proposals::ProposalBackup>,
}
fn present_proposal_backup<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Option<crate::foreshadow_proposals::ProposalBackup>, D::Error> {
    // Old formats must not have this field at all, even explicitly null.
    crate::foreshadow_proposals::ProposalBackup::deserialize(d).map(Some)
}
pub struct Service {
    pub(crate) split_candidate: Option<crate::chapter_split::SplitCandidate>,
    pub(crate) conn: Connection,
    pub(crate) replace_candidate: Option<crate::book_replace::ReplaceCandidate>,
    pub(crate) import_candidate: Option<crate::txt_import::ImportCandidate>,
    pub(crate) project_import: Option<crate::project_import::ProjectImportSession>,
    pub(crate) organization: Option<crate::project_import::OrganizationCandidate>,
    pub(crate) export_candidate: Option<crate::txt_export::ExportCandidate>,
    pub(crate) draft_recovery_path: Option<std::path::PathBuf>,
    pub(crate) draft_recovery_session: Option<crate::DraftRecoverySession>,
    _writer_lock: Option<std::fs::File>,
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn title(t: &str) -> Result<()> {
    if t.trim().is_empty() || t.len() > 4096 {
        Err("标题为空或过长".into())
    } else {
        Ok(())
    }
}
impl Service {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let (writer_lock, draft_recovery_path) = if path == Path::new(":memory:") {
            (None, None)
        } else {
            // Canonicalize the database file so relative paths and symlinks use
            // the same sidecar. Never truncate an existing database or lockfile.
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(path)
                .map_err(err)?;
            let canonical = std::fs::canonicalize(path).map_err(err)?;
            let mut name = canonical.as_os_str().to_os_string();
            name.push(".writer.lock");
            let lock = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(std::path::PathBuf::from(name))
                .map_err(err)?;
            fs2::FileExt::try_lock_exclusive(&lock)
                .map_err(|_| "writer_locked: 此数据库已有写入进程".to_string())?;
            let mut drafts = canonical.into_os_string();
            drafts.push(".draft-recovery.sqlite");
            (Some(lock), Some(std::path::PathBuf::from(drafts)))
        };
        let mut conn = Connection::open(path).map_err(err)?;
        // A current file must not acquire a fresh epoch via legacy migration:
        // that would silently archive its live pending suggestions.
        if crate::foreshadow_proposals::schema_generation(&conn).map_err(err)? == 2 {
            crate::foreshadow_proposals::backup_epoch(&conn).map_err(err)?;
        }
        conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS books(id TEXT PRIMARY KEY,title TEXT NOT NULL,author TEXT NOT NULL,deleted_at INTEGER); CREATE TABLE IF NOT EXISTS volumes(id TEXT PRIMARY KEY,book_id TEXT NOT NULL REFERENCES books(id),title TEXT NOT NULL,sort_order INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS chapters(id TEXT PRIMARY KEY,volume_id TEXT NOT NULL REFERENCES volumes(id),title TEXT NOT NULL,body TEXT NOT NULL,revision INTEGER NOT NULL,sort_order INTEGER NOT NULL);").map_err(err)?;
        // Each column is checked independently so reopening either an old database
        // or an already migrated one is safe. Both migrations commit atomically.
        let migration = conn.transaction().map_err(err)?;
        for table in ["volumes", "chapters"] {
            let columns = migration
                .prepare(&format!("PRAGMA table_info({table})"))
                .map_err(err)?
                .query_map([], |row| row.get::<_, String>(1))
                .map_err(err)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(err)?;
            if !columns.iter().any(|column| column == "deleted_at") {
                migration
                    .execute_batch(&format!(
                        "ALTER TABLE {table} ADD COLUMN deleted_at INTEGER;"
                    ))
                    .map_err(err)?;
            }
        }
        migration.commit().map_err(err)?;
        crate::workflow::migrate(&conn).map_err(err)?;
        crate::fanqie_jobs::migrate(&conn).map_err(err)?;
        crate::history_browser::migrate(&conn).map_err(err)?;
        crate::materials::migrate(&conn).map_err(err)?;
        crate::material_workflow::migrate(&conn).map_err(err)?;
        crate::context_jobs::migrate(&conn).map_err(err)?;
        crate::constrained_writing::migrate(&conn).map_err(err)?;
        crate::foreshadow_proposals::migrate(&conn).map_err(err)?;
        Ok(Self {
            conn,
            _writer_lock: writer_lock,
            replace_candidate: None,
            import_candidate: None,
            project_import: None,
            organization: None,
            export_candidate: None,
            split_candidate: None,
            draft_recovery_path,
            draft_recovery_session: None,
        })
    }
    pub fn load(&self) -> Result<LibraryState> {
        let books = self
            .conn
            .prepare("SELECT id,title,author,deleted_at FROM books ORDER BY rowid")
            .map_err(err)?
            .query_map([], |r| {
                Ok(Book {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    author: r.get(2)?,
                    deleted_at: r.get(3)?,
                })
            })
            .map_err(err)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(err)?;
        let volumes = self
            .conn
            .prepare(
                "SELECT id,book_id,title,sort_order,deleted_at FROM volumes ORDER BY book_id,sort_order,id",
            )
            .map_err(err)?
            .query_map([], |r| {
                Ok(Volume {
                    id: r.get(0)?,
                    book_id: r.get(1)?,
                    title: r.get(2)?,
                    sort_order: r.get(3)?,
                    deleted_at:r.get(4)?,
                })
            })
            .map_err(err)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(err)?;
        let chapters=self.conn.prepare("SELECT id,volume_id,title,body,revision,sort_order,deleted_at FROM chapters ORDER BY volume_id,sort_order,id").map_err(err)?.query_map([],|r|Ok(Chapter{id:r.get(0)?,volume_id:r.get(1)?,title:r.get(2)?,body:r.get(3)?,revision:r.get(4)?,sort_order:r.get(5)?,deleted_at:r.get(6)?})).map_err(err)?.collect::<rusqlite::Result<Vec<_>>>().map_err(err)?;
        Ok(LibraryState {
            books,
            volumes,
            chapters,
        })
    }
    fn write(&mut self, s: &LibraryState) -> Result<()> {
        validate(s)?;
        let t = self.conn.transaction().map_err(err)?;
        write_state(&t, s)?;
        t.commit().map_err(err)
    }
    pub fn apply(&mut self, command: Command) -> Result<LibraryState> {
        let command = match command {
            Command::SaveChapter {
                id,
                body,
                expected_revision,
            } => {
                let transaction = self.conn.transaction().map_err(err)?;
                crate::workflow::save_in_transaction(
                    &transaction,
                    &id,
                    &body,
                    expected_revision,
                    "gui",
                )
                .map_err(err)?;
                transaction.commit().map_err(err)?;
                return self.load();
            }
            other => other,
        };
        let mut s = self.load()?;
        match &command {
            Command::UpdateBook { id, .. } | Command::CreateVolume { book_id: id, .. } => {
                active_book(&s, id)?;
            }
            Command::RenameVolume { id, .. }
            | Command::MoveVolume { id, .. }
            | Command::CreateChapter { volume_id: id, .. } => {
                active_volume(&s, id)?;
            }
            Command::RenameChapter { id, .. } | Command::MoveChapter { id, .. } => {
                active_chapter(&s, id)?;
            }
            Command::TransferChapter { id, volume_id } => {
                active_chapter(&s, id)?;
                active_volume(&s, volume_id)?;
            }
            Command::PlaceChapter { id, volume_id, .. } => {
                active_chapter(&s, id)?;
                active_volume(&s, volume_id)?;
            }
            Command::PlaceVolume { id, .. } => {
                active_volume(&s, id)?;
            }
            _ => {}
        }
        match command {
            Command::TrashVolume { id } => {
                s.volumes
                    .iter_mut()
                    .find(|v| v.id == id)
                    .ok_or("卷不存在")?
                    .deleted_at = Some(now()?)
            }
            Command::RestoreVolume { id } => {
                let v = s.volumes.iter().find(|v| v.id == id).ok_or("卷不存在")?;
                active_book(&s, &v.book_id).map_err(|_| "请先恢复所属作品".to_string())?;
                s.volumes
                    .iter_mut()
                    .find(|v| v.id == id)
                    .unwrap()
                    .deleted_at = None;
            }
            Command::TrashChapter { id } => {
                s.chapters
                    .iter_mut()
                    .find(|c| c.id == id)
                    .ok_or("章不存在")?
                    .deleted_at = Some(now()?)
            }
            Command::RestoreChapter { id } => {
                let c = s.chapters.iter().find(|c| c.id == id).ok_or("章不存在")?;
                active_volume(&s, &c.volume_id).map_err(|_| "请先恢复所属作品和卷".to_string())?;
                s.chapters
                    .iter_mut()
                    .find(|c| c.id == id)
                    .unwrap()
                    .deleted_at = None;
            }
            Command::PurgeBook { id } => {
                let b = s.books.iter().find(|b| b.id == id).ok_or("作品不存在")?;
                if b.deleted_at.is_none() {
                    return Err("仅可永久删除已在回收站的作品".into());
                }
                let volumes: HashSet<String> = s
                    .volumes
                    .iter()
                    .filter(|v| v.book_id == id)
                    .map(|v| v.id.clone())
                    .collect();
                s.chapters.retain(|c| !volumes.contains(&c.volume_id));
                s.volumes.retain(|v| v.book_id != id);
                s.books.retain(|b| b.id != id);
            }
            Command::PurgeVolume { id } => {
                let v = s.volumes.iter().find(|v| v.id == id).ok_or("卷不存在")?;
                if v.deleted_at.is_none() {
                    return Err("仅可永久删除已在回收站的卷".into());
                }
                s.chapters.retain(|c| c.volume_id != id);
                s.volumes.retain(|v| v.id != id);
            }
            Command::PurgeChapter { id } => {
                let c = s.chapters.iter().find(|c| c.id == id).ok_or("章不存在")?;
                if c.deleted_at.is_none() {
                    return Err("仅可永久删除已在回收站的章".into());
                }
                s.chapters.retain(|c| c.id != id);
            }
            Command::CreateBook { title: t, author } => {
                title(&t)?;
                let b = id();
                let v = id();
                s.books.push(Book {
                    id: b.clone(),
                    title: t,
                    author,
                    deleted_at: None,
                });
                s.volumes.push(Volume {
                    id: v.clone(),
                    book_id: b,
                    title: "启程".into(),
                    sort_order: 0,
                    deleted_at: None,
                });
                s.chapters.push(Chapter {
                    id: id(),
                    volume_id: v,
                    title: "初章".into(),
                    body: String::new(),
                    revision: 0,
                    sort_order: 0,
                    deleted_at: None,
                });
            }
            Command::UpdateBook {
                id,
                title: t,
                author,
            } => {
                let b = s
                    .books
                    .iter_mut()
                    .find(|x| x.id == id)
                    .ok_or("作品不存在")?;
                b.title = t;
                b.author = author;
            }
            Command::TrashBook { id } => {
                s.books
                    .iter_mut()
                    .find(|x| x.id == id)
                    .ok_or("作品不存在")?
                    .deleted_at = Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_err(err)?
                        .as_secs() as i64,
                );
            }
            Command::RestoreBook { id } => {
                s.books
                    .iter_mut()
                    .find(|x| x.id == id)
                    .ok_or("作品不存在")?
                    .deleted_at = None
            }
            Command::CreateVolume { book_id, title } => {
                if !s.books.iter().any(|b| b.id == book_id) {
                    return Err("作品不存在".into());
                }
                let sort_order = s
                    .volumes
                    .iter()
                    .filter(|v| v.book_id == book_id)
                    .map(|v| v.sort_order)
                    .max()
                    .map_or(Ok(0), |n| n.checked_add(1).ok_or("排序溢出"))?;
                s.volumes.push(Volume {
                    id: id(),
                    book_id,
                    title,
                    sort_order,
                    deleted_at: None,
                });
            }
            Command::RenameVolume { id, title } => {
                s.volumes
                    .iter_mut()
                    .find(|v| v.id == id)
                    .ok_or("卷不存在")?
                    .title = title
            }
            Command::MoveVolume { id, direction } => {
                let i = s
                    .volumes
                    .iter()
                    .position(|v| v.id == id)
                    .ok_or("卷不存在")?;
                let peers: Vec<usize> = s
                    .volumes
                    .iter()
                    .enumerate()
                    .filter(|(_, v)| v.book_id == s.volumes[i].book_id && v.deleted_at.is_none())
                    .map(|(i, _)| i)
                    .collect();
                if let Some(j) = neighbor(&peers, i, direction)? {
                    let n = s.volumes[i].sort_order;
                    s.volumes[i].sort_order = s.volumes[j].sort_order;
                    s.volumes[j].sort_order = n;
                }
            }
            Command::CreateChapter { volume_id, title } => {
                if !s.volumes.iter().any(|v| v.id == volume_id) {
                    return Err("卷不存在".into());
                }
                let sort_order = s
                    .chapters
                    .iter()
                    .filter(|c| c.volume_id == volume_id)
                    .map(|c| c.sort_order)
                    .max()
                    .map_or(Ok(0), |n| n.checked_add(1).ok_or("排序溢出"))?;
                s.chapters.push(Chapter {
                    id: id(),
                    volume_id,
                    title,
                    body: String::new(),
                    revision: 0,
                    sort_order,
                    deleted_at: None,
                });
            }
            Command::RenameChapter { id, title } => {
                s.chapters
                    .iter_mut()
                    .find(|c| c.id == id)
                    .ok_or("章不存在")?
                    .title = title
            }
            Command::MoveChapter { id, direction } => {
                let i = s
                    .chapters
                    .iter()
                    .position(|c| c.id == id)
                    .ok_or("章不存在")?;
                let peers: Vec<usize> = s
                    .chapters
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| {
                        c.volume_id == s.chapters[i].volume_id && c.deleted_at.is_none()
                    })
                    .map(|(i, _)| i)
                    .collect();
                if let Some(j) = neighbor(&peers, i, direction)? {
                    let n = s.chapters[i].sort_order;
                    s.chapters[i].sort_order = s.chapters[j].sort_order;
                    s.chapters[j].sort_order = n;
                }
            }
            Command::TransferChapter { id, volume_id } => {
                if !s.volumes.iter().any(|v| v.id == volume_id) {
                    return Err("目的卷不存在".into());
                }
                let index = s
                    .chapters
                    .iter()
                    .position(|c| c.id == id)
                    .ok_or("章不存在")?;
                let source = s.chapters[index].volume_id.clone();
                let source_book = &s
                    .volumes
                    .iter()
                    .find(|v| v.id == source)
                    .ok_or("原卷不存在")?
                    .book_id;
                if s.volumes
                    .iter()
                    .find(|v| v.id == volume_id)
                    .ok_or("目的卷不存在")?
                    .book_id
                    != *source_book
                {
                    return Err("不能跨作品移动章节".into());
                }
                if source != volume_id {
                    let next = s
                        .chapters
                        .iter()
                        .filter(|c| c.volume_id == volume_id)
                        .map(|c| c.sort_order)
                        .max()
                        .map_or(Ok(0), |n| n.checked_add(1).ok_or("排序溢出"))?;
                    s.chapters[index].volume_id = volume_id;
                    s.chapters[index].sort_order = next;
                    let mut peers: Vec<usize> = s
                        .chapters
                        .iter()
                        .enumerate()
                        .filter(|(_, c)| c.volume_id == source)
                        .map(|(i, _)| i)
                        .collect();
                    peers.sort_by_key(|i| s.chapters[*i].sort_order);
                    for (order, index) in peers.into_iter().enumerate() {
                        s.chapters[index].sort_order = order as i64;
                    }
                }
            }
            Command::PlaceChapter {
                id,
                volume_id,
                before_id,
            } => {
                if before_id.as_deref() == Some(id.as_str()) {
                    return self.load();
                }
                let index = s
                    .chapters
                    .iter()
                    .position(|c| c.id == id)
                    .ok_or("章不存在")?;
                let source = s.chapters[index].volume_id.clone();
                let source_book = s
                    .volumes
                    .iter()
                    .find(|v| v.id == source)
                    .ok_or("原卷不存在")?
                    .book_id
                    .clone();
                if s.volumes
                    .iter()
                    .find(|v| v.id == volume_id)
                    .ok_or("目的卷不存在")?
                    .book_id
                    != source_book
                {
                    return Err("不能跨作品移动章节".into());
                }
                if let Some(before) = before_id.as_deref() {
                    let target = s
                        .chapters
                        .iter()
                        .find(|c| c.id == before)
                        .ok_or("目标章节不存在")?;
                    if target.volume_id != volume_id || target.deleted_at.is_some() {
                        return Err("目标章节不属于目的卷".into());
                    }
                }
                let mut target: Vec<usize> = s
                    .chapters
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.volume_id == volume_id && c.id != id)
                    .map(|(i, _)| i)
                    .collect();
                target.sort_by_key(|i| (s.chapters[*i].sort_order, s.chapters[*i].id.clone()));
                let position = match before_id.as_deref() {
                    Some(before) => target
                        .iter()
                        .position(|i| s.chapters[*i].id == before)
                        .ok_or("目标章节不属于目的卷")?,
                    None => target.len(),
                };
                target.insert(position, index);
                if source != volume_id {
                    s.chapters[index].volume_id = volume_id.clone();
                }
                for (order, i) in target.into_iter().enumerate() {
                    s.chapters[i].sort_order = order as i64;
                }
                if source != volume_id {
                    let mut peers: Vec<usize> = s
                        .chapters
                        .iter()
                        .enumerate()
                        .filter(|(_, c)| c.volume_id == source)
                        .map(|(i, _)| i)
                        .collect();
                    peers.sort_by_key(|i| (s.chapters[*i].sort_order, s.chapters[*i].id.clone()));
                    for (order, i) in peers.into_iter().enumerate() {
                        s.chapters[i].sort_order = order as i64;
                    }
                }
            }
            Command::PlaceVolume { id, before_id } => {
                if before_id.as_deref() == Some(id.as_str()) {
                    return self.load();
                }
                let index = s
                    .volumes
                    .iter()
                    .position(|v| v.id == id)
                    .ok_or("卷不存在")?;
                let book_id = s.volumes[index].book_id.clone();
                if let Some(before) = before_id.as_deref() {
                    let target = s
                        .volumes
                        .iter()
                        .find(|v| v.id == before)
                        .ok_or("目标卷不存在")?;
                    if target.book_id != book_id {
                        return Err("目标卷不属于同一作品".into());
                    }
                    if target.deleted_at.is_some() {
                        return Err("目标卷不在目录中".into());
                    }
                }
                let mut order: Vec<usize> = s
                    .volumes
                    .iter()
                    .enumerate()
                    .filter(|(_, v)| v.book_id == book_id)
                    .map(|(i, _)| i)
                    .collect();
                order.sort_by_key(|i| (s.volumes[*i].sort_order, s.volumes[*i].id.clone()));
                order.retain(|i| *i != index);
                let position = match before_id.as_deref() {
                    Some(before) => order
                        .iter()
                        .position(|i| s.volumes[*i].id == before)
                        .ok_or("目标卷不存在")?,
                    None => order.len(),
                };
                order.insert(position, index);
                for (order, i) in order.into_iter().enumerate() {
                    s.volumes[i].sort_order = order as i64;
                }
            }
            Command::SaveChapter { .. } => unreachable!("save handled by conditional SQL update"),
        }

        self.write(&s)?;
        self.load()
    }
    pub fn backup(&self) -> Result<String> {
        // Reject oversized raw text before collecting any manuscript/history bodies.
        let (raw, _) = crate::backup_restore::canonical_text_bytes(&self.conn)?;
        if raw > 64 * 1024 * 1024 {
            return Err(
                "正文、历史和资料超64MiB，原库未更改；请使用SQLite一致性快照保留完整数据".into(),
            );
        }
        let mut encoded = crate::backup_restore::BoundedJsonWriter::default();
        serde_json::to_writer_pretty(
            &mut encoded,
            &Backup {
                version: 13,
                state: self.load()?,
                history: Some(crate::workflow::all_versions(&self.conn)?),
                materials: Some(crate::materials::all(&self.conn)?),
                version_labels: Some(crate::history_browser::all_labels(&self.conn)?),
                material_content_history: Some(crate::material_history::all(&self.conn)?),
                writing_evidence: Some(crate::constrained_writing::all(&self.conn).map_err(err)?),
                foreshadow_occurrences: Some(crate::foreshadow::all(&self.conn).map_err(err)?),
                foreshadow_proposals: {
                    let rows = crate::foreshadow_proposals::all(&self.conn).map_err(err)?;
                    let epoch = (!rows.is_empty()).then(|| crate::foreshadow_proposals::backup_epoch(&self.conn)).transpose().map_err(err)?;
                    Some(crate::foreshadow_proposals::ProposalBackup { epoch, rows })
                },
            },
        )
        .map_err(|_| {
            "JSON编码超64MiB或失败，原库未更改；请使用SQLite一致性快照保留完整数据".to_string()
        })?;
        let text = String::from_utf8(encoded.0).map_err(err)?;
        if text.len() > 64 * 1024 * 1024 {
            return Err(
                "正文、历史和资料超64MiB，原库未更改；请使用SQLite一致性快照保留完整数据".into(),
            );
        }
        Ok(text)
    }
    pub fn restore(&mut self, text: &str) -> Result<LibraryState> { self.restore_with_writing_ack(text,false) }
    pub fn restore_with_writing_ack(&mut self, text: &str, writing_ack:bool) -> Result<LibraryState> {
        crate::constrained_writing::restore_guard(&self.conn,writing_ack).map_err(err)?;
        if text.len() > 64 * 1024 * 1024 {
            return Err("备份过大".into());
        }
        let b: Backup = serde_json::from_str(text).map_err(err)?;
        if !(1..=13).contains(&b.version) {
            return Err("不支持的备份版本".into());
        }
        validate(&b.state)?;
        let evidence=if b.version>=9 {b.writing_evidence.ok_or("v9及以上缺少写作依据证据")?}else{b.writing_evidence.unwrap_or_default()};
        if b.version < 10 && evidence.proposals.iter().any(|p| p.author_review.is_some()) {
            return Err("旧版JSON备份不能包含逐项作者审阅".into());
        }
        if b.version < 11 && evidence.has_prior_context_format() {
            return Err("旧版JSON备份不能包含前文冻结配置或新版句柄".into());
        }
        let occurrences = if b.version >= 12 {
            b.foreshadow_occurrences.ok_or("v12备份缺少伏笔出现记录")?
        } else {
            if b.foreshadow_occurrences.is_some_and(|rows| !rows.is_empty()) || evidence.has_foreshadow_format() {
                return Err("旧版JSON备份不能包含伏笔出现证据或新版句柄".into());
            }
            Vec::new()
        };
        let suggestion_backup = if b.version == 13 {
            Some(b.foreshadow_proposals.ok_or("v13备份缺少伏笔建议审计")?)
        } else {
            if b.foreshadow_proposals.is_some() {
                return Err("旧版JSON备份不能包含伏笔建议审计".into());
            }
            None
        };
        crate::constrained_writing::validate_evidence(&b.state,&evidence).map_err(err)?;
        let history = if b.version >= 3 {
            b.history.ok_or("v3/v4 备份缺少历史记录")?
        } else {
            Vec::new()
        };
        crate::workflow::validate_history(&b.state, &history)?;
        let labels = if b.version >= 7 {
            b.version_labels.ok_or("v7备份缺少version_labels")?
        } else {
            Vec::new()
        };
        crate::history_browser::validate_labels(&history, &labels)?;
        let mut materials = if b.version >= 4 {
            b.materials.ok_or("v4/v5 备份缺少materials")?
        } else {
            Vec::new()
        };
        if b.version < 5 {
            for m in &mut materials {
                m.source_chapter_id = None;
                m.source_revision = None;
                m.source_status = "none".into();
            }
        }
        if b.version < 6 {
            for m in &mut materials {
                m.story_order = 0;
                m.story_time = None;
                m.tracking_status = None;
                m.payoff_chapter_id = None;
                m.payoff_revision = None;
                m.payoff_status = "none".into();
            }
        }
        crate::materials::validate(&b.state, &materials)?;
        crate::foreshadow::validate(&b.state, &materials, &occurrences, |ch, rows| {
            crate::foreshadow::validate_quotes(&ch.body, rows)
        }).map_err(err)?;
        let copies = crate::foreshadow::validate_copies(&occurrences, &evidence.packs).map_err(err)?;
        let (suggestion_rows, proposal_epoch) = match &suggestion_backup {
            Some(suggestions) => {
                if suggestions.epoch.is_none() && !suggestions.rows.is_empty() {
                    return Err("v13备份缺少伏笔建议源书库代际".into());
                }
                (suggestions.rows.as_slice(), suggestions.epoch.as_deref())
            }
            None => (&[][..], None),
        };
        crate::foreshadow_proposals::validate(
            &b.state,
            &materials,
            &occurrences,
            &copies,
            suggestion_rows,
            proposal_epoch,
            |ch, rows| crate::foreshadow::validate_quotes(&ch.body, rows),
        ).map_err(err)?;

        let supplied_material_history=b.material_content_history.is_some();
        let material_history=if b.version>=8 {b.material_content_history.ok_or("v8备份缺少资料历史")?}else{b.material_content_history.unwrap_or_default()};
        if supplied_material_history {crate::material_history::validate(&materials,&material_history)?;}
        let tx = self.conn.transaction().map_err(err)?;
        crate::context_jobs::clear_for_restore(&tx).map_err(err)?;
        crate::material_workflow::clear_for_restore(&tx).map_err(err)?;
        crate::constrained_writing::clear(&tx).map_err(err)?;
        tx.execute_batch("DELETE FROM materials;").map_err(err)?;
        write_state(&tx, &b.state)?;
        crate::materials::insert_all(&tx, &materials)?;
        crate::foreshadow::insert_all(&tx, &occurrences).map_err(err)?;
        if let Some(suggestions) = &suggestion_backup {
            crate::foreshadow_proposals::insert_all(&tx, &suggestions.rows).map_err(err)?;
        }
        crate::material_history::insert_all(&tx,&material_history)?;
        crate::material_history::migrate(&tx).map_err(err)?;
        crate::workflow::replace_workflow(&tx, &history)?;
        crate::history_browser::replace_labels(&tx, &labels)?;
        crate::constrained_writing::insert_all(&tx,&evidence).map_err(err)?;
        crate::constrained_writing::archive(&tx).map_err(err)?;
        tx.commit().map_err(err)?;
        self.load()
    }
    pub fn export_book(&self, id: &str) -> Result<String> {
        let s = self.load()?;
        let b = s.books.iter().find(|b| b.id == id).ok_or("作品不存在")?;
        active_book(&s, id)?;
        let mut out = format!("{}\n作者：{}\n\n", b.title, b.author);
        let mut n = 0;
        for (i, v) in s
            .volumes
            .iter()
            .filter(|v| v.book_id == id && v.deleted_at.is_none())
            .enumerate()
        {
            out.push_str(&format!("第{}卷 {}\n\n", i + 1, v.title));
            for c in s
                .chapters
                .iter()
                .filter(|c| c.volume_id == v.id && c.deleted_at.is_none())
            {
                n += 1;
                out.push_str(&format!("第{}章 {}\n\n{}\n\n", n, c.title, c.body));
            }
        }
        Ok(out
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\n', "\r\n"))
    }
}
fn neighbor(peers: &[usize], i: usize, d: i32) -> Result<Option<usize>> {
    if d != 1 && d != -1 {
        return Err("排序方向必须为 -1 或 1".into());
    }
    let p = peers.iter().position(|x| *x == i).unwrap() as i64 + d as i64;
    Ok(if p < 0 {
        None
    } else {
        peers.get(p as usize).copied()
    })
}
pub(crate) fn validate(s: &LibraryState) -> Result<()> {
    validate_directory(s, false)
}

// Frozen snapshots can preserve legacy directory ties. Their order is
// determined by the full stable key, not only the numeric position.
pub(crate) fn validate_frozen_directory(s: &LibraryState) -> Result<()> {
    validate_directory(s, true)
}

fn validate_directory(s: &LibraryState, allow_order_ties: bool) -> Result<()> {
    if s.books.len() + s.volumes.len() + s.chapters.len() > 100000 {
        return Err("记录过多".into());
    }
    let mut ids = HashSet::new();
    for x in s
        .books
        .iter()
        .map(|b| &b.id)
        .chain(s.volumes.iter().map(|v| &v.id))
        .chain(s.chapters.iter().map(|c| &c.id))
    {
        if x.is_empty() || x.len() > 128 || !ids.insert(x) {
            return Err("ID 为空、过长或重复".into());
        }
    }
    for b in &s.books {
        title(&b.title)?;
        if b.author.len() > 4096 {
            return Err("作者过长".into());
        }
    }
    let book_ids: HashSet<_> = s.books.iter().map(|b| b.id.as_str()).collect();
    let volume_ids: HashSet<_> = s.volumes.iter().map(|v| v.id.as_str()).collect();
    let mut order = HashSet::new();
    for v in &s.volumes {
        title(&v.title)?;
        if !book_ids.contains(v.book_id.as_str())
            || v.sort_order < 0
            || (!allow_order_ties && !order.insert((&v.book_id, v.sort_order)))
        {
            return Err("卷引用或顺序无效".into());
        }
    }
    let mut order = HashSet::new();
    for c in &s.chapters {
        title(&c.title)?;
        if !volume_ids.contains(c.volume_id.as_str())
            || c.revision < 0
            || c.body.len() > 10 * 1024 * 1024
            || c.sort_order < 0
            || (!allow_order_ties && !order.insert((&c.volume_id, c.sort_order)))
        {
            return Err("章引用、版本、正文或顺序无效".into());
        }
    }
    Ok(())
}

fn now() -> Result<i64> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(err)?
        .as_secs() as i64)
}
fn active_book<'a>(s: &'a LibraryState, id: &str) -> Result<&'a Book> {
    let b = s.books.iter().find(|b| b.id == id).ok_or("作品不存在")?;
    if b.deleted_at.is_some() {
        return Err("作品在回收站，请先恢复作品".into());
    }
    Ok(b)
}
fn active_volume<'a>(s: &'a LibraryState, id: &str) -> Result<&'a Volume> {
    let v = s.volumes.iter().find(|v| v.id == id).ok_or("卷不存在")?;
    active_book(s, &v.book_id)?;
    if v.deleted_at.is_some() {
        return Err("卷在回收站，请先恢复卷".into());
    }
    Ok(v)
}
fn active_chapter<'a>(s: &'a LibraryState, id: &str) -> Result<&'a Chapter> {
    let c = s.chapters.iter().find(|c| c.id == id).ok_or("章不存在")?;
    active_volume(s, &c.volume_id)?;
    if c.deleted_at.is_some() {
        return Err("章在回收站，请先恢复章".into());
    }
    Ok(c)
}

fn write_state(t: &rusqlite::Transaction, s: &LibraryState) -> Result<()> {
    for b in &s.books {
        t.execute(
                "INSERT INTO books(id,title,author,deleted_at) VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET title=excluded.title,author=excluded.author,deleted_at=excluded.deleted_at",
                params![b.id, b.title, b.author, b.deleted_at],
            )
            .map_err(err)?;
    }
    for v in &s.volumes {
        t.execute(
                "INSERT INTO volumes(id,book_id,title,sort_order,deleted_at) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET book_id=excluded.book_id,title=excluded.title,sort_order=excluded.sort_order,deleted_at=excluded.deleted_at",
                params![v.id, v.book_id, v.title, v.sort_order,v.deleted_at],
            )
            .map_err(err)?;
    }
    for c in &s.chapters {
        t.execute(
                "INSERT INTO chapters(id,volume_id,title,body,revision,sort_order,deleted_at) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(id) DO UPDATE SET volume_id=excluded.volume_id,title=excluded.title,body=excluded.body,revision=excluded.revision,sort_order=excluded.sort_order,deleted_at=excluded.deleted_at",
                params![c.id, c.volume_id, c.title, c.body, c.revision, c.sort_order,c.deleted_at],
            )
            .map_err(err)?;
    }

    crate::materials::detach_removed_targets(t, s)?;
    for (table, ids) in [
        (
            "chapters",
            s.chapters
                .iter()
                .map(|c| c.id.as_str())
                .collect::<HashSet<_>>(),
        ),
        (
            "volumes",
            s.volumes
                .iter()
                .map(|v| v.id.as_str())
                .collect::<HashSet<_>>(),
        ),
        (
            "books",
            s.books
                .iter()
                .map(|b| b.id.as_str())
                .collect::<HashSet<_>>(),
        ),
    ] {
        let existing = t
            .prepare(&format!("SELECT id FROM {table}"))
            .map_err(err)?
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(err)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(err)?;
        for id in existing {
            if !ids.contains(id.as_str()) {
                t.execute(&format!("DELETE FROM {table} WHERE id=?1"), [id])
                    .map_err(err)?;
            }
        }
    }

    Ok(())
}

/// Progress is reported after each bounded SQLite backup step. Returning an
/// error cancels the operation before a recovery path is published.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct SnapshotProgress {
    pub remaining_pages: i32,
    pub total_pages: i32,
}
impl Service {
    /// Create a verified, no-clobber, page-streamed SQLite recovery copy. Unlike
    /// JSON exports this preserves all history/proposals without a size cap.
    pub fn recovery_snapshot(&self, path: impl AsRef<Path>) -> Result<std::path::PathBuf> {
        self.recovery_snapshot_with_progress(path, |_| Ok(()))
    }
    pub fn recovery_snapshot_with_cancel(
        &self,
        path: impl AsRef<Path>,
        cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
        progress: impl FnMut(SnapshotProgress) -> Result<()>,
    ) -> Result<std::path::PathBuf> {
        snapshot_to_control(
            &self.conn,
            path.as_ref(),
            progress,
            |target| {
                target
                    .execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;")
                    .map_err(err)
            },
            cancel,
        )
    }
    pub fn recovery_snapshot_with_progress(
        &self,
        path: impl AsRef<Path>,
        progress: impl FnMut(SnapshotProgress) -> Result<()>,
    ) -> Result<std::path::PathBuf> {
        snapshot_to(&self.conn, path.as_ref(), progress, |target| {
            target
                .execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;")
                .map_err(err)
        })
    }
}
struct SnapshotTemporary(std::path::PathBuf);
impl Drop for SnapshotTemporary {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn snapshot_to(
    source: &Connection,
    path: &Path,
    progress: impl FnMut(SnapshotProgress) -> Result<()>,
    configure: impl FnOnce(&Connection) -> Result<()>,
) -> Result<std::path::PathBuf> {
    snapshot_to_control(
        source,
        path,
        progress,
        configure,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    )
}
fn snapshot_to_control(
    source: &Connection,
    path: &Path,
    mut progress: impl FnMut(SnapshotProgress) -> Result<()>,
    configure: impl FnOnce(&Connection) -> Result<()>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<std::path::PathBuf> {
    use std::sync::atomic::Ordering;
    if cancel.load(Ordering::Relaxed) {
        return Err("backup_job_cancelled".into());
    }
    use rusqlite::backup::{Backup, StepResult};
    if path.file_name().is_none() {
        return Err("快照目标必须是文件路径".into());
    }
    if path.try_exists().map_err(err)? {
        return Err("快照目标已存在，不会覆盖".into());
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let temporary = parent.join(format!(".recovery-{}.sqlite.tmp", id()));
    let reserved = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(err)?;
    let temporary = SnapshotTemporary(temporary);
    drop(reserved);
    let mut target = Connection::open(&temporary.0).map_err(err)?;
    configure(&target)?;
    let mut final_progress: SnapshotProgress;
    {
        let backup = Backup::new(source, &mut target).map_err(err)?;
        let mut retries = 0;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err("backup_job_cancelled".into());
            }
            let step = backup
                .step(64)
                .map_err(|e| format!("SQLite快照复制失败 ({:?}): {e}", e.sqlite_error_code()))?;
            let p = backup.progress();
            final_progress = SnapshotProgress {
                remaining_pages: p.remaining,
                total_pages: p.pagecount,
            };
            progress(final_progress)?;
            match step {
                StepResult::Done => break,
                StepResult::More => {
                    retries = 0;
                }
                StepResult::Busy | StepResult::Locked => {
                    retries += 1;
                    if retries > 20 {
                        return Err("快照数据库持续繁忙，未创建可用留底".into());
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                _ => return Err("未知SQLite快照状态".into()),
            }
        }
    }
    // A standalone recovery file must not depend on an unexported WAL file.
    target
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;")
        .map_err(err)?;
    target.close().map_err(|(_, e)| err(e))?;
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&temporary.0)
        .map_err(err)?
        .sync_all()
        .map_err(err)?;
    let mut verification =
        Connection::open_with_flags(&temporary.0, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(err)?;
    let verify_cancel = cancel.clone();
    let deadline = std::time::Instant::now();
    verification.progress_handler(
        1000,
        Some(move || {
            verify_cancel.load(Ordering::Relaxed)
                || deadline.elapsed() > std::time::Duration::from_secs(120)
        }),
    ).map_err(err)?;
    if let Err(error) = validate_recovery_database(&verification) {
        if cancel.load(Ordering::Relaxed) {
            return Err("backup_job_cancelled".into());
        }
        return Err(error);
    }
    progress(final_progress)?;
    if cancel.load(Ordering::Relaxed) {
        return Err("backup_job_cancelled".into());
    }
    verification.close().map_err(|(_, e)| err(e))?;
    // hard_link publishes an already verified file atomically and fails if
    // anything appeared at the destination, unlike overwrite-capable rename.
    std::fs::hard_link(&temporary.0, path).map_err(|e| format!("无法无覆盖发布快照：{e}"))?;
    #[cfg(unix)]
    if let Err(e) = std::fs::File::open(parent).and_then(|d| d.sync_all()) {
        // Only unlink our own published inode, never a replacement path.
        use std::os::unix::fs::MetadataExt;
        if let (Ok(a), Ok(b)) = (std::fs::metadata(&temporary.0), std::fs::metadata(path)) {
            if a.dev() == b.dev() && a.ino() == b.ino() {
                let _ = std::fs::remove_file(path);
            }
        }
        return Err(format!("快照目录同步失败，未放行恢复：{e}"));
    }
    Ok(path.to_path_buf())
}
fn validate_recovery_database(conn: &Connection) -> Result<()> {
    let mut integrity = conn.prepare("PRAGMA integrity_check").map_err(err)?;
    let checks = integrity
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(err)?;
    let mut count = 0;
    for check in checks {
        count += 1;
        if check.map_err(err)? != "ok" {
            return Err("快照完整性检查失败".into());
        }
    }
    if count != 1 {
        return Err("快照完整性检查结果异常".into());
    }
    if conn
        .prepare("PRAGMA foreign_key_check")
        .map_err(err)?
        .query([])
        .map_err(err)?
        .next()
        .map_err(err)?
        .is_some()
    {
        return Err("快照包含无效树或历史引用".into());
    }
    for query in [
        "SELECT id,title,author,deleted_at FROM books LIMIT 0",
        "SELECT id,book_id,title,sort_order,deleted_at FROM volumes LIMIT 0",
        "SELECT id,volume_id,title,body,revision,sort_order,deleted_at FROM chapters LIMIT 0",
        "SELECT id,chapter_id,revision,body,source,created_at FROM chapter_versions LIMIT 0",
        "SELECT version_id,label,label_revision,updated_at FROM chapter_version_labels LIMIT 0",
        "SELECT id,client_id,idempotency_key,payload,chapter_id,base_revision,old_body,new_body,reason,source,status,created_at FROM edit_proposals LIMIT 0",
        "SELECT id,chapter_id,action,source,revision,created_at FROM workflow_audit LIMIT 0",
        "SELECT id,book_id,kind,title,fields,volume_id,chapter_id,revision,deleted_at,updated_at,field_bytes,source_chapter_id,source_revision,story_order,story_time,tracking_status,payoff_chapter_id,payoff_revision FROM materials LIMIT 0",
        "SELECT id,client_id,material_id,base_revision,status,record,payload,idempotency_key,created_at FROM material_proposals LIMIT 0",
        "SELECT id,proposal_id,material_id,action,actor,revision,created_at FROM material_proposal_audit LIMIT 0",
        "SELECT id,book_id,status,record,manifest,input_epoch,request_key,request_payload,created_at,storage_bytes FROM context_tasks LIMIT 0",
        "SELECT task_id,ordinal,record FROM context_task_items LIMIT 0",
        "SELECT task_id,ordinal,record FROM context_task_plan LIMIT 0",
    ]{conn.prepare(query).map_err(|e|format!("快照schema校验失败：{e}"))?;}
    if crate::foreshadow_proposals::schema_generation(conn).map_err(err)? != 2 {
        return Err("当前留底快照缺少SQLite代际标记".into());
    }
    let epoch: String = conn
        .query_row(
            "SELECT value FROM workflow_meta WHERE key='epoch'",
            [],
            |r| r.get(0),
        )
        .map_err(err)?;
    if epoch.is_empty() || epoch.len() > 128 {
        return Err("快照epoch无效".into());
    }
    Ok(())
}
#[cfg(test)]
mod recovery_capacity_tests {
    use super::*;
    #[test]
    fn sqlite_full_failure_does_not_publish_or_mutate_source() {
        let dir = std::env::temp_dir().join(format!("mojuan-capacity-fixture-{}", id()));
        std::fs::create_dir(&dir).unwrap();
        let mut source = Service::open(":memory:").unwrap();
        let a = source
            .apply(Command::CreateBook {
                title: "容量测试".into(),
                author: "".into(),
            })
            .unwrap();
        source
            .apply(Command::SaveChapter {
                id: a.chapters[0].id.clone(),
                body: "x".repeat(1024 * 1024),
                expected_revision: 0,
            })
            .unwrap();
        let before = source.load().unwrap();
        let destination = dir.join("full.sqlite");
        let error = snapshot_to(
            &source.conn,
            &destination,
            |_| Ok(()),
            |target| {
                target
                    .execute_batch("PRAGMA journal_mode=DELETE; PRAGMA max_page_count=1;")
                    .map_err(err)
            },
        )
        .unwrap_err();
        assert!(error.to_lowercase().contains("full"), "{error}");
        assert!(!destination.exists());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        assert_eq!(source.load().unwrap(), before);
        std::fs::remove_dir(&dir).unwrap();
    }
}
#[cfg(test)]
mod bounded_tree_validation_tests {
    use super::*;
    #[test]
    fn validates_one_hundred_thousand_tree_records_with_indexed_references() {
        let mut state = LibraryState {
            books: Vec::new(),
            volumes: Vec::new(),
            chapters: Vec::new(),
        };
        for n in 0..33333 {
            let b = format!("b{n}");
            let v = format!("v{n}");
            state.books.push(Book {
                id: b.clone(),
                title: "fiction".into(),
                author: String::new(),
                deleted_at: None,
            });
            state.volumes.push(Volume {
                id: v.clone(),
                book_id: b,
                title: "volume".into(),
                sort_order: 0,
                deleted_at: None,
            });
            state.chapters.push(Chapter {
                id: format!("c{n}"),
                volume_id: v,
                title: "chapter".into(),
                body: String::new(),
                revision: 0,
                sort_order: 0,
                deleted_at: None,
            });
        }
        validate(&state).unwrap();
        state.chapters.last_mut().unwrap().volume_id = "missing".into();
        assert!(validate(&state).is_err());
    }
}
