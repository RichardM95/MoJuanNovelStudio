use novel_core::*;
fn setup(body: &str) -> (Service, String, String) {
    let mut s = Service::open(":memory:").unwrap();
    let a = s
        .apply(Command::CreateBook {
            title: "书".into(),
            author: "".into(),
        })
        .unwrap();
    let b = a.books[0].id.clone();
    let c = a.chapters[0].id.clone();
    s.apply(Command::SaveChapter {
        id: c.clone(),
        body: body.into(),
        expected_revision: 0,
    })
    .unwrap();
    (s, b, c)
}
#[test]
fn chinese_preview_commit_history_and_replay() {
    let (mut s, b, c) = setup("阿明🙂阿明%_");
    let p = s.preview_book_replace(&b, "阿明", "小舟").unwrap();
    assert_eq!(p.total_matches, 2);
    let d = s.get_replace_detail(&p.id, &c).unwrap();
    assert_eq!(d.after, "小舟🙂小舟%_");
    let o = s.commit_book_replace(&p.id).unwrap();
    assert_eq!(o.chapters, 1);
    assert_eq!(s.get_chapter(&c).unwrap().body, d.after);
    assert_eq!(s.get_chapter(&c).unwrap().revision, 2);
    assert!(s.commit_book_replace(&p.id).is_err());
    assert!(!s
        .list_chapter_versions(&b, &c, None, 50, false)
        .unwrap()
        .items
        .is_empty());
}
#[test]
fn limits_and_literal_nonoverlapping() {
    let (mut s, b, c) = setup("aaaa🙂%_");
    assert!(s.preview_book_replace(&b, "", "x").is_err());
    assert!(s.preview_book_replace(&b, &"中".repeat(86), "x").is_err());
    assert!(s.preview_book_replace(&b, "a", &"x".repeat(1025)).is_err());
    assert!(s.preview_book_replace(&b, "a", "a").is_err());
    let p = s.preview_book_replace(&b, "aa", "").unwrap();
    assert_eq!(p.total_matches, 2);
    assert_eq!(s.get_replace_detail(&p.id, &c).unwrap().after, "🙂%_");
}
#[test]
fn any_book_revision_change_rejects_entire_candidate() {
    let (mut s, b, c) = setup("旧名");
    let p = s.preview_book_replace(&b, "旧名", "新名").unwrap();
    s.apply(Command::SaveChapter {
        id: c.clone(),
        body: "旧名修改".into(),
        expected_revision: 1,
    })
    .unwrap();
    assert!(s.commit_book_replace(&p.id).is_err());
    assert_eq!(s.get_chapter(&c).unwrap().body, "旧名修改");
}
#[test]
fn new_chapter_invalidates_manifest() {
    let (mut s, b, c) = setup("旧名");
    let p = s.preview_book_replace(&b, "旧名", "新名").unwrap();
    let v = s.get_chapter(&c).unwrap().volume_id;
    s.apply(Command::CreateChapter {
        volume_id: v,
        title: "新章".into(),
    })
    .unwrap();
    assert!(s.commit_book_replace(&p.id).is_err());
    assert_eq!(s.get_chapter(&c).unwrap().body, "旧名");
}
#[test]
fn match_cap_refuses_without_changing_body() {
    let body = "甲".repeat(100001);
    let (mut s, b, c) = setup(&body);
    assert!(s.preview_book_replace(&b, "甲", "乙").is_err());
    assert_eq!(s.get_chapter(&c).unwrap().body, body);
}
#[test]
fn nonmatching_chapter_change_and_zero_matches() {
    let (mut s, b, c) = setup("甲");
    let p = s.preview_book_replace(&b, "没有", "乙").unwrap();
    assert!(p.chapters.is_empty());
    assert_eq!(s.commit_book_replace(&p.id).unwrap().chapters, 0);
    assert_eq!(s.get_chapter(&c).unwrap().revision, 1);
    let v = s.get_chapter(&c).unwrap().volume_id;
    let a = s
        .apply(Command::CreateChapter {
            volume_id: v,
            title: "无命中".into(),
        })
        .unwrap();
    let other = a.chapters.iter().find(|x| x.id != c).unwrap().id.clone();
    let p = s.preview_book_replace(&b, "甲", "乙").unwrap();
    s.apply(Command::SaveChapter {
        id: other,
        body: "无".into(),
        expected_revision: 0,
    })
    .unwrap();
    assert!(s.commit_book_replace(&p.id).is_err());
    assert_eq!(s.get_chapter(&c).unwrap().body, "甲");
}
#[test]
fn snippets_are_bounded_unicode() {
    let (mut s, b, _) = setup(&format!("{}甲{}", "🙂".repeat(250), "中".repeat(250)));
    let p = s.preview_book_replace(&b, "甲", "新词").unwrap();
    assert!(p.chapters[0].before_snippet.chars().count() <= 200);
    assert!(p.chapters[0].after_snippet.chars().count() <= 200);
    assert!(p.chapters[0].before_snippet.contains("甲"));
}
#[test]
fn after_snippet_tracks_first_changed_location() {
    let (mut s, b, _) = setup(&format!("乙{}甲结尾", "前".repeat(250)));
    let p = s.preview_book_replace(&b, "甲", "乙").unwrap();
    assert!(p.chapters[0].after_snippet.contains("乙结尾"));
    let p = s.preview_book_replace(&b, "甲", "").unwrap();
    assert!(p.chapters[0].after_snippet.contains("结尾"));
}
