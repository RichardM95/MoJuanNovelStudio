use novel_core::*;
#[test]
fn bom_mixed_newlines_and_preamble_are_preserved() {
    let mut s = Service::open(":memory:").unwrap();
    let p = s
        .prepare_txt_import(
            "\u{feff}前言\r\n第1卷 风\r\n第一章 开始\r\n正文🙂\r\n\n第二章 结尾\n末尾".as_bytes(),
            "旧稿.txt",
        )
        .unwrap();
    assert_eq!(p.title, "旧稿");
    assert_eq!(p.total_chapters, 3);
    assert_eq!(s.get_txt_import_chapter(&p.id, 0).unwrap().body, "前言\r\n");
    assert_eq!(
        s.get_txt_import_chapter(&p.id, 1).unwrap().body,
        "正文🙂\r\n\n"
    );
    assert_eq!(s.get_txt_import_chapter(&p.id, 2).unwrap().body, "末尾");
    let o = s.commit_txt_import(&p.id).unwrap();
    assert_eq!(o.chapters, 3);
    assert!(s.commit_txt_import(&p.id).is_err());
    let state = s.load().unwrap();
    assert_eq!(state.books[0].id, o.book_id);
    assert_eq!(state.chapters.len(), 3);
    assert!(state.chapters.iter().all(|c| c.revision == 0));
}
#[test]
fn revised_single_mode_retains_every_non_bom_byte_and_invalidates_old_id() {
    let mut s = Service::open(":memory:").unwrap();
    let text = "第一章 是标题\r\n正文\r无尾行";
    let p = s.prepare_txt_import(text.as_bytes(), "稿.txt").unwrap();
    assert!(s.revise_txt_import("wrong", "书", "", "single").is_err());
    let next = s.revise_txt_import(&p.id, "书", "作者", "single").unwrap();
    assert_eq!(next.total_chapters, 1);
    assert_eq!(s.get_txt_import_chapter(&next.id, 0).unwrap().body, text);
    assert!(s.get_txt_import_chapter(&p.id, 0).is_err());
    assert!(s.commit_txt_import(&p.id).is_err());
    assert_eq!(s.commit_txt_import(&next.id).unwrap().chapters, 1);
}
#[test]
fn decoding_limits_and_conservative_heading() {
    let mut s = Service::open(":memory:").unwrap();
    for data in [
        &b""[..],
        &b"\xff\xfea\0"[..],
        &b"\xc4\xe3\xba\xc3"[..],
        &b"a\0b"[..],
    ] {
        assert!(s.prepare_txt_import(data, "稿.txt").is_err());
    }
    assert!(s
        .prepare_txt_import(&vec![b'x'; 16 * 1024 * 1024 + 1], "大.txt")
        .is_err());
    let text = "第一章她这样说\n第2章后面是叙述\n普通文";
    let p = s.prepare_txt_import(text.as_bytes(), "a.txt").unwrap();
    assert_eq!(p.total_chapters, 1);
    assert_eq!(s.get_txt_import_chapter(&p.id, 0).unwrap().body, text);
}
#[test]
fn creates_distinct_same_name_book_and_keeps_existing_work() {
    let mut s = Service::open(":memory:").unwrap();
    let old = s
        .apply(Command::CreateBook {
            title: "稿".into(),
            author: "原作者".into(),
        })
        .unwrap();
    let p = s
        .prepare_txt_import("第一章 新章\n新稿".as_bytes(), "稿.txt")
        .unwrap();
    let o = s.commit_txt_import(&p.id).unwrap();
    let state = s.load().unwrap();
    assert_ne!(o.book_id, old.books[0].id);
    assert_eq!(state.books.len(), 2);
    assert!(state.books.contains(&old.books[0]));
    assert!(state.chapters.contains(&old.chapters[0]));
    let c = state
        .chapters
        .iter()
        .find(|c| !old.chapters.contains(c))
        .unwrap();
    assert_eq!(
        s.list_chapter_versions(&o.book_id, &c.id, None, 50, false)
            .unwrap()
            .items
            .len(),
        1
    );
}
#[test]
fn single_mode_recovers_from_heading_limit_without_dropping_text() {
    let mut s = Service::open(":memory:").unwrap();
    let text = (0..2001)
        .map(|n| format!("第{n}章 标题\n正文\n"))
        .collect::<String>();
    assert!(s.prepare_txt_import(text.as_bytes(), "稿.txt").is_err());
    let p = s
        .prepare_txt_import_mode(text.as_bytes(), "稿.txt", "single")
        .unwrap();
    assert_eq!(s.get_txt_import_chapter(&p.id, 0).unwrap().body, text);
}
#[test]
fn failed_revision_disables_old_config_but_keeps_source_for_retry() {
    let mut s = Service::open(":memory:").unwrap();
    let p = s.prepare_txt_import(b"body", "a.txt").unwrap();
    assert!(s.revise_txt_import(&p.id, "", "", "single").is_err());
    assert!(s.get_txt_import_chapter(&p.id, 0).is_err());
    let next = s.revise_txt_import(&p.id, "new", "", "single").unwrap();
    assert!(s
        .revise_txt_import(&next.id, "new", &"x".repeat(4097), "single")
        .is_err());
    assert!(s.commit_txt_import(&next.id).is_err());
    assert!(s.load().unwrap().books.is_empty());
}
#[test]
fn lone_cr_fullwidth_digits_and_empty_heading_titles() {
    let mut s = Service::open(":memory:").unwrap();
    let p = s
        .prepare_txt_import(
            "第１卷\r第两章\r甲\r第３章 末\r乙\u{feff}".as_bytes(),
            "稿.txt",
        )
        .unwrap();
    assert_eq!(p.volumes[0].title, "未命名卷");
    assert_eq!(p.volumes[0].chapters[0].title, "未命名章");
    assert_eq!(s.get_txt_import_chapter(&p.id, 0).unwrap().body, "甲\r");
    assert_eq!(
        s.get_txt_import_chapter(&p.id, 1).unwrap().body,
        "乙\u{feff}"
    );
}
#[test]
fn body_volume_and_metadata_limits_are_not_truncated() {
    let mut s = Service::open(":memory:").unwrap();
    assert!(s
        .prepare_txt_import_mode(&vec![b'x'; 10 * 1024 * 1024 + 1], "large.txt", "single")
        .is_err());
    let text = (0..201)
        .map(|n| format!("第{n}卷 标题\n"))
        .collect::<String>();
    assert!(s.prepare_txt_import(text.as_bytes(), "a.txt").is_err());
    let p = s
        .prepare_txt_import_mode(text.as_bytes(), "a.txt", "single")
        .unwrap();
    assert_eq!(s.get_txt_import_chapter(&p.id, 0).unwrap().body, text);
    assert!(s
        .revise_txt_import(&p.id, &"x".repeat(4097), "", "single")
        .is_err());
    assert!(s.commit_txt_import(&p.id).is_err());
    assert!(s.load().unwrap().books.is_empty());
}
#[test]
fn wrong_discard_or_detail_index_cannot_affect_new_candidate() {
    let mut s = Service::open(":memory:").unwrap();
    let old = s.prepare_txt_import(b"old", "old.txt").unwrap();
    let new = s.prepare_txt_import(b"new", "new.txt").unwrap();
    s.discard_txt_import(&old.id).unwrap();
    assert!(s.get_txt_import_chapter(&new.id, 1).is_err());
    assert_eq!(s.get_txt_import_chapter(&new.id, 0).unwrap().body, "new");
    s.discard_txt_import(&new.id).unwrap();
    assert!(s.commit_txt_import(&new.id).is_err());
}
#[test]
fn overlong_raw_heading_line_is_preserved_as_body_even_when_padded() {
    let mut s = Service::open(":memory:").unwrap();
    let text = format!("{}第一章 标题\r\n正文", " ".repeat(201));
    let preview = s.prepare_txt_import(text.as_bytes(), "稿.txt").unwrap();
    assert_eq!(preview.total_chapters, 1);
    assert_eq!(s.get_txt_import_chapter(&preview.id, 0).unwrap().body, text);
}
