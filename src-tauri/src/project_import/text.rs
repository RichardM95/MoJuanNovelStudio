use super::LineRange;
use crate::txt_import::{
    fail, heading_parts, heading_title, heading_title_checked, lines, push_chapter, push_volume,
    special_heading, Volume, CHAPTER_COUNT_LIMIT, CHAPTER_LIMIT, VOLUME_COUNT_LIMIT,
};
use crate::{ImportSourceHeading, WorkflowResult};

fn ascii_width(c: char) -> char {
    if ('！'..='～').contains(&c) {
        char::from_u32(c as u32 - 0xfee0).unwrap()
    } else {
        c
    }
}

fn digit(c: char) -> Option<u32> {
    ascii_width(c).to_digit(10).or_else(|| {
        "零一二三四五六七八九"
            .chars()
            .position(|n| n == c)
            .map(|n| n as u32)
            .or(match c {
                '〇' => Some(0),
                '两' => Some(2),
                _ => None,
            })
    })
}

fn numeral(raw: &str) -> Option<u32> {
    if raw.chars().all(|c| digit(c).is_some()) {
        return raw
            .chars()
            .try_fold(0u32, |n, c| n.checked_mul(10)?.checked_add(digit(c)?));
    }
    if let Some((upper, lower)) = raw.split_once('万') {
        if lower.contains('万') {
            return None;
        }
        return numeral(upper)?
            .checked_mul(10000)?
            .checked_add(numeral(lower)?);
    }
    let mut value = 0;
    let mut pending = None;
    let mut previous_unit = 10_000;
    for c in raw.chars() {
        if let Some(n) = digit(c) {
            if pending.unwrap_or(0) != 0 {
                return None;
            }
            pending = Some(n);
            continue;
        }
        let unit = match c {
            '十' => 10,
            '百' => 100,
            '千' => 1000,
            _ => return None,
        };
        if unit >= previous_unit {
            return None;
        }
        previous_unit = unit;
        value += pending.take().unwrap_or(1) * unit;
    }
    Some(value + pending.unwrap_or(0))
}

pub(super) fn ordinal(raw: &str) -> Option<u32> {
    numeral(raw).filter(|n| *n > 0)
}

pub(super) fn normalized_title(title: &str) -> String {
    title
        .chars()
        .map(ascii_width)
        .filter(|c| {
            !c.is_whitespace()
                && !c.is_ascii_punctuation()
                && !"、。，；！？‘’“”《》〈〉「」『』【】〔〕—–…·・".contains(*c)
        })
        .flat_map(char::to_lowercase)
        .collect()
}

fn separator(line: &str) -> bool {
    let line = line.trim();
    line.len() >= 10 && line.bytes().all(|b| b == b'-')
}

struct Heading {
    volume: bool,
    title: String,
    source: ImportSourceHeading,
}

fn manuscript_heading(line: &str, after_separator: bool, number: usize) -> Option<Heading> {
    if line.chars().take(201).count() > 200 {
        return None;
    }
    let line = markdown_heading(line).map_or(line, |(_, title)| title);
    let (prefix, prefixed) = match line.trim_start().strip_prefix("--") {
        Some(rest) if after_separator => (rest, true),
        _ => (line, false),
    };
    if !prefixed {
        if let Some(title) = special_heading(prefix) {
            return Some(Heading {
                volume: false,
                title: title.into(),
                source: ImportSourceHeading {
                    line: number,
                    number: None,
                    unit: None,
                },
            });
        }
    }
    let (volume, unit, raw_number, suffix) = heading_parts(prefix)?;
    if prefixed && volume {
        return None;
    }
    Some(Heading {
        volume,
        title: heading_title_checked(volume, suffix)?,
        source: ImportSourceHeading {
            line: number,
            number: ordinal(raw_number),
            unit: Some(unit),
        },
    })
}

fn front_title(text: &str) -> Option<(usize, String)> {
    let mut source = lines(text).enumerate();
    let (line_number, (first, _)) =
        source.find(|(_, (line, _))| !line.trim().is_empty() && !separator(line))?;
    let title = first.trim().strip_prefix('《')?.strip_suffix('》')?;
    if title.trim().is_empty() || title.contains(['《', '》']) || title.len() > 4096 {
        return None;
    }
    let mut after_separator = false;
    for (number, (line, _)) in source {
        if separator(line) {
            after_separator = true;
        } else if !line.trim().is_empty() {
            manuscript_heading(line, after_separator, number + 1)?;
            return Some((line_number + 1, title.into()));
        }
    }
    None
}

pub(super) struct Manuscript {
    pub(super) title: Option<String>,
    pub(super) volumes: Vec<Volume>,
    pub(super) separator_lines: Vec<LineRange>,
}

pub(super) fn manuscript(text: &str, omit_separators: bool) -> WorkflowResult<Manuscript> {
    manuscript_excluding(text, omit_separators, &[])
}

pub(super) fn manuscript_excluding(
    text: &str,
    omit_separators: bool,
    excluded: &[LineRange],
) -> WorkflowResult<Manuscript> {
    let front = front_title(text);
    let title_line = front.as_ref().map(|(number, _)| *number);
    let mut headings = Vec::new();
    let mut separator_lines = Vec::new();
    let mut pending: Option<LineRange> = None;
    for (index, (line, _)) in lines(text).enumerate() {
        let number = index + 1;
        if excluded_line(excluded, number) {
            pending = None;
            continue;
        }
        if Some(number) == title_line {
            separator_lines.extend(pending.take());
        } else if separator(line) {
            pending
                .get_or_insert(LineRange {
                    first: number,
                    last: number,
                })
                .last = number;
        } else if !line.trim().is_empty() {
            if let Some(heading) = manuscript_heading(line, pending.is_some(), number) {
                if headings.len() == VOLUME_COUNT_LIMIT + CHAPTER_COUNT_LIMIT {
                    return Err(fail("limit_exceeded", "卷章标题超过导入上限"));
                }
                headings.push(heading);
                separator_lines.extend(pending.take());
            } else {
                pending = None;
            }
        }
    }
    let has_headings = !headings.is_empty();
    if has_headings {
        separator_lines.extend(pending);
    }

    let mut volumes = Vec::new();
    let mut count = 0;
    let mut leading = String::new();
    let mut headings = headings.into_iter().peekable();
    let mut separators = separator_lines.iter().peekable();
    for (index, (line, raw)) in lines(text).enumerate() {
        let number = index + 1;
        if excluded_line(excluded, number) {
            continue;
        }
        if Some(number) == title_line {
            continue;
        }
        if headings.peek().is_some_and(|h| h.source.line == number) {
            let heading = headings.next().unwrap();
            leading.clear();
            if heading.volume {
                push_volume(&mut volumes, heading.title)?;
                volumes.last_mut().unwrap().source = Some(heading.source);
            } else {
                push_chapter(&mut volumes, &mut count, heading.title)?;
                volumes
                    .last_mut()
                    .unwrap()
                    .chapters
                    .last_mut()
                    .unwrap()
                    .source = Some(heading.source);
            }
            continue;
        }
        while separators.peek().is_some_and(|range| range.last < number) {
            separators.next();
        }
        if omit_separators
            && separators.peek().is_some_and(|range| range.first <= number)
            && separator(line)
        {
            continue;
        }
        let needs_chapter = volumes.last().is_none_or(|v| v.chapters.is_empty());
        if needs_chapter {
            if line.trim().is_empty() {
                leading.push_str(raw);
                continue;
            }
            let label = if has_headings { "前言" } else { "正文" };
            push_chapter(&mut volumes, &mut count, label.into())?;
        }
        let chapter = volumes.last_mut().unwrap().chapters.last_mut().unwrap();
        if chapter.body.len() + leading.len() + raw.len() > CHAPTER_LIMIT {
            return Err(fail(
                "limit_exceeded",
                "单章超过10MiB，请检查分章标题；未创建作品",
            ));
        }
        chapter.body.push_str(&leading);
        leading.clear();
        chapter.body.push_str(raw);
    }
    if count == 0 {
        push_chapter(&mut volumes, &mut count, "正文".into())?;
    }
    Ok(Manuscript {
        title: front.map(|(_, title)| title),
        volumes,
        separator_lines,
    })
}

fn excluded_line(ranges: &[LineRange], number: usize) -> bool {
    let index = ranges.partition_point(|range| range.last < number);
    ranges.get(index).is_some_and(|range| range.first <= number)
}

pub(super) fn markdown_heading(line: &str) -> Option<(usize, &str)> {
    let line = line.trim();
    let level = line.bytes().take_while(|c| *c == b'#').count();
    if !(1..=6).contains(&level) || !line[level..].starts_with(char::is_whitespace) {
        return None;
    }
    Some((level, line[level..].trim()))
}

pub(super) struct Outline {
    pub(super) number: u32,
    pub(super) title: String,
    pub(super) content: String,
    pub(super) lines: LineRange,
}

pub(super) fn volume_outlines(text: &str) -> Vec<Outline> {
    let mut outlines = Vec::new();
    let mut active: Option<Outline> = None;
    for (index, (line, raw)) in lines(text).enumerate() {
        if let Some((level, heading)) = markdown_heading(line).filter(|(level, _)| *level <= 3) {
            outlines.extend(active.take());
            if level == 3 {
                if let Some((true, _, raw_number, suffix)) = heading_parts(heading) {
                    if let Some(number) = ordinal(raw_number) {
                        active = Some(Outline {
                            number,
                            title: heading_title(true, suffix),
                            content: String::new(),
                            lines: LineRange {
                                first: index + 1,
                                last: index + 1,
                            },
                        });
                    }
                }
            }
        } else if let Some(outline) = active.as_mut() {
            outline.content.push_str(raw);
            outline.lines.last = index + 1;
        }
    }
    outlines.extend(active);
    outlines
}

pub(super) fn chapter_outline_volume(text: &str) -> Option<u32> {
    let (_, title) = lines(text)
        .filter_map(|(line, _)| markdown_heading(line))
        .find(|(level, _)| *level == 1)?;
    title.match_indices('第').find_map(|(start, _)| {
        let (volume, _, raw_number, _) = heading_parts(&title[start..])?;
        (volume).then(|| ordinal(raw_number)).flatten()
    })
}

pub(super) fn chapter_outlines(text: &str) -> Vec<Outline> {
    lines(text)
        .enumerate()
        .filter_map(|(index, (line, _))| {
            let item = line.trim().strip_prefix(['*', '-', '+'])?.trim_start();
            let (heading, tail) = item.strip_prefix("**")?.split_once("**")?;
            let (false, _, number, suffix) = heading_parts(heading)? else {
                return None;
            };
            let title = suffix.strip_prefix(['：', ':'])?.trim();
            let content = tail
                .trim_start()
                .strip_prefix(['—', '–', '-'])?
                .trim_start();
            Some(Outline {
                number: ordinal(number)?,
                title: title.into(),
                content: content.into(),
                lines: LineRange {
                    first: index + 1,
                    last: index + 1,
                },
            })
        })
        .collect()
}
