use crate::{Service, WorkflowError, WorkflowResult};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};

const SCAN_BYTES: usize = 64 * 1024 * 1024;
const QUOTE_CHARS: usize = 420;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalRequest {
    pub chapter_id: String,
    pub query: String,
    pub limit: Option<u32>,
    #[serde(default)]
    pub include_references: bool,
}

#[derive(Debug, Serialize)]
pub struct Evidence {
    pub id: String,
    pub kind: &'static str,
    pub status: &'static str,
    pub title: String,
    pub revision: i64,
    pub volume_id: Option<String>,
    pub chapter_id: Option<String>,
    pub field: String,
    pub start: usize,
    pub end: usize,
    pub quote: String,
    pub source_files: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RetrievalResults {
    pub items: Vec<Evidence>,
    pub truncated: bool,
}

fn fail(code: &str, message: &str) -> WorkflowError {
    WorkflowError {
        code: code.into(),
        message: message.into(),
    }
}

fn terms(query: &str) -> Vec<String> {
    let mut terms = BTreeSet::new();
    for word in query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
    {
        terms.insert(word.to_ascii_lowercase());
        if word.chars().any(|c| !c.is_ascii()) {
            let chars: Vec<_> = word.chars().collect();
            terms.extend(chars.windows(2).map(|pair| pair.iter().collect()));
        }
    }
    terms.into_iter().collect()
}

fn match_text(text: &str, terms: &[String]) -> (usize, usize) {
    let comparable = text.to_ascii_lowercase();
    let mut score = 0;
    let mut best = (0, 0);
    for term in terms {
        if let Some(offset) = comparable.find(term) {
            let weight = term.chars().count().min(12);
            score += weight;
            if weight > best.0 {
                best = (weight, offset);
            }
        }
    }
    (score, best.1)
}

fn fill_quote(hit: &mut Evidence, text: &str, matched: usize, tail: bool) {
    let start = if tail {
        text.char_indices()
            .rev()
            .nth(QUOTE_CHARS - 1)
            .map(|(i, _)| i)
            .unwrap_or(0)
    } else {
        text[..matched]
            .char_indices()
            .rev()
            .nth(84)
            .map(|(i, _)| i)
            .unwrap_or(0)
    };
    let end = text[start..]
        .char_indices()
        .nth(QUOTE_CHARS)
        .map(|(i, _)| start + i)
        .unwrap_or(text.len());
    hit.start = start;
    hit.end = end;
    hit.quote = text[start..end].into();
}

impl Service {
    pub fn retrieve_writing_context(
        &self,
        book: &str,
        target: &str,
        query: &str,
        limit: u32,
        include_references: bool,
    ) -> WorkflowResult<RetrievalResults> {
        if query.trim().is_empty() || query.len() > 256 || !(1..=30).contains(&limit) {
            return Err(fail(
                "invalid_input",
                "检索词须为1至256字节，返回数量须为1至30",
            ));
        }
        let terms = terms(query);
        if terms.is_empty() {
            return Err(fail("invalid_input", "请提供可检索的词或人名"));
        }
        let mut statement = self.conn.prepare(
            "SELECT c.id,c.title,c.revision,v.id,v.title,length(CAST(c.body AS BLOB))
             FROM chapters c JOIN volumes v ON v.id=c.volume_id JOIN books b ON b.id=v.book_id
             WHERE b.id=?1 AND b.deleted_at IS NULL AND v.deleted_at IS NULL AND c.deleted_at IS NULL
             ORDER BY v.sort_order,v.id,c.sort_order,c.id",
        )?;
        let chapters = statement
            .query_map([book], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, usize>(5)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let position = chapters
            .iter()
            .position(|ch| ch.0 == target)
            .ok_or_else(|| fail("not_found", "目标章节不在本作品中"))?;
        let (_, _, _, target_volume, target_volume_title, _) = &chapters[position];
        let order: HashMap<_, _> = chapters
            .iter()
            .enumerate()
            .map(|(i, row)| (row.0.as_str(), i))
            .collect();
        let mut ranked = Vec::new();
        let mut truncated = false;
        let mut remaining = SCAN_BYTES;
        let mut found_prior = false;
        let mut chapter_body = self.conn.prepare("SELECT body FROM chapters WHERE id=?1")?;
        for (index, (id, title, revision, volume, _, size)) in
            chapters[..=position].iter().enumerate().rev()
        {
            if *size > remaining {
                truncated = true;
                continue;
            }
            remaining -= size;
            let body: String = chapter_body.query_row([id], |row| row.get(0))?;
            let (content, offset) = match_text(&body, &terms);
            let (label, _) = match_text(title, &terms);
            let prior = index < position && !found_prior && !body.trim().is_empty();
            found_prior |= prior;
            if content + label == 0 && !prior {
                continue;
            }
            let mut hit = Evidence {
                id: id.clone(),
                kind: "chapter",
                status: "manuscript",
                title: title.clone(),
                revision: *revision,
                volume_id: Some(volume.clone()),
                chapter_id: Some(id.clone()),
                field: "body".into(),
                start: 0,
                end: 0,
                quote: String::new(),
                source_files: None,
            };
            fill_quote(&mut hit, &body, offset, prior);
            ranked.push((content + 3 * label + if prior { 1000 } else { 0 }, hit));
        }
        for summary in self.list_materials(book, false)? {
            if [
                summary.chapter_id.as_deref(),
                summary.source_chapter_id.as_deref(),
                summary.payoff_chapter_id.as_deref(),
            ]
            .into_iter()
            .flatten()
            .any(|id| order.get(id).is_some_and(|index| *index > position))
            {
                continue;
            }
            let document = summary.kind == "document" || summary.title.starts_with("原文 · ");
            if document && !include_references {
                continue;
            }
            let material = self.get_material(&summary.id, false)?;
            let reference = document
                || material.fields.get("style").is_some_and(|style| {
                    !style.is_empty()
                        && material.fields.iter().all(|(key, value)| {
                            matches!(key.as_str(), "style" | "source_files")
                                || value.trim().is_empty()
                        })
                });
            if reference && !include_references {
                continue;
            }
            let status = if reference {
                "reference"
            } else if material.content_review_required
                || ["stale", "unavailable"].contains(&material.source_status.as_str())
                || material.title.starts_with("［待")
            {
                "review"
            } else if material.kind == "outline"
                || material.tracking_status.as_deref() == Some("planned")
            {
                "plan"
            } else {
                "material"
            };
            let (title, _) = match_text(&material.title, &terms);
            let alias = material
                .fields
                .get("aliases")
                .map_or(0, |value| match_text(value, &terms).0);
            let label = title.max(alias);
            let chapter_match = material.chapter_id.as_deref() == Some(target);
            let volume_plan = material.kind == "outline"
                && (material.volume_id.as_ref() == Some(target_volume)
                    || (material.volume_id.is_none()
                        && material.chapter_id.is_none()
                        && target_volume_title.chars().count() >= 2
                        && material.title.contains(target_volume_title)));
            let field = material
                .fields
                .iter()
                .filter(|(key, value)| {
                    !value.trim().is_empty() && !matches!(key.as_str(), "source_files" | "aliases")
                })
                .map(|(key, value)| {
                    let (score, offset) = match_text(value, &terms);
                    (score, offset, key, value)
                })
                .max_by_key(|(score, _, _, _)| *score);
            let Some((content, offset, field, value)) = field else {
                continue;
            };
            if content + label == 0 && !chapter_match && !volume_plan {
                continue;
            }
            let mut hit = Evidence {
                id: material.id,
                kind: "material",
                status,
                title: material.title,
                revision: material.revision,
                volume_id: material.volume_id,
                chapter_id: material.chapter_id,
                field: field.clone(),
                start: 0,
                end: 0,
                quote: String::new(),
                source_files: material.fields.get("source_files").cloned(),
            };
            fill_quote(&mut hit, value, offset, false);
            let named_character =
                material.kind == "character" && (query.contains(&hit.title) || alias >= 4);
            let score = content
                + 4 * label
                + if chapter_match || volume_plan { 300 } else { 0 }
                + if named_character { 80 } else { 0 };
            ranked.push((if status == "review" { score / 2 } else { score }, hit));
        }
        ranked.sort_by(|(left, hit), (right, other)| {
            right.cmp(left).then_with(|| hit.id.cmp(&other.id))
        });
        truncated |= ranked.len() > limit as usize;
        Ok(RetrievalResults {
            items: ranked
                .into_iter()
                .take(limit as usize)
                .map(|(_, hit)| hit)
                .collect(),
            truncated,
        })
    }
}
