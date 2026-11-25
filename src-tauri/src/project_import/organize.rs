use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Setting(&'static str),
    Character,
    Volume,
    Chapter,
}

struct Note {
    kind: Kind,
    title: String,
    text: String,
    lines: LineRange,
    volume: Option<u32>,
    number: Option<u32>,
}

fn label(line: &str) -> &str {
    let line = line.trim_start_matches(|c: char| matches!(c, '>' | '-' | '*' | ' ' | '\t'));
    line.trim().trim_matches('*')
}

fn named_field(line: &str) -> Option<(&str, &str)> {
    let (key, value) = label(line).split_once(['：', ':'])?;
    Some((key.trim_matches('*').trim(), value.trim().trim_matches('*')))
}

fn name(value: &str) -> Option<&str> {
    let value = value
        .split(['（', '(', '，', '、', '；', '。'])
        .next()?
        .trim();
    (!value.is_empty() && value.chars().count() <= 45).then_some(value)
}

fn character_label(label: &str) -> Option<&str> {
    let (name, role) = label.split_once(['（', '('])?;
    let role = role.trim_end_matches(['）', ')']);
    let name = name.trim();
    let known = [
        "主角",
        "配角",
        "弟弟",
        "哥哥",
        "姐姐",
        "妹妹",
        "父亲",
        "母亲",
        "辅助智脑",
        "男主",
        "女主",
    ];
    (known.contains(&role)
        && !name.is_empty()
        && name.chars().count() <= 16
        && !name.chars().any(char::is_whitespace))
    .then_some(name)
}

fn structural(line: &str) -> Option<(bool, char, u32, &str)> {
    let (volume, unit, number, tail) = txt_import::heading_parts(line)?;
    Some((
        volume,
        unit,
        text::ordinal(number)?,
        tail.trim_start().trim_start_matches(['：', ':']).trim(),
    ))
}

fn volume_in(line: &str) -> Option<u32> {
    line.match_indices('第')
        .find_map(|(i, _)| match structural(&line[i..]) {
            Some((true, _, n, _)) => Some(n),
            _ => None,
        })
}

fn outline_line(line: &str) -> Option<(u32, &str, &str)> {
    let (head, rest) = label(line)
        .split_once(" — ")
        .or_else(|| label(line).split_once(" -- "))?;
    let (false, _, number, title) = structural(head.trim_matches('*'))? else {
        return None;
    };
    Some((number, title, rest.trim()))
}

fn topic(heading: &str) -> Option<Kind> {
    if heading.contains("画像")
        || heading.contains("角色档案")
        || heading.starts_with("人物")
        || heading.starts_with("角色")
    {
        return Some(Kind::Character);
    }
    let field = if heading.contains("风格") || heading.contains("写作方法") {
        "style"
    } else if heading.contains("题材") {
        "genre"
    } else if heading.contains("简介")
        || heading.contains("核心定位")
        || heading.contains("创作基调")
    {
        "synopsis"
    } else if ["规则", "体系", "锚点", "数值", "货币", "生态"]
        .iter()
        .any(|word| heading.contains(word))
    {
        "rules"
    } else if ["世界", "背景", "设定", "势力"]
        .iter()
        .any(|word| heading.contains(word))
    {
        "world"
    } else {
        return None;
    };
    Some(Kind::Setting(field))
}

fn add_note(notes: &mut Vec<Note>, mut note: Note) {
    if note.kind == Kind::Character {
        if let Some(found) = txt_import::lines(&note.text).find_map(|(line, _)| {
            let (key, value) = named_field(line)?;
            matches!(key, "姓名" | "主角" | "角色" | "人物")
                .then(|| name(value))
                .flatten()
        }) {
            note.title = found.into();
        }
    }
    if !note.text.trim().is_empty() && !note.title.trim().is_empty() {
        notes.push(note);
    }
}

fn style_reference_title(title: &str) -> bool {
    ["创作风格", "写作方法", "风格参考"]
        .iter()
        .any(|term| title.contains(term))
        && !title.contains("设定")
        && !title.contains("大纲")
}

fn extract(source: &str) -> (Vec<Note>, Vec<LineRange>) {
    if let Some((n, (_, title))) = txt_import::lines(source)
        .enumerate()
        .find_map(|(i, (line, _))| {
            text::markdown_heading(line)
                .filter(|(level, _)| *level == 1)
                .map(|heading| (i, heading))
        })
        .filter(|(start, (_, title))| {
            style_reference_title(title)
                && !txt_import::lines(source)
                    .enumerate()
                    .any(|(index, (line, _))| {
                        let heading = text::markdown_heading(line);
                        let plain = heading.map_or_else(|| label(line), |(_, plain)| plain);
                        (index != *start && heading.is_some_and(|(level, _)| level == 1))
                            || structural(plain).is_some()
                            || outline_line(line).is_some()
                    })
        })
    {
        let range = LineRange {
            first: n + 1,
            last: txt_import::lines(source).count(),
        };
        return (
            vec![Note {
                kind: Kind::Setting("style"),
                title: title.into(),
                text: source.into(),
                lines: range,
                volume: None,
                number: None,
            }],
            vec![range],
        );
    }
    let mut notes = Vec::new();
    let mut ignored = Vec::new();
    let mut active: Option<Note> = None;
    let mut planning_level: Option<usize> = None;
    let mut volume = None;
    let mut fence = None;
    for (i, (line, raw)) in txt_import::lines(source).enumerate() {
        let line_no = i + 1;
        if let Some(mark) = line
            .trim_start()
            .chars()
            .next()
            .filter(|c| matches!(c, '\x60' | '~'))
        {
            if line.trim_start().chars().take(3).all(|c| c == mark) && line.trim().len() >= 3 {
                if fence == Some(mark) {
                    fence = None;
                } else if fence.is_none() {
                    fence = Some(mark);
                }
                if let Some(note) = active.as_mut() {
                    note.text.push_str(raw);
                    note.lines.last = line_no;
                }
                continue;
            }
        }
        if fence.is_some() {
            if let Some(note) = active.as_mut() {
                note.text.push_str(raw);
                note.lines.last = line_no;
            }
            continue;
        }
        if let Some((key, value)) = named_field(line) {
            if let Some(character) = character_label(key).filter(|_| !value.trim().is_empty()) {
                let range = LineRange {
                    first: line_no,
                    last: line_no,
                };
                notes.push(Note {
                    kind: Kind::Character,
                    title: character.into(),
                    text: value.into(),
                    lines: range,
                    volume: None,
                    number: None,
                });
                ignored.push(range);
                continue;
            }
        }
        if let Some((number, title, body)) = outline_line(line) {
            if let Some(note) = active.take() {
                add_note(&mut notes, note);
            }
            let range = LineRange {
                first: line_no,
                last: line_no,
            };
            notes.push(Note {
                kind: Kind::Chapter,
                title: title.into(),
                text: body.into(),
                lines: range,
                volume,
                number: Some(number),
            });
            ignored.push(range);
            continue;
        }
        let heading = text::markdown_heading(line);
        let headline = heading.map_or_else(|| label(line), |(_, body)| body);
        let short = headline.chars().count() <= 180;
        let key_value = named_field(headline).filter(|(key, _)| {
            matches!(
                *key,
                "人物"
                    | "角色"
                    | "主角"
                    | "世界观"
                    | "世界背景"
                    | "规则"
                    | "设定"
                    | "简介"
                    | "风格"
                    | "题材"
            )
        });
        let numbered = structural(headline);
        let section = heading.is_some() || key_value.is_some() || (short && numbered.is_some());
        if section {
            if let Some(note) = active.take() {
                add_note(&mut notes, note);
            }
            let level = heading.map_or(9, |(level, _)| level);
            let heading_plan = short
                && (headline.contains("大纲")
                    || headline.contains("细纲")
                    || headline.contains("章纲"))
                && !headline.contains("核心设定");
            if let Some((_, _)) = heading {
                if planning_level.is_some_and(|old| level <= old) {
                    planning_level = None;
                }
            }
            if heading_plan {
                planning_level = Some(level);
                volume = volume_in(headline).or(volume);
                ignored.push(LineRange {
                    first: line_no,
                    last: line_no,
                });
                continue;
            }
            if ["正文", "正文稿", "小说正文"].contains(&headline) {
                planning_level = None;
                ignored.push(LineRange {
                    first: line_no,
                    last: line_no,
                });
                continue;
            }
            if let Some((is_volume, _, n, title)) = numbered {
                if is_volume {
                    volume = Some(n);
                }
                if planning_level.is_some() {
                    let kind = if is_volume {
                        Kind::Volume
                    } else {
                        Kind::Chapter
                    };
                    active = Some(Note {
                        kind,
                        title: title.into(),
                        text: String::new(),
                        lines: LineRange {
                            first: line_no,
                            last: line_no,
                        },
                        volume,
                        number: Some(n),
                    });
                }
                continue;
            }
            let explicit =
                key_value.and_then(|(key, value)| topic(key).map(|kind| (key, value, kind)));
            let heading_topic = heading.and_then(|(level, head)| {
                ((level > 1 || !head.contains('《') || style_reference_title(head))
                    && !head.contains("大纲"))
                .then(|| topic(head))
                .flatten()
                .map(|kind| (head, "", kind))
            });
            if let Some((title, value, kind)) = explicit.or(heading_topic) {
                let (title, body) = if kind == Kind::Character && !value.is_empty() {
                    (name(value).unwrap_or(title), value)
                } else {
                    (title, value)
                };
                active = Some(Note {
                    kind,
                    title: title.into(),
                    text: if body.is_empty() {
                        String::new()
                    } else {
                        format!("{body}\n")
                    },
                    lines: LineRange {
                        first: line_no,
                        last: line_no,
                    },
                    volume: None,
                    number: None,
                });
                continue;
            }
        }
        if let Some(note) = active.as_mut() {
            note.text.push_str(raw);
            note.lines.last = line_no;
        }
    }
    if let Some(note) = active {
        add_note(&mut notes, note);
    }
    ignored.extend(notes.iter().map(|note| note.lines));
    ignored.sort_by_key(|range| range.first);
    let mut ranges: Vec<LineRange> = Vec::new();
    for range in ignored {
        if let Some(last) = ranges
            .last_mut()
            .filter(|last| last.last + 1 >= range.first)
        {
            last.last = last.last.max(range.last);
        } else {
            ranges.push(range);
        }
    }
    (notes, ranges)
}

fn source_location(file: &str, line: LineRange) -> String {
    if line.first == line.last {
        format!("{file} · 第 {} 行", line.first)
    } else {
        format!("{file} · 第 {}–{} 行", line.first, line.last)
    }
}

fn note_card(file: &str, note: Note, volumes: &[ImportVolume]) -> PendingCard {
    let (material_kind, field, preview_kind) = match note.kind {
        Kind::Setting(field) => ("setting", field, "setting"),
        Kind::Character => ("character", "identity", "character"),
        Kind::Volume => ("outline", "content", "volume_outline"),
        Kind::Chapter => ("outline", "content", "chapter_outline"),
    };
    let scope = if matches!(note.kind, Kind::Chapter | Kind::Volume) {
        volume_index(volumes, note.volume)
    } else {
        None
    };
    let association = if note.kind == Kind::Chapter {
        note.number.map(|number| {
            let mut result = association(
                volumes,
                scope,
                &text::Outline {
                    number,
                    title: note.title.clone(),
                    content: String::new(),
                    lines: note.lines,
                },
            );
            result.volume_number = note.volume;
            result
        })
    } else {
        None
    };
    let title = match (note.kind, note.number) {
        (Kind::Chapter, Some(n)) => format!("第{n}章 {}", note.title),
        (Kind::Volume, Some(n)) => format!("第{n}卷 {}", note.title),
        _ => note.title,
    };
    PendingCard {
        preview: ProjectImportCard {
            title,
            kind: preview_kind,
            file: file.into(),
            first_line: note.lines.first,
            last_line: note.lines.last,
            association,
            volume_index: scope,
        },
        material_kind,
        field,
        content: note.text,
        source_files: Some(source_location(file, note.lines)),
    }
}

fn documents(file: &str, content: &str, unclassified: bool) -> Vec<PendingCard> {
    let mut result = Vec::new();
    let mut text = content;
    let mut start = 1;
    let mut part = 0;
    while !text.is_empty() {
        let mut end = text.floor_char_boundary(text.len().min(CARD_BYTES - 4096));
        if end < text.len() {
            if let Some(newline) = text[..end].rfind('\n').filter(|i| *i > 0) {
                end = newline + 1;
            }
        }
        let chunk = &text[..end];
        let count = txt_import::lines(chunk).count();
        let last = start + count.saturating_sub(1);
        part += 1;
        let mut card = original_card(file, "document", "content", chunk.into());
        card.preview.first_line = start;
        card.preview.last_line = last;
        card.preview.title = format!("{} · {file}", if unclassified { "待整理" } else { "原文" });
        if part > 1 || end < text.len() {
            card.preview.title.push_str(&format!(" · {part}"));
        }
        card.source_files = Some(source_location(file, LineRange { first: start, last }));
        result.push(card);
        start = last + usize::from(chunk.ends_with(['\n', '\r']));
        text = &text[end..];
    }
    result
}

fn mark_alternatives(cards: &mut [PendingCard]) {
    let group = |row: &ProjectAssociation| {
        (
            row.volume_index,
            row.volume_index
                .is_none()
                .then_some(row.volume_number)
                .flatten(),
            row.number,
        )
    };
    let mut count = BTreeMap::new();
    for card in cards.iter() {
        if let Some(row) = &card.preview.association {
            *count.entry(group(row)).or_insert(0usize) += 1;
        }
    }
    for card in cards {
        if let Some(row) = &mut card.preview.association {
            if count[&group(row)] > 1 {
                row.status = "ambiguous";
            }
        }
    }
}

pub(super) fn prepare(
    session: &ProjectImportSession,
    options: ProjectImportOptions,
) -> WorkflowResult<ProjectCandidate> {
    let mut sources = Vec::new();
    let mut best: Option<(usize, usize, text::Manuscript)> = None;
    let mut manuscript_count = 0;
    let mut warnings = Vec::new();
    let mut size = 0;
    for file in &session.selection.files {
        if !file.selectable {
            warnings.push(format!("{} 超出单文件大小限制。", file.name));
            continue;
        }
        size += file.bytes;
        if size > 32 * 1024 * 1024 {
            return Err(fail(
                "limit_exceeded",
                "本次自动整理的文件总量超过32MiB，请使用较小的目录",
            ));
        }
        let content = match session.read(&file.name, file.kind) {
            Ok(text) => text,
            Err(error) => {
                warnings.push(format!("{}：{}", file.name, error.message));
                continue;
            }
        };
        let (notes, exclude) = extract(&content);
        if let Ok(draft) = text::manuscript_excluding(&content, options.omit_separators, &exclude) {
            let count = draft
                .volumes
                .iter()
                .flat_map(|volume| &volume.chapters)
                .filter(|chapter| chapter.source.is_some() && !chapter.body.trim().is_empty())
                .count();
            manuscript_count += usize::from(count > 0);
            let explicit = options.manuscript_override.as_deref() == Some(file.name.as_str());
            if explicit
                || (options.manuscript_override.is_none()
                    && count > 0
                    && best.as_ref().is_none_or(|(_, score, _)| count > *score))
            {
                best = Some((sources.len(), count, draft));
            }
        }
        sources.push((file.name.clone(), content, notes));
    }
    let (index, _, draft) = best.ok_or_else(|| {
        fail(
            "invalid_input",
            "找不到带卷章标题的正文候选，可用手动选择指定正文",
        )
    })?;
    let source_name = sources[index].0.clone();
    if manuscript_count > 1 {
        warnings.push(format!("发现 {manuscript_count} 份含卷章的稿件。当前正文采用 {source_name}，其余稿件可在来源文档核对。"));
    }
    let title = options.title.or(draft.title).unwrap_or_else(|| {
        Path::new(&source_name)
            .file_stem()
            .and_then(|v| v.to_str())
            .unwrap_or("导入作品")
            .into()
    });
    txt_import::validate_import_metadata(&title, &options.author)?;
    let (volumes, total_chapters, _) = txt_import::summarize_volumes(&draft.volumes);
    let mut cards = Vec::new();
    for (i, (file, source, notes)) in sources.into_iter().enumerate() {
        if i != index || !notes.is_empty() {
            cards.extend(documents(&file, &source, notes.is_empty()));
        }
        cards.extend(
            notes
                .into_iter()
                .map(|note| note_card(&file, note, &volumes)),
        );
    }
    mark_alternatives(&mut cards);
    check_cards(&cards)?;
    let preview = ProjectImportPreview {
        id: uid(),
        book_id: uid(),
        source_name,
        title,
        author: options.author,
        expires_at: session.selection.expires_at,
        numbering: numbering_issues(&volumes),
        volumes,
        total_chapters,
        separator_lines: draft.separator_lines,
        omitted_separators: options.omit_separators,
        cards: cards.iter().map(|card| card.preview.clone()).collect(),
        warnings,
    };
    Ok(ProjectCandidate {
        preview,
        volumes: draft.volumes,
        cards,
    })
}

fn check_cards(cards: &[PendingCard]) -> WorkflowResult<()> {
    if cards.len() > 1000
        || cards.iter().any(|card| {
            card.content.len() + card.source_files.as_ref().map_or(0, String::len) > CARD_BYTES
        })
    {
        return Err(fail("limit_exceeded", "整理结果超出资料卡数量或容量限制"));
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize)]
pub struct OrganizationCard {
    pub title: String,
    pub kind: &'static str,
    pub source_files: String,
    pub content: String,
    pub previous_fields: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Serialize)]
pub struct OrganizationPreview {
    pub id: String,
    pub source_count: usize,
    pub cards: Vec<OrganizationCard>,
}

pub(crate) struct OrganizationCandidate {
    id: String,
    book: String,
    epoch: String,
    started: Instant,
    sources: Vec<(String, i64)>,
    cards: Vec<PendingCard>,
    updates: BTreeMap<usize, (String, i64)>,
}

fn legacy_source_field(kind: &str, title: &str) -> Option<&'static str> {
    if kind == "document" {
        return Some("content");
    }
    if !title.starts_with("原文 · ") {
        return None;
    }
    match kind {
        "setting" => Some("world"),
        "outline" => Some("content"),
        _ => None,
    }
}

fn source_text(material: &crate::Material) -> Option<&str> {
    if material.title.starts_with("待整理 · ") && material.kind != "document" {
        return None;
    }
    if material.kind == "setting" && material.title.starts_with("原文 · ") {
        return material
            .fields
            .get("world")
            .or_else(|| material.fields.get("style"))
            .map(String::as_str);
    }
    material
        .fields
        .get(legacy_source_field(&material.kind, &material.title)?)
        .map(String::as_str)
}

fn strip_origin(content: &str) -> &str {
    if content.starts_with("来源：") {
        content.split_once("\n\n").map_or(content, |(_, text)| text)
    } else {
        content
    }
}

fn identity(title: &str) -> &str {
    title
        .strip_prefix("［待补稿］")
        .or_else(|| title.strip_prefix("［待核对］"))
        .unwrap_or(title)
}

fn present(existing: &[crate::Material], card: &PendingCard) -> bool {
    let fields = material_fields(card);
    existing.iter().any(|item| {
        item.kind == card.material_kind
            && identity(&item.title) == identity(&card.preview.title)
            && fields
                .iter()
                .filter(|(key, _)| key.as_str() != "source_files")
                .all(|(key, value)| {
                    item.fields
                        .get(key)
                        .is_some_and(|previous| strip_origin(previous).trim() == value.trim())
                })
    })
}

fn source_position(origin: &str) -> Option<(&str, usize)> {
    let (file, range) = origin.rsplit_once(" · 第 ")?;
    let first = range.split(['–', ' ']).next()?.parse::<u32>().ok()?;
    Some((file, first.saturating_sub(1) as usize))
}

fn unique_target<'a>(
    existing: &'a [crate::Material],
    card: &PendingCard,
) -> Option<&'a crate::Material> {
    let mut matches = existing.iter().filter(|item| {
        if item.kind != card.material_kind || identity(&item.title) != identity(&card.preview.title)
        {
            return false;
        }
        let origin = item
            .fields
            .get("source_files")
            .map(String::as_str)
            .or_else(|| {
                item.fields
                    .get("content")?
                    .lines()
                    .next()?
                    .strip_prefix("来源：")
            });
        origin
            .and_then(source_position)
            .is_some_and(|(file, _)| file == card.preview.file)
    });
    let item = matches.next()?;
    matches.next().is_none().then_some(item)
}

pub(super) fn material_fields(card: &PendingCard) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    if card.material_kind == "character" {
        for (line, raw) in txt_import::lines(&card.content) {
            let labeled = named_field(line).and_then(|(key, value)| {
                let kind = match key {
                    "身份" => "identity",
                    "性格" => "personality",
                    "外貌" => "appearance",
                    "目标" | "动机" => "goal",
                    "关系" => "relationships",
                    "状态" => "status",
                    "别名" | "绰号" | "代号" => "aliases",
                    _ => return None,
                };
                Some((kind, value))
            });
            let (key, value) = labeled.unwrap_or(("identity", raw));
            let slot = fields.entry(key.into()).or_insert_with(String::new);
            slot.push_str(value);
            if labeled.is_some() {
                slot.push('\n');
            }
        }
    } else {
        fields.insert(card.field.into(), card.content.clone());
    }
    if let Some(source) = &card.source_files {
        fields.insert("source_files".into(), source.clone());
    }
    fields
}

impl Service {
    pub fn preview_material_organization(
        &mut self,
        book: &str,
    ) -> WorkflowResult<OrganizationPreview> {
        self.organization = None;
        let existing = self
            .list_materials(book, false)?
            .into_iter()
            .map(|item| self.get_material(&item.id, false))
            .collect::<WorkflowResult<Vec<_>>>()?;
        let mut sources = Vec::new();
        let mut extracted = Vec::new();
        // Scope is suggested from explicit volume numbers. Existing chapters may
        // have been reordered since import, so no new chapter links are guessed.
        for material in &existing {
            let Some(content) = source_text(material) else {
                continue;
            };
            let filename = material
                .title
                .strip_prefix("原文 · ")
                .or_else(|| material.title.strip_prefix("待整理 · "))
                .unwrap_or(&material.title);
            let (filename, offset) = material
                .fields
                .get("source_files")
                .filter(|_| material.kind == "document")
                .and_then(|origin| source_position(origin))
                .unwrap_or((filename, 0));
            sources.push((material.id.clone(), material.revision));
            let (notes, _) = extract(content);
            extracted.extend(notes.into_iter().map(|mut note| {
                note.lines.first = note.lines.first.saturating_add(offset);
                note.lines.last = note.lines.last.saturating_add(offset);
                note_card(filename, note, &[])
            }));
        }
        let targets: Vec<_> = extracted
            .iter()
            .map(|card| unique_target(&existing, card))
            .collect();
        let mut target_counts = BTreeMap::new();
        for target in targets.iter().flatten() {
            *target_counts.entry(target.id.as_str()).or_insert(0usize) += 1;
        }
        let mut updates = BTreeMap::new();
        let mut preview_cards = Vec::new();
        let mut cards = Vec::new();
        for (card, target) in extracted.into_iter().zip(targets) {
            if present(&existing, &card) {
                continue;
            }
            let previous_fields = target
                .filter(|item| target_counts[item.id.as_str()] == 1)
                .map(|item| {
                    updates.insert(cards.len(), (item.id.clone(), item.revision));
                    material_fields(&card)
                        .into_iter()
                        .filter(|(key, _)| key != "source_files")
                        .map(|(key, _)| {
                            let previous = item.fields.get(&key).cloned().unwrap_or_default();
                            (key, previous)
                        })
                        .collect()
                });
            preview_cards.push(OrganizationCard {
                title: card.preview.title.clone(),
                kind: card.material_kind,
                source_files: card.source_files.clone().unwrap_or_default(),
                content: card.content.clone(),
                previous_fields,
            });
            cards.push(card);
        }
        check_cards(&cards)?;
        let view = OrganizationPreview {
            id: uid(),
            source_count: sources.len(),
            cards: preview_cards,
        };
        self.organization = Some(OrganizationCandidate {
            id: view.id.clone(),
            book: book.into(),
            epoch: self.epoch()?,
            started: Instant::now(),
            sources,
            cards,
            updates,
        });
        Ok(view)
    }

    pub fn apply_material_organization(
        &mut self,
        id: &str,
        accepted: Vec<usize>,
    ) -> WorkflowResult<usize> {
        let candidate = self
            .organization
            .as_ref()
            .filter(|c| c.id == id)
            .ok_or_else(not_found)?;
        if candidate.started.elapsed() >= LIFETIME || candidate.epoch != self.epoch()? {
            return Err(fail("revision_conflict", "资料整理预览已失效，请重新预览"));
        }
        let count = accepted.len();
        let choices: HashSet<_> = accepted.into_iter().collect();
        if count == 0
            || choices.len() != count
            || choices.iter().any(|&i| i >= candidate.cards.len())
        {
            return Err(fail("invalid_input", "请选择本次预览中的有效资料"));
        }
        let candidate = self.organization.take().unwrap();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        if epoch(&tx)? != candidate.epoch {
            return Err(fail("revision_conflict", "书库已恢复，请重新预览"));
        }
        let current = crate::materials::metadata_in(&tx, &candidate.book, false, None)?
            .into_iter()
            .map(|item| crate::materials::raw(&tx, &item.id))
            .collect::<WorkflowResult<Vec<_>>>()?;
        for (source, revision) in &candidate.sources {
            if !current
                .iter()
                .any(|item| item.id == *source && item.revision == *revision)
            {
                return Err(fail("revision_conflict", "来源资料已修改，请重新预览"));
            }
        }
        let mut applied = 0;
        for (i, card) in candidate.cards.into_iter().enumerate() {
            if !choices.contains(&i) {
                continue;
            }
            if let Some((id, revision)) = candidate.updates.get(&i) {
                let item = current
                    .iter()
                    .find(|item| item.id == *id && item.revision == *revision)
                    .ok_or_else(|| fail("revision_conflict", "待更新的资料卡已修改，请重新预览"))?;
                let mut fields = item.fields.clone();
                fields.extend(material_fields(&card));
                crate::materials::save_material_in_transaction(
                    &tx,
                    crate::SaveMaterialInput {
                        id: item.id.clone(),
                        title: item.title.clone(),
                        fields,
                        volume_id: item.volume_id.clone(),
                        chapter_id: item.chapter_id.clone(),
                        expected_revision: *revision,
                        source_chapter_id: item.source_chapter_id.clone(),
                        source_revision: item.source_revision,
                        story_order: item.story_order,
                        story_time: item.story_time.clone(),
                        tracking_status: item.tracking_status.clone(),
                        payoff_chapter_id: item.payoff_chapter_id.clone(),
                        payoff_revision: item.payoff_revision,
                    },
                )?;
            } else if !present(&current, &card) {
                let fields = material_fields(&card);
                Service::create_material_in_transaction(
                    &tx,
                    CreateMaterialInput {
                        book_id: candidate.book.clone(),
                        kind: card.material_kind.into(),
                        title: card.preview.title,
                        fields,
                        volume_id: None,
                        chapter_id: None,
                        source_chapter_id: None,
                        source_revision: None,
                        story_order: 0,
                        story_time: None,
                        tracking_status: None,
                        payoff_chapter_id: None,
                        payoff_revision: None,
                    },
                )?;
            } else {
                continue;
            }
            applied += 1;
        }
        tx.commit().map_err(db)?;
        Ok(applied)
    }
}
