use novel_core::*;
use std::collections::BTreeMap;

fn fixture() -> (Service, String, String, String) {
    fixture_in(Service::open(":memory:").unwrap())
}
fn fixture_in(mut s: Service) -> (Service, String, String, String) {
    let a = s
        .apply(Command::CreateBook {
            title: "虚构谜案".into(),
            author: "".into(),
        })
        .unwrap();
    let (book, chapter) = (a.books[0].id.clone(), a.chapters[0].id.clone());
    s.apply(Command::SaveChapter {
        id: chapter.clone(),
        body: "甲😀\r\n线索乙".into(),
        expected_revision: 0,
    })
    .unwrap();
    let card = s
        .create_material(CreateMaterialInput {
            book_id: book.clone(),
            kind: "foreshadow".into(),
            title: "缺口".into(),
            fields: BTreeMap::from([("setup".into(), "虚构".into())]),
            volume_id: None,
            chapter_id: None,
            source_chapter_id: None,
            source_revision: None,
            story_order: 0,
            story_time: None,
            tracking_status: Some("planned".into()),
            payoff_chapter_id: None,
            payoff_revision: None,
        })
        .unwrap();
    (s, book, card.id, chapter)
}
fn request(s: &Service, book: &str, card: &str, chapter: &str) -> CreateForeshadowOccurrence {
    CreateForeshadowOccurrence {
        book_id: book.into(),
        material_id: card.into(),
        chapter_id: chapter.into(),
        chapter_revision: 1,
        start: 1,
        end: 6,
        quote: "😀\r\n线索".into(),
        role: ForeshadowRole::Payoff,
        note: "作者备注".into(),
        epoch: s.epoch().unwrap(),
        request_key: "fictional-request".into(),
    }
}
#[test]
fn exact_scalar_excerpt_and_replay_are_atomic_and_do_not_auto_resolve() {
    let (mut s, book, card, chapter) = fixture();
    let ok = request(&s, &book, &card, &chapter);
    let mut bad = ok.clone();
    bad.quote = "😀\n线索".into();
    assert_eq!(
        s.create_foreshadow_occurrence(bad).unwrap_err().code,
        "invalid_input"
    );
    let mut bad = ok.clone();
    bad.end = 99;
    assert_eq!(
        s.create_foreshadow_occurrence(bad).unwrap_err().code,
        "invalid_input"
    );
    let mut bad = ok.clone();
    bad.chapter_revision = 0;
    assert_eq!(
        s.create_foreshadow_occurrence(bad).unwrap_err().code,
        "revision_conflict"
    );
    let mut bad = ok.clone();
    bad.epoch = "previous-epoch".into();
    assert!(s.create_foreshadow_occurrence(bad).is_err());
    let mut bad = ok.clone();
    bad.book_id = "other-book".into();
    assert!(s.create_foreshadow_occurrence(bad).is_err());
    let foreign = s
        .apply(Command::CreateBook {
            title: "其他虚构书".into(),
            author: "".into(),
        })
        .unwrap()
        .chapters
        .into_iter()
        .find(|c| c.id != chapter)
        .unwrap()
        .id;
    let mut bad = ok.clone();
    bad.chapter_id = foreign.clone();
    assert!(s.create_foreshadow_occurrence(bad).is_err());
    assert!(s
        .get_foreshadow_chapter(&book, &foreign, &ok.epoch)
        .is_err());
    assert_eq!(s.get_material(&card, false).unwrap().revision, 0);
    let row = s
        .create_foreshadow_occurrence(ok.clone())
        .unwrap()
        .occurrence;
    assert_eq!(row.quote, "😀\r\n线索");
    assert_eq!(s.get_material(&card, false).unwrap().revision, 1);
    assert_eq!(
        s.get_material(&card, false)
            .unwrap()
            .tracking_status
            .as_deref(),
        Some("planned")
    );
    let repeat = s.create_foreshadow_occurrence(ok.clone()).unwrap();
    assert_eq!(repeat.occurrence.id, row.id);
    assert_eq!(
        s.lookup_foreshadow_request(&book, &card, &ok.request_key, &ok.epoch)
            .unwrap()
            .unwrap()
            .occurrence
            .id,
        row.id
    );
    let mut changed = ok;
    changed.note = "不同请求".into();
    assert!(s.create_foreshadow_occurrence(changed).is_err());
    assert_eq!(s.get_material(&card, false).unwrap().revision, 1);
}
#[test]
fn stale_or_missing_sources_keep_frozen_history_and_cas_cannot_refresh_it() {
    let (mut s, book, card, chapter) = fixture();
    let input = request(&s, &book, &card, &chapter);
    let row = s.create_foreshadow_occurrence(input.clone()).unwrap();
    let evidence = row.occurrence;
    assert_eq!(row.source_status, ForeshadowSourceStatus::Current);
    let alt = s
        .apply(Command::CreateVolume {
            book_id: book.clone(),
            title: "次卷".into(),
        })
        .unwrap()
        .volumes[1]
        .id
        .clone();
    s.apply(Command::TransferChapter {
        id: chapter.clone(),
        volume_id: alt.clone(),
    })
    .unwrap();
    let moved = s
        .get_foreshadow_occurrence(&book, &card, &evidence.id, &input.epoch)
        .unwrap();
    assert_eq!(moved.source_status, ForeshadowSourceStatus::Stale);
    assert_eq!(moved.current.unwrap().volume_id, alt);
    assert_eq!(moved.occurrence.source, evidence.source);
    assert!(s
        .set_foreshadow_occurrence_deleted(&book, &card, &evidence.id, 10, &input.epoch, true)
        .is_err());
    s.set_foreshadow_occurrence_deleted(&book, &card, &evidence.id, 0, &input.epoch, true)
        .unwrap();
    assert_eq!(
        s.list_book_foreshadow(&book, &input.epoch).unwrap()[0].occurrence_count,
        0
    );
    let restored = s
        .set_foreshadow_occurrence_deleted(&book, &card, &evidence.id, 1, &input.epoch, false)
        .unwrap();
    assert_eq!(restored.source_status, ForeshadowSourceStatus::Stale);
    assert_eq!(restored.occurrence.quote, evidence.quote);
    assert_eq!(s.get_material(&card, false).unwrap().revision, 3);
    s.apply(Command::TrashChapter {
        id: chapter.clone(),
    })
    .unwrap();
    s.apply(Command::PurgeChapter { id: chapter }).unwrap();
    let gone = s
        .get_foreshadow_occurrence(&book, &card, &evidence.id, &input.epoch)
        .unwrap();
    assert_eq!(gone.source_status, ForeshadowSourceStatus::Unavailable);
    assert!(gone.current.is_none());
    assert_eq!(gone.occurrence.quote, "😀\r\n线索");
}
#[test]
fn current_directory_sorting_groups_unavailable_separately_from_frozen_position() {
    let (mut s, book, card, chapter) = fixture();
    let before = s.load().unwrap();
    let vol1 = before.volumes[0].id.clone();
    let mut input = request(&s, &book, &card, &chapter);
    let first = s.create_foreshadow_occurrence(input.clone()).unwrap();
    let vol2 = s
        .apply(Command::CreateVolume {
            book_id: book.clone(),
            title: "后卷".into(),
        })
        .unwrap()
        .volumes[1]
        .id
        .clone();
    let after = s
        .apply(Command::CreateChapter {
            volume_id: vol2.clone(),
            title: "候选章".into(),
        })
        .unwrap();
    let other = after
        .chapters
        .iter()
        .find(|c| c.volume_id == vol2)
        .unwrap()
        .id
        .clone();
    s.apply(Command::SaveChapter {
        id: other.clone(),
        body: "甲😀\r\n线索乙".into(),
        expected_revision: 0,
    })
    .unwrap();
    input.request_key = "second".into();
    input.chapter_id = other;
    let second = s.create_foreshadow_occurrence(input.clone()).unwrap();
    s.apply(Command::MoveVolume {
        id: vol2,
        direction: -1,
    })
    .unwrap();
    let rows = s
        .list_foreshadow_occurrences(&book, &card, &input.epoch)
        .unwrap();
    assert_eq!(
        rows.iter().map(|r| &r.occurrence.id).collect::<Vec<_>>(),
        vec![&second.occurrence.id, &first.occurrence.id]
    );
    assert!(rows
        .iter()
        .all(|r| r.source_status == ForeshadowSourceStatus::Stale));
    s.apply(Command::TrashVolume { id: vol1 }).unwrap();
    let rows = s
        .list_foreshadow_occurrences(&book, &card, &input.epoch)
        .unwrap();
    assert_eq!(rows[1].source_status, ForeshadowSourceStatus::Unavailable);
    assert_eq!(rows[1].occurrence.source.volume_sort_order, 0);
}
#[test]
fn per_card_limit_includes_trash_and_encoded_bytes_grow_with_escaping() {
    let (mut s, book, card, chapter) = fixture();
    let mut input = request(&s, &book, &card, &chapter);
    input.note = "中".repeat(342);
    assert!(s.create_foreshadow_occurrence(input.clone()).is_err());
    input.note.clear();
    for i in 0..32 {
        input.request_key = format!("request-{i}");
        let row = s
            .create_foreshadow_occurrence(input.clone())
            .unwrap()
            .occurrence;
        s.set_foreshadow_occurrence_deleted(&book, &card, &row.id, 0, &input.epoch, true)
            .unwrap();
    }
    input.request_key = "33rd".into();
    assert_eq!(
        s.create_foreshadow_occurrence(input.clone())
            .unwrap_err()
            .code,
        "limit_exceeded"
    );
    assert_eq!(s.get_material(&card, false).unwrap().revision, 64);
    assert_eq!(
        s.list_foreshadow_occurrences(&book, &card, &input.epoch)
            .unwrap()
            .len(),
        32
    );
}

#[test]
fn json_twelve_preserves_missing_source_and_rejects_wrong_owner_or_legacy_payload() {
    use serde_json::{json, Value};
    let (mut s, book, card, chapter) = fixture();
    let req = request(&s, &book, &card, &chapter);
    let row = s
        .create_foreshadow_occurrence(req.clone())
        .unwrap()
        .occurrence;
    s.apply(Command::TrashChapter {
        id: chapter.clone(),
    })
    .unwrap();
    s.apply(Command::PurgeChapter { id: chapter }).unwrap();
    let raw = s.backup().unwrap();
    let value: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(value["version"], 13);
    assert_eq!(
        value["foreshadow_occurrences"][0]["occurrence"]["quote"],
        "😀\r\n线索"
    );
    let mut target = Service::open(":memory:").unwrap();
    for (field, replacement) in [
        ("/version", json!(11)),
        (
            "/foreshadow_occurrences/0/occurrence/material_id",
            json!("wrong-card"),
        ),
        ("/foreshadow_occurrences/0/occurrence/end", json!(17)),
        ("/foreshadow_occurrences/0/occurrence/revision", json!(99)),
    ] {
        let mut bad = value.clone();
        *bad.pointer_mut(field).unwrap() = replacement;
        let before = target.backup().unwrap();
        assert!(target.restore(&bad.to_string()).is_err(), "{field}");
        assert_eq!(before, target.backup().unwrap());
    }
    target.restore(&raw).unwrap();
    let epoch = target.epoch().unwrap();
    assert!(target
        .get_foreshadow_occurrence(&book, &card, &row.id, &req.epoch)
        .is_err());
    let recovered = target
        .get_foreshadow_occurrence(&book, &card, &row.id, &epoch)
        .unwrap();
    assert_eq!(recovered.occurrence, row);
    assert_eq!(recovered.source_status, ForeshadowSourceStatus::Unavailable);
    let mut legacy = value;
    legacy["version"] = json!(11);
    legacy.as_object_mut().unwrap().remove("foreshadow_proposals");
    legacy
        .as_object_mut()
        .unwrap()
        .remove("foreshadow_occurrences");
    target.restore(&legacy.to_string()).unwrap();
    assert!(target
        .list_foreshadow_occurrences(&book, &card, &target.epoch().unwrap())
        .unwrap()
        .is_empty());
}

#[test]
fn sqlite_copy_validates_occurrence_schema_and_aliases_and_rotates_epoch() {
    let (mut s, book, card, chapter) = fixture();
    let req = request(&s, &book, &card, &chapter);
    let row = s
        .create_foreshadow_occurrence(req.clone())
        .unwrap()
        .occurrence;
    let dir = std::env::temp_dir().join(format!("mojuan-fictional-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let valid = dir.join("valid.sqlite");
    s.recovery_snapshot(&valid).unwrap();
    let mut target = Service::open(":memory:").unwrap();
    let guard = target.restore_guard().unwrap();
    let prepared = prepare_restore(&valid, |_| Ok(())).unwrap();
    target
        .commit_prepared_restore(prepared, &guard, &dir.join("before.sqlite"), |_| Ok(()))
        .unwrap();
    assert_ne!(target.epoch().unwrap(), req.epoch);
    assert_eq!(
        target
            .get_foreshadow_occurrence(&book, &card, &row.id, &target.epoch().unwrap())
            .unwrap()
            .occurrence,
        row
    );

    for (name, sql) in [
        (
            "bytes",
            "UPDATE foreshadow_occurrences SET byte_count=byte_count+1",
        ),
        (
            "alias",
            "UPDATE foreshadow_occurrences SET request_key='wrong'",
        ),
        (
            "schema",
            "ALTER TABLE foreshadow_occurrences ADD COLUMN added TEXT",
        ),
        ("missing_table_no_v4", "DROP TABLE foreshadow_occurrences"),
    ] {
        let file = dir.join(format!("{name}.sqlite"));
        std::fs::copy(&valid, &file).unwrap();
        rusqlite::Connection::open(&file)
            .unwrap()
            .execute_batch(sql)
            .unwrap();
        assert!(prepare_restore(&file, |_| Ok(())).is_err(), "{name}");
    }
    let legacy = dir.join("legacy.sqlite");
    // An independent unmarked fixture generated by the actual pre-feature
    // Service, not by dropping history from a current backup.
    rusqlite::Connection::open(&legacy)
        .unwrap()
        .execute_batch(include_str!("fixtures/pre-foreshadow-v0.sql"))
        .unwrap();
    let old_stage = prepare_restore(&legacy, |_| Ok(())).unwrap();
    let guard = target.restore_guard().unwrap();
    target
        .commit_prepared_restore(
            old_stage,
            &guard,
            &dir.join("before-old.sqlite"),
            |_| Ok(()),
        )
        .unwrap();
    assert_eq!(
        target.load().unwrap().books[0].title,
        "Pre-feature fictional fixture"
    );
    let rebuilt = dir.join("rebuilt.sqlite");
    target.recovery_snapshot(&rebuilt).unwrap();
    let conn = rusqlite::Connection::open(&rebuilt).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    conn.execute_batch("DROP TABLE foreshadow_occurrences")
        .unwrap();
    drop(conn);
    assert!(prepare_restore(&rebuilt, |_| Ok(())).is_err());
    assert!(Service::open(&rebuilt).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn new_writing_pack_exposes_exact_quote_and_checks_source_on_every_live_write() {
    use serde_json::json;
    let (mut s, book, card, chapter) = fixture();
    let vol = s.get_chapter(&chapter).unwrap().volume_id;
    let target = s
        .apply(Command::CreateChapter {
            volume_id: vol,
            title: "目标稿".into(),
        })
        .unwrap()
        .chapters
        .into_iter()
        .find(|c| c.id != chapter)
        .unwrap()
        .id;
    let i = request(&s, &book, &card, &chapter);
    let epoch = i.epoch.clone();
    let occ = s
        .create_foreshadow_occurrence(i.clone())
        .unwrap()
        .occurrence;
    s.configure_writing_policy_with_context(
        &book,
        &target,
        None,
        "active",
        "虚构目标",
        vec![card.clone()],
        vec![chapter.clone()],
        &epoch,
    )
    .unwrap();
    let m = s
        .prepare_writing_pack(&book, &target, 0, 0, &epoch)
        .unwrap();
    let p = s.get_writing_pack(&book, &m.id).unwrap();
    assert_eq!(p.handle_version, 4);
    let entry = format!("material:{card}");
    let field = format!("occurrence:{}", occ.id);
    assert!(m
        .entries
        .iter()
        .find(|e| e.id == entry)
        .unwrap()
        .fields
        .contains(&field));
    assert_eq!(
        s.read_writing_pack_entry(&book, &m.id, &entry, &field, 0)
            .unwrap()
            .text,
        "😀\r\n线索"
    );
    let proposal = |key: &str| {
        serde_json::from_value::<ConstrainedEditInput>(json!({
        "pack_id":m.id,"new_body":"仅虚构稿","idempotency_key":key,
        "citations":[{"entry_id":entry,"field_key":field,"start":0,"end":1,"explanation":"核对证据"}],
        "conflicts":[]
    })).unwrap()
    };
    let accepted = s
        .propose_constrained_edit(&book, "fixture", proposal("p1"))
        .unwrap();
    let review = vec![AuthorReviewItem {
        material_id: card.clone(),
        verdict: AuthorReviewVerdict::Conforms,
        note: String::new(),
    }];
    let saved = s
        .save_constrained_review(&book, &accepted.id, None, &epoch, review.clone())
        .unwrap();
    s.apply(Command::SaveChapter {
        id: chapter.clone(),
        body: "新😀\r\n线索仍在".into(),
        expected_revision: 1,
    })
    .unwrap();
    assert!(s
        .prepare_writing_pack(&book, &target, 0, 0, &epoch)
        .is_err());
    assert!(s
        .propose_constrained_edit(&book, "fixture", proposal("p2"))
        .is_err());
    assert!(s
        .save_constrained_review(&book, &accepted.id, Some(saved.revision), &epoch, review)
        .is_err());
    assert!(s
        .accept_reviewed_constrained_proposal(
            &book,
            &accepted.id,
            0,
            0,
            &epoch,
            saved.revision,
            false
        )
        .is_err());
    assert_eq!(
        s.read_writing_pack_entry(&book, &m.id, &entry, &field, 0)
            .unwrap()
            .text,
        "😀\r\n线索"
    );
    let mut bad: serde_json::Value = serde_json::from_str(&s.backup().unwrap()).unwrap();
    bad["writing_evidence"]["packs"][0]["foreshadow_occurrences"][0]["source"]["chapter_title"] =
        json!("与冻结前文不匹配");
    assert!(s.restore_with_writing_ack(&bad.to_string(), true).is_err());
    let good = s.backup().unwrap();
    s.restore_with_writing_ack(&good, true).unwrap();
    assert!(s.get_writing_pack_manifest(&book, &m.id).unwrap().archived);
    assert_eq!(
        s.read_writing_pack_entry(&book, &m.id, &entry, &field, 0)
            .unwrap()
            .text,
        "😀\r\n线索"
    );
}

#[test]
fn changing_the_occurrence_set_invalidates_a_pack_without_touching_target_body() {
    let (mut s, book, card, chapter) = fixture();
    let i = request(&s, &book, &card, &chapter);
    let epoch = i.epoch.clone();
    s.configure_writing_policy(
        &book,
        &chapter,
        None,
        "active",
        "虚构",
        vec![card.clone()],
        &epoch,
    )
    .unwrap();
    let p = s
        .prepare_writing_pack(&book, &chapter, 0, 1, &epoch)
        .unwrap();
    let new_row = s.create_foreshadow_occurrence(i).unwrap().occurrence;
    let input = |pack_id: String, key: &str| ConstrainedEditInput {
        pack_id,
        new_body: "虚构".into(),
        idempotency_key: key.into(),
        conflicts: vec![],
        citations: vec![WritingCitation {
            entry_id: format!("material:{card}"),
            field_key: "$title".into(),
            start: 0,
            end: 1,
            explanation: "核对".into(),
        }],
    };
    assert!(s
        .propose_constrained_edit(&book, "fixture", input(p.id, "add"))
        .is_err());
    let p = s
        .prepare_writing_pack(&book, &chapter, 0, 1, &epoch)
        .unwrap();
    s.set_foreshadow_occurrence_deleted(&book, &card, &new_row.id, 0, &epoch, true)
        .unwrap();
    assert!(s
        .propose_constrained_edit(&book, "fixture", input(p.id, "trash"))
        .is_err());
    assert_eq!(s.get_chapter(&chapter).unwrap().revision, 1);
}

#[test]
fn occurrence_only_source_changes_block_pack_without_context_or_primary_link() {
    let (mut s, book, card, chapter) = fixture();
    let volume_id = s.get_chapter(&chapter).unwrap().volume_id;
    let target = s
        .apply(Command::CreateChapter {
            volume_id,
            title: "独立目标".into(),
        })
        .unwrap()
        .chapters
        .into_iter()
        .find(|c| c.id != chapter)
        .unwrap()
        .id;
    let input = request(&s, &book, &card, &chapter);
    s.create_foreshadow_occurrence(input.clone()).unwrap();
    s.configure_writing_policy(
        &book,
        &target,
        None,
        "active",
        "虚构",
        vec![card.clone()],
        &input.epoch,
    )
    .unwrap();
    let p = s
        .prepare_writing_pack(&book, &target, 0, 0, &input.epoch)
        .unwrap();
    let pack = s.get_writing_pack(&book, &p.id).unwrap();
    assert!(pack.sources.is_empty() && pack.context_chapters.is_empty());
    let parent = s.get_material(&card, false).unwrap();
    s.apply(Command::RenameChapter {
        id: chapter,
        title: "来源已重命名".into(),
    })
    .unwrap();
    assert_eq!(s.get_material(&card, false).unwrap(), parent);
    assert!(s
        .prepare_writing_pack(&book, &target, 0, 0, &input.epoch)
        .is_err());
    assert!(s
        .propose_constrained_edit(
            &book,
            "fixture",
            ConstrainedEditInput {
                pack_id: p.id,
                new_body: "虚构".into(),
                idempotency_key: "source-only".into(),
                conflicts: vec![],
                citations: vec![WritingCitation {
                    entry_id: format!("material:{card}"),
                    field_key: "$title".into(),
                    start: 0,
                    end: 1,
                    explanation: "核对".into()
                }],
            }
        )
        .is_err());
}

#[test]
fn escaped_position_metadata_is_charged_to_single_occurrence_limit() {
    let (mut s, book, card, chapter) = fixture();
    let volume = s.get_chapter(&chapter).unwrap().volume_id;
    let huge = format!("x{}", "\0".repeat(4095));
    s.apply(Command::RenameVolume {
        id: volume,
        title: huge.clone(),
    })
    .unwrap();
    s.apply(Command::RenameChapter {
        id: chapter.clone(),
        title: huge,
    })
    .unwrap();
    let input = request(&s, &book, &card, &chapter);
    assert_eq!(
        s.create_foreshadow_occurrence(input.clone())
            .unwrap_err()
            .code,
        "limit_exceeded"
    );
    assert!(s
        .list_foreshadow_occurrences(&book, &card, &input.epoch)
        .unwrap()
        .is_empty());
    assert_eq!(s.get_material(&card, false).unwrap().revision, 0);
}

#[test]
fn agent_keeps_pages_with_maximal_escaped_titles_under_the_transport_budget() {
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    let (mut s, book, card, chapter) = fixture();
    let mut input = request(&s, &book, &card, &chapter);
    for n in 0..21 {
        input.request_key = format!("escaped-{n}");
        s.create_foreshadow_occurrence(input.clone()).unwrap();
    }
    let volume = s.get_chapter(&chapter).unwrap().volume_id;
    let huge = format!("x{}", "\0".repeat(4095));
    s.apply(Command::RenameVolume {
        id: volume,
        title: huge.clone(),
    })
    .unwrap();
    s.apply(Command::RenameChapter {
        id: chapter,
        title: huge,
    })
    .unwrap();
    let broker = Arc::new(novel_core::agent::AgentBroker::new(Arc::new(Mutex::new(s))));
    let connection = broker.connect().unwrap();
    connection.call(json!({"method":"request_access","params":{"client_name":"fixture"}}));
    broker.grant_session(connection.id(), &book).unwrap();
    let r = connection.call(
        json!({"method":"list_foreshadow_occurrences","params":{"material_id":card,"limit":20}}),
    );
    let items = r["result"]["items"].as_array().unwrap();
    assert!(
        items.len() < 20,
        "worst escaped titles must split a page before it approaches a double-encoded frame"
    );
    let cursor = r["result"]["next_after_id"].as_str().unwrap();
    assert_eq!(items.last().unwrap()["occurrence"]["id"], cursor);
    let next=connection.call(json!({"method":"list_foreshadow_occurrences","params":{"material_id":card,"limit":20,"after_id":cursor}}));
    assert_eq!(
        items.len() + next["result"]["items"].as_array().unwrap().len(),
        21
    );
}

#[test]
fn legacy_same_sort_numbers_use_full_directory_key_and_body_preflight_is_scoped() {
    let path = std::env::temp_dir().join(format!("mojuan-order-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&path).unwrap();
    let db = path.join("order.sqlite");
    let (mut s, book, card, chapter) = fixture_in(Service::open(&db).unwrap());
    let mut input = request(&s, &book, &card, &chapter);
    s.create_foreshadow_occurrence(input.clone()).unwrap();
    let first_volume = s.get_chapter(&chapter).unwrap().volume_id;
    let second_volume = s
        .apply(Command::CreateVolume {
            book_id: book.clone(),
            title: "同序卷".into(),
        })
        .unwrap()
        .volumes
        .into_iter()
        .find(|v| v.id != first_volume)
        .unwrap()
        .id;
    let second = s
        .apply(Command::CreateChapter {
            volume_id: second_volume.clone(),
            title: "同序章".into(),
        })
        .unwrap()
        .chapters
        .into_iter()
        .find(|c| c.volume_id == second_volume)
        .unwrap()
        .id;
    s.apply(Command::SaveChapter {
        id: second.clone(),
        body: "甲😀\r\n线索乙".into(),
        expected_revision: 0,
    })
    .unwrap();
    input.request_key = "second-legacy".into();
    input.chapter_id = second.clone();
    s.create_foreshadow_occurrence(input.clone()).unwrap();
    drop(s);
    let raw = rusqlite::Connection::open(&db).unwrap();
    raw.execute("UPDATE volumes SET sort_order=0", []).unwrap();
    raw.execute(
        "UPDATE chapters SET body=CAST(zeroblob(10485761) AS TEXT) WHERE id=?1",
        [&second],
    )
    .unwrap();
    drop(raw);
    let s = Service::open(&db).unwrap();
    let rows = s
        .list_foreshadow_occurrences(&book, &card, &input.epoch)
        .unwrap();
    let mut ids = vec![first_volume, second_volume];
    ids.sort();
    assert_eq!(
        rows.iter()
            .map(|r| r.current.as_ref().unwrap().volume_id.clone())
            .collect::<Vec<_>>(),
        ids
    );
    assert_eq!(
        s.get_foreshadow_chapter(&book, &second, &input.epoch)
            .unwrap_err()
            .code,
        "limit_exceeded"
    );
    drop(s);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn frozen_escaped_historical_evidence_checks_each_encoded_record_limit() {
    use serde_json::{json, Value};
    let (mut s, book, card, source) = fixture();
    let vol = s.get_chapter(&source).unwrap().volume_id;
    let target = s
        .apply(Command::CreateChapter {
            volume_id: vol,
            title: "目标".into(),
        })
        .unwrap()
        .chapters
        .into_iter()
        .find(|c| c.id != source)
        .unwrap()
        .id;
    s.apply(Command::RenameChapter {
        id: source.clone(),
        title: format!("线索{}", "\0".repeat(1024)),
    })
    .unwrap();
    let req = request(&s, &book, &card, &source);
    s.create_foreshadow_occurrence(req.clone()).unwrap();
    s.configure_writing_policy(
        &book,
        &target,
        None,
        "active",
        "虚构",
        vec![card],
        &req.epoch,
    )
    .unwrap();
    let pack = s
        .prepare_writing_pack(&book, &target, 0, 0, &req.epoch)
        .unwrap();
    s.apply(Command::TrashChapter { id: source.clone() })
        .unwrap();
    s.apply(Command::PurgeChapter { id: source }).unwrap();
    let original = s.backup().unwrap();
    let mut bad: Value = serde_json::from_str(&original).unwrap();
    let record = &mut bad["writing_evidence"]["packs"][0]["foreshadow_occurrences"][0];
    record["source"]["chapter_title"] = json!(format!("章{}", "\0".repeat(4093)));
    record["quote"] = json!("\0".repeat(4096));
    record["start"] = json!(0);
    record["end"] = json!(4096);
    assert!(serde_json::to_vec(&record).unwrap().len() > 32768);
    assert!(s.restore_with_writing_ack(&bad.to_string(), true).is_err());
    assert_eq!(s.backup().unwrap(), original);
    s.restore_with_writing_ack(&original, true).unwrap();
    assert_eq!(
        s.get_writing_pack(&book, &pack.id)
            .unwrap()
            .foreshadow_occurrences[0]
            .quote,
        "😀\r\n线索"
    );
}

fn archived_copies_fixture() -> (Service, String, String, String, String, String) {
    let (mut s, book, card, source) = fixture();
    let volume_id = s.get_chapter(&source).unwrap().volume_id;
    let target = s
        .apply(Command::CreateChapter {
            volume_id,
            title: "独立虚构目标".into(),
        })
        .unwrap()
        .chapters
        .into_iter()
        .find(|c| c.id != source)
        .unwrap()
        .id;
    let r = request(&s, &book, &card, &source);
    let id = s
        .create_foreshadow_occurrence(r.clone())
        .unwrap()
        .occurrence
        .id;
    s.configure_writing_policy(
        &book,
        &target,
        None,
        "active",
        "虚构",
        vec![card.clone()],
        &r.epoch,
    )
    .unwrap();
    let first = s
        .prepare_writing_pack(&book, &target, 0, 0, &r.epoch)
        .unwrap()
        .id;
    s.set_foreshadow_occurrence_deleted(&book, &card, &id, 0, &r.epoch, true)
        .unwrap();
    s.set_foreshadow_occurrence_deleted(&book, &card, &id, 1, &r.epoch, false)
        .unwrap();
    let second = s
        .prepare_writing_pack(&book, &target, 0, 0, &r.epoch)
        .unwrap()
        .id;
    s.set_foreshadow_occurrence_deleted(&book, &card, &id, 2, &r.epoch, true)
        .unwrap();
    s.apply(Command::TrashChapter { id: source.clone() })
        .unwrap();
    s.apply(Command::PurgeChapter { id: source }).unwrap();
    (s, book, card, id, first, second)
}

#[test]
fn json_restore_requires_same_immutable_occurrence_across_rows_and_archived_packs() {
    use serde_json::{json, Value};
    let (mut s, book, card, id, first, second) = archived_copies_fixture();
    let original = s.backup().unwrap();
    let archived: Value = serde_json::from_str(&original).unwrap();
    let mut single = archived.clone();
    single["writing_evidence"]["packs"]
        .as_array_mut()
        .unwrap()
        .retain(|p| p["id"] == first);
    single["writing_evidence"]["packs"][0]["foreshadow_occurrences"][0]["quote"] = json!("12345");
    let err = s
        .restore_with_writing_ack(&single.to_string(), true)
        .unwrap_err();
    assert!(err.contains("不可变内容不一致"), "{err}");
    assert_eq!(s.backup().unwrap(), original);

    let mut no_owner = archived.clone();
    no_owner["materials"] = json!([]);
    no_owner["material_content_history"] = json!([]);
    no_owner["foreshadow_occurrences"] = json!([]);
    let mut disagrees = no_owner.clone();
    disagrees["writing_evidence"]["packs"][0]["foreshadow_occurrences"][0]["quote"] =
        json!("12345");
    let err = s
        .restore_with_writing_ack(&disagrees.to_string(), true)
        .unwrap_err();
    assert!(err.contains("不可变内容不一致"), "{err}");
    assert_eq!(s.backup().unwrap(), original);

    s.restore_with_writing_ack(&original, true).unwrap();
    let current = s
        .get_foreshadow_occurrence(&book, &card, &id, &s.epoch().unwrap())
        .unwrap();
    assert_eq!(current.source_status, ForeshadowSourceStatus::Unavailable);
    assert_eq!(current.occurrence.revision, 3);
    assert!(current.occurrence.deleted_at.is_some());
    for (pack, rev) in [(&first, 0), (&second, 2)] {
        let row = &s
            .get_writing_pack(&book, pack)
            .unwrap()
            .foreshadow_occurrences[0];
        assert_eq!(row.id, id);
        assert_eq!(row.revision, rev);
        assert!(row.deleted_at.is_none());
    }

    s.restore_with_writing_ack(&no_owner.to_string(), true)
        .unwrap();
    assert!(s.get_material(&card, true).is_err());
    for pack in [&first, &second] {
        assert_eq!(
            s.get_writing_pack(&book, pack)
                .unwrap()
                .foreshadow_occurrences[0]
                .quote,
            "😀\r\n线索"
        );
    }
}

#[test]
fn sqlite_prepare_rejects_contradictory_occurrence_copies_before_live_replacement() {
    use serde_json::{json, Value};
    let (s, book, card, _, first, second) = archived_copies_fixture();
    let dir =
        std::env::temp_dir().join(format!("mojuan-fictional-copies-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let valid = dir.join("valid.sqlite");
    s.recovery_snapshot(&valid).unwrap();
    assert!(prepare_restore(&valid, |_| Ok(())).is_ok());
    for (case, remove_owner) in [("row-pack", false), ("pack-pack", true)] {
        let path = dir.join(format!("{case}.sqlite"));
        std::fs::copy(&valid, &path).unwrap();
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        if remove_owner {
            conn.execute("DELETE FROM materials WHERE id=?1", [&card])
                .unwrap();
        } else {
            conn.execute("DELETE FROM writing_packs WHERE id=?1", [&second])
                .unwrap();
        }
        let raw: String = conn
            .query_row(
                "SELECT payload FROM writing_packs WHERE id=?1",
                [&first],
                |r| r.get(0),
            )
            .unwrap();
        let mut pack: Value = serde_json::from_str(&raw).unwrap();
        pack["foreshadow_occurrences"][0]["quote"] = json!("12345");
        let changed = pack.to_string();
        conn.execute(
            "UPDATE writing_packs SET payload=?1,byte_count=?2 WHERE id=?3",
            rusqlite::params![changed, changed.len(), first],
        )
        .unwrap();
        drop(conn);
        let err = prepare_restore(&path, |_| Ok(())).err().expect(case);
        assert!(err.contains("不可变内容不一致"), "{case}: {err}");
    }

    let missing = dir.join("missing-owner.sqlite");
    std::fs::copy(&valid, &missing).unwrap();
    let conn = rusqlite::Connection::open(&missing).unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    conn.execute("DELETE FROM materials WHERE id=?1", [&card])
        .unwrap();
    drop(conn);
    let mut restored = Service::open(":memory:").unwrap();
    let guard = restored.restore_guard().unwrap();
    restored
        .commit_prepared_restore(
            prepare_restore(&missing, |_| Ok(())).unwrap(),
            &guard,
            &dir.join("before.sqlite"),
            |_| Ok(()),
        )
        .unwrap();
    assert!(restored.get_material(&card, true).is_err());
    for pack in [&first, &second] {
        assert_eq!(
            restored
                .get_writing_pack(&book, pack)
                .unwrap()
                .foreshadow_occurrences[0]
                .quote,
            "😀\r\n线索"
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn trusted_source_window_is_same_book_epoch_bound_unicode_exact_and_bounded() {
    let (mut s, book, _, chapter) = fixture();
    let epoch = s.epoch().unwrap();
    let prefix = format!("{}🌊\r\n\0", "甲".repeat(90000));
    let quote = "两短一长，走背潮。";
    let body = format!("{prefix}{quote}{}", "末".repeat(90000));
    s.apply(Command::SaveChapter { id: chapter.clone(), body, expected_revision: 1 }).unwrap();
    let start = prefix.chars().count() as u32;
    let end = start + quote.chars().count() as u32;
    let window = s.get_foreshadow_source_window(&book, &chapter, &epoch, start, end).unwrap();
    assert_eq!(window.position.chapter_id, chapter);
    assert_eq!(window.position.chapter_revision, 2);
    assert_eq!(window.before.chars().count(), 80);
    assert!(window.before.ends_with("🌊\r\n\0"));
    assert_eq!(window.at_range, quote);
    assert_eq!(window.after, "末".repeat(80));
    let moved_range = s.get_foreshadow_source_window(&book, &chapter, &epoch, 900000, 900001).unwrap();
    assert!(moved_range.at_range.is_empty());
    assert!(moved_range.before.is_empty());
    assert!(moved_range.after.is_empty());
    assert!(s.get_foreshadow_source_window(&book, &chapter, &epoch, 0, 4097).is_err());
    assert!(s.get_foreshadow_source_window(&book, &chapter, &epoch, 2, 2).is_err());
    let data = s.apply(Command::CreateBook {title:"异书".into(), author:"".into()}).unwrap();
    let another_book = &data.books.iter().find(|b| b.id != book).unwrap().id;
    assert_eq!(s.get_foreshadow_source_window(another_book, &chapter, &epoch, start, end).unwrap_err().code, "not_found");
    assert!(s.get_foreshadow_source_window(&book, &chapter, "old-epoch", start, end).is_err());
    s.apply(Command::SaveChapter {
        id: chapter.clone(), body: "🌊".repeat(4257), expected_revision: 2,
    }).unwrap();
    let max = s.get_foreshadow_source_window(&book, &chapter, &epoch, 80, 4176).unwrap();
    assert_eq!(max.before.len() + max.at_range.len() + max.after.len(), 17024);
    s.apply(Command::TrashChapter {id: chapter.clone()}).unwrap();
    assert_eq!(s.get_foreshadow_source_window(&book, &chapter, &epoch, start, end).unwrap_err().code, "not_found");
}
