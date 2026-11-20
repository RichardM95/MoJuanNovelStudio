use novel_core::*;
use std::collections::BTreeMap;
fn setup() -> (Service, LibraryState) {
    let mut s = Service::open(":memory:").unwrap();
    let a = s
        .apply(Command::CreateBook {
            title: "拆章书".into(),
            author: "作者".into(),
        })
        .unwrap();
    let a = s
        .apply(Command::SaveChapter {
            id: a.chapters[0].id.clone(),
            body: "甲🙂\r\n乙\r丙\n尾".into(),
            expected_revision: 0,
        })
        .unwrap();
    (s, a)
}
#[test]
fn split_preserves_bytes_history_and_order() {
    let (mut s, a) = setup();
    let c = &a.chapters[0];
    let p = s
        .prepare_chapter_split(&a.books[0].id, &c.id, c.revision, 5, "后半")
        .unwrap();
    assert_eq!(p.before, "甲🙂\r\n");
    assert_eq!(p.after, "乙\r丙\n尾");
    assert_eq!(s.get_split_detail(&p.id, "after").unwrap(), p.after);
    let o = s.commit_chapter_split(&p.id).unwrap();
    let n = s.load().unwrap();
    let left = n.chapters.iter().find(|x| x.id == c.id).unwrap();
    let right = n
        .chapters
        .iter()
        .find(|x| x.id == o.new_chapter_id)
        .unwrap();
    assert_eq!(left.body.clone() + &right.body, c.body);
    assert_eq!(left.revision, c.revision + 1);
    assert_eq!(right.revision, 0);
    assert_eq!(right.sort_order, left.sort_order + 1);
    assert!(s.commit_chapter_split(&p.id).is_err());
    let hist = s
        .list_chapter_versions(&a.books[0].id, &c.id, None, 50, false)
        .unwrap();
    assert!(hist.items.iter().any(|v| v.revision == c.revision));
}
#[test]
fn rejects_invalid_boundaries_and_invalidates_candidate() {
    let (mut s, a) = setup();
    let c = &a.chapters[0];
    let b = &a.books[0].id;
    for pos in [0, 2, 4, 12, 99] {
        assert!(
            s.prepare_chapter_split(b, &c.id, c.revision, pos, "后")
                .is_err(),
            "{pos}"
        );
    }
    let p = s
        .prepare_chapter_split(b, &c.id, c.revision, 3, "后")
        .unwrap();
    assert!(s
        .prepare_chapter_split(b, &c.id, c.revision, 3, " ")
        .is_err());
    assert!(s.commit_chapter_split(&p.id).is_err());
    assert!(s
        .prepare_chapter_split("foreign", &c.id, c.revision, 3, "后")
        .is_err());
    assert!(s
        .prepare_chapter_split(b, &c.id, c.revision - 1, 3, "后")
        .is_err());
}
#[test]
fn concurrent_content_or_directory_or_epoch_refuses_atomic_split() {
    for kind in 0..5 {
        let (mut s, a) = setup();
        let c = &a.chapters[0];
        let b = &a.books[0].id;
        let p = s
            .prepare_chapter_split(b, &c.id, c.revision, 3, "后")
            .unwrap();
        match kind {
            0 => {
                s.apply(Command::SaveChapter {
                    id: c.id.clone(),
                    body: "改稿".into(),
                    expected_revision: c.revision,
                })
                .unwrap();
            }
            1 => {
                s.apply(Command::RenameChapter {
                    id: c.id.clone(),
                    title: "新标题".into(),
                })
                .unwrap();
            }
            2 => {
                s.apply(Command::CreateChapter {
                    volume_id: c.volume_id.clone(),
                    title: "邻居".into(),
                })
                .unwrap();
            }
            3 => {
                let backup = s.backup().unwrap();
                s.restore(&backup).unwrap();
            }
            _ => {
                s.apply(Command::TrashVolume {
                    id: c.volume_id.clone(),
                })
                .unwrap();
            }
        }
        let before = s.backup().unwrap();
        assert!(s.commit_chapter_split(&p.id).is_err());
        assert_eq!(s.backup().unwrap(), before);
    }
}
#[test]
fn old_summary_and_proposal_stale_new_chapter_has_no_links() {
    let (mut s, a) = setup();
    let c = &a.chapters[0];
    let b = &a.books[0].id;
    let m = s
        .create_material(CreateMaterialInput {
            book_id: b.clone(),
            kind: "summary".into(),
            title: "摘要".into(),
            fields: BTreeMap::from([("content".into(), "旧摘要".into())]),
            volume_id: None,
            chapter_id: None,
            source_chapter_id: Some(c.id.clone()),
            source_revision: Some(c.revision),
            story_order: 0,
            story_time: None,
            tracking_status: None,
            payoff_chapter_id: None,
            payoff_revision: None,
        })
        .unwrap();
    let prop = s
        .propose_edit(ProposalInput {
            client_id: "test".into(),
            idempotency_key: "one".into(),
            chapter_id: c.id.clone(),
            base_revision: c.revision,
            new_body: "旧建议".into(),
            reason: "测试".into(),
        })
        .unwrap();
    let p = s
        .prepare_chapter_split(b, &c.id, c.revision, 3, "后")
        .unwrap();
    let o = s.commit_chapter_split(&p.id).unwrap();
    let m = s.get_material(&m.id, false).unwrap();
    assert_eq!(m.source_chapter_id, Some(c.id.clone()));
    assert_eq!(m.source_status, "stale");
    assert_ne!(m.source_chapter_id, Some(o.new_chapter_id));
    assert_eq!(s.list_materials(b, false).unwrap().len(), 1);
    assert!(s.decide_proposal(&prop.id, true, c.revision).is_err());
}
#[test]
fn outline_and_tracking_associations_stay_on_original_and_sources_become_stale() {
    let (mut s, a) = setup();
    let c = &a.chapters[0];
    let b = &a.books[0].id;
    let base = CreateMaterialInput {
        book_id: b.clone(),
        kind: "outline".into(),
        title: "卡片".into(),
        fields: BTreeMap::from([("content".into(), "关联".into())]),
        volume_id: None,
        chapter_id: Some(c.id.clone()),
        source_chapter_id: None,
        source_revision: None,
        story_order: 0,
        story_time: None,
        tracking_status: None,
        payoff_chapter_id: None,
        payoff_revision: None,
    };
    let outline = s.create_material(base.clone()).unwrap();
    let mut t = base.clone();
    t.kind = "timeline".into();
    t.chapter_id = None;
    t.source_chapter_id = Some(c.id.clone());
    t.source_revision = Some(c.revision);
    t.tracking_status = Some("written".into());
    let timeline = s.create_material(t.clone()).unwrap();
    t.kind = "foreshadow".into();
    t.fields = BTreeMap::from([
        ("setup".into(), "铺设".into()),
        ("resolution".into(), "回收".into()),
    ]);
    t.tracking_status = Some("resolved".into());
    t.payoff_chapter_id = Some(c.id.clone());
    t.payoff_revision = Some(c.revision);
    let foreshadow = s.create_material(t).unwrap();
    let p = s
        .prepare_chapter_split(b, &c.id, c.revision, 3, "后")
        .unwrap();
    let o = s.commit_chapter_split(&p.id).unwrap();
    assert_eq!(
        s.get_material(&outline.id, false).unwrap().chapter_id,
        Some(c.id.clone())
    );
    let t = s.get_material(&timeline.id, false).unwrap();
    assert_eq!(t.source_chapter_id, Some(c.id.clone()));
    assert_eq!(t.source_status, "stale");
    let f = s.get_material(&foreshadow.id, false).unwrap();
    assert_eq!(f.source_status, "stale");
    assert_eq!(f.payoff_status, "stale");
    assert_eq!(f.payoff_chapter_id, Some(c.id.clone()));
    assert!(s
        .list_materials(b, false)
        .unwrap()
        .iter()
        .all(|m| m.chapter_id.as_deref() != Some(&o.new_chapter_id)
            && m.source_chapter_id.as_deref() != Some(&o.new_chapter_id)
            && m.payoff_chapter_id.as_deref() != Some(&o.new_chapter_id)));
    let hist = s.chapter_history(&c.id).unwrap();
    assert_eq!(
        hist.iter().find(|v| v.revision == c.revision).unwrap().body,
        c.body
    );
}
#[test]
fn split_orders_new_chapter_immediately_after_source_without_touching_recycle() {
    let (mut s, a) = setup();
    let c = &a.chapters[0];
    let n = s
        .apply(Command::CreateChapter {
            volume_id: c.volume_id.clone(),
            title: "后一章".into(),
        })
        .unwrap();
    let other = n.chapters.iter().find(|x| x.id != c.id).unwrap().clone();
    s.apply(Command::CreateChapter {
        volume_id: c.volume_id.clone(),
        title: "回收章".into(),
    })
    .unwrap();
    let trash = s
        .load()
        .unwrap()
        .chapters
        .into_iter()
        .find(|x| x.title == "回收章")
        .unwrap();
    s.apply(Command::TrashChapter {
        id: trash.id.clone(),
    })
    .unwrap();
    let before = s
        .load()
        .unwrap()
        .chapters
        .into_iter()
        .find(|x| x.id == trash.id)
        .unwrap();
    let p = s
        .prepare_chapter_split(&a.books[0].id, &c.id, c.revision, 3, "拆出章")
        .unwrap();
    let o = s.commit_chapter_split(&p.id).unwrap();
    let n = s.load().unwrap();
    let mut active: Vec<_> = n
        .chapters
        .iter()
        .filter(|x| x.deleted_at.is_none())
        .collect();
    active.sort_by_key(|x| x.sort_order);
    assert_eq!(
        active.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(),
        vec![c.id.as_str(), o.new_chapter_id.as_str(), other.id.as_str()]
    );
    assert_eq!(
        n.chapters.iter().find(|x| x.id == trash.id).unwrap(),
        &before
    );
}
#[test]
fn truncated_preview_has_exact_full_detail_and_readonly_preparation() {
    let (mut s, a) = setup();
    let c = &a.chapters[0];
    let text = "汉🙂".repeat(2500);
    let n = s
        .apply(Command::SaveChapter {
            id: c.id.clone(),
            body: text.clone(),
            expected_revision: c.revision,
        })
        .unwrap();
    let c = n.chapters.iter().find(|x| x.id == c.id).unwrap();
    let before = s.backup().unwrap();
    let p = s
        .prepare_chapter_split(&a.books[0].id, &c.id, c.revision, 3750, "后")
        .unwrap();
    assert!(p.before_truncated && p.after_truncated);
    assert_eq!(p.before.chars().count(), 2000);
    let left = s.get_split_detail(&p.id, "before").unwrap();
    let right = s.get_split_detail(&p.id, "after").unwrap();
    assert_eq!(left + &right, text);
    assert_eq!(before, s.backup().unwrap());
    assert!(s.get_split_detail(&p.id, "unknown").is_err());
    s.discard_chapter_split("wrong");
    assert!(s.get_split_detail(&p.id, "before").is_ok());
    s.discard_chapter_split(&p.id);
    assert!(s.commit_chapter_split(&p.id).is_err());
}
#[test]
fn recycled_reserved_slots_remain_unique_and_library_stays_writable_and_restorable() {
    for recycled_position in [0usize, 1, 3] {
        let (mut s, a) = setup();
        let volume = a.chapters[0].volume_id.clone();
        for title in ["第二章", "第三章", "末章"] {
            s.apply(Command::CreateChapter {
                volume_id: volume.clone(),
                title: title.into(),
            })
            .unwrap();
        }
        let mut chapters = s.load().unwrap().chapters;
        chapters.sort_by_key(|c| c.sort_order);
        let recycled = chapters[recycled_position].clone();
        let source = chapters[if recycled_position == 0 { 1 } else { 0 }].clone();
        s.apply(Command::SaveChapter {
            id: source.id.clone(),
            body: "abcd".into(),
            expected_revision: source.revision,
        })
        .unwrap();
        s.apply(Command::TrashChapter {
            id: recycled.id.clone(),
        })
        .unwrap();
        let state = s.load().unwrap();
        let before = state
            .chapters
            .iter()
            .find(|c| c.id == recycled.id)
            .unwrap()
            .clone();
        let revision = state
            .chapters
            .iter()
            .find(|c| c.id == source.id)
            .unwrap()
            .revision;
        let p = s
            .prepare_chapter_split(&a.books[0].id, &source.id, revision, 2, "拆出章")
            .unwrap();
        let result = s.commit_chapter_split(&p.id).unwrap();
        let split = s.load().unwrap();
        let mut positions = std::collections::HashSet::new();
        assert!(
            split
                .chapters
                .iter()
                .all(|c| positions.insert(c.sort_order)),
            "recycled slot {recycled_position}"
        );
        assert_eq!(
            split.chapters.iter().find(|c| c.id == recycled.id).unwrap(),
            &before
        );
        let mut active: Vec<_> = split
            .chapters
            .iter()
            .filter(|c| c.deleted_at.is_none())
            .collect();
        active.sort_by_key(|c| c.sort_order);
        let index = active.iter().position(|c| c.id == source.id).unwrap();
        assert_eq!(active[index + 1].id, result.new_chapter_id);
        s.apply(Command::RenameChapter {
            id: source.id.clone(),
            title: "拆后仍可改名".into(),
        })
        .unwrap();
        s.apply(Command::CreateChapter {
            volume_id: volume,
            title: "拆后新增".into(),
        })
        .unwrap();
        s.apply(Command::RestoreChapter { id: recycled.id })
            .unwrap();
        let before_restore = s.load().unwrap();
        let backup = s.backup().unwrap();
        s.restore(&backup).unwrap();
        assert_eq!(s.load().unwrap(), before_restore);
    }
}
