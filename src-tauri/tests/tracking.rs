use novel_core::*;
use std::collections::BTreeMap;
fn setup() -> (Service, LibraryState) {
    let mut s = Service::open(":memory:").unwrap();
    let a = s
        .apply(Command::CreateBook {
            title: "时间线虚构书".into(),
            author: "".into(),
        })
        .unwrap();
    (s, a)
}
fn draft(a: &LibraryState, kind: &str, status: &str) -> CreateMaterialInput {
    CreateMaterialInput {
        book_id: a.books[0].id.clone(),
        kind: kind.into(),
        title: "虚构跟踪卡".into(),
        fields: if kind == "timeline" {
            BTreeMap::from([("content".into(), "事件说明".into())])
        } else {
            BTreeMap::from([
                ("setup".into(), "铺设文字".into()),
                ("resolution".into(), "回收文字".into()),
            ])
        },
        volume_id: None,
        chapter_id: None,
        source_chapter_id: None,
        source_revision: None,
        story_order: 10,
        story_time: Some("第三日黎明".into()),
        tracking_status: Some(status.into()),
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
fn planned_timeline_metadata_requires_valid_order_state_and_bounded_time() {
    let (mut s, a) = setup();
    let m = s.create_material(draft(&a, "timeline", "planned")).unwrap();
    assert_eq!(m.source_status, "none");
    assert_eq!(m.payoff_status, "none");
    let rows = s.list_materials(&a.books[0].id, false).unwrap();
    assert_eq!(rows[0].story_order, 10);
    assert_eq!(rows[0].tracking_status.as_deref(), Some("planned"));
    assert!(serde_json::to_value(rows).unwrap()[0]
        .get("fields")
        .is_none());
    for mut invalid in [
        draft(&a, "timeline", "unknown"),
        draft(&a, "timeline", "written"),
        draft(&a, "foreshadow", "planted"),
        draft(&a, "foreshadow", "resolved"),
    ] {
        assert!(s.create_material(invalid.clone()).is_err());
        invalid.story_order = -1;
        assert!(s.create_material(invalid).is_err());
    }
    let mut invalid = draft(&a, "timeline", "planned");
    invalid.story_order = 1_000_000_001;
    assert!(s.create_material(invalid).is_err());
    let mut invalid = draft(&a, "timeline", "planned");
    invalid.story_time = Some("字".repeat(86));
    assert!(s.create_material(invalid).is_err());
}
#[test]
fn lifecycle_transitions_require_fresh_sources_but_existing_stale_state_remains_editable() {
    let (mut s, a) = setup();
    let mut create = draft(&a, "timeline", "planned");
    create.source_chapter_id = Some(a.chapters[0].id.clone());
    create.source_revision = Some(0);
    let m = s.create_material(create).unwrap();
    s.apply(Command::SaveChapter {
        id: a.chapters[0].id.clone(),
        body: "已更改事件正文".into(),
        expected_revision: 0,
    })
    .unwrap();
    let mut update = edit(&m);
    update.tracking_status = Some("written".into());
    assert!(s.save_material(update).is_err());
    let mut update = edit(&m);
    update.source_revision = Some(1);
    update.tracking_status = Some("written".into());
    let written = s.save_material(update).unwrap();
    assert_eq!(written.tracking_status.as_deref(), Some("written"));
    s.apply(Command::SaveChapter {
        id: a.chapters[0].id.clone(),
        body: "再次更改".into(),
        expected_revision: 1,
    })
    .unwrap();
    let mut update = edit(&written);
    update.story_order = 99;
    let stale = s.save_material(update).unwrap();
    assert_eq!(stale.source_status, "stale");
    assert_eq!(stale.tracking_status.as_deref(), Some("written"));
    assert_eq!(stale.source_revision, Some(1));
}
#[test]
fn foreshadow_resolution_requires_real_payoff_and_preserves_both_refs_when_unavailable() {
    let (mut s, a) = setup();
    let mut create = draft(&a, "foreshadow", "resolved");
    create.source_chapter_id = Some(a.chapters[0].id.clone());
    create.source_revision = Some(0);
    assert!(s.create_material(create.clone()).is_err());
    create.payoff_chapter_id = Some(a.chapters[0].id.clone());
    create.payoff_revision = Some(0);
    let m = s.create_material(create).unwrap();
    assert_eq!(m.payoff_status, "current");
    s.apply(Command::SaveChapter {
        id: a.chapters[0].id.clone(),
        body: "正文变更".into(),
        expected_revision: 0,
    })
    .unwrap();
    let stale = s.get_material(&m.id, false).unwrap();
    assert_eq!(stale.source_status, "stale");
    assert_eq!(stale.payoff_status, "stale");
    let mut update = edit(&stale);
    update.title = "保留已回收但需复核".into();
    s.save_material(update).unwrap();
    s.apply(Command::TrashChapter {
        id: a.chapters[0].id.clone(),
    })
    .unwrap();
    s.apply(Command::PurgeChapter {
        id: a.chapters[0].id.clone(),
    })
    .unwrap();
    let missing = s.get_material(&m.id, false).unwrap();
    assert_eq!(missing.tracking_status.as_deref(), Some("resolved"));
    assert_eq!(missing.payoff_chapter_id, Some(a.chapters[0].id.clone()));
    assert_eq!(missing.source_chapter_id, Some(a.chapters[0].id.clone()));
    assert_eq!(missing.source_status, "unavailable");
    assert_eq!(missing.payoff_status, "unavailable");
    assert_eq!(missing.fields["resolution"], "回收文字");
    s.save_material(edit(&missing)).unwrap();
}
#[test]
fn existing_kinds_cannot_smuggle_tracking_and_payoff_metadata() {
    let (mut s, a) = setup();
    let mut create = draft(&a, "timeline", "planned");
    create.kind = "outline".into();
    assert!(s.create_material(create.clone()).is_err());
    create.story_order = 0;
    create.story_time = None;
    create.tracking_status = None;
    create.payoff_chapter_id = Some(a.chapters[0].id.clone());
    create.payoff_revision = Some(0);
    assert!(s.create_material(create).is_err());
}
#[test]
fn v6_tracker_backup_rejects_invalid_state_refs_and_preserves_unavailable_refs() {
    let (mut s, a) = setup();
    let mut create = draft(&a, "foreshadow", "resolved");
    create.source_chapter_id = Some(a.chapters[0].id.clone());
    create.source_revision = Some(0);
    create.payoff_chapter_id = Some(a.chapters[0].id.clone());
    create.payoff_revision = Some(0);
    let m = s.create_material(create).unwrap();
    let before = s.backup().unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&before).unwrap()["version"],
        13
    );
    let epoch = s.epoch().unwrap();
    for (field, value) in [
        ("story_order", serde_json::json!(-1)),
        ("tracking_status", serde_json::json!("unknown")),
        ("payoff_revision", serde_json::json!(99)),
        ("payoff_revision", serde_json::Value::Null),
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
    let snapshot = s.backup().unwrap();
    s.restore(&snapshot).unwrap();
    let m = s.get_material(&m.id, false).unwrap();
    assert_eq!(m.payoff_status, "unavailable");
    assert_eq!(m.tracking_status.as_deref(), Some("resolved"));
}
#[test]
fn distinct_payoff_changes_and_status_transitions_never_certify_stale_anchors() {
    let (mut s, a) = setup();
    let all = s
        .apply(Command::CreateChapter {
            volume_id: a.volumes[0].id.clone(),
            title: "回收章".into(),
        })
        .unwrap();
    let payoff = all
        .chapters
        .iter()
        .find(|c| c.id != a.chapters[0].id)
        .unwrap()
        .id
        .clone();
    let mut create = draft(&a, "foreshadow", "planted");
    create.source_chapter_id = Some(a.chapters[0].id.clone());
    create.source_revision = Some(0);
    create.payoff_chapter_id = Some(payoff.clone());
    create.payoff_revision = Some(0);
    let m = s.create_material(create).unwrap();
    s.apply(Command::SaveChapter {
        id: payoff.clone(),
        body: "回收章新稿".into(),
        expected_revision: 0,
    })
    .unwrap();
    let stale = s.get_material(&m.id, false).unwrap();
    assert_eq!(stale.source_status, "current");
    assert_eq!(stale.payoff_status, "stale");
    let mut resolve = edit(&stale);
    resolve.tracking_status = Some("resolved".into());
    assert!(s.save_material(resolve).is_err());
    let mut resolve = edit(&stale);
    resolve.tracking_status = Some("resolved".into());
    resolve.payoff_revision = Some(1);
    let resolved = s.save_material(resolve).unwrap();
    assert_eq!(resolved.payoff_status, "current");
    let mut erase = edit(&resolved);
    erase.fields.insert("resolution".into(), "  ".into());
    assert!(s.save_material(erase).is_err());
    s.apply(Command::TrashVolume {
        id: a.volumes[0].id.clone(),
    })
    .unwrap();
    let unavailable = s.get_material(&m.id, false).unwrap();
    assert_eq!(unavailable.payoff_status, "unavailable");
    assert_eq!(s.list_materials(&a.books[0].id, false).unwrap().len(), 1);
    let mut move_only = edit(&unavailable);
    move_only.story_order = 20;
    s.save_material(move_only).unwrap();
}
#[test]
fn v5_database_and_json_preserve_old_sources_with_default_tracking_metadata() {
    let dir = std::env::temp_dir().join(format!("tracking-v5-fixture-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("library.sqlite");
    let mut s = Service::open(&path).unwrap();
    let a = s
        .apply(Command::CreateBook {
            title: "旧版来源".into(),
            author: "".into(),
        })
        .unwrap();
    let mut create = draft(&a, "timeline", "planned");
    create.kind = "summary".into();
    create.story_order = 0;
    create.story_time = None;
    create.tracking_status = None;
    create.source_chapter_id = Some(a.chapters[0].id.clone());
    create.source_revision = Some(0);
    let m = s.create_material(create).unwrap();
    let mut legacy: serde_json::Value = serde_json::from_str(&s.backup().unwrap()).unwrap();
    legacy["version"] = serde_json::json!(5);
    legacy.as_object_mut().unwrap().remove("foreshadow_proposals");
    for key in [
        "story_order",
        "story_time",
        "tracking_status",
        "payoff_chapter_id",
        "payoff_revision",
        "payoff_status",
    ] {
        legacy["materials"][0].as_object_mut().unwrap().remove(key);
    }
    s.restore(&legacy.to_string()).unwrap();
    assert_eq!(s.get_material(&m.id, false).unwrap(), m);
    drop(s);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("ALTER TABLE materials DROP COLUMN story_order;ALTER TABLE materials DROP COLUMN story_time;ALTER TABLE materials DROP COLUMN tracking_status;ALTER TABLE materials DROP COLUMN payoff_chapter_id;ALTER TABLE materials DROP COLUMN payoff_revision;").unwrap();
    drop(conn);
    let s = Service::open(&path).unwrap();
    assert_eq!(s.get_material(&m.id, false).unwrap(), m);
    let copy = s.recovery_snapshot(dir.join("recovery.sqlite")).unwrap();
    let recovered = Service::open(copy).unwrap();
    assert_eq!(recovered.get_material(&m.id, false).unwrap(), m);
    drop(recovered);
    drop(s);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn tracker_backup_scope_and_snapshot_preserve_unavailable_payoff_and_recycle_state() {
    let (mut s, a) = setup();
    let mut create = draft(&a, "foreshadow", "resolved");
    create.source_chapter_id = Some(a.chapters[0].id.clone());
    create.source_revision = Some(0);
    create.payoff_chapter_id = Some(a.chapters[0].id.clone());
    create.payoff_revision = Some(0);
    let m = s.create_material(create).unwrap();
    let all = s
        .apply(Command::CreateBook {
            title: "其他书".into(),
            author: "".into(),
        })
        .unwrap();
    let other = all
        .chapters
        .iter()
        .find(|c| c.id != a.chapters[0].id)
        .unwrap()
        .id
        .clone();
    let before = s.backup().unwrap();
    let mut invalid: serde_json::Value = serde_json::from_str(&before).unwrap();
    invalid["materials"][0]["payoff_chapter_id"] = serde_json::json!(other);
    assert!(s.restore(&invalid.to_string()).is_err());
    assert_eq!(s.backup().unwrap(), before);
    s.apply(Command::TrashChapter {
        id: a.chapters[0].id.clone(),
    })
    .unwrap();
    s.apply(Command::PurgeChapter {
        id: a.chapters[0].id.clone(),
    })
    .unwrap();
    let m = s.trash_material(&m.id, 0).unwrap();
    assert_eq!(s.get_material(&m.id, false).unwrap_err().code, "deleted");
    let path = std::env::temp_dir().join(format!(
        "tracking-snapshot-fixture-{}.sqlite",
        uuid::Uuid::new_v4()
    ));
    s.recovery_snapshot(&path).unwrap();
    let recovered = Service::open(&path).unwrap();
    assert_eq!(recovered.get_material(&m.id, true).unwrap(), m);
    drop(recovered);
    std::fs::remove_file(&path).unwrap();
    let lock = std::path::PathBuf::from(format!("{}.writer.lock", path.display()));
    std::fs::remove_file(lock).unwrap();
    let restored = s.restore_material(&m.id, 1).unwrap();
    assert_eq!(restored.tracking_status.as_deref(), Some("resolved"));
    assert_eq!(restored.payoff_status, "unavailable");
}
