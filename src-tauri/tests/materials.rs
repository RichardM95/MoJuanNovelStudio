use novel_core::*;
use std::{collections::BTreeMap, path::PathBuf};
struct Fixture {
    dir: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let dir =
            std::env::temp_dir().join(format!("mojuan-materials-fixture-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        Self { dir }
    }
    fn db(&self) -> PathBuf {
        self.dir.join("library.sqlite")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
fn setup(f: &Fixture) -> (Service, LibraryState) {
    let mut s = Service::open(f.db()).unwrap();
    let a = s
        .apply(Command::CreateBook {
            title: "虚构资料书".into(),
            author: "隔离测试".into(),
        })
        .unwrap();
    (s, a)
}
fn input(b: &str, kind: &str) -> CreateMaterialInput {
    CreateMaterialInput {
        book_id: b.into(),
        kind: kind.into(),
        title: "资料卡".into(),
        fields: BTreeMap::from([(
            if kind == "outline" {
                "content"
            } else if kind == "character" {
                "identity"
            } else {
                "synopsis"
            }
            .into(),
            "仅测试文本".into(),
        )]),
        volume_id: None,
        chapter_id: None,
        source_chapter_id: None,
        source_revision: None,
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
fn crud_revision_soft_delete_and_reopen() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = &a.books[0].id;
    for kind in ["setting", "character", "outline"] {
        let m = s.create_material(input(b, kind)).unwrap();
        assert_eq!(m.revision, 0);
        assert_eq!(s.get_material(&m.id, false).unwrap(), m);
    }
    let summaries = s.list_materials(b, false).unwrap();
    assert_eq!(summaries.len(), 3);
    assert!(serde_json::to_value(&summaries).unwrap()[0]
        .get("fields")
        .is_none());
    let m = s.get_material(&summaries[0].id, false).unwrap();
    let mut next = edit(&m);
    next.title = "改过标题".into();
    let saved = s.save_material(next).unwrap();
    assert_eq!(saved.revision, 1);
    assert_eq!(
        s.save_material(edit(&m)).unwrap_err().code,
        "revision_conflict"
    );
    let deleted = s.trash_material(&saved.id, 1).unwrap();
    assert_eq!(deleted.revision, 2);
    assert!(deleted.deleted_at.is_some());
    assert_eq!(
        s.get_material(&saved.id, false).unwrap_err().code,
        "deleted"
    );
    assert_eq!(s.list_materials(b, false).unwrap().len(), 2);
    assert_eq!(s.list_materials(b, true).unwrap().len(), 3);
    assert!(s.save_material(edit(&deleted)).is_err());
    let restored = s.restore_material(&saved.id, 2).unwrap();
    assert_eq!(restored.revision, 3);
    assert!(restored.deleted_at.is_none());
    drop(s);
    let s = Service::open(f.db()).unwrap();
    assert_eq!(s.get_material(&saved.id, false).unwrap(), restored);
}
#[test]
fn target_scope_recycle_and_purge_detaches_without_text_loss() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = &a.books[0].id;
    let v = &a.volumes[0].id;
    let c = &a.chapters[0].id;
    let mut create = input(b, "outline");
    create.chapter_id = Some(c.clone());
    let m = s.create_material(create).unwrap();
    s.apply(Command::TrashVolume { id: v.clone() }).unwrap();
    assert!(s.list_materials(b, false).unwrap().is_empty());
    assert_eq!(s.list_materials(b, true).unwrap().len(), 1);
    assert_eq!(s.get_material(&m.id, true).unwrap(), m);
    assert_eq!(s.get_material(&m.id, false).unwrap_err().code, "deleted");
    assert!(s.save_material(edit(&m)).is_err());
    s.apply(Command::PurgeVolume { id: v.clone() }).unwrap();
    let detached = s.get_material(&m.id, false).unwrap();
    assert!(detached.chapter_id.is_none() && detached.volume_id.is_none());
    assert_eq!(detached.fields, m.fields);
    s.apply(Command::TrashBook { id: b.clone() }).unwrap();
    assert!(s.get_material(&m.id, true).is_err());
    assert!(s.list_materials(b, true).is_err());
    s.apply(Command::PurgeBook { id: b.clone() }).unwrap();
    assert_eq!(s.get_material(&m.id, true).unwrap_err().code, "not_found");
}
#[test]
fn invalid_kind_fields_scope_and_duplicate_target_never_write() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = &a.books[0].id;
    let other = s
        .apply(Command::CreateBook {
            title: "另一本虚构书".into(),
            author: "".into(),
        })
        .unwrap();
    let other_volume = other
        .volumes
        .iter()
        .find(|v| v.book_id != *b)
        .unwrap()
        .id
        .clone();
    let mut invalids = vec![];
    let mut x = input(b, "unknown");
    x.fields.clear();
    invalids.push(x);
    let mut x = input(b, "setting");
    x.fields.insert("unknown".into(), "x".into());
    invalids.push(x);
    let mut x = input(b, "outline");
    x.volume_id = Some(other_volume.clone());
    invalids.push(x);
    let mut x = input(b, "outline");
    x.volume_id = Some(a.volumes[0].id.clone());
    x.chapter_id = Some(a.chapters[0].id.clone());
    invalids.push(x);
    let mut x = input(b, "character");
    x.volume_id = Some(a.volumes[0].id.clone());
    invalids.push(x);
    for x in invalids {
        assert!(s.create_material(x).is_err());
        assert!(s.list_materials(b, true).unwrap().is_empty());
    }
    assert!(s
        .apply(Command::TransferChapter {
            id: a.chapters[0].id.clone(),
            volume_id: other_volume
        })
        .is_err());
}
#[test]
fn v5_roundtrip_and_v1_v2_v3_restore_clear_materials() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = &a.books[0].id;
    let m = s.create_material(input(b, "setting")).unwrap();
    s.trash_material(&m.id, 0).unwrap();
    s.apply(Command::SaveChapter {
        id: a.chapters[0].id.clone(),
        body: "正文历史".into(),
        expected_revision: 0,
    })
    .unwrap();
    let backup = s.backup().unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&backup).unwrap()["version"],
        13
    );
    s.restore(&backup).unwrap();
    assert_eq!(s.get_material(&m.id, true).unwrap().revision, 1);
    assert_eq!(s.chapter_history(&a.chapters[0].id).unwrap().len(), 2);
    for version in 1..=3 {
        let mut old: serde_json::Value = serde_json::from_str(&backup).unwrap();
        old["version"] = serde_json::json!(version);
        old.as_object_mut().unwrap().remove("foreshadow_proposals");
        old.as_object_mut().unwrap().remove("materials");
        old.as_object_mut().unwrap().remove("material_content_history");
        s.restore(&old.to_string()).unwrap();
        assert!(s.list_materials(b, true).unwrap().is_empty());
        if version == 3 {
            assert_eq!(s.chapter_history(&a.chapters[0].id).unwrap().len(), 2);
        }
        s.restore(&backup).unwrap();
    }
}
#[test]
fn invalid_v5_materials_are_rejected_before_any_library_or_epoch_change() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = &a.books[0].id;
    let m = s.create_material(input(b, "outline")).unwrap();
    let backup = s.backup().unwrap();
    let original: serde_json::Value = serde_json::from_str(&backup).unwrap();
    let epoch = s.epoch().unwrap();
    let mut cases = vec![];
    let mut x = original.clone();
    x.as_object_mut().unwrap().remove("materials");
    cases.push(x);
    let mut x = original.clone();
    let duplicate = x["materials"][0].clone();
    x["materials"].as_array_mut().unwrap().push(duplicate);
    cases.push(x);
    for (field, value) in [
        ("book_id", serde_json::json!("missing")),
        ("chapter_id", serde_json::json!("missing")),
        ("kind", serde_json::json!("unknown")),
        ("fields", serde_json::json!({"unknown":"x"})),
        ("revision", serde_json::json!(-1)),
    ] {
        let mut x = original.clone();
        x["materials"][0][field] = value;
        cases.push(x);
    }
    for x in cases {
        assert!(s.restore(&x.to_string()).is_err());
        assert_eq!(s.backup().unwrap(), backup);
        assert_eq!(s.epoch().unwrap(), epoch);
        assert_eq!(s.get_material(&m.id, true).unwrap(), m);
    }
}
#[test]
fn existing_database_without_materials_migrates_and_snapshot_contains_cards() {
    let f = Fixture::new();
    let (s, a) = setup(&f);
    let original = s.load().unwrap();
    drop(s);
    let conn = rusqlite::Connection::open(f.db()).unwrap();
    conn.execute_batch("DROP TABLE IF EXISTS materials;")
        .unwrap();
    drop(conn);
    let mut s = Service::open(f.db()).unwrap();
    assert_eq!(s.load().unwrap(), original);
    let m = s
        .create_material(input(&a.books[0].id, "character"))
        .unwrap();
    let path = s.recovery_snapshot(f.dir.join("snapshot.sqlite")).unwrap();
    let recovered = Service::open(path).unwrap();
    assert_eq!(recovered.get_material(&m.id, false).unwrap(), m);
}
#[test]
fn field_and_card_quotas_count_recycled_cards_and_do_not_change_existing_data() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = &a.books[0].id;
    let mut card = input(b, "outline");
    card.fields.insert("content".into(), "x".repeat(256 * 1024));
    let m = s.create_material(card).unwrap();
    let mut too_big = edit(&m);
    too_big
        .fields
        .insert("content".into(), "字".repeat(256 * 1024 / 3 + 1));
    assert_eq!(s.save_material(too_big).unwrap_err().code, "limit_exceeded");
    assert_eq!(s.get_material(&m.id, false).unwrap(), m);
    for n in 1..1000 {
        let mut card = input(b, "outline");
        card.title = format!("空资料{n}");
        card.fields.clear();
        s.create_material(card).unwrap();
    }
    s.trash_material(&m.id, 0).unwrap();
    assert_eq!(
        s.create_material(input(b, "outline")).unwrap_err().code,
        "limit_exceeded"
    );
    assert_eq!(s.list_materials(b, true).unwrap().len(), 1000);
}
#[test]
fn global_fields_quota_releases_only_replaced_data_not_recycled_data() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = &a.books[0].id;
    let mut first = None;
    for _ in 0..128 {
        let mut card = input(b, "outline");
        card.fields.insert("content".into(), "x".repeat(256 * 1024));
        let m = s.create_material(card).unwrap();
        if first.is_none() {
            first = Some(m)
        }
    }
    let first = first.unwrap();
    s.trash_material(&first.id, 0).unwrap();
    assert_eq!(
        s.create_material(input(b, "setting")).unwrap_err().code,
        "limit_exceeded"
    );
    let m = s.restore_material(&first.id, 1).unwrap();
    let mut smaller = edit(&m);
    smaller.fields.clear();
    s.save_material(smaller).unwrap();
    s.create_material(input(b, "setting")).unwrap();
    assert_eq!(s.list_materials(b, true).unwrap().len(), 129);
}
#[test]
fn material_restore_requires_active_target_parent_and_cas() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let mut create = input(&a.books[0].id, "outline");
    create.volume_id = Some(a.volumes[0].id.clone());
    let m = s.create_material(create).unwrap();
    s.trash_material(&m.id, 0).unwrap();
    s.apply(Command::TrashVolume {
        id: a.volumes[0].id.clone(),
    })
    .unwrap();
    assert!(s.restore_material(&m.id, 1).is_err());
    assert_eq!(s.get_material(&m.id, true).unwrap().revision, 1);
    s.apply(Command::RestoreVolume {
        id: a.volumes[0].id.clone(),
    })
    .unwrap();
    assert_eq!(
        s.restore_material(&m.id, 0).unwrap_err().code,
        "revision_conflict"
    );
    let m = s.restore_material(&m.id, 1).unwrap();
    assert_eq!(m.revision, 2);
    s.apply(Command::TrashBook {
        id: a.books[0].id.clone(),
    })
    .unwrap();
    assert!(s.create_material(input(&a.books[0].id, "setting")).is_err());
    assert!(s.save_material(edit(&m)).is_err());
}
#[test]
fn failed_material_sql_write_rolls_back_card_and_revision() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let m = s.create_material(input(&a.books[0].id, "setting")).unwrap();
    let conn = rusqlite::Connection::open(f.db()).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_material_update BEFORE UPDATE ON materials BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
    let mut update = edit(&m);
    update.title = "不能生效".into();
    assert!(s.save_material(update).is_err());
    assert_eq!(s.get_material(&m.id, false).unwrap(), m);
    assert!(s.trash_material(&m.id, 0).is_err());
    assert_eq!(s.get_material(&m.id, false).unwrap(), m);
}
#[test]
fn permanent_target_deletion_bumps_card_revision_and_rejects_old_draft() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let mut create = input(&a.books[0].id, "outline");
    create.chapter_id = Some(a.chapters[0].id.clone());
    let original = s.create_material(create).unwrap();
    s.apply(Command::TrashChapter {
        id: a.chapters[0].id.clone(),
    })
    .unwrap();
    s.apply(Command::PurgeChapter {
        id: a.chapters[0].id.clone(),
    })
    .unwrap();
    let detached = s.get_material(&original.id, false).unwrap();
    assert_eq!(detached.revision, original.revision + 1);
    assert!(detached.updated_at > original.updated_at);
    assert_eq!(detached.fields, original.fields);
    assert_eq!(
        s.save_material(edit(&original)).unwrap_err().code,
        "revision_conflict"
    );
}
#[test]
fn target_detach_revision_overflow_rejects_purge_without_partial_changes() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let mut create = input(&a.books[0].id, "outline");
    create.volume_id = Some(a.volumes[0].id.clone());
    let material = s.create_material(create).unwrap();
    let conn = rusqlite::Connection::open(f.db()).unwrap();
    conn.execute(
        "UPDATE materials SET revision=?1 WHERE id=?2",
        rusqlite::params![i64::MAX, material.id],
    )
    .unwrap();
    s.apply(Command::TrashVolume {
        id: a.volumes[0].id.clone(),
    })
    .unwrap();
    let state = s.load().unwrap();
    let m = s.get_material(&material.id, true).unwrap();
    assert!(s
        .apply(Command::PurgeVolume {
            id: a.volumes[0].id.clone()
        })
        .is_err());
    assert_eq!(s.load().unwrap(), state);
    assert_eq!(s.get_material(&material.id, true).unwrap(), m);
}
#[test]
fn strict_v5_rejects_cross_book_oversized_and_excess_card_payloads_atomically() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = &a.books[0].id;
    let m = s.create_material(input(b, "outline")).unwrap();
    let state = s
        .apply(Command::CreateBook {
            title: "第二本测试书".into(),
            author: "".into(),
        })
        .unwrap();
    let other_volume = state
        .volumes
        .iter()
        .find(|v| v.book_id != *b)
        .unwrap()
        .id
        .clone();
    let before = s.backup().unwrap();
    let epoch = s.epoch().unwrap();
    let base: serde_json::Value = serde_json::from_str(&before).unwrap();
    let mut cases = vec![];
    let mut x = base.clone();
    x["materials"][0]["volume_id"] = serde_json::json!(other_volume);
    cases.push(x);
    let mut x = base.clone();
    x["materials"][0]["id"] = serde_json::json!(a.chapters[0].id);
    cases.push(x);
    let mut x = base.clone();
    x["materials"][0]["fields"] = serde_json::json!({"content":"x".repeat(256*1024+1)});
    cases.push(x);
    let mut x = base.clone();
    x["materials"][0].as_object_mut().unwrap().remove("fields");
    cases.push(x);
    let mut x = base.clone();
    x["materials"] = serde_json::Value::Null;
    cases.push(x);
    let mut x = base.clone();
    let prototype = x["materials"][0].clone();
    x["materials"] = serde_json::json!((0..1001)
        .map(|n| {
            let mut item = prototype.clone();
            item["id"] = serde_json::json!(format!("too-many-{n}"));
            item
        })
        .collect::<Vec<_>>());
    cases.push(x);
    for x in cases {
        assert!(s.restore(&x.to_string()).is_err());
        assert_eq!(s.backup().unwrap(), before);
        assert_eq!(s.epoch().unwrap(), epoch);
    }
    let mut oversized = base.clone();
    let prototype = oversized["materials"][0].clone();
    oversized["materials"] = serde_json::json!((0..129)
        .map(|n| {
            let mut item = prototype.clone();
            item["id"] = serde_json::json!(format!("too-large-{n}"));
            item["fields"] = serde_json::json!({"content":"x".repeat(256*1024)});
            item
        })
        .collect::<Vec<_>>());
    assert!(s.restore(&oversized.to_string()).is_err());
    assert_eq!(s.backup().unwrap(), before);
    assert_eq!(s.epoch().unwrap(), epoch);
    assert_eq!(s.get_material(&m.id, false).unwrap(), m);
}
#[test]
fn tree_replacement_clears_old_target_fks_before_inserting_new_materials() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let mut create = input(&a.books[0].id, "outline");
    create.chapter_id = Some(a.chapters[0].id.clone());
    let old = s.create_material(create).unwrap();
    let mut other = Service::open(":memory:").unwrap();
    let b = other
        .apply(Command::CreateBook {
            title: "替换测试库".into(),
            author: "".into(),
        })
        .unwrap();
    let new = other
        .create_material(input(&b.books[0].id, "character"))
        .unwrap();
    let backup = other.backup().unwrap();
    s.restore(&backup).unwrap();
    assert_eq!(s.get_material(&old.id, true).unwrap_err().code, "not_found");
    assert_eq!(s.get_material(&new.id, false).unwrap(), new);
    assert_eq!(s.backup().unwrap(), backup);
}
