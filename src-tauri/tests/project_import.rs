use novel_core::{Command, ProjectImportOptions, ProjectImportRoles, ProjectLink, Service};
use std::{fs, path::PathBuf};

const MANUSCRIPT: &str = "《边界抄本》第一卷.txt";
const WORLD: &str = "核心设定与卷纲.md";
const OUTLINES: &str = "第一卷分章细纲.md";
const STYLE: &str = "创作风格.md";
const TEXT: &str = concat!(
    "《边界抄本》\r\n----------\r\n第一卷：开篇\r\n----------\r\n",
    "第一章 起点\r\n甲\r\n----------\r\n乙\r\n----------\r\n",
    "--第二章：转折\r\n丙\r\n----------\r\n",
    "第四章 收束\r\n丁\r\n（第一卷终）\r\n----------\r\n"
);
const CHAPTER_OUTLINES: &str = concat!(
    "# 边界抄本：第一卷分章细纲\n",
    "* **第1章：起点** — 甲乙\n",
    "* **第2章：旧转折** — 旧计划\n",
    "* **第3章：待补的一章** — 计划\n",
    "* **第4章：收束** — 丁\n"
);

struct Fixture {
    root: PathBuf,
    project: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("mojuan-project-{}", uuid::Uuid::new_v4()));
        let project = root.join("边界抄本");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&project).unwrap();
        fs::write(project.join(MANUSCRIPT), TEXT).unwrap();
        fs::write(
            project.join(WORLD),
            "# 核心设定\n### 第一卷：开篇\n卷一计划\n### 第二卷：未来\n卷二计划\n",
        )
        .unwrap();
        fs::write(project.join(OUTLINES), CHAPTER_OUTLINES).unwrap();
        fs::write(project.join(STYLE), "# 创作风格\n仅供作者参考。\n").unwrap();
        fs::File::create(project.join("参考作品.txt"))
            .unwrap()
            .set_len(16 * 1024 * 1024 + 1)
            .unwrap();
        Self { root, project }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for dir in [&self.project, &self.root] {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    if entry.file_type().is_ok_and(|kind| kind.is_file()) {
                        let _ = fs::remove_file(entry.path());
                    }
                }
            }
            let _ = fs::remove_dir(dir);
        }
    }
}

fn options(roles: ProjectImportRoles) -> ProjectImportOptions {
    ProjectImportOptions {
        roles,
        title: None,
        author: String::new(),
        omit_separators: true,
        automatic: false,
        manuscript_override: None,
    }
}

#[test]
fn mixed_notes_are_organized_by_content_and_overlapping_outlines_remain_reviewable() {
    let input = Fixture::new();
    fs::write(input.project.join("fragment.txt"),
        "# 创作风格\n短句收束。\n\n# 人物：陆沉\n身份：维修工\n\n# 规则：信用点不能透支\n\n# 第一卷分章细纲\n* **第2章：转折** — 新计划\n\n# 随手记\n红色那一张也许会用。\n").unwrap();
    fs::write(
        input.project.join("untitled.md"),
        "# 第一卷章纲\n第2章 转折 — 旧计划\n",
    )
    .unwrap();
    let mut service = Service::open(":memory:").unwrap();
    let picked = service.choose_project_import(&input.project).unwrap();
    let mut settings = options(ProjectImportRoles::default());
    settings.automatic = true;
    let preview = service
        .prepare_project_import(&picked.id, settings)
        .unwrap();
    assert_eq!(preview.total_chapters, 3);
    assert!(preview
        .cards
        .iter()
        .any(|card| card.title == "陆沉" && card.kind == "character"));
    let competing: Vec<_> = preview
        .cards
        .iter()
        .filter_map(|card| card.association.as_ref())
        .filter(|a| a.number == 2)
        .collect();
    assert!(competing.len() >= 2 && competing.iter().all(|a| a.status == "ambiguous"));
    let outcome = service
        .commit_project_import(&preview.id, Vec::new())
        .unwrap();
    let items = service.list_materials(&outcome.book_id, false).unwrap();
    let source = items
        .iter()
        .find(|item| item.kind == "document" && item.title.contains("fragment.txt"))
        .unwrap();
    assert!(
        service.get_material(&source.id, false).unwrap().fields["content"]
            .contains("红色那一张也许会用。")
    );
    let style = items
        .iter()
        .filter(|item| item.title == "创作风格")
        .map(|item| service.get_material(&item.id, false).unwrap())
        .find(|item| item.fields["source_files"].starts_with("fragment.txt · "))
        .unwrap();
    assert_eq!(style.fields["style"].trim(), "短句收束。");
    let card = items.iter().find(|item| item.title == "陆沉").unwrap();
    assert!(
        service.get_material(&card.id, false).unwrap().fields["source_files"]
            .contains("fragment.txt")
    );
}

#[test]
fn existing_book_can_preview_and_apply_selected_knowledge_from_legacy_originals() {
    let mut service = Service::open(":memory:").unwrap();
    let state = service
        .apply(Command::CreateBook {
            title: "旧项目".into(),
            author: "".into(),
        })
        .unwrap();
    let book = &state.books[0].id;
    let source = service
        .create_material(
            serde_json::from_value(serde_json::json!({
                "book_id":book,"title":"原文 · notes.md","kind":"setting",
                "fields":{"world":"# 人物：陆沉\n身份：维修工\n\n# 规则：信用点不能透支"}
            }))
            .unwrap(),
        )
        .unwrap();
    let preview = service.preview_material_organization(book).unwrap();
    assert_eq!(preview.source_count, 1);
    let person = preview
        .cards
        .iter()
        .position(|c| c.title == "陆沉")
        .unwrap();
    assert_eq!(
        service
            .apply_material_organization(&preview.id, vec![person])
            .unwrap(),
        1
    );
    assert_eq!(service.load().unwrap(), state);
    assert_eq!(service.list_materials(book, false).unwrap().len(), 2);
    assert!(service
        .preview_material_organization(book)
        .unwrap()
        .cards
        .iter()
        .all(|c| c.title != "陆沉"));

    let person = service
        .list_materials(book, false)
        .unwrap()
        .into_iter()
        .find(|m| m.title == "陆沉")
        .unwrap();
    let current = service.get_material(&person.id, false).unwrap();
    service
        .save_material(
            serde_json::from_value(serde_json::json!({
                "id":source.id,"title":source.title,"expected_revision":source.revision,
                "fields":{"world":"# 人物：陆沉\n\n身份：领航员\n\n# 规则：信用点不能透支"}
            }))
            .unwrap(),
        )
        .unwrap();
    let preview = service.preview_material_organization(book).unwrap();
    let choice = preview
        .cards
        .iter()
        .position(|c| c.title == "陆沉")
        .unwrap();
    let change = serde_json::to_value(&preview.cards[choice]).unwrap();
    assert_eq!(
        change["previous_fields"]["identity"],
        current.fields["identity"]
    );

    let mut manual = current.fields.clone();
    manual.insert("appearance".into(), "手背有旧伤".into());
    let revised = service
        .save_material(
            serde_json::from_value(serde_json::json!({
                "id":current.id,"title":current.title,"expected_revision":current.revision,
                "fields":manual
            }))
            .unwrap(),
        )
        .unwrap();
    let conflict = service
        .apply_material_organization(&preview.id, vec![choice])
        .unwrap_err();
    assert_eq!(conflict.code, "revision_conflict");
    assert_eq!(service.get_material(&person.id, false).unwrap(), revised);

    let preview = service.preview_material_organization(book).unwrap();
    let choice = preview
        .cards
        .iter()
        .position(|c| c.title == "陆沉")
        .unwrap();
    assert_eq!(
        service
            .apply_material_organization(&preview.id, vec![choice])
            .unwrap(),
        1
    );
    let updated = service.get_material(&person.id, false).unwrap();
    assert_eq!(updated.revision, revised.revision + 1);
    assert!(updated.fields["identity"]
        .lines()
        .any(|line| line == "领航员"));
    assert_eq!(updated.fields["appearance"], "手背有旧伤");
    assert_eq!(service.list_materials(book, false).unwrap().len(), 2);
    assert!(service
        .preview_material_organization(book)
        .unwrap()
        .cards
        .iter()
        .all(|c| c.title != "陆沉"));
}

#[test]
fn previews_source_boundaries_and_pairs_outlines_without_reading_reference_txt() {
    let input = Fixture::new();
    let mut service = Service::open(":memory:").unwrap();
    let choice = service.choose_project_import(&input.project).unwrap();
    assert_eq!(choice.files.len(), 5);
    assert_eq!(
        choice.suggested_roles.manuscript.as_deref(),
        Some(MANUSCRIPT)
    );
    assert_eq!(choice.suggested_roles.style.as_deref(), Some(STYLE));
    assert!(choice
        .files
        .iter()
        .any(|f| f.name == "参考作品.txt" && !f.selectable));

    let preview = service
        .prepare_project_import(&choice.id, options(choice.suggested_roles))
        .unwrap();
    assert_eq!(preview.title, "边界抄本");
    assert_eq!(preview.volumes.len(), 1);
    assert_eq!(preview.volumes[0].title, "开篇");
    assert_eq!(preview.total_chapters, 3);
    assert_eq!(
        preview.volumes[0].chapters[1].source.as_ref().unwrap().line,
        10
    );
    assert_eq!(
        preview.volumes[0].chapters[1]
            .source
            .as_ref()
            .unwrap()
            .number,
        Some(2)
    );
    assert!(preview
        .numbering
        .iter()
        .any(|n| n.kind == "missing" && n.first == 3 && n.last == 3));
    let body = |index| {
        service
            .get_project_import_detail(&preview.id, "chapter", index)
            .unwrap()
            .body
    };
    assert_eq!(body(0), "甲\r\n----------\r\n乙\r\n");
    assert_eq!(body(1), "丙\r\n");
    assert_eq!(body(2), "丁\r\n（第一卷终）\r\n");
    assert_eq!(preview.cards.len(), 9);
    let statuses: Vec<_> = preview
        .cards
        .iter()
        .filter_map(|card| {
            card.association
                .as_ref()
                .map(|a| (a.number, a.status, a.chapter_index))
        })
        .collect();
    assert_eq!(
        statuses,
        [
            (1, "same_title", Some(0)),
            (2, "different_title", Some(1)),
            (3, "missing_chapter", None),
            (4, "same_title", Some(2))
        ]
    );
    assert!(service.load().unwrap().books.is_empty());
}

#[test]
fn commits_one_reviewed_snapshot_with_editable_materials_and_recovers_through_backup() {
    let input = Fixture::new();
    let mut service = Service::open(":memory:").unwrap();
    let choice = service.choose_project_import(&input.project).unwrap();
    let preview = service
        .prepare_project_import(&choice.id, options(choice.suggested_roles))
        .unwrap();
    let card = preview
        .cards
        .iter()
        .position(|row| row.association.as_ref().is_some_and(|a| a.number == 2))
        .unwrap();
    // Confirmation applies the bytes reviewed in the preview.
    fs::write(input.project.join(OUTLINES), "# Later edit\n").unwrap();
    let result = service
        .commit_project_import(
            &preview.id,
            vec![ProjectLink {
                card_index: card,
                chapter_index: 1,
            }],
        )
        .unwrap();
    assert_eq!(result.book_id, preview.book_id);
    assert_eq!((result.volumes, result.chapters, result.cards), (1, 3, 9));
    assert!(service.commit_project_import(&preview.id, vec![]).is_err());

    let saved = service.load().unwrap();
    let chapter_id = &saved
        .chapters
        .iter()
        .find(|c| c.title == "转折")
        .unwrap()
        .id;
    let volume_id = &saved.volumes[0].id;
    let cards = service.list_materials(&result.book_id, false).unwrap();
    let linked = cards.iter().find(|m| m.title.contains("旧转折")).unwrap();
    assert_eq!(linked.chapter_id.as_ref(), Some(chapter_id));
    let missing = cards
        .iter()
        .find(|m| m.title.contains("待补的一章"))
        .unwrap();
    assert!(missing.title.contains("待补稿"));
    assert_eq!(missing.volume_id.as_ref(), Some(volume_id));
    assert!(missing.chapter_id.is_none());
    let future = cards
        .iter()
        .find(|m| m.title.contains("第二卷") || m.title.contains("第2卷"))
        .unwrap();
    assert!(future.volume_id.is_none());
    let original = cards.iter().find(|m| m.title.contains(OUTLINES)).unwrap();
    let original = service.get_material(&original.id, false).unwrap();
    assert_eq!(original.fields["content"], CHAPTER_OUTLINES);
    assert_eq!(
        service
            .list_chapter_versions(&result.book_id, chapter_id, None, 50, false)
            .unwrap()
            .items
            .len(),
        1
    );

    let backup = service.backup().unwrap();
    let mut restored = Service::open(":memory:").unwrap();
    restored.restore(&backup).unwrap();
    let restored_card = restored.get_material(&linked.id, false).unwrap();
    assert_eq!(restored_card.chapter_id.as_ref(), Some(chapter_id));
    assert_eq!(
        restored.get_material(&original.id, false).unwrap().fields["content"],
        CHAPTER_OUTLINES
    );
}

#[test]
fn card_insert_failure_rolls_back_book_chapters_histories_and_cards() {
    let input = Fixture::new();
    let database = input.root.join("library.sqlite");
    let mut service = Service::open(&database).unwrap();
    let old = service
        .apply(Command::CreateBook {
            title: "原作品".into(),
            author: String::new(),
        })
        .unwrap();
    let conn = rusqlite::Connection::open(&database).unwrap();
    let counts = || {
        [
            "materials",
            "material_content_versions",
            "chapter_versions",
            "workflow_audit",
        ]
        .map(|table| {
            conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap()
        })
    };
    let before = counts();
    conn.execute_batch(
        "CREATE TRIGGER fail_import_card BEFORE INSERT ON materials
         WHEN NEW.kind='outline' BEGIN SELECT RAISE(ABORT, 'injected'); END;",
    )
    .unwrap();
    let choice = service.choose_project_import(&input.project).unwrap();
    let preview = service
        .prepare_project_import(&choice.id, options(choice.suggested_roles))
        .unwrap();
    assert!(service.commit_project_import(&preview.id, vec![]).is_err());
    let actual = service.load().unwrap();
    assert_eq!(actual.books, old.books);
    assert_eq!(actual.volumes, old.volumes);
    assert_eq!(actual.chapters, old.chapters);
    assert_eq!(counts(), before);
}

#[test]
fn preview_scope_errors_and_cancel_prevent_old_candidate_from_committing() {
    let input = Fixture::new();
    let mut service = Service::open(":memory:").unwrap();
    let choice = service.choose_project_import(&input.project).unwrap();
    let preview = service
        .prepare_project_import(&choice.id, options(choice.suggested_roles.clone()))
        .unwrap();
    let mut wrong = choice.suggested_roles.clone();
    wrong.manuscript = Some("..\\source.txt".into());
    assert!(service
        .prepare_project_import(&choice.id, options(wrong))
        .is_err());
    assert!(service.commit_project_import(&preview.id, vec![]).is_err());

    let preview = service
        .prepare_project_import(&choice.id, options(choice.suggested_roles))
        .unwrap();
    service.discard_project_import(&choice.id).unwrap();
    assert!(service.commit_project_import(&preview.id, vec![]).is_err());
    assert!(service.load().unwrap().books.is_empty());
}

#[test]
#[ignore = "requires the local updated novel directory via MOJUAN_PROJECT_IMPORT_FIXTURE"]
fn imports_the_updated_sixty_chapter_project_into_a_temporary_library() {
    let source = PathBuf::from(std::env::var_os("MOJUAN_PROJECT_IMPORT_FIXTURE").unwrap());
    let temp = Fixture::new();
    let mut service = Service::open(temp.root.join("acceptance.sqlite")).unwrap();
    let choice = service.choose_project_import(&source).unwrap();
    assert!(choice
        .files
        .iter()
        .any(|file| file.kind == "txt" && !file.selectable));
    let preview = service
        .prepare_project_import(&choice.id, options(choice.suggested_roles))
        .unwrap();
    assert_eq!(preview.title, "星海余烬");
    assert_eq!(preview.volumes[0].title, "玛·萨拉铁幕");
    assert_eq!(
        (
            preview.volumes.len(),
            preview.total_chapters,
            preview.cards.len()
        ),
        (1, 60, 73)
    );
    assert!(preview.numbering.is_empty());
    let chapter = |number| {
        preview.volumes[0]
            .chapters
            .iter()
            .find(|chapter| {
                chapter.source.as_ref().and_then(|source| source.number) == Some(number)
            })
            .unwrap()
    };
    assert_eq!(chapter(12).index, 11);
    assert_eq!(chapter(34).index, 33);
    let matches = preview
        .cards
        .iter()
        .enumerate()
        .filter_map(|(card_index, card)| {
            let link = card.association.as_ref()?;
            (link.status == "same_title").then(|| ProjectLink {
                card_index,
                chapter_index: link.chapter_index.unwrap(),
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(matches.len(), 41);
    assert_eq!(
        preview
            .cards
            .iter()
            .filter_map(|c| c.association.as_ref())
            .filter(|link| link.status == "different_title")
            .count(),
        19
    );
    assert!(preview
        .cards
        .iter()
        .filter_map(|c| c.association.as_ref())
        .any(|link| link.number == 34
            && link.status == "same_title"
            && link.chapter_index == Some(33)));
    let outcome = service.commit_project_import(&preview.id, matches).unwrap();
    let written = service.load().unwrap();
    assert_eq!(written.chapters.len(), 60);
    assert_eq!(
        service
            .list_materials(&outcome.book_id, false)
            .unwrap()
            .len(),
        73
    );
}

#[test]
#[ignore = "requires the local updated novel directory via MOJUAN_PROJECT_IMPORT_FIXTURE"]
fn retrieves_book_two_evidence_from_automatically_organized_sources() {
    let source = PathBuf::from(std::env::var_os("MOJUAN_PROJECT_IMPORT_FIXTURE").unwrap());
    let mut service = Service::open(":memory:").unwrap();
    let chosen = service.choose_project_import(&source).unwrap();
    let mut prefs = options(ProjectImportRoles::default());
    prefs.automatic = true;
    let preview = service.prepare_project_import(&chosen.id, prefs).unwrap();
    assert_eq!(preview.total_chapters, 60);
    assert!(preview
        .cards
        .iter()
        .any(|card| card.kind == "character" && card.title.contains("陆沉")));
    assert_eq!(
        preview
            .cards
            .iter()
            .filter(|card| card.kind == "volume_outline")
            .count(),
        10
    );
    let links = preview
        .cards
        .iter()
        .enumerate()
        .filter_map(|(card_index, card)| {
            let row = card.association.as_ref()?;
            (row.status == "same_title").then(|| ProjectLink {
                card_index,
                chapter_index: row.chapter_index.unwrap(),
            })
        })
        .collect();
    let saved = service.commit_project_import(&preview.id, links).unwrap();
    let book = saved.book_id;
    let state = service
        .apply(Command::CreateVolume {
            book_id: book.clone(),
            title: "亡者之港黑市".into(),
        })
        .unwrap();
    let volume = state
        .volumes
        .iter()
        .find(|volume| volume.title == "亡者之港黑市")
        .unwrap();
    let state = service
        .apply(Command::CreateChapter {
            volume_id: volume.id.clone(),
            title: "第二卷开篇".into(),
        })
        .unwrap();
    let target = state
        .chapters
        .iter()
        .find(|chapter| chapter.title == "第二卷开篇")
        .unwrap();
    let results = service
        .retrieve_writing_context(
            &book,
            &target.id,
            "续写第二卷，亡者之港、陆沉、零号、黑曜石",
            20,
            false,
        )
        .unwrap();
    assert!(results
        .items
        .iter()
        .any(|hit| hit.status == "manuscript" && hit.title.contains("黑曜石前夜")));
    assert!(results.items.iter().any(|hit| hit.status == "plan"
        && hit.title.starts_with("第2卷")
        && hit.source_files.is_some()));
    assert!(results
        .items
        .iter()
        .any(|hit| hit.title.contains("陆沉") && hit.source_files.is_some()));
    assert!(results.items.iter().all(|hit| hit.status != "reference"));
}
