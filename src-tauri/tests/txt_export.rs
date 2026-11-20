use novel_core::*;
fn setup() -> (Service, String) {
    let mut s = Service::open(":memory:").unwrap();
    let p = s
        .prepare_txt_import(
            "第1卷 归途\r\n第一章 来信\r\n中🙂\r\n乙\r丙\n第二章 末尾\n尾".as_bytes(),
            "书.txt",
        )
        .unwrap();
    let o = s.commit_txt_import(&p.id).unwrap();
    (s, o.book_id)
}
fn options() -> ExportOptions {
    ExportOptions {
        scope: "book".into(),
        target_id: None,
        include_metadata: false,
        include_headings: false,
        newline: "lf".into(),
        bom: true,
    }
}
#[test]
fn frozen_bytes_match_preview_and_preserve_unicode() {
    let (mut s, id) = setup();
    let p = s.prepare_txt_export(&id, options()).unwrap();
    assert_eq!(p.chapters, 2);
    assert_eq!(p.text, "中🙂\n乙\n丙\n\n\n尾\n\n");
    let bytes = s.take_txt_export(&p.id).unwrap();
    assert_eq!(&bytes[..3], b"\xef\xbb\xbf");
    assert_eq!(String::from_utf8(bytes[3..].to_vec()).unwrap(), p.text);
    assert_eq!(bytes.len(), p.bytes);
    assert!(s.take_txt_export(&p.id).is_err());
}
#[test]
fn scoped_selection_checks_ownership_and_invalidates_old() {
    let (mut s, id) = setup();
    let state = s.load().unwrap();
    let c = &state.chapters[0];
    let mut o = options();
    o.scope = "chapter".into();
    o.target_id = Some(c.id.clone());
    let p = s.prepare_txt_export(&id, o.clone()).unwrap();
    assert_eq!(p.chapters, 1);
    o.target_id = Some("foreign".into());
    assert!(s.prepare_txt_export(&id, o).is_err());
    assert!(s.take_txt_export(&p.id).is_err());
}
#[test]
fn options_are_strict_and_preview_is_bounded() {
    let (mut s, id) = setup();
    let mut o = options();
    o.newline = "wat".into();
    assert!(s.prepare_txt_export(&id, o).is_err());
    let p = s.prepare_txt_export(&id, options()).unwrap();
    s.discard_txt_export("wrong");
    assert!(s.take_txt_export(&p.id).is_ok());
}
#[test]
fn content_bom_is_not_hidden_when_encoding_bom_disabled() {
    let (mut s, id) = setup();
    let state = s.load().unwrap();
    let c = &state.chapters[0];
    s.apply(Command::SaveChapter {
        id: c.id.clone(),
        body: "\u{feff}正文".into(),
        expected_revision: c.revision,
    })
    .unwrap();
    let mut o = options();
    o.scope = "chapter".into();
    o.target_id = Some(c.id.clone());
    o.bom = false;
    let p = s.prepare_txt_export(&id, o).unwrap();
    assert!(p.text.starts_with('\u{feff}'));
    assert_eq!(s.take_txt_export(&p.id).unwrap(), p.text.as_bytes());
}
#[test]
fn rendering_orders_and_scopes_and_excludes_recycle() {
    let (mut s, id) = setup();
    let state = s.load().unwrap();
    let first = state.chapters.iter().find(|c| c.title == "来信").unwrap();
    let last = state.chapters.iter().find(|c| c.title == "末尾").unwrap();
    let mut o = options();
    o.include_headings = true;
    o.include_metadata = true;
    o.newline = "crlf".into();
    o.bom = false;
    let p = s.prepare_txt_export(&id, o.clone()).unwrap();
    assert!(p.text.find("来信").unwrap() < p.text.find("末尾").unwrap());
    assert!(!p.text.replace("\r\n", "").contains(['\r', '\n']));
    let frozen = p.text.clone();
    s.apply(Command::TrashChapter {
        id: first.id.clone(),
    })
    .unwrap();
    assert_eq!(
        String::from_utf8(s.take_txt_export(&p.id).unwrap()).unwrap(),
        frozen
    );
    o.scope = "chapter".into();
    o.target_id = Some(first.id.clone());
    assert!(s.prepare_txt_export(&id, o.clone()).is_err());
    o.target_id = Some(last.id.clone());
    assert_eq!(s.prepare_txt_export(&id, o).unwrap().chapters, 1);
}
#[test]
fn truncates_unicode_preview_without_changing_export() {
    let (mut s, id) = setup();
    let c = s.load().unwrap().chapters[0].clone();
    s.apply(Command::SaveChapter {
        id: c.id,
        body: "🙂".repeat(5000),
        expected_revision: c.revision,
    })
    .unwrap();
    let p = s.prepare_txt_export(&id, options()).unwrap();
    assert_eq!(p.text.chars().count(), 4000);
    assert!(p.truncated);
    assert_eq!(s.take_txt_export(&p.id).unwrap().len(), p.bytes);
}
#[test]
fn crlf_expansion_is_in_output_budget() {
    let mut s = Service::open(":memory:").unwrap();
    let text = format!(
        "第一章 甲\n{}\n第二章 乙\n{}",
        "\n".repeat(9 * 1024 * 1024),
        "\n".repeat(7 * 1024 * 1024 - 100)
    );
    let p = s.prepare_txt_import(text.as_bytes(), "x").unwrap();
    let id = s.commit_txt_import(&p.id).unwrap().book_id;
    let mut o = options();
    o.newline = "crlf".into(); // Below32MiB source expands but remains bounded.
    let p = s.prepare_txt_export(&id, o.clone()).unwrap();
    assert!(p.bytes <= 32 * 1024 * 1024);
    let c = s
        .load()
        .unwrap()
        .chapters
        .into_iter()
        .find(|c| c.title == "乙")
        .unwrap();
    s.apply(Command::SaveChapter {
        id: c.id,
        body: "\n".repeat(10 * 1024 * 1024),
        expected_revision: c.revision,
    })
    .unwrap();
    assert!(s
        .prepare_txt_export(&id, o)
        .unwrap_err()
        .message
        .contains("32MiB"));
    assert!(s.take_txt_export(&p.id).is_err());
}
#[test]
fn volume_scope_and_deleted_ancestor_and_foreign_chapter() {
    let (mut s, id) = setup();
    let state = s.load().unwrap();
    let v = state.volumes[0].clone();
    let mut o = options();
    o.scope = "volume".into();
    o.target_id = Some(v.id.clone());
    let p = s.prepare_txt_export(&id, o.clone()).unwrap();
    assert_eq!((p.volumes, p.chapters), (1, 2));
    let q = s.prepare_txt_import(b"other", "other.txt").unwrap();
    let other = s.commit_txt_import(&q.id).unwrap();
    let state = s.load().unwrap();
    let ov = state
        .volumes
        .iter()
        .find(|v| v.book_id == other.book_id)
        .unwrap();
    let oc = state
        .chapters
        .iter()
        .find(|c| c.volume_id == ov.id)
        .unwrap();
    let mut c = options();
    c.scope = "chapter".into();
    c.target_id = Some(oc.id.clone());
    assert!(s.prepare_txt_export(&id, c).is_err());
    s.apply(Command::TrashVolume { id: v.id }).unwrap();
    assert!(s.prepare_txt_export(&id, o).is_err());
    assert_eq!(s.prepare_txt_export(&id, options()).unwrap().chapters, 0);
}
