use novel_core::*;
use serde_json::{json, Value};

fn fixture() -> (Service, String, String, String, String) {
    let mut s = Service::open(":memory:").unwrap();
    let state = s
        .apply(Command::CreateBook {
            title: "渡河".into(),
            author: "".into(),
        })
        .unwrap();
    let book = state.books[0].id.clone();
    let volume = state.volumes[0].id.clone();
    let prior = state.chapters[0].id.clone();
    s.apply(Command::SaveChapter {
        id: prior.clone(),
        body: "旧岸🚣\r\n风起".into(),
        expected_revision: 0,
    })
    .unwrap();
    let state = s
        .apply(Command::CreateChapter {
            volume_id: volume,
            title: "第一章（标题与实际顺序不同）".into(),
        })
        .unwrap();
    let target = state
        .chapters
        .iter()
        .find(|c| c.id != prior)
        .unwrap()
        .id
        .clone();
    let card = s
        .create_material(
            serde_json::from_value(json!({
                "book_id":book,"kind":"summary","title":"前章提要",
                "fields":{"content":"仍留在岸边"},"source_chapter_id":prior,"source_revision":1
            }))
            .unwrap(),
        )
        .unwrap()
        .id;
    (s, book, prior, target, card)
}

#[test]
fn version_eleven_archives_frozen_context_without_current_chapter() {
    let (mut s, book, prior, target, card) = fixture();
    let epoch = s.epoch().unwrap();
    s.configure_writing_policy(&book, &target, None, "active", "渡河", vec![card], &epoch)
        .unwrap();
    let manifest = s
        .prepare_writing_pack(&book, &target, 0, 0, &epoch)
        .unwrap();
    let mut backup: Value = serde_json::from_str(&s.backup().unwrap()).unwrap();
    backup["version"] = json!(11);
    backup.as_object_mut().unwrap().remove("foreshadow_proposals");
    let chapter = backup["chapters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == prior)
        .unwrap()
        .clone();
    let volume = backup["volumes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == chapter["volume_id"])
        .unwrap()
        .clone();
    let evidence = &mut backup["writing_evidence"];
    evidence["policies"][0]["context_chapter_ids"] = json!([prior]);
    let pack = &mut evidence["packs"][0];
    pack["policy"]["context_chapter_ids"] = json!([prior]);
    pack["context_chapters"] = json!([{ "chapter": chapter, "volume": volume }]);
    pack["handle_version"] = json!(3);
    for (path, val) in [
        (
            "/writing_evidence/packs/0/context_chapters/0/chapter/deleted_at",
            json!(1),
        ),
        (
            "/writing_evidence/packs/0/context_chapters/0/chapter/revision",
            json!(999),
        ),
        (
            "/writing_evidence/packs/0/context_chapters/0/chapter/sort_order",
            json!(999),
        ),
        (
            "/writing_evidence/packs/0/context_chapters/0/volume/book_id",
            json!("foreign"),
        ),
        ("/writing_evidence/packs/0/handle_version", json!(2)),
        (
            "/writing_evidence/packs/0/context_chapters/0/chapter/title",
            json!("不同标题"),
        ),
        (
            "/writing_evidence/packs/0/context_chapters/0/volume/title",
            json!("不同父卷"),
        ),
        ("/writing_evidence/packs/0/sources/0/sort_order", json!(1)),
        ("/version", json!(10)),
    ] {
        let mut damaged = backup.clone();
        *damaged.pointer_mut(path).unwrap() = val;
        assert!(
            s.restore_with_writing_ack(&damaged.to_string(), true)
                .is_err(),
            "{path}"
        );
        assert_eq!(s.epoch().unwrap(), epoch);
    }
    let mut missing_marker = backup.clone();
    missing_marker["writing_evidence"]["packs"][0]["context_chapters"][0]["chapter"]
        .as_object_mut()
        .unwrap()
        .remove("deleted_at");
    assert!(s
        .restore_with_writing_ack(&missing_marker.to_string(), true)
        .is_err());
    s.restore_with_writing_ack(&backup.to_string(), true)
        .unwrap();
    s.apply(Command::TrashChapter { id: prior.clone() })
        .unwrap();
    s.apply(Command::PurgeChapter { id: prior.clone() })
        .unwrap();
    let backup = s.backup().unwrap();
    s.restore_with_writing_ack(&backup, true).unwrap();
    let m = s.get_writing_pack_manifest(&book, &manifest.id).unwrap();
    assert!(m.archived);
    assert_eq!(m.entries[2].kind, "context_chapter");
    assert_eq!(m.entries[2].metadata["chapter_id"], prior);
    let r = s
        .read_writing_pack_entry(&book, &manifest.id, &format!("context:{prior}"), "body", 0)
        .unwrap();
    assert_eq!(r.text, "旧岸🚣\r\n风起");
}

fn plain_card(s: &mut Service, book: &str) -> String {
    s.create_material(
        serde_json::from_value(json!({
            "book_id":book,"kind":"outline","title":"无来源大纲","fields":{"content":"目的地"}
        }))
        .unwrap(),
    )
    .unwrap()
    .id
}

fn submission(pack: &WritingPackManifest, card: &str, key: &str) -> ConstrainedEditInput {
    ConstrainedEditInput {
        pack_id: pack.id.clone(),
        new_body: "新稿".into(),
        citations: vec![WritingCitation {
            entry_id: format!("material:{card}"),
            field_key: "content".into(),
            start: 0,
            end: 1,
            explanation: "必需大纲".into(),
        }],
        conflicts: vec![],
        idempotency_key: key.into(),
    }
}
fn conform(card: &str) -> Vec<AuthorReviewItem> {
    vec![AuthorReviewItem {
        material_id: card.into(),
        verdict: AuthorReviewVerdict::Conforms,
        note: "".into(),
    }]
}

#[test]
fn prior_mutations_block_new_submission_review_save_and_acceptance_without_touching_target() {
    for mutation in [
        "body",
        "title",
        "parent title",
        "reorder",
        "transfer",
        "trash",
        "purge",
    ] {
        let (mut s, b, prior, target, _) = fixture();
        let card = plain_card(&mut s, &b);
        let e = s.epoch().unwrap();
        let old_volume = s.get_chapter(&prior).unwrap().volume_id;
        let other = s
            .apply(Command::CreateVolume {
                book_id: b.clone(),
                title: "第二卷".into(),
            })
            .unwrap()
            .volumes
            .into_iter()
            .find(|v| v.id != old_volume)
            .unwrap();
        s.configure_writing_policy_with_context(
            &b,
            &target,
            None,
            "active",
            "目标",
            vec![card.clone()],
            vec![prior.clone()],
            &e,
        )
        .unwrap();
        let pack = s.prepare_writing_pack(&b, &target, 0, 0, &e).unwrap();
        let q = s
            .propose_constrained_edit(&b, "client", submission(&pack, &card, "before"))
            .unwrap();
        let r = s
            .save_constrained_review(&b, &q.id, None, &e, conform(&card))
            .unwrap();
        match mutation {
            "body" => s.apply(Command::SaveChapter {
                id: prior.clone(),
                body: "改".into(),
                expected_revision: 1,
            }),
            "title" => s.apply(Command::RenameChapter {
                id: prior.clone(),
                title: "改名".into(),
            }),
            "parent title" => s.apply(Command::RenameVolume {
                id: old_volume,
                title: "改卷名".into(),
            }),
            "reorder" => s.apply(Command::MoveChapter {
                id: prior.clone(),
                direction: 1,
            }),
            "transfer" => s.apply(Command::TransferChapter {
                id: prior.clone(),
                volume_id: other.id,
            }),
            "trash" => s.apply(Command::TrashChapter { id: prior.clone() }),
            "purge" => {
                s.apply(Command::TrashChapter { id: prior.clone() })
                    .unwrap();
                s.apply(Command::PurgeChapter { id: prior.clone() })
            }
            _ => unreachable!(),
        }
        .unwrap();
        assert!(
            s.propose_constrained_edit(&b, "client", submission(&pack, &card, "after"))
                .is_err(),
            "{mutation}"
        );
        assert!(
            s.save_constrained_review(&b, &q.id, Some(r.revision), &e, conform(&card))
                .is_err(),
            "{mutation}"
        );
        assert!(
            s.accept_reviewed_constrained_proposal(&b, &q.id, 0, 0, &e, r.revision, false)
                .is_err(),
            "{mutation}"
        );
        assert_eq!(s.get_chapter(&target).unwrap().revision, 0);
        let preserved = s.get_constrained_proposal(&b, &q.id).unwrap();
        assert_eq!(preserved.decision, "pending");
        assert_eq!(preserved.author_review, Some(r));
        assert_eq!(
            s.get_writing_pack(&b, &pack.id).unwrap().context_chapters[0]
                .chapter
                .body,
            "旧岸🚣\r\n风起"
        );
    }
}

#[test]
fn legacy_configure_retains_selection_and_can_pause_invalid_dependencies_then_author_clears() {
    let (mut s, b, prior, target, _) = fixture();
    let card = plain_card(&mut s, &b);
    let e = s.epoch().unwrap();
    s.configure_writing_policy_with_context(
        &b,
        &target,
        None,
        "active",
        "目标",
        vec![card.clone()],
        vec![prior.clone()],
        &e,
    )
    .unwrap();
    let p = s
        .configure_writing_policy(
            &b,
            &target,
            Some(0),
            "active",
            "新目标",
            vec![card.clone()],
            &e,
        )
        .unwrap();
    assert_eq!(p.context_chapter_ids, [prior.clone()]);
    let pack = s.prepare_writing_pack(&b, &target, 1, 0, &e).unwrap();
    let q = s
        .propose_constrained_edit(&b, "client", submission(&pack, &card, "before"))
        .unwrap();
    s.apply(Command::TrashChapter { id: prior.clone() })
        .unwrap();
    assert!(s
        .configure_writing_policy(
            &b,
            &target,
            Some(1),
            "active",
            "失败",
            vec![card.clone()],
            &e
        )
        .is_err());
    assert_eq!(
        s.get_constrained_proposal(&b, &q.id).unwrap().decision,
        "pending"
    );
    let paused = s
        .configure_writing_policy(
            &b,
            &target,
            Some(1),
            "paused",
            "暂停",
            vec![card.clone()],
            &e,
        )
        .unwrap();
    assert_eq!(paused.context_chapter_ids, [prior]);
    assert_eq!(
        s.get_constrained_proposal(&b, &q.id).unwrap().decision,
        "cancelled"
    );
    let cleared = s
        .configure_writing_policy_with_context(
            &b,
            &target,
            Some(2),
            "active",
            "已修正",
            vec![card.clone()],
            vec![],
            &e,
        )
        .unwrap();
    assert!(cleared.context_chapter_ids.is_empty());
    assert!(s
        .prepare_writing_pack(&b, &target, 3, 0, &e)
        .unwrap()
        .entries
        .iter()
        .all(|e| e.kind != "context_chapter"));
    assert_eq!(
        s.get_writing_pack(&b, &pack.id)
            .unwrap()
            .context_chapters
            .len(),
        1
    );
}

#[test]
fn author_context_configuration_rejects_ninth_duplicate_self_later_and_foreign_ids_without_writes()
{
    let (mut s, b, prior, target, card) = fixture();
    let e = s.epoch().unwrap();
    let v = s.get_chapter(&prior).unwrap().volume_id;
    let late = s
        .apply(Command::CreateChapter {
            volume_id: v.clone(),
            title: "后章".into(),
        })
        .unwrap()
        .chapters
        .into_iter()
        .find(|ch| ch.sort_order == 2)
        .unwrap()
        .id;
    let st = s
        .apply(Command::CreateBook {
            title: "另一本".into(),
            author: "".into(),
        })
        .unwrap();
    let foreign = st
        .chapters
        .iter()
        .find(|c| ![&prior, &target, &late].contains(&&c.id))
        .unwrap()
        .id
        .clone();
    let mut older = vec![prior.clone()];
    for i in 0..8 {
        let title = format!("更早{i}");
        let id = s
            .apply(Command::CreateChapter {
                volume_id: v.clone(),
                title: title.clone(),
            })
            .unwrap()
            .chapters
            .into_iter()
            .find(|c| c.title == title)
            .unwrap()
            .id;
        while s.get_chapter(&id).unwrap().sort_order > s.get_chapter(&target).unwrap().sort_order {
            s.apply(Command::MoveChapter {
                id: id.clone(),
                direction: -1,
            })
            .unwrap();
        }
        older.push(id);
    }
    for (mode, ids) in [
        ("active", older.clone()),
        ("active", vec![prior.clone(), prior.clone()]),
        ("active", vec![target.clone()]),
        ("active", vec![late]),
        ("active", vec![foreign.clone()]),
        ("paused", vec![foreign]),
    ] {
        assert!(s
            .configure_writing_policy_with_context(
                &b,
                &target,
                None,
                mode,
                "拒绝",
                vec![card.clone()],
                ids,
                &e
            )
            .is_err());
        let st = s.get_writing_state(&b, &target).unwrap();
        assert!(st.policy.is_none() && st.packs.is_empty() && st.proposals.is_empty());
    }
    older.pop();
    s.configure_writing_policy_with_context(
        &b,
        &target,
        None,
        "active",
        "恰好八章",
        vec![card],
        older,
        &e,
    )
    .unwrap();
    assert_eq!(
        s.prepare_writing_pack(&b, &target, 0, 0, &e)
            .unwrap()
            .entries
            .iter()
            .filter(|x| x.kind == "context_chapter")
            .count(),
        8
    );
}

#[test]
fn per_chapter_raw_total_and_encoded_pack_limits_fail_atomically() {
    let (mut s, b, prior, target, _) = fixture();
    let card = plain_card(&mut s, &b);
    let e = s.epoch().unwrap();
    s.configure_writing_policy_with_context(
        &b,
        &target,
        None,
        "active",
        "大小",
        vec![card.clone()],
        vec![prior.clone()],
        &e,
    )
    .unwrap();
    for (expected, text) in [(1, "x".repeat(1_048_577)), (2, "\0".repeat(750_000))] {
        s.apply(Command::SaveChapter {
            id: prior.clone(),
            body: text,
            expected_revision: expected,
        })
        .unwrap();
        assert!(s.prepare_writing_pack(&b, &target, 0, 0, &e).is_err());
        assert!(s.get_writing_state(&b, &target).unwrap().packs.is_empty());
    }
    s.apply(Command::SaveChapter {
        id: prior.clone(),
        body: "x".repeat(1_048_576),
        expected_revision: 3,
    })
    .unwrap();
    s.prepare_writing_pack(&b, &target, 0, 0, &e).unwrap();
    let v = s.get_chapter(&prior).unwrap().volume_id;
    let mut contexts = vec![prior];
    for i in 0..4 {
        let title = format!("大章{i}");
        let id = s
            .apply(Command::CreateChapter {
                volume_id: v.clone(),
                title: title.clone(),
            })
            .unwrap()
            .chapters
            .into_iter()
            .find(|c| c.title == title)
            .unwrap()
            .id;
        s.apply(Command::SaveChapter {
            id: id.clone(),
            body: "x".repeat(1_048_576),
            expected_revision: 0,
        })
        .unwrap();
        s.apply(Command::MoveChapter {
            id: id.clone(),
            direction: -1,
        })
        .unwrap();
        contexts.push(id);
    }
    s.configure_writing_policy_with_context(
        &b,
        &target,
        Some(0),
        "active",
        "总大小",
        vec![card],
        contexts,
        &e,
    )
    .unwrap();
    assert!(s.prepare_writing_pack(&b, &target, 1, 0, &e).is_err());
    assert_eq!(s.get_writing_state(&b, &target).unwrap().packs.len(), 1);
}

#[test]
fn context_is_citable_but_not_a_substitute_for_required_material_or_review() {
    let (mut s, b, prior, target, card) = fixture();
    let e = s.epoch().unwrap();
    s.configure_writing_policy_with_context(
        &b,
        &target,
        None,
        "active",
        "衔接",
        vec![card.clone()],
        vec![prior.clone()],
        &e,
    )
    .unwrap();
    let pack = s.prepare_writing_pack(&b, &target, 0, 0, &e).unwrap();
    let extra = WritingCitation {
        entry_id: format!("context:{prior}"),
        field_key: "body".into(),
        start: 2,
        end: 3,
        explanation: "核对船只".into(),
    };
    let mut i = submission(&pack, &card, "with_context");
    i.citations = vec![extra.clone()];
    assert!(s.propose_constrained_edit(&b, "client", i).is_err());
    let mut i = submission(&pack, &card, "with_context");
    i.citations.push(extra.clone());
    i.conflicts = vec![WritingConflict {
        entry_ids: vec![prior.clone()],
        explanation: "并非资料 ID".into(),
    }];
    assert!(s.propose_constrained_edit(&b, "client", i.clone()).is_err());
    i.conflicts.clear();
    i.citations[1].end = 100;
    assert!(s.propose_constrained_edit(&b, "client", i.clone()).is_err());
    i.citations[1] = extra;
    let q = s.propose_constrained_edit(&b, "client", i).unwrap();
    assert!(s
        .save_constrained_review(&b, &q.id, None, &e, conform(&prior))
        .is_err());
    assert!(s
        .accept_reviewed_constrained_proposal(&b, &q.id, 0, 0, &e, 1, false)
        .is_err());
    let r = s
        .save_constrained_review(&b, &q.id, None, &e, conform(&card))
        .unwrap();
    s.accept_reviewed_constrained_proposal(&b, &q.id, 0, 0, &e, r.revision, false)
        .unwrap();
    assert_eq!(s.get_chapter(&target).unwrap().body, "新稿");
}

struct TestDir(std::path::PathBuf);
impl TestDir {
    fn new() -> Self {
        let p =
            std::env::temp_dir().join(format!("mojuan-prior-evidence-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn path(&self, name: &str) -> std::path::PathBuf {
        self.0.join(name)
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn sqlite_preserves_context_archive_and_rejects_conflicting_prior_source_rows() {
    let (mut s, book, prior, target, card) = fixture();
    let e = s.epoch().unwrap();
    s.configure_writing_policy_with_context(
        &book,
        &target,
        None,
        "active",
        "上下文",
        vec![card.clone()],
        vec![prior.clone()],
        &e,
    )
    .unwrap();
    let pack = s.prepare_writing_pack(&book, &target, 0, 0, &e).unwrap();
    let q = s
        .propose_constrained_edit(&book, "c", submission(&pack, &card, "archive"))
        .unwrap();
    let review = s
        .save_constrained_review(&book, &q.id, None, &e, conform(&card))
        .unwrap();
    s.apply(Command::TrashChapter { id: prior.clone() })
        .unwrap();
    s.apply(Command::PurgeChapter { id: prior.clone() })
        .unwrap();
    let dir = TestDir::new();
    let current = dir.path("current.sqlite");
    let malformed = dir.path("malformed.sqlite");
    s.recovery_snapshot(&current).unwrap();
    std::fs::copy(&current, &malformed).unwrap();
    {
        let c = rusqlite::Connection::open(&malformed).unwrap();
        let payload: String = c
            .query_row(
                "SELECT payload FROM writing_packs WHERE id=?1",
                [&pack.id],
                |r| r.get(0),
            )
            .unwrap();
        let mut bad: Value = serde_json::from_str(&payload).unwrap();
        bad["sources"][0]["title"] = json!("伪造的重叠来源");
        let bad = bad.to_string();
        c.execute(
            "UPDATE writing_packs SET payload=?1,byte_count=?2 WHERE id=?3",
            rusqlite::params![bad, bad.len(), pack.id],
        )
        .unwrap();
    }
    assert!(prepare_restore(&malformed, |_| Ok(())).is_err());
    let guard = s.restore_guard().unwrap();
    let prepared = prepare_restore(&current, |_| Ok(())).unwrap();
    assert!(s
        .commit_prepared_restore(prepared, &guard, &dir.path("unapproved.sqlite"), |_| Ok(()))
        .is_err());
    assert!(!dir.path("unapproved.sqlite").exists());
    let prepared = prepare_restore(&current, |_| Ok(())).unwrap();
    s.commit_prepared_restore_with_writing_ack(
        prepared,
        &guard,
        &dir.path("recovery.sqlite"),
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        true,
        |_| Ok(()),
    )
    .unwrap();
    assert!(dir.path("recovery.sqlite").exists());
    let post_epoch = s.epoch().unwrap();
    assert_ne!(e, post_epoch);
    let state = s.get_writing_state(&book, &target).unwrap();
    assert_eq!(state.policy.unwrap().mode, "paused");
    assert!(state.packs[0].archived);
    assert!(state.proposals[0].archived);
    assert_eq!(
        s.get_constrained_proposal(&book, &q.id)
            .unwrap()
            .author_review,
        Some(review.clone())
    );
    assert!(s
        .accept_reviewed_constrained_proposal(
            &book,
            &q.id,
            0,
            0,
            &post_epoch,
            review.revision,
            false
        )
        .is_err());
    let prior_body = s
        .read_writing_pack_entry(&book, &pack.id, &format!("context:{prior}"), "body", 0)
        .unwrap();
    assert_eq!(prior_body.text, "旧岸🚣\r\n风起");
}

#[test]
fn legacy_sqlite_without_context_keys_keeps_cached_manifest_and_archived_fields() {
    let (mut s, book, _prior, target, card) = fixture();
    let e = s.epoch().unwrap();
    let p = s
        .configure_writing_policy(
            &book,
            &target,
            None,
            "active",
            "旧格式",
            vec![card.clone()],
            &e,
        )
        .unwrap();
    assert!(p.context_chapter_ids.is_empty());
    let pack = s.prepare_writing_pack(&book, &target, 0, 0, &e).unwrap();
    let dir = TestDir::new();
    let old = dir.path("legacy.sqlite");
    s.recovery_snapshot(&old).unwrap();
    {
        let c = rusqlite::Connection::open(&old).unwrap();
        let policy: String = c
            .query_row(
                "SELECT record FROM writing_policies WHERE chapter_id=?1",
                [&target],
                |r| r.get(0),
            )
            .unwrap();
        let policy: Value = serde_json::from_str(&policy).unwrap();
        assert!(policy.get("context_chapter_ids").is_none());
        let payload: String = c
            .query_row(
                "SELECT payload FROM writing_packs WHERE id=?1",
                [&pack.id],
                |r| r.get(0),
            )
            .unwrap();
        let mut payload: Value = serde_json::from_str(&payload).unwrap();
        assert!(payload.get("context_chapters").is_none());
        assert!(payload["policy"].get("context_chapter_ids").is_none());
        payload["handle_version"] = json!(2);
        let payload = payload.to_string();
        c.execute(
            "UPDATE writing_packs SET payload=?1,byte_count=?2 WHERE id=?3",
            rusqlite::params![payload, payload.len(), pack.id],
        )
        .unwrap();
    }
    let guard = s.restore_guard().unwrap();
    let prepared = prepare_restore(&old, |_| Ok(())).unwrap();
    s.commit_prepared_restore_with_writing_ack(
        prepared,
        &guard,
        &dir.path("recovery.sqlite"),
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        true,
        |_| Ok(()),
    )
    .unwrap();
    let archived = s.get_writing_pack(&book, &pack.id).unwrap();
    assert_eq!(archived.handle_version, 2);
    assert!(archived.context_chapters.is_empty());
    assert!(
        s.get_writing_pack_manifest(&book, &pack.id)
            .unwrap()
            .archived
    );
    assert_eq!(
        s.read_writing_pack_entry(&book, &pack.id, &format!("material:{card}"), "content", 0)
            .unwrap()
            .text,
        "仍留在岸边"
    );
}
