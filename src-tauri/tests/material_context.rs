use novel_core::*;
use std::collections::BTreeMap;
fn setup() -> (Service, LibraryState) {
    let mut s = Service::open(":memory:").unwrap();
    let a = s
        .apply(Command::CreateBook {
            title: "来源测试书".into(),
            author: "".into(),
        })
        .unwrap();
    (s, a)
}
fn summary(a: &LibraryState) -> CreateMaterialInput {
    CreateMaterialInput {
        book_id: a.books[0].id.clone(),
        kind: "summary".into(),
        title: "章节摘要".into(),
        fields: BTreeMap::from([("content".into(), "虚构摘要".into())]),
        volume_id: None,
        chapter_id: None,
        source_chapter_id: Some(a.chapters[0].id.clone()),
        source_revision: Some(a.chapters[0].revision),
        story_order: 0,
        story_time: None,
        tracking_status: None,
        payoff_chapter_id: None,
        payoff_revision: None,
    }
}
fn edit(m: &Material) -> SaveMaterialInput {
    SaveMaterialInput {
        id: m.id.clone(),
        title: m.title.clone(),
        fields: m.fields.clone(),
        volume_id: m.volume_id.clone(),
        chapter_id: m.chapter_id.clone(),
        expected_revision: m.revision,
        source_chapter_id: m.source_chapter_id.clone(),
        source_revision: m.source_revision,
        story_order: m.story_order,
        story_time: m.story_time.clone(),
        tracking_status: m.tracking_status.clone(),
        payoff_chapter_id: m.payoff_chapter_id.clone(),
        payoff_revision: m.payoff_revision,
    }
}
#[test]
fn summary_source_freshness_changes_only_with_explicit_revision() {
    let (mut s, a) = setup();
    let m = s.create_material(summary(&a)).unwrap();
    assert_eq!(m.source_status, "current");
    s.apply(Command::SaveChapter {
        id: a.chapters[0].id.clone(),
        body: "新正文".into(),
        expected_revision: 0,
    })
    .unwrap();
    let m = s.get_material(&m.id, false).unwrap();
    assert_eq!(m.source_status, "stale");
    let mut update = edit(&m);
    update.title = "仅修改标题".into();
    let m = s.save_material(update).unwrap();
    assert_eq!(m.source_revision, Some(0));
    assert_eq!(m.source_status, "stale");
    let mut update = edit(&m);
    update.source_revision = Some(1);
    let m = s.save_material(update).unwrap();
    assert_eq!(m.source_status, "current");
    assert_eq!(m.source_revision, Some(1));
}
#[test]
fn recycled_and_purged_source_is_unavailable_without_losing_summary_or_source_id() {
    let (mut s, a) = setup();
    let m = s.create_material(summary(&a)).unwrap();
    s.apply(Command::TrashChapter {
        id: a.chapters[0].id.clone(),
    })
    .unwrap();
    assert_eq!(
        s.get_material(&m.id, false).unwrap().source_status,
        "unavailable"
    );
    assert_eq!(
        s.list_materials(&a.books[0].id, false).unwrap()[0].source_status,
        "unavailable"
    );
    s.apply(Command::PurgeChapter {
        id: a.chapters[0].id.clone(),
    })
    .unwrap();
    let m = s.get_material(&m.id, false).unwrap();
    assert_eq!(m.source_chapter_id, Some(a.chapters[0].id.clone()));
    assert_eq!(m.source_revision, Some(0));
    assert_eq!(m.fields["content"], "虚构摘要");
    assert_eq!(m.source_status, "unavailable");
    let m = s.save_material(edit(&m)).unwrap();
    assert_eq!(m.source_status, "unavailable");
    let backup = s.backup().unwrap();
    s.restore(&backup).unwrap();
    assert_eq!(
        s.get_material(&m.id, false).unwrap().source_status,
        "unavailable"
    );
}
#[test]
fn new_source_requires_same_active_book_and_current_revision() {
    let (mut s, a) = setup();
    let mut missing = summary(&a);
    missing.source_chapter_id = None;
    missing.source_revision = None;
    assert!(s.create_material(missing).is_err());
    let mut pair = summary(&a);
    pair.source_revision = None;
    assert!(s.create_material(pair).is_err());
    s.apply(Command::SaveChapter {
        id: a.chapters[0].id.clone(),
        body: "新".into(),
        expected_revision: 0,
    })
    .unwrap();
    assert!(s.create_material(summary(&a)).is_err());
    let b = s
        .apply(Command::CreateBook {
            title: "其他书".into(),
            author: "".into(),
        })
        .unwrap();
    let mut cross = summary(&a);
    cross.source_chapter_id = Some(
        b.chapters
            .iter()
            .find(|c| c.id != a.chapters[0].id)
            .unwrap()
            .id
            .clone(),
    );
    assert!(s.create_material(cross).is_err());
}
#[test]
fn v5_sources_roundtrip_legacy_v4_migrates_and_invalid_source_does_not_replace_library() {
    let (mut s, a) = setup();
    let m = s.create_material(summary(&a)).unwrap();
    let backup = s.backup().unwrap();
    let mut json: serde_json::Value = serde_json::from_str(&backup).unwrap();
    assert_eq!(json["version"], 13);
    let epoch = s.epoch().unwrap();
    json["materials"][0]["source_revision"] = serde_json::json!(99);
    assert!(s.restore(&json.to_string()).is_err());
    assert_eq!(s.epoch().unwrap(), epoch);
    assert_eq!(s.get_material(&m.id, false).unwrap(), m);
    s.restore(&backup).unwrap();
    assert_eq!(s.get_material(&m.id, false).unwrap(), m);
    let mut legacy: serde_json::Value = serde_json::from_str(&backup).unwrap();
    legacy["version"] = serde_json::json!(4);
    legacy.as_object_mut().unwrap().remove("foreshadow_proposals");
    legacy["materials"][0]["kind"] = serde_json::json!("outline");
    for field in ["source_chapter_id", "source_revision", "source_status"] {
        legacy["materials"][0]
            .as_object_mut()
            .unwrap()
            .remove(field);
    }
    s.restore(&legacy.to_string()).unwrap();
    let m = s.get_material(&m.id, false).unwrap();
    assert_eq!(m.source_status, "none");
    assert!(m.source_chapter_id.is_none() && m.source_revision.is_none());
}
#[test]
fn chapter_proposal_acceptance_and_history_restore_also_make_summary_stale() {
    let (mut s, a) = setup();
    let m = s.create_material(summary(&a)).unwrap();
    let p = s
        .propose_edit(ProposalInput {
            client_id: "fixture".into(),
            idempotency_key: "source".into(),
            chapter_id: a.chapters[0].id.clone(),
            base_revision: 0,
            new_body: "采纳的新正文".into(),
            reason: "虚构".into(),
        })
        .unwrap();
    s.decide_proposal(&p.id, true, 0).unwrap();
    assert_eq!(s.get_material(&m.id, false).unwrap().source_status, "stale");
    let m = s.get_material(&m.id, false).unwrap();
    let mut update = edit(&m);
    update.source_revision = Some(1);
    s.save_material(update).unwrap();
    let baseline = s
        .chapter_history(&a.chapters[0].id)
        .unwrap()
        .into_iter()
        .find(|v| v.revision == 0)
        .unwrap();
    s.restore_chapter_version(&baseline.id, 1).unwrap();
    assert_eq!(s.get_material(&m.id, false).unwrap().source_status, "stale");
}
#[test]
fn existing_v4_database_columns_migrate_without_changing_card_data() {
    let dir =
        std::env::temp_dir().join(format!("source-migration-fixture-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("legacy.sqlite");
    let mut s = Service::open(&path).unwrap();
    let a = s
        .apply(Command::CreateBook {
            title: "旧库".into(),
            author: "".into(),
        })
        .unwrap();
    let mut create = summary(&a);
    create.kind = "outline".into();
    create.source_chapter_id = None;
    create.source_revision = None;
    let m = s.create_material(create).unwrap();
    drop(s);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("ALTER TABLE materials DROP COLUMN source_chapter_id;ALTER TABLE materials DROP COLUMN source_revision;").unwrap();
    drop(conn);
    let s = Service::open(&path).unwrap();
    assert_eq!(s.get_material(&m.id, false).unwrap(), m);
    let snapshot = s.recovery_snapshot(dir.join("snapshot.sqlite")).unwrap();
    let recovered = Service::open(snapshot).unwrap();
    assert_eq!(recovered.get_material(&m.id, false).unwrap(), m);
    drop(recovered);
    drop(s);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn v5_does_not_trust_source_status_and_rejects_invalid_pairs_or_cross_book_sources() {
    let (mut s, a) = setup();
    let m = s.create_material(summary(&a)).unwrap();
    let other = s
        .apply(Command::CreateBook {
            title: "别书".into(),
            author: "".into(),
        })
        .unwrap();
    let other_chapter = other
        .chapters
        .iter()
        .find(|c| c.id != a.chapters[0].id)
        .unwrap()
        .id
        .clone();
    let before = s.backup().unwrap();
    let epoch = s.epoch().unwrap();
    for (field, value) in [
        ("source_revision", serde_json::Value::Null),
        ("source_revision", serde_json::json!(-1)),
        ("source_chapter_id", serde_json::json!("")),
        ("source_chapter_id", serde_json::json!(other_chapter)),
    ] {
        let mut json: serde_json::Value = serde_json::from_str(&before).unwrap();
        json["materials"][0][field] = value;
        assert!(s.restore(&json.to_string()).is_err());
        assert_eq!(s.backup().unwrap(), before);
        assert_eq!(s.epoch().unwrap(), epoch);
    }
    s.apply(Command::TrashChapter {
        id: a.chapters[0].id.clone(),
    })
    .unwrap();
    s.apply(Command::PurgeChapter {
        id: a.chapters[0].id.clone(),
    })
    .unwrap();
    let mut orphan: serde_json::Value = serde_json::from_str(&s.backup().unwrap()).unwrap();
    orphan["materials"][0]["source_status"] = serde_json::json!("current");
    s.restore(&orphan.to_string()).unwrap();
    assert_eq!(
        s.get_material(&m.id, false).unwrap().source_status,
        "unavailable"
    );
    let mut clear = edit(&s.get_material(&m.id, false).unwrap());
    clear.source_chapter_id = None;
    clear.source_revision = None;
    assert!(s.save_material(clear).is_err());
}
#[test]
fn sqlite_snapshot_preserves_recorded_source_while_freshness_is_derived() {
    let dir =
        std::env::temp_dir().join(format!("source-snapshot-fixture-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let (mut s, a) = setup();
    let m = s.create_material(summary(&a)).unwrap();
    s.apply(Command::SaveChapter {
        id: a.chapters[0].id.clone(),
        body: "快照前新稿".into(),
        expected_revision: 0,
    })
    .unwrap();
    let path = s.recovery_snapshot(dir.join("snapshot.sqlite")).unwrap();
    let recovered = Service::open(path).unwrap();
    let snapshot_card = recovered.get_material(&m.id, false).unwrap();
    assert_eq!(snapshot_card.source_chapter_id, m.source_chapter_id);
    assert_eq!(snapshot_card.source_revision, Some(0));
    assert_eq!(snapshot_card.source_status, "stale");
    drop(recovered);
    std::fs::remove_dir_all(dir).unwrap();
}
