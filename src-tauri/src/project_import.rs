//! Local project imports freeze one reviewed manuscript and its selected notes.
mod organize;
mod text;
pub(crate) use organize::OrganizationCandidate;
pub use organize::{OrganizationCard, OrganizationPreview};

use crate::materials::CARD_BYTES;
use crate::txt_import::{
    self, db, decode_import_text, epoch, fail, now, uid, Chapter, Volume, SOURCE_LIMIT,
};
use crate::{CreateMaterialInput, ImportDetail, ImportVolume, Service, WorkflowResult};
use rusqlite::TransactionBehavior;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const LIFETIME: Duration = Duration::from_secs(600);
const FILE_COUNT_LIMIT: usize = 256;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectImportRoles {
    pub manuscript: Option<String>,
    pub world: Option<String>,
    pub outlines: Option<String>,
    pub style: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectImportOptions {
    pub roles: ProjectImportRoles,
    pub title: Option<String>,
    pub author: String,
    pub omit_separators: bool,
    #[serde(default)]
    pub automatic: bool,
    pub manuscript_override: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectImportFile {
    pub name: String,
    pub bytes: u64,
    pub kind: &'static str,
    pub selectable: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectImportSelection {
    pub id: String,
    pub folder_name: String,
    pub expires_at: i64,
    pub files: Vec<ProjectImportFile>,
    pub suggested_roles: ProjectImportRoles,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct LineRange {
    pub first: usize,
    pub last: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct NumberingIssue {
    pub volume: usize,
    pub first: u32,
    pub last: u32,
    pub kind: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectAssociation {
    pub number: u32,
    pub volume_index: Option<usize>,
    pub status: &'static str,
    pub chapter_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_number: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectImportCard {
    pub title: String,
    pub kind: &'static str,
    pub file: String,
    pub first_line: usize,
    pub last_line: usize,
    pub association: Option<ProjectAssociation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_index: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectImportPreview {
    pub id: String,
    pub book_id: String,
    pub source_name: String,
    pub title: String,
    pub author: String,
    pub expires_at: i64,
    pub volumes: Vec<ImportVolume>,
    pub total_chapters: usize,
    pub separator_lines: Vec<LineRange>,
    pub omitted_separators: bool,
    pub numbering: Vec<NumberingIssue>,
    pub cards: Vec<ProjectImportCard>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectLink {
    pub card_index: usize,
    pub chapter_index: u32,
}

#[derive(Debug, Serialize)]
pub struct ProjectImportOutcome {
    pub book_id: String,
    pub volumes: usize,
    pub chapters: usize,
    pub cards: usize,
}

pub(crate) struct ProjectImportSession {
    folder: PathBuf,
    selection: ProjectImportSelection,
    epoch: String,
    started: Instant,
    candidate: Option<ProjectCandidate>,
}

struct ProjectCandidate {
    preview: ProjectImportPreview,
    volumes: Vec<Volume>,
    cards: Vec<PendingCard>,
}

struct PendingCard {
    preview: ProjectImportCard,
    material_kind: &'static str,
    field: &'static str,
    content: String,
    source_files: Option<String>,
}

fn io_error() -> crate::WorkflowError {
    fail("file_error", "无法读取所选作品文件或目录，请重新选择")
}

fn not_found() -> crate::WorkflowError {
    fail("not_found", "作品导入预览不存在或已处理，请先核对书架")
}

fn file_limit(kind: &str) -> usize {
    if kind == "txt" {
        SOURCE_LIMIT
    } else {
        CARD_BYTES
    }
}

fn scan(folder: &Path) -> WorkflowResult<Vec<ProjectImportFile>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(folder).map_err(|_| io_error())? {
        let entry = entry.map_err(|_| io_error())?;
        let Some(name) = entry.file_name().into_string().ok() else {
            continue;
        };
        let kind = match Path::new(&name).extension().and_then(|s| s.to_str()) {
            Some(ext) if ext.eq_ignore_ascii_case("txt") => "txt",
            Some(ext) if ext.eq_ignore_ascii_case("md") => "md",
            _ => continue,
        };
        if !entry.file_type().map_err(|_| io_error())?.is_file() {
            continue;
        }
        if files.len() == FILE_COUNT_LIMIT {
            return Err(fail(
                "limit_exceeded",
                "目录文件过多，请选择包含作品资料的子目录",
            ));
        }
        let bytes = entry.metadata().map_err(|_| io_error())?.len();
        files.push(ProjectImportFile {
            name,
            bytes,
            kind,
            selectable: bytes <= file_limit(kind) as u64,
        });
    }
    files.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(files)
}

fn only_matching(
    files: &[ProjectImportFile],
    kind: &str,
    matches: impl Fn(&str) -> bool,
) -> Option<String> {
    let mut candidates = files
        .iter()
        .filter(|file| file.kind == kind && file.selectable && matches(&file.name));
    let first = candidates.next()?;
    candidates.next().is_none().then(|| first.name.clone())
}

fn suggest_roles(files: &[ProjectImportFile], folder: &str) -> ProjectImportRoles {
    ProjectImportRoles {
        manuscript: only_matching(files, "txt", |name| {
            !folder.is_empty() && name.contains(folder)
        })
        .or_else(|| only_matching(files, "txt", |_| true)),
        world: only_matching(files, "md", |name| {
            name.contains("核心设定") || name.contains("卷纲")
        }),
        outlines: only_matching(files, "md", |name| {
            name.contains("分章") || name.contains("章纲")
        }),
        style: only_matching(files, "md", |name| {
            name.contains("创作风格") || name.contains("写作方法")
        }),
    }
}

impl ProjectImportSession {
    fn read(&self, name: &str, kind: &str) -> WorkflowResult<String> {
        let file = self
            .selection
            .files
            .iter()
            .find(|file| file.name == name && file.kind == kind)
            .ok_or_else(|| fail("invalid_input", "请从所选目录的文件列表选择对应资料"))?;
        let limit = file_limit(kind);
        if !file.selectable {
            return Err(fail("limit_exceeded", &format!("文件超过大小限制：{name}")));
        }
        let path = self.folder.join(name);
        let resolved = fs::canonicalize(&path).map_err(|_| io_error())?;
        if resolved.parent() != Some(self.folder.as_path())
            || !fs::symlink_metadata(&path)
                .map_err(|_| io_error())?
                .is_file()
        {
            return Err(io_error());
        }
        let file = File::open(resolved).map_err(|_| io_error())?;
        let metadata = file.metadata().map_err(|_| io_error())?;
        if !metadata.is_file() || metadata.len() > limit as u64 {
            return Err(fail("limit_exceeded", &format!("文件超过大小限制：{name}")));
        }
        let mut data = Vec::with_capacity(metadata.len() as usize);
        file.take((limit + 1) as u64)
            .read_to_end(&mut data)
            .map_err(|_| io_error())?;
        if data.len() > limit {
            return Err(fail("limit_exceeded", &format!("文件超过大小限制：{name}")));
        }
        let format = if kind == "txt" { "TXT" } else { "Markdown" };
        decode_import_text(&data, format)
            .map(str::to_owned)
            .map_err(|mut error| {
                error.message = format!("{name}：{}", error.message);
                error
            })
    }
}

fn numbering_issues(volumes: &[ImportVolume]) -> Vec<NumberingIssue> {
    let mut issues = Vec::new();
    for (volume, v) in volumes.iter().enumerate() {
        let mut numbers = BTreeMap::new();
        for number in v
            .chapters
            .iter()
            .filter_map(|chapter| chapter.source.as_ref()?.number)
        {
            *numbers.entry(number).or_insert(0) += 1;
        }
        let mut previous: Option<u32> = None;
        for (number, count) in numbers {
            if let Some(prev) = previous.filter(|prev| number - prev > 1) {
                issues.push(NumberingIssue {
                    volume,
                    first: prev + 1,
                    last: number - 1,
                    kind: "missing",
                });
            }
            if count > 1 {
                issues.push(NumberingIssue {
                    volume,
                    first: number,
                    last: number,
                    kind: "duplicate",
                });
            }
            previous = Some(number);
        }
    }
    issues
}

fn volume_index(volumes: &[ImportVolume], number: Option<u32>) -> Option<usize> {
    if let Some(number) = number {
        let mut matching = volumes.iter().enumerate().filter(|(_, volume)| {
            volume.source.as_ref().and_then(|source| source.number) == Some(number)
        });
        if let Some((i, _)) = matching.next() {
            return matching.next().is_none().then_some(i);
        }
        if volumes.len() == 1 && volumes[0].source.is_none() && number == 1 {
            return Some(0);
        }
        None
    } else {
        (volumes.len() == 1).then_some(0)
    }
}

fn association(
    volumes: &[ImportVolume],
    volume_index: Option<usize>,
    row: &text::Outline,
) -> ProjectAssociation {
    let mut result = ProjectAssociation {
        number: row.number,
        volume_index,
        status: "ambiguous",
        chapter_index: None,
        volume_number: None,
    };
    if let Some(volume) = volume_index {
        let mut candidates = volumes[volume].chapters.iter().filter(|chapter| {
            chapter.source.as_ref().and_then(|source| source.number) == Some(row.number)
        });
        if let Some(chapter) = candidates.next() {
            if candidates.next().is_none() {
                result.chapter_index = Some(chapter.index);
                result.status = if text::normalized_title(&chapter.title)
                    == text::normalized_title(&row.title)
                {
                    "same_title"
                } else {
                    "different_title"
                };
            }
        } else {
            result.status = "missing_chapter";
        }
    }
    result
}

fn original_card(
    file: &str,
    material_kind: &'static str,
    field: &'static str,
    content: String,
) -> PendingCard {
    let last_line = txt_import::lines(&content).count();
    PendingCard {
        preview: ProjectImportCard {
            title: format!("原文 · {file}"),
            kind: "original",
            file: file.into(),
            first_line: 1,
            last_line,
            association: None,
            volume_index: None,
        },
        material_kind,
        field,
        content,
        source_files: None,
    }
}

fn outline_card(
    file: &str,
    row: text::Outline,
    volume: Option<usize>,
    association: Option<ProjectAssociation>,
) -> PendingCard {
    let (kind, unit) = if association.is_some() {
        ("chapter_outline", '章')
    } else {
        ("volume_outline", '卷')
    };
    let LineRange { first, last } = row.lines;
    let location = if first == last {
        format!("第 {first} 行")
    } else {
        format!("第 {first}–{last} 行")
    };
    PendingCard {
        preview: ProjectImportCard {
            title: format!("第{}{unit} {}", row.number, row.title),
            kind,
            file: file.into(),
            first_line: first,
            last_line: last,
            association,
            volume_index: volume,
        },
        material_kind: "outline",
        field: "content",
        content: format!("来源：{file} · {location}\n\n{}", row.content),
        source_files: None,
    }
}

fn prepare(
    session: &ProjectImportSession,
    options: ProjectImportOptions,
) -> WorkflowResult<ProjectCandidate> {
    if options.automatic {
        return organize::prepare(session, options);
    }
    let ProjectImportOptions {
        roles,
        title,
        author,
        omit_separators,
        ..
    } = options;
    let ProjectImportRoles {
        manuscript,
        world,
        outlines,
        style,
    } = roles;
    let source_name = manuscript.ok_or_else(|| fail("invalid_input", "请选择作品正文 TXT"))?;
    let mut assigned = HashSet::new();
    if [&world, &outlines, &style]
        .into_iter()
        .flatten()
        .any(|name| !assigned.insert(name))
    {
        return Err(fail("invalid_input", "一份 Markdown 只能选择一种资料用途"));
    }
    let source = session.read(&source_name, "txt")?;
    let text::Manuscript {
        title: found_title,
        volumes,
        separator_lines,
    } = text::manuscript(&source, omit_separators)?;
    let title = title.or(found_title).unwrap_or_else(|| {
        Path::new(&source_name)
            .file_stem()
            .and_then(|v| v.to_str())
            .unwrap_or("导入作品")
            .into()
    });
    txt_import::validate_import_metadata(&title, &author)?;
    let (summaries, total_chapters, _) = txt_import::summarize_volumes(&volumes);
    let mut warnings = Vec::new();
    let mut cards = Vec::new();
    if let Some(file) = world {
        let content = session.read(&file, "md")?;
        let rows = text::volume_outlines(&content);
        if rows.is_empty() {
            warnings.push(format!("{file} 未识别到卷纲条目，原文将保存为设定卡。"));
        }
        cards.push(original_card(&file, "setting", "world", content));
        cards.extend(rows.into_iter().map(|row| {
            let target = volume_index(&summaries, Some(row.number));
            outline_card(&file, row, target, None)
        }));
    }
    if let Some(file) = outlines {
        let content = session.read(&file, "md")?;
        let target = volume_index(&summaries, text::chapter_outline_volume(&content));
        let rows = text::chapter_outlines(&content);
        if rows.is_empty() {
            warnings.push(format!("{file} 未识别到分章条目，原文将保存为大纲卡。"));
        }
        cards.push(original_card(&file, "outline", "content", content));
        let mut counts = BTreeMap::new();
        for row in &rows {
            *counts.entry(row.number).or_insert(0) += 1;
        }
        cards.extend(rows.into_iter().map(|row| {
            let mut link = association(&summaries, target, &row);
            if counts[&row.number] > 1 {
                link.status = "ambiguous";
            }
            outline_card(&file, row, target, Some(link))
        }));
    }
    if let Some(file) = style {
        let content = session.read(&file, "md")?;
        cards.push(original_card(&file, "setting", "style", content));
    }
    if cards.len() > 1000 || cards.iter().any(|card| card.content.len() > CARD_BYTES) {
        return Err(fail(
            "limit_exceeded",
            "导入资料超出每书1000卡或单卡256KiB的限制",
        ));
    }
    let preview = ProjectImportPreview {
        id: uid(),
        book_id: uid(),
        source_name,
        title,
        author,
        expires_at: session.selection.expires_at,
        numbering: numbering_issues(&summaries),
        volumes: summaries,
        total_chapters,
        separator_lines,
        omitted_separators: omit_separators,
        cards: cards.iter().map(|card| card.preview.clone()).collect(),
        warnings,
    };
    Ok(ProjectCandidate {
        preview,
        volumes,
        cards,
    })
}

impl ProjectCandidate {
    fn chapter(&self, index: u32) -> Option<(usize, &Chapter)> {
        self.volumes
            .iter()
            .enumerate()
            .flat_map(|(i, volume)| volume.chapters.iter().map(move |chapter| (i, chapter)))
            .nth(index as usize)
    }

    fn validate_links(&self, links: Vec<ProjectLink>) -> WorkflowResult<BTreeMap<usize, u32>> {
        let invalid = || fail("invalid_input", "章纲关联须使用本次预览中的同卷章节");
        if links.len() > self.cards.len() {
            return Err(invalid());
        }
        let mut by_card = BTreeMap::new();
        for link in links {
            let association = self
                .cards
                .get(link.card_index)
                .and_then(|card| card.preview.association.as_ref())
                .ok_or_else(invalid)?;
            let (volume, chapter) = self.chapter(link.chapter_index).ok_or_else(invalid)?;
            if association.volume_index != Some(volume)
                || chapter.source.is_none()
                || by_card
                    .insert(link.card_index, link.chapter_index)
                    .is_some()
            {
                return Err(invalid());
            }
        }
        Ok(by_card)
    }
}

impl Service {
    pub fn choose_project_import(
        &mut self,
        folder: &Path,
    ) -> WorkflowResult<ProjectImportSelection> {
        let folder = fs::canonicalize(folder).map_err(|_| io_error())?;
        let files = scan(&folder)?;
        let folder_name = folder
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("作品文件夹")
            .to_owned();
        let selection = ProjectImportSelection {
            id: uid(),
            expires_at: now()? + LIFETIME.as_secs() as i64,
            suggested_roles: suggest_roles(&files, &folder_name),
            folder_name,
            files,
        };
        self.project_import = Some(ProjectImportSession {
            folder,
            selection: selection.clone(),
            epoch: epoch(&self.conn)?,
            started: Instant::now(),
            candidate: None,
        });
        Ok(selection)
    }

    fn project_session(&self) -> WorkflowResult<&ProjectImportSession> {
        let session = self.project_import.as_ref().ok_or_else(not_found)?;
        if session.started.elapsed() >= LIFETIME {
            return Err(fail("expired", "导入预览已过期，请重新选择作品文件夹"));
        }
        if session.epoch != epoch(&self.conn)? {
            return Err(fail(
                "revision_conflict",
                "书库已恢复，请重新选择作品文件夹",
            ));
        }
        Ok(session)
    }

    fn project_candidate(&self, id: &str) -> WorkflowResult<&ProjectCandidate> {
        self.project_session()?
            .candidate
            .as_ref()
            .filter(|candidate| candidate.preview.id == id)
            .ok_or_else(not_found)
    }

    pub fn prepare_project_import(
        &mut self,
        selection_id: &str,
        options: ProjectImportOptions,
    ) -> WorkflowResult<ProjectImportPreview> {
        if self.project_session()?.selection.id != selection_id {
            return Err(not_found());
        }
        let session = self.project_import.as_mut().unwrap();
        session.candidate = None;
        let candidate = prepare(session, options)?;
        let preview = candidate.preview.clone();
        session.candidate = Some(candidate);
        Ok(preview)
    }

    pub fn get_project_import_detail(
        &self,
        id: &str,
        kind: &str,
        index: u32,
    ) -> WorkflowResult<ImportDetail> {
        let candidate = self.project_candidate(id)?;
        let body = match kind {
            "chapter" => candidate.chapter(index).map(|(_, chapter)| &chapter.body),
            "card" => candidate
                .cards
                .get(index as usize)
                .map(|card| &card.content),
            _ => None,
        }
        .ok_or_else(|| fail("not_found", "预览条目不存在"))?;
        Ok(ImportDetail {
            index,
            body: body.clone(),
        })
    }

    pub fn commit_project_import(
        &mut self,
        id: &str,
        links: Vec<ProjectLink>,
    ) -> WorkflowResult<ProjectImportOutcome> {
        self.commit_reviewed_project_import(id, links, Vec::new())
    }

    pub fn commit_reviewed_project_import(
        &mut self,
        id: &str,
        links: Vec<ProjectLink>,
        excluded: Vec<usize>,
    ) -> WorkflowResult<ProjectImportOutcome> {
        self.project_candidate(id)?;
        let session = self.project_import.take().unwrap();
        let candidate = session.candidate.unwrap();
        let links = candidate.validate_links(links)?;
        let excluded: HashSet<usize> = excluded.into_iter().collect();
        if excluded.iter().any(|i| {
            links.contains_key(i)
                || candidate
                    .cards
                    .get(*i)
                    .is_none_or(|card| card.preview.kind == "original")
        }) {
            return Err(fail("invalid_input", "排除项须为本次预览中的整理资料"));
        }
        let p = candidate.preview;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        if session.epoch != epoch(&tx)? {
            return Err(fail(
                "revision_conflict",
                "书库已恢复，请重新选择作品文件夹",
            ));
        }
        let ids = txt_import::insert_import_tree(
            &tx,
            &p.book_id,
            &p.title,
            &p.author,
            &candidate.volumes,
        )?;
        for (i, card) in candidate.cards.into_iter().enumerate() {
            if excluded.contains(&i) {
                continue;
            }
            let fields = organize::material_fields(&card);
            let chapter_id = links
                .get(&i)
                .map(|index| ids.chapters[*index as usize].clone());
            let volume_id = card
                .preview
                .volume_index
                .filter(|_| chapter_id.is_none())
                .map(|index| ids.volumes[index].clone());
            let title = match (&card.preview.association, &chapter_id) {
                (Some(link), None) if link.status == "missing_chapter" => {
                    format!("［待补稿］{}", card.preview.title)
                }
                (Some(_), None) => format!("［待核对］{}", card.preview.title),
                _ => card.preview.title,
            };
            Service::create_material_in_transaction(
                &tx,
                CreateMaterialInput {
                    book_id: p.book_id.clone(),
                    kind: card.material_kind.into(),
                    title,
                    fields,
                    volume_id,
                    chapter_id,
                    source_chapter_id: None,
                    source_revision: None,
                    story_order: 0,
                    story_time: None,
                    tracking_status: None,
                    payoff_chapter_id: None,
                    payoff_revision: None,
                },
            )?;
        }
        tx.commit().map_err(db)?;
        Ok(ProjectImportOutcome {
            book_id: p.book_id,
            volumes: p.volumes.len(),
            chapters: p.total_chapters,
            cards: p.cards.len() - excluded.len(),
        })
    }

    pub fn discard_project_import(&mut self, selection_id: &str) -> WorkflowResult<()> {
        if self
            .project_import
            .as_ref()
            .is_some_and(|session| session.selection.id == selection_id)
        {
            self.project_import = None;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expired_preview_stops_before_writing() {
        let mut service = Service::open(":memory:").unwrap();
        service.project_import = Some(ProjectImportSession {
            folder: PathBuf::new(),
            selection: ProjectImportSelection {
                id: "selection".into(),
                folder_name: "作品".into(),
                expires_at: now().unwrap() - 1,
                files: vec![],
                suggested_roles: ProjectImportRoles::default(),
            },
            epoch: epoch(&service.conn).unwrap(),
            started: Instant::now() - LIFETIME,
            candidate: None,
        });
        let failure = service
            .commit_project_import("expired", vec![])
            .unwrap_err();
        assert_eq!(failure.code, "expired");
        assert!(service.load().unwrap().books.is_empty());
    }
}
