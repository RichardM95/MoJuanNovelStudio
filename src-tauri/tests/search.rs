use novel_core::*;
use std::collections::BTreeMap;
fn setup() -> (Service, LibraryState) {
    let mut s = Service::open(":memory:").unwrap();
    let a = s
        .apply(Command::CreateBook {
            title: "搜索书".into(),
            author: "".into(),
        })
        .unwrap();
    (s, a)
}
#[test]
fn literal_search_scope_snippet_and_limits() {
    let (mut s, a) = setup();
    let b = &a.books[0].id;
    let c = &a.chapters[0].id;
    s.apply(Command::SaveChapter {
        id: c.clone(),
        body: format!("{}中文%_字面Case{}", "前".repeat(220), "后".repeat(220)),
        expected_revision: 0,
    })
    .unwrap();
    s.create_material(CreateMaterialInput {
        book_id: b.clone(),
        kind: "outline".into(),
        title: "检索卡".into(),
        fields: BTreeMap::from([("content".into(), "中文%_字面Case".into())]),
        volume_id: None,
        chapter_id: None,
        source_chapter_id: None,
        source_revision: None,
        story_order: 0,
        story_time: None,
        tracking_status: None,
        payoff_chapter_id: None,
        payoff_revision: None,
    })
    .unwrap();
    let r = s.search_book(b, "%_", 1).unwrap();
    assert_eq!(r.items.len(), 1);
    assert!(r.truncated);
    assert!(r.items[0].snippet.contains("%_"));
    assert!(r.items[0].snippet.chars().count() <= 200);
    let r = s.search_book(b, "中文", 50).unwrap();
    assert_eq!(r.items.len(), 2);
    assert!(!r.truncated);
    assert!(r.items.iter().any(|h| h.kind == "chapter"));
    assert!(r.items.iter().any(|h| h.kind == "material"));
    assert!(s.search_book(b, "case", 50).unwrap().items.is_empty());
    assert!(s.search_book(b, "content", 50).unwrap().items.is_empty());
    for (query, limit) in [("", 1), ("x", 0), ("x", 51)] {
        assert!(s.search_book(b, query, limit).is_err());
    }
    assert!(s.search_book(b, &"字".repeat(86), 1).is_err());
    let other = s
        .apply(Command::CreateBook {
            title: "另书".into(),
            author: "".into(),
        })
        .unwrap();
    let otherbook = &other.books.iter().find(|book| book.id != *b).unwrap().id;
    assert!(s
        .search_book(otherbook, "中文", 50)
        .unwrap()
        .items
        .is_empty());
    s.apply(Command::TrashChapter { id: c.clone() }).unwrap();
    assert_eq!(s.search_book(b, "中文", 50).unwrap().items.len(), 1);
    s.apply(Command::TrashBook { id: b.clone() }).unwrap();
    assert!(s.search_book(b, "中文", 50).is_err());
}
#[test]
fn search_handles_quotes_unicode_and_only_own_active_targets() {
    let (mut s, a) = setup();
    let b = &a.books[0].id;
    let mut input = CreateMaterialInput {
        book_id: b.clone(),
        kind: "outline".into(),
        title: "quote \" '<script>".into(),
        fields: BTreeMap::from([(
            "content".into(),
            format!("{}' OR 1=1 -- 字面{}", "🍀".repeat(220), "字".repeat(220)),
        )]),
        volume_id: Some(a.volumes[0].id.clone()),
        chapter_id: None,
        source_chapter_id: None,
        source_revision: None,
        story_order: 0,
        story_time: None,
        tracking_status: None,
        payoff_chapter_id: None,
        payoff_revision: None,
    };
    let m = s.create_material(input.clone()).unwrap();
    let query = "' OR 1=1 --";
    let first = s.search_book(b, query, 50).unwrap();
    assert_eq!(first.items.len(), 1);
    assert_eq!(first.items[0].id, m.id);
    assert!(first.items[0].snippet.contains(query));
    assert!(first.items[0].snippet.chars().count() <= 200);
    assert_eq!(s.search_book(b, query, 50).unwrap(), first);
    s.apply(Command::TrashVolume {
        id: a.volumes[0].id.clone(),
    })
    .unwrap();
    assert!(s.search_book(b, query, 50).unwrap().items.is_empty());
    s.apply(Command::RestoreVolume {
        id: a.volumes[0].id.clone(),
    })
    .unwrap();
    assert_eq!(s.search_book(b, query, 50).unwrap(), first);
    s.trash_material(&m.id, 0).unwrap();
    assert!(s.search_book(b, query, 50).unwrap().items.is_empty());
    input.volume_id = None;
    let m = s.create_material(input).unwrap();
    assert_eq!(
        s.search_book(b, "<script>", 1).unwrap().items[0].title,
        m.title
    );
}
#[test]
fn search_matches_bounded_story_time_label_without_reading_tracking_state_as_text() {
    let (mut s, a) = setup();
    let m = s
        .create_material(CreateMaterialInput {
            book_id: a.books[0].id.clone(),
            kind: "timeline".into(),
            title: "虚构事件".into(),
            fields: BTreeMap::from([("content".into(), "叙事内容不含时间标签".into())]),
            volume_id: None,
            chapter_id: None,
            source_chapter_id: None,
            source_revision: None,
            story_order: 5,
            story_time: Some("第三日夜".into()),
            tracking_status: Some("planned".into()),
            payoff_chapter_id: None,
            payoff_revision: None,
        })
        .unwrap();
    let results = s.search_book(&a.books[0].id, "第三日夜", 50).unwrap();
    assert_eq!(results.items.len(), 1);
    assert_eq!(results.items[0].id, m.id);
    assert!(results.items[0].snippet.contains("第三日夜"));
    assert!(s
        .search_book(&a.books[0].id, "planned", 50)
        .unwrap()
        .items
        .is_empty());
}
