use novel_core::{Command, Service};

#[test]
fn ranks_chapter_context_and_keeps_future_prose_out_of_evidence() {
    let mut library = Service::open(":memory:").unwrap();
    let draft = library.prepare_txt_import(
        "第一卷 港口\n第一章 抵达\n零号刚在港口报告。\n第二卷 黑市\n第二章 开场\n\n第三章 后文\n零号在港口已经揭开秘密。".as_bytes(),
        "样稿.txt",
    ).unwrap();
    let book = library.commit_txt_import(&draft.id).unwrap().book_id;
    let saved = library.load().unwrap();
    let target = saved.chapters.iter().find(|c| c.title == "开场").unwrap();
    let prior = saved.chapters.iter().find(|c| c.title == "抵达").unwrap();
    let future = saved.chapters.iter().find(|c| c.title == "后文").unwrap();
    let volume = &saved
        .volumes
        .iter()
        .find(|volume| volume.title == "黑市")
        .unwrap()
        .id;
    let plan = library
        .create_material(
            serde_json::from_value(serde_json::json!({
                "book_id": book, "kind": "outline", "title": "第二卷卷纲", "volume_id": volume,
                "fields": { "content": "零号与港口黑市的交易安排" }
            }))
            .unwrap(),
        )
        .unwrap();
    let person = library.create_material(serde_json::from_value(serde_json::json!({
        "book_id": book, "kind": "character", "title": "陆沉",
        "fields": { "aliases": "修理师", "status": "刚到黑市", "source_files": "人物.md · 第 2 行" }
    })).unwrap()).unwrap();
    let raw = library
        .create_material(
            serde_json::from_value(serde_json::json!({
                "book_id": book, "kind": "document", "title": "原始笔记",
                "fields": { "content": "修理师过去的参考资料" }
            }))
            .unwrap(),
        )
        .unwrap();
    let results = library
        .retrieve_writing_context(&book, &target.id, "零号在港口有什么进展", 10, false)
        .unwrap();
    let original = results.items.iter().find(|hit| hit.id == prior.id).unwrap();
    assert_eq!(original.status, "manuscript");
    assert_eq!(original.revision, prior.revision);
    assert_eq!(&prior.body[original.start..original.end], original.quote);
    assert!(results
        .items
        .iter()
        .any(|hit| hit.id == plan.id && hit.status == "plan"));
    assert!(results.items.iter().all(|hit| hit.id != future.id));
    let alias = library
        .retrieve_writing_context(&book, &target.id, "修理师", 12, false)
        .unwrap();
    let evidence = alias
        .items
        .iter()
        .find(|item| item.id == person.id)
        .unwrap();
    assert_eq!(evidence.field, "status");
    assert_eq!(evidence.source_files.as_deref(), Some("人物.md · 第 2 行"));
    assert!(alias.items.iter().all(|item| item.id != raw.id));
    assert!(library
        .retrieve_writing_context(&book, &target.id, "修理师", 12, true)
        .unwrap()
        .items
        .iter()
        .any(|item| item.id == raw.id && item.status == "reference"));

    let other = library
        .apply(Command::CreateBook {
            title: "另一部".into(),
            author: "".into(),
        })
        .unwrap();
    let other_id = &other.books.iter().find(|b| b.id != book).unwrap().id;
    assert!(library
        .retrieve_writing_context(other_id, &target.id, "零号", 10, false)
        .is_err());
}
