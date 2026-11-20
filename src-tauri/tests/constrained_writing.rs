use novel_core::*;
use serde_json::json;
fn setup() -> (Service, String, String, String) {
    let mut s = Service::open(":memory:").unwrap();
    let st = s
        .apply(Command::CreateBook {
            title: "河".into(),
            author: "".into(),
        })
        .unwrap();
    let b = st.books[0].id.clone();
    let c = st.chapters[0].id.clone();
    let m=s.create_material(serde_json::from_value(json!({"book_id":b,"kind":"setting","title":"规则","fields":{"synopsis":"角色不能游泳"},"volume_id":null,"chapter_id":null})).unwrap()).unwrap();
    (s, b, c, m.id)
}
fn configure(s: &mut Service, b: &str, c: &str, m: &str) -> WritingPolicy {
    s.configure_writing_policy(
        b,
        c,
        None,
        "active",
        "过河",
        vec![m.into()],
        &s.epoch().unwrap(),
    )
    .unwrap()
}
fn input(pack: &WritingPackManifest, m: &str) -> ConstrainedEditInput {
    ConstrainedEditInput {
        pack_id: pack.id.clone(),
        new_body: "角色坐船过河".into(),
        citations: vec![WritingCitation {
            entry_id: format!("material:{m}"),
            field_key: "synopsis".into(),
            start: 0,
            end: 2,
            explanation: "遵循不会游泳设定".into(),
        }],
        conflicts: vec![],
        idempotency_key: "key".into(),
    }
}
#[test]
fn strict_guards_both_legacy_paths_and_dependency_changes() {
    let (mut s, b, c, m) = setup();
    let old = s
        .propose_edit(ProposalInput {
            client_id: "a".into(),
            idempotency_key: "old".into(),
            chapter_id: c.clone(),
            base_revision: 0,
            new_body: "old".into(),
            reason: "".into(),
        })
        .unwrap();
    let p = configure(&mut s, &b, &c, &m);
    assert!(s
        .propose_edit(ProposalInput {
            client_id: "a".into(),
            idempotency_key: "other".into(),
            chapter_id: c.clone(),
            base_revision: 0,
            new_body: "bad".into(),
            reason: "".into()
        })
        .is_err());
    assert!(s.decide_proposal(&old.id, true, 0).is_err());
    let pack = s
        .prepare_writing_pack(&b, &c, p.revision, 0, &s.epoch().unwrap())
        .unwrap();
    let proposal = s
        .propose_constrained_edit(&b, "a", input(&pack, &m))
        .unwrap();
    assert_eq!(
        s.propose_constrained_edit(&b, "a", input(&pack, &m))
            .unwrap()
            .id,
        proposal.id
    );
    let e = s.epoch().unwrap();
    let review = s
        .save_constrained_review(&b, &proposal.id, None, &e, approved(&m))
        .unwrap();
    let card = s.get_material(&m, false).unwrap();
    let mut v = serde_json::to_value(&card).unwrap();
    v["expected_revision"] = json!(card.revision);
    v.as_object_mut().unwrap().remove("revision");
    v["title"] = json!("已改变");
    let edit = SaveMaterialInput {
        id: card.id,
        title: "changed".into(),
        fields: card.fields,
        volume_id: card.volume_id,
        chapter_id: card.chapter_id,
        expected_revision: card.revision,
        source_chapter_id: card.source_chapter_id,
        source_revision: card.source_revision,
        story_order: card.story_order,
        story_time: card.story_time,
        tracking_status: card.tracking_status,
        payoff_chapter_id: card.payoff_chapter_id,
        payoff_revision: card.payoff_revision,
    };
    s.save_material(edit).unwrap();
    assert!(s
        .accept_reviewed_constrained_proposal(
            &b,
            &proposal.id,
            p.revision,
            0,
            &e,
            review.revision,
            false,
        )
        .is_err());
    assert_eq!(s.get_chapter(&c).unwrap().body, "");
}
#[test]
fn quotes_conflicts_and_restore_archive() {
    let (mut s, b, c, m) = setup();
    let p = configure(&mut s, &b, &c, &m);
    let pack = s
        .prepare_writing_pack(&b, &c, p.revision, 0, &s.epoch().unwrap())
        .unwrap();
    let mut i = input(&pack, &m);
    i.citations[0].end = 999;
    assert!(s.propose_constrained_edit(&b, "a", i).is_err());
    let mut i = input(&pack, &m);
    i.conflicts = vec![WritingConflict {
        entry_ids: vec![m.clone()],
        explanation: "需要作者判断".into(),
    }];
    let q = s.propose_constrained_edit(&b, "a", i).unwrap();
    let e = s.epoch().unwrap();
    let review = s
        .save_constrained_review(&b, &q.id, None, &e, approved(&m))
        .unwrap();
    assert!(s
        .accept_reviewed_constrained_proposal(&b, &q.id, 0, 0, &e, review.revision, false)
        .is_err());
    let accepted = s
        .accept_reviewed_constrained_proposal(&b, &q.id, 0, 0, &e, review.revision, true)
        .unwrap();
    assert_eq!(accepted.author_review, Some(review.clone()));
    assert_eq!(accepted.accepted_revision, Some(1));
    assert_eq!(s.get_chapter(&c).unwrap().body, "角色坐船过河");
    let backup = s.backup().unwrap();
    assert!(s.restore(&backup).is_err());
    s.restore_with_writing_ack(&backup, true).unwrap();
    let status = s.get_writing_state(&b, &c).unwrap();
    assert_eq!(status.policy.unwrap().mode, "paused");
    let archived = s.get_constrained_proposal(&b, &q.id).unwrap();
    assert!(archived.live_epoch.is_none());
    assert_eq!(archived.author_review, Some(review));
}
#[test]
fn sqlite_evidence_and_old_json_require_explicit_replacement() {
    let (mut s, b, c, m) = setup();
    let old = s.backup().unwrap();
    let p = configure(&mut s, &b, &c, &m);
    let pack = s
        .prepare_writing_pack(&b, &c, p.revision, 0, &s.epoch().unwrap())
        .unwrap();
    let q = s
        .propose_constrained_edit(&b, "a", input(&pack, &m))
        .unwrap();
    let e = s.epoch().unwrap();
    let review = s
        .save_constrained_review(&b, &q.id, None, &e, approved(&m))
        .unwrap();
    let dir = TestDir::new();
    let snap = dir.path().join("e.sqlite");
    s.recovery_snapshot(&snap).unwrap();
    let prepared = prepare_restore(&snap, |_| Ok(())).unwrap();
    let guard = s.restore_guard().unwrap();
    assert!(s
        .commit_prepared_restore(prepared, &guard, &dir.path().join("no.sqlite"), |_| Ok(()))
        .is_err());
    assert!(!dir.path().join("no.sqlite").exists());
    let prepared = prepare_restore(&snap, |_| Ok(())).unwrap();
    s.commit_prepared_restore_with_writing_ack(
        prepared,
        &guard,
        &dir.path().join("yes.sqlite"),
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        true,
        |_| Ok(()),
    )
    .unwrap();
    let archived = s.get_constrained_proposal(&b, &q.id).unwrap();
    assert!(archived.live_epoch.is_none());
    assert_eq!(archived.author_review, Some(review.clone()));
    let restored_epoch = s.epoch().unwrap();
    assert_ne!(e, restored_epoch);
    assert!(s
        .save_constrained_review(
            &b,
            &q.id,
            Some(review.revision),
            &restored_epoch,
            approved(&m)
        )
        .is_err());
    assert!(s
        .accept_reviewed_constrained_proposal(
            &b,
            &q.id,
            0,
            0,
            &restored_epoch,
            review.revision,
            true
        )
        .is_err());
    assert_eq!(
        s.get_writing_state(&b, &c).unwrap().policy.unwrap().mode,
        "paused"
    );
    assert!(s.restore(&old).is_err());
    s.restore_with_writing_ack(&old, true).unwrap();
    assert!(s.get_writing_state(&b, &c).unwrap().policy.is_none());
}

struct TestDir(std::path::PathBuf);
impl TestDir {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("writing-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn pause_disable_cancels_without_revival() {
    let (mut s, b, c, m) = setup();
    let p = configure(&mut s, &b, &c, &m);
    let e = s.epoch().unwrap();
    let pack = s.prepare_writing_pack(&b, &c, p.revision, 0, &e).unwrap();
    let q = s
        .propose_constrained_edit(&b, "a", input(&pack, &m))
        .unwrap();
    let paused = s
        .configure_writing_policy(&b, &c, Some(0), "paused", "过河", vec![m.clone()], &e)
        .unwrap();
    assert_eq!(
        s.get_constrained_proposal(&b, &q.id).unwrap().decision,
        "cancelled"
    );
    assert!(s
        .prepare_writing_pack(&b, &c, paused.revision, 0, &e)
        .is_err());
    s.configure_writing_policy(
        &b,
        &c,
        Some(paused.revision),
        "disabled",
        "过河",
        vec![m],
        &e,
    )
    .unwrap();
    assert!(s
        .decide_constrained_proposal(&b, &q.id, true, 2, 0, &e, false)
        .is_err());
    assert!(s
        .propose_edit(ProposalInput {
            client_id: "a".into(),
            chapter_id: c,
            base_revision: 0,
            new_body: "human-approved ordinary route".into(),
            reason: "".into(),
            idempotency_key: "new".into()
        })
        .is_ok());
}
#[test]
fn scalar_quotes_and_utf8_chunks() {
    let (mut s, b, c, m) = setup();
    let text = "🙂中文".repeat(11000);
    s.apply(Command::SaveChapter {
        id: c.clone(),
        body: text.clone(),
        expected_revision: 0,
    })
    .unwrap();
    let p = configure(&mut s, &b, &c, &m);
    let pack = s
        .prepare_writing_pack(&b, &c, p.revision, 1, &s.epoch().unwrap())
        .unwrap();
    let mut cursor = 0;
    let mut body = String::new();
    loop {
        let chunk = s
            .read_writing_pack_entry(&b, &pack.id, &format!("chapter:{c}"), "body", cursor)
            .unwrap();
        assert!(chunk.text.len() <= 32768);
        body.push_str(&chunk.text);
        if let Some(next) = chunk.next_cursor {
            assert!(next > cursor);
            cursor = next;
        } else {
            break;
        }
    }
    assert_eq!(body, text);
    assert!(s
        .read_writing_pack_entry(&b, &pack.id, &format!("chapter:{c}"), "body", 1)
        .is_err());
    let mut i = input(&pack, &m);
    i.citations[0].end = 6;
    assert!(s.propose_constrained_edit(&b, "a", i).is_ok());
}
#[test]
fn policy_limits_cross_book_and_version_conflicts() {
    let (mut s, b, c, m) = setup();
    let e = s.epoch().unwrap();
    assert!(s
        .configure_writing_policy(&b, &c, None, "active", "", vec![m.clone()], &e)
        .is_err());
    assert!(s
        .configure_writing_policy(&b, &c, None, "active", "x", vec![], &e)
        .is_err());
    assert!(s
        .configure_writing_policy(&b, &c, None, "active", "x", vec![m.clone(); 33], &e)
        .is_err());
    let st = s
        .apply(Command::CreateBook {
            title: "other".into(),
            author: "".into(),
        })
        .unwrap();
    let other = st.books.iter().find(|x| x.id != b).unwrap().id.clone();
    assert!(s
        .configure_writing_policy(&other, &c, None, "active", "x", vec![m.clone()], &e)
        .is_err());
    let p = configure(&mut s, &b, &c, &m);
    assert!(s
        .configure_writing_policy(&b, &c, None, "active", "x", vec![m], &e)
        .is_err());
    assert!(s.prepare_writing_pack(&b, &c, p.revision, 999, &e).is_err());
}
#[test]
fn restored_material_requires_review_and_deleted_dependency_blocks_pack() {
    let (mut s, b, c, m) = setup();
    let p = configure(&mut s, &b, &c, &m);
    let e = s.epoch().unwrap();
    s.trash_material(&m, 0).unwrap();
    assert!(s.prepare_writing_pack(&b, &c, p.revision, 0, &e).is_err());
    s.configure_writing_policy(&b, &c, Some(0), "disabled", "过河", vec![m], &e)
        .unwrap();
}
#[test]
fn changed_book_metadata_and_body_block_submit_and_approve() {
    let (mut s, b, c, m) = setup();
    let p = configure(&mut s, &b, &c, &m);
    let e = s.epoch().unwrap();
    let pack = s.prepare_writing_pack(&b, &c, p.revision, 0, &e).unwrap();
    s.apply(Command::UpdateBook {
        id: b.clone(),
        title: "新名".into(),
        author: "".into(),
    })
    .unwrap();
    assert!(s
        .propose_constrained_edit(&b, "a", input(&pack, &m))
        .is_err());
    let fresh = s.prepare_writing_pack(&b, &c, p.revision, 0, &e).unwrap();
    let q = s
        .propose_constrained_edit(&b, "a", input(&fresh, &m))
        .unwrap();
    s.apply(Command::SaveChapter {
        id: c.clone(),
        body: "human".into(),
        expected_revision: 0,
    })
    .unwrap();
    assert!(s
        .decide_constrained_proposal(&b, &q.id, true, p.revision, 1, &e, false)
        .is_err());
    assert_eq!(s.get_chapter(&c).unwrap().body, "human");
}
#[test]
fn invalid_backup_evidence_is_atomic() {
    let (mut s, b, c, m) = setup();
    let p = configure(&mut s, &b, &c, &m);
    let e = s.epoch().unwrap();
    s.prepare_writing_pack(&b, &c, p.revision, 0, &e).unwrap();
    let old = s.backup().unwrap();
    let mut v: serde_json::Value = serde_json::from_str(&old).unwrap();
    v["writing_evidence"]["packs"][0]["expires_at"] = json!(1);
    assert!(s.restore_with_writing_ack(&v.to_string(), true).is_err());
    assert_eq!(s.epoch().unwrap(), e);
    assert_eq!(s.backup().unwrap(), old);
}
#[test]
fn evidence_survives_target_purge_and_book_purge_cascades() {
    let (mut s, b, c, m) = setup();
    let p = configure(&mut s, &b, &c, &m);
    let pack = s
        .prepare_writing_pack(&b, &c, p.revision, 0, &s.epoch().unwrap())
        .unwrap();
    let q = s
        .propose_constrained_edit(&b, "a", input(&pack, &m))
        .unwrap();
    s.apply(Command::TrashChapter { id: c.clone() }).unwrap();
    s.apply(Command::PurgeChapter { id: c }).unwrap();
    assert_eq!(s.get_writing_pack(&b, &pack.id).unwrap().materials[0].id, m);
    let backup = s.backup().unwrap();
    s.restore_with_writing_ack(&backup, true).unwrap();
    assert!(s
        .get_constrained_proposal(&b, &q.id)
        .unwrap()
        .live_epoch
        .is_none());
    s.apply(Command::TrashBook { id: b.clone() }).unwrap();
    s.apply(Command::PurgeBook { id: b.clone() }).unwrap();
    assert!(s.get_writing_pack(&b, &pack.id).is_err());
}
#[test]
fn pending_quota_and_semantic_disclaimer_have_distinct_roles() {
    let (mut s, b, c, m) = setup();
    let p = configure(&mut s, &b, &c, &m);
    let e = s.epoch().unwrap();
    let pack = s.prepare_writing_pack(&b, &c, p.revision, 0, &e).unwrap();
    for n in 0..20 {
        let mut i = input(&pack, &m);
        i.idempotency_key = format!("key{n}");
        i.new_body = "他游过了河。".into();
        let q = s.propose_constrained_edit(&b, "a", i).unwrap();
        assert_eq!(q.decision, "pending");
    }
    let mut i = input(&pack, &m);
    i.idempotency_key = "twentyfirst".into();
    assert!(s.propose_constrained_edit(&b, "a", i).is_err());
    assert_eq!(s.get_chapter(&c).unwrap().body, "");
}
#[test]
fn complete_required_materials_over_four_mib_are_not_truncated() {
    let (mut s, b, c, m) = setup();
    let mut ids = vec![m];
    for n in 0..17 {
        let m=s.create_material(serde_json::from_value(json!({"book_id":b,"kind":"setting","title":format!("规则{n}"),"fields":{"synopsis":"x".repeat(250*1024)}})).unwrap()).unwrap();
        ids.push(m.id);
    }
    let e = s.epoch().unwrap();
    s.configure_writing_policy(&b, &c, None, "active", "完整依据", ids, &e)
        .unwrap();
    assert!(s.prepare_writing_pack(&b, &c, 0, 0, &e).is_err());
    assert!(s.get_writing_state(&b, &c).unwrap().packs.is_empty());
}
#[test]
fn sqlite_schema_generation_and_alias_tampering_are_validated() {
    let (mut s, b, c, m) = setup();
    let p = configure(&mut s, &b, &c, &m);
    s.prepare_writing_pack(&b, &c, p.revision, 0, &s.epoch().unwrap())
        .unwrap();
    let dir = TestDir::new();
    let malformed = dir.path().join("malformed.sqlite");
    s.recovery_snapshot(&malformed).unwrap();
    {
        let db = rusqlite::Connection::open(&malformed).unwrap();
        db.execute("UPDATE writing_packs SET byte_count=1", [])
            .unwrap();
    }
    assert!(prepare_restore(&malformed, |_| Ok(())).is_err());
    let partial = dir.path().join("partial.sqlite");
    s.recovery_snapshot(&partial).unwrap();
    {
        let db = rusqlite::Connection::open(&partial).unwrap();
        db.execute_batch("DROP TABLE writing_proposals;").unwrap();
    }
    assert!(prepare_restore(&partial, |_| Ok(())).is_err());
    let old = dir.path().join("old.sqlite");
    s.recovery_snapshot(&old).unwrap();
    {
        let db = rusqlite::Connection::open(&old).unwrap();
        db.execute_batch(
            "DROP TABLE writing_proposals;DROP TABLE writing_packs;DROP TABLE writing_policies;",
        )
        .unwrap();
    }
    let candidate = prepare_restore(&old, |_| Ok(())).unwrap();
    assert_eq!(candidate.summary().writing_policies, 0);
    let guard = s.restore_guard().unwrap();
    assert!(s
        .commit_prepared_restore(candidate, &guard, &dir.path().join("no.sqlite"), |_| Ok(()))
        .is_err());
    assert_eq!(
        s.get_writing_state(&b, &c).unwrap().policy.unwrap().mode,
        "active"
    );
}
#[test]
fn escaped_goal_remains_readable_backupable_and_disableable() {
    let (mut s, b, c, m) = setup();
    let e = s.epoch().unwrap();
    let goal = format!("x{}", "\0".repeat(4095));
    let p = s
        .configure_writing_policy(&b, &c, None, "active", &goal, vec![m.clone()], &e)
        .unwrap();
    assert_eq!(
        s.get_writing_state(&b, &c).unwrap().policy.unwrap().goal,
        goal
    );
    s.backup().unwrap();
    s.configure_writing_policy(&b, &c, Some(p.revision), "disabled", "done", vec![m], &e)
        .unwrap();
}
#[test]
fn cyclic_pack_and_proposal_payload_aliases_are_rejected() {
    let (mut s, b, c, m) = setup();
    let p = configure(&mut s, &b, &c, &m);
    let e = s.epoch().unwrap();
    let a = s.prepare_writing_pack(&b, &c, p.revision, 0, &e).unwrap();
    let z = s.prepare_writing_pack(&b, &c, p.revision, 0, &e).unwrap();
    let qa = s.propose_constrained_edit(&b, "a", input(&a, &m)).unwrap();
    let mut i = input(&a, &m);
    i.idempotency_key = "kez".into();
    let qz = s.propose_constrained_edit(&b, "a", i).unwrap();
    let dir = TestDir::new();
    for (table, x, y) in [
        ("writing_packs", a.id, z.id),
        ("writing_proposals", qa.id, qz.id),
    ] {
        let path = dir.path().join(format!("{table}.sqlite"));
        s.recovery_snapshot(&path).unwrap();
        {
            let db = rusqlite::Connection::open(&path).unwrap();
            let a: String = db
                .query_row(
                    &format!("SELECT payload FROM {table} WHERE id=?1"),
                    [&x],
                    |r| r.get(0),
                )
                .unwrap();
            let z: String = db
                .query_row(
                    &format!("SELECT payload FROM {table} WHERE id=?1"),
                    [&y],
                    |r| r.get(0),
                )
                .unwrap();
            db.execute(
                &format!("UPDATE {table} SET payload=?1 WHERE id=?2"),
                rusqlite::params![z, x],
            )
            .unwrap();
            db.execute(
                &format!("UPDATE {table} SET payload=?1 WHERE id=?2"),
                rusqlite::params![a, y],
            )
            .unwrap();
        }
        assert!(
            prepare_restore(&path, |_| Ok(())).is_err(),
            "{table} aliases accepted"
        );
    }
}
#[test]
fn frozen_material_provenance_cannot_claim_impossible_current_or_deleted() {
    let (mut s, b, c, _) = setup();
    s.apply(Command::SaveChapter {
        id: c.clone(),
        body: "source".into(),
        expected_revision: 0,
    })
    .unwrap();
    let m=s.create_material(serde_json::from_value(json!({"book_id":b,"kind":"summary","title":"摘要","fields":{"content":"不能游泳"},"source_chapter_id":c,"source_revision":1})).unwrap()).unwrap();
    configure(&mut s, &b, &c, &m.id);
    s.prepare_writing_pack(&b, &c, 0, 1, &s.epoch().unwrap())
        .unwrap();
    let original: serde_json::Value = serde_json::from_str(&s.backup().unwrap()).unwrap();
    for kind in 0..3 {
        let mut bad = original.clone();
        let card = &mut bad["writing_evidence"]["packs"][0]["materials"][0];
        match kind {
            0 => card["source_revision"] = json!(0),
            1 => {
                card.as_object_mut().unwrap().remove("source_status");
            }
            _ => card["deleted_at"] = json!(1),
        }
        assert!(
            s.restore_with_writing_ack(&bad.to_string(), true).is_err(),
            "mutation {kind} accepted"
        );
    }
}
#[test]
fn namespaced_handles_support_arbitrary_imported_ids_without_coverage_collision() {
    let (mut s, b, c, m) = setup();
    let backup = s.backup().unwrap().replace(&m, "$chapter");
    s.restore(&backup).unwrap();
    let p = configure(&mut s, &b, &c, "$chapter");
    let pack = s
        .prepare_writing_pack(&b, &c, p.revision, 0, &s.epoch().unwrap())
        .unwrap();
    let entries: Vec<_> = pack.entries.iter().map(|e| e.id.as_str()).collect();
    assert!(entries.contains(&"material:$chapter"));
    assert!(entries.contains(&format!("chapter:{c}").as_str()));
    let mut wrong = input(&pack, "$chapter");
    wrong.citations[0].entry_id = format!("chapter:{c}");
    wrong.citations[0].field_key = "$title".into();
    wrong.citations[0].end = 1;
    assert!(s.propose_constrained_edit(&b, "a", wrong).is_err());
    let q = s
        .propose_constrained_edit(&b, "a", input(&pack, "$chapter"))
        .unwrap();
    assert_eq!(q.decision, "pending");
    let mut legacy: serde_json::Value = serde_json::from_str(&s.backup().unwrap()).unwrap();
    legacy["writing_evidence"]["packs"][0]
        .as_object_mut()
        .unwrap()
        .remove("handle_version");
    assert!(s
        .restore_with_writing_ack(&legacy.to_string(), true)
        .is_err());
}
#[test]
fn unambiguous_legacy_handles_preserve_archived_evidence() {
    let (mut s, b, c, m) = setup();
    let p = configure(&mut s, &b, &c, &m);
    let pack = s
        .prepare_writing_pack(&b, &c, p.revision, 0, &s.epoch().unwrap())
        .unwrap();
    let q = s
        .propose_constrained_edit(&b, "a", input(&pack, &m))
        .unwrap();
    let mut legacy: serde_json::Value = serde_json::from_str(&s.backup().unwrap()).unwrap();
    legacy["writing_evidence"]["packs"][0]
        .as_object_mut()
        .unwrap()
        .remove("handle_version");
    legacy["writing_evidence"]["proposals"][0]["input"]["citations"][0]["entry_id"] = json!(m);
    s.restore_with_writing_ack(&legacy.to_string(), true)
        .unwrap();
    assert!(s
        .get_constrained_proposal(&b, &q.id)
        .unwrap()
        .live_epoch
        .is_none());
    assert_eq!(s.get_writing_pack(&b, &pack.id).unwrap().handle_version, 1);
    assert!(s
        .get_writing_pack_manifest(&b, &pack.id)
        .unwrap()
        .entries
        .iter()
        .any(|e| e.id == m));
}
#[test]
fn cyclic_policy_records_are_rejected_using_selected_row_identity() {
    let (mut s, b, c, m) = setup();
    configure(&mut s, &b, &c, &m);
    let st = s
        .apply(Command::CreateChapter {
            volume_id: s.get_chapter(&c).unwrap().volume_id,
            title: "第二章".into(),
        })
        .unwrap();
    let second = st.chapters.iter().find(|x| x.id != c).unwrap().id.clone();
    s.configure_writing_policy(
        &b,
        &second,
        None,
        "active",
        "过河",
        vec![m],
        &s.epoch().unwrap(),
    )
    .unwrap();
    let dir = TestDir::new();
    let path = dir.path().join("policy.sqlite");
    s.recovery_snapshot(&path).unwrap();
    {
        let db = rusqlite::Connection::open(&path).unwrap();
        let a: String = db
            .query_row(
                "SELECT record FROM writing_policies WHERE chapter_id=?1",
                [&c],
                |r| r.get(0),
            )
            .unwrap();
        let z: String = db
            .query_row(
                "SELECT record FROM writing_policies WHERE chapter_id=?1",
                [&second],
                |r| r.get(0),
            )
            .unwrap();
        db.execute(
            "UPDATE writing_policies SET record=?1 WHERE chapter_id=?2",
            rusqlite::params![z, c],
        )
        .unwrap();
        db.execute(
            "UPDATE writing_policies SET record=?1 WHERE chapter_id=?2",
            rusqlite::params![a, second],
        )
        .unwrap();
    }
    assert!(prepare_restore(&path, |_| Ok(())).is_err());
}

#[test]
fn legacy_strict_approval_without_review_cannot_write_body_or_history() {
    let (mut s, b, c, m) = setup();
    configure(&mut s, &b, &c, &m);
    let e = s.epoch().unwrap();
    let pack = s.prepare_writing_pack(&b, &c, 0, 0, &e).unwrap();
    let q = s
        .propose_constrained_edit(&b, "author-review-fixture", input(&pack, &m))
        .unwrap();
    let history = s.chapter_history(&c).unwrap();
    assert!(s
        .decide_constrained_proposal(&b, &q.id, true, 0, 0, &e, true)
        .is_err());
    assert_eq!(s.get_chapter(&c).unwrap().body, "");
    assert_eq!(s.chapter_history(&c).unwrap().len(), history.len());
    assert_eq!(
        s.get_constrained_proposal(&b, &q.id).unwrap().decision,
        "pending"
    );
    assert_eq!(
        s.decide_constrained_proposal(&b, &q.id, false, 0, 0, &e, false)
            .unwrap()
            .decision,
        "rejected"
    );
}

#[test]
fn backups_write_version_eleven_and_can_read_reviewless_version_nine() {
    let (mut s, b, c, m) = setup();
    configure(&mut s, &b, &c, &m);
    let e = s.epoch().unwrap();
    let pack = s.prepare_writing_pack(&b, &c, 0, 0, &e).unwrap();
    let q = s
        .propose_constrained_edit(&b, "old-fixture", input(&pack, &m))
        .unwrap();
    s.apply(Command::SaveChapter {
        id: c,
        body: q.input.new_body.clone(),
        expected_revision: 0,
    })
    .unwrap();
    let mut backup: serde_json::Value = serde_json::from_str(&s.backup().unwrap()).unwrap();
    assert_eq!(backup["version"], 13);
    backup["version"] = json!(9);
    backup.as_object_mut().unwrap().remove("foreshadow_proposals");
    backup["writing_evidence"]["packs"][0]["handle_version"] = json!(2);
    let p = &mut backup["writing_evidence"]["proposals"][0];
    p["decision"] = json!("accepted");
    p["accepted_revision"] = json!(1);
    p["decided_at"] = p["created_at"].clone();
    s.restore_with_writing_ack(&backup.to_string(), true)
        .unwrap();
    let history = serde_json::to_value(s.get_constrained_proposal(&b, &q.id).unwrap()).unwrap();
    assert_eq!(history["decision"], "accepted");
    assert!(history["live_epoch"].is_null());
    assert!(history["author_review"].is_null());
    assert_eq!(s.get_chapter(&q.chapter_id).unwrap().body, q.input.new_body);
    let exported = s.backup().unwrap();
    s.restore_with_writing_ack(&exported, true).unwrap();
    assert!(s
        .get_constrained_proposal(&b, &q.id)
        .unwrap()
        .author_review
        .is_none());
}

fn approved(material_id: &str) -> Vec<AuthorReviewItem> {
    vec![AuthorReviewItem {
        material_id: material_id.into(),
        verdict: AuthorReviewVerdict::Conforms,
        note: "作者核对：坐船，未游泳".into(),
    }]
}

#[test]
fn exact_item_set_and_latest_conforming_revision_required() {
    let (mut s, b, c, m) = setup();
    configure(&mut s, &b, &c, &m);
    let e = s.epoch().unwrap();
    let pack = s.prepare_writing_pack(&b, &c, 0, 0, &e).unwrap();
    let q = s
        .propose_constrained_edit(&b, "a", input(&pack, &m))
        .unwrap();
    assert!(s
        .accept_reviewed_constrained_proposal(&b, &q.id, 0, 0, &e, 1, true)
        .is_err());
    assert!(s
        .save_constrained_review(&b, &q.id, None, &e, vec![])
        .is_err());
    assert!(s
        .save_constrained_review(&b, &q.id, None, &e, approved("other-pack-card"))
        .is_err());
    assert!(s
        .save_constrained_review(&b, &q.id, None, &e, [approved(&m), approved(&m)].concat())
        .is_err());
    assert!(s
        .save_constrained_review("another-book", &q.id, None, &e, approved(&m))
        .is_err());
    assert!(s
        .save_constrained_review(&b, &q.id, None, "another-epoch", approved(&m))
        .is_err());
    let mut too_big = approved(&m);
    too_big[0].note = "界".repeat(1366);
    assert!(s
        .save_constrained_review(&b, &q.id, None, &e, too_big)
        .is_err());
    assert!(s
        .get_constrained_proposal(&b, &q.id)
        .unwrap()
        .author_review
        .is_none());
    let mut expected = None;
    for (i, verdict) in [
        AuthorReviewVerdict::Unreviewed,
        AuthorReviewVerdict::Question,
        AuthorReviewVerdict::Conflict,
    ]
    .into_iter()
    .enumerate()
    {
        let item = AuthorReviewItem {
            material_id: m.clone(),
            verdict,
            note: "待处理".into(),
        };
        let review = s
            .save_constrained_review(&b, &q.id, expected, &e, vec![item])
            .unwrap();
        assert_eq!(review.revision, i as i64 + 1);
        assert!(s
            .accept_reviewed_constrained_proposal(&b, &q.id, 0, 0, &e, review.revision, true)
            .is_err());
        expected = Some(review.revision);
    }
    let mut boundary = approved(&m);
    boundary[0].note = format!("{}a", "界".repeat(1365));
    assert!(s
        .save_constrained_review(&b, &q.id, Some(2), &e, boundary.clone())
        .is_err());
    let review = s
        .save_constrained_review(&b, &q.id, expected, &e, boundary)
        .unwrap();
    assert_eq!(review.revision, 4);
    assert_eq!(
        s.get_constrained_proposal(&b, &q.id).unwrap().author_review,
        Some(review.clone())
    );
    assert!(s
        .accept_reviewed_constrained_proposal(&b, &q.id, 0, 0, &e, 3, true)
        .is_err());
    assert!(s
        .accept_reviewed_constrained_proposal(&b, &q.id, 1, 0, &e, 4, true)
        .is_err());
    assert!(s
        .accept_reviewed_constrained_proposal(&b, &q.id, 0, 1, &e, 4, true)
        .is_err());
    assert_eq!(s.get_chapter(&c).unwrap().body, "");
    let accepted = s
        .accept_reviewed_constrained_proposal(&b, &q.id, 0, 0, &e, 4, false)
        .unwrap();
    assert_eq!(accepted.author_review, Some(review));
    assert_eq!(s.get_chapter(&c).unwrap().body, "角色坐船过河");
    assert!(s
        .save_constrained_review(&b, &q.id, Some(4), &e, approved(&m))
        .is_err());
}

#[test]
fn total_review_encoding_limit_accounts_for_escaped_notes() {
    let (mut s, b, c, first) = setup();
    let mut ids = vec![first];
    for index in 1..3 {
        let m=s.create_material(serde_json::from_value(json!({"book_id":b,"kind":"setting","title":format!("规则 {index}"),"fields":{"synopsis":"不能游泳"}})).unwrap()).unwrap();
        ids.push(m.id);
    }
    let e = s.epoch().unwrap();
    s.configure_writing_policy(&b, &c, None, "active", "过河", ids.clone(), &e)
        .unwrap();
    let pack = s.prepare_writing_pack(&b, &c, 0, 0, &e).unwrap();
    let mut i = input(&pack, &ids[0]);
    for id in ids.iter().skip(1) {
        let mut citation = i.citations[0].clone();
        citation.entry_id = format!("material:{id}");
        i.citations.push(citation);
    }
    let q = s.propose_constrained_edit(&b, "a", i).unwrap();
    let items: Vec<_> = ids
        .iter()
        .map(|id| AuthorReviewItem {
            material_id: id.clone(),
            verdict: AuthorReviewVerdict::Conforms,
            note: "\0".repeat(4096),
        })
        .collect();
    assert!(s
        .save_constrained_review(&b, &q.id, None, &e, items)
        .is_err());
    assert!(s
        .get_constrained_proposal(&b, &q.id)
        .unwrap()
        .author_review
        .is_none());
}

#[test]
fn json10_rejects_tampered_author_review_and_restores_valid_history() {
    let (mut s, b, c, m) = setup();
    configure(&mut s, &b, &c, &m);
    let e = s.epoch().unwrap();
    let pack = s.prepare_writing_pack(&b, &c, 0, 0, &e).unwrap();
    let q = s
        .propose_constrained_edit(&b, "a", input(&pack, &m))
        .unwrap();
    let review = s
        .save_constrained_review(&b, &q.id, None, &e, approved(&m))
        .unwrap();
    s.accept_reviewed_constrained_proposal(&b, &q.id, 0, 0, &e, review.revision, false)
        .unwrap();
    let original = s.backup().unwrap();
    let mut v10: serde_json::Value = serde_json::from_str(&original).unwrap();
    v10["version"] = json!(10);
    v10.as_object_mut().unwrap().remove("foreshadow_proposals");
    v10["writing_evidence"]["packs"][0]["handle_version"] = json!(2);
    let mut downgraded = v10.clone();
    downgraded["version"] = json!(9);
    assert!(s
        .restore_with_writing_ack(&downgraded.to_string(), true)
        .is_err());
    downgraded["writing_evidence"]["proposals"][0]["author_review"] = json!(null);
    assert!(s
        .restore_with_writing_ack(&downgraded.to_string(), true)
        .is_err());
    assert_eq!(s.backup().unwrap(), original);
    for (field, value) in [
        ("proposal_id", json!("other")),
        ("pack_id", json!("other")),
        ("book_id", json!("other")),
        ("chapter_id", json!("other")),
        ("policy_revision", json!(1)),
        ("chapter_revision", json!(1)),
        ("originating_epoch", json!("other")),
        ("revision", json!(0)),
        ("revision", json!(9_007_199_254_740_001i64)),
        ("items", json!([])),
        (
            "items",
            json!([{"material_id":m,"verdict":"conflict","note":""}]),
        ),
        (
            "items",
            json!([{"material_id":m,"verdict":"autocertified","note":""}]),
        ),
        (
            "items",
            json!([{"material_id":m,"verdict":"conforms","note":"界".repeat(1366)}]),
        ),
    ] {
        let mut bad = v10.clone();
        bad["writing_evidence"]["proposals"][0]["author_review"][field] = value;
        assert!(
            s.restore_with_writing_ack(&bad.to_string(), true).is_err(),
            "accepted invalid {field}"
        );
        assert_eq!(s.backup().unwrap(), original);
    }
    s.restore_with_writing_ack(&v10.to_string(), true).unwrap();
    let archived = s.get_constrained_proposal(&b, &q.id).unwrap();
    assert_eq!(archived.author_review, Some(review));
    assert_eq!(archived.decision, "accepted");
    assert!(archived.live_epoch.is_none());
}

#[test]
fn near_limit_proposal_remains_readable_after_review_and_sqlite_snapshot() {
    let (mut s, b, c, m) = setup();
    configure(&mut s, &b, &c, &m);
    let e = s.epoch().unwrap();
    let pack = s.prepare_writing_pack(&b, &c, 0, 0, &e).unwrap();
    let mut i = input(&pack, &m);
    i.new_body = "\"".repeat(1_047_900);
    let q = s.propose_constrained_edit(&b, "a", i.clone()).unwrap();
    assert!(serde_json::to_vec(&q).unwrap().len() <= 2_097_152);
    let mut items = approved(&m);
    items[0].note = "界".repeat(1000);
    let r = s
        .save_constrained_review(&b, &q.id, None, &e, items)
        .unwrap();
    let stored = s.get_constrained_proposal(&b, &q.id).unwrap();
    assert!(serde_json::to_vec(&stored).unwrap().len() > 2_097_152);
    assert_eq!(stored.input, i);
    assert_eq!(
        s.propose_constrained_edit(&b, "a", stored.input.clone())
            .unwrap()
            .author_review,
        Some(r.clone())
    );
    let dir = TestDir::new();
    let snap = dir.path().join("near-limit.sqlite");
    s.recovery_snapshot(&snap).unwrap();
    prepare_restore(&snap, |_| Ok(())).unwrap();
    {
        let db = rusqlite::Connection::open(&snap).unwrap();
        let mut invalid = serde_json::to_value(&stored).unwrap();
        invalid["author_review"]["proposal_id"] = json!("other-proposal");
        let text = invalid.to_string();
        db.execute(
            "UPDATE writing_proposals SET payload=?1,byte_count=?2 WHERE id=?3",
            rusqlite::params![text, text.len(), q.id],
        )
        .unwrap();
    }
    assert!(prepare_restore(&snap, |_| Ok(())).is_err());
    assert_eq!(
        s.get_constrained_proposal(&b, &q.id).unwrap().author_review,
        Some(r)
    );
}

#[test]
fn exact_admission_boundary_allows_reviewed_acceptance_and_other_decisions() {
    const CAP: usize = 2_097_152;
    for (action, decision) in [
        ("accept", "accepted"),
        ("reject", "rejected"),
        ("withdraw", "cancelled"),
        ("configure", "cancelled"),
    ] {
        let (mut s, b, c, m) = setup();
        configure(&mut s, &b, &c, &m);
        let e = s.epoch().unwrap();
        let p = s.prepare_writing_pack(&b, &c, 0, 0, &e).unwrap();
        let empty = s
            .propose_constrained_edit(&b, "seed", input(&p, &m))
            .unwrap();
        let mut sized = empty.input.clone();
        sized.new_body.clear();
        let mut envelope = empty.clone();
        envelope.input = sized.clone();
        let room = CAP - serde_json::to_vec(&envelope).unwrap().len();
        // Same client-byte length, ID length and idempotency-key length as seed.
        sized.new_body = "\"".repeat(room / 2) + if room % 2 == 1 { "x" } else { "" };
        let mut over = sized.clone();
        over.new_body.push('x');
        assert!(s.propose_constrained_edit(&b, "over", over).is_err());
        let q = s.propose_constrained_edit(&b, "size", sized).unwrap();
        assert_eq!(serde_json::to_vec(&q).unwrap().len(), CAP);
        let saved = s
            .save_constrained_review(&b, &q.id, None, &e, approved(&m))
            .unwrap();
        match action {
            "accept" => {
                s.accept_reviewed_constrained_proposal(&b, &q.id, 0, 0, &e, saved.revision, false)
                    .unwrap_or_else(|e| panic!("{action}: {}", e.message));
                assert_eq!(s.get_chapter(&c).unwrap().body, q.input.new_body);
                assert_eq!(s.get_chapter(&c).unwrap().revision, 1);
            }
            "reject" => {
                s.decide_constrained_proposal(&b, &q.id, false, 0, 0, &e, false)
                    .unwrap_or_else(|e| panic!("{action}: {}", e.message));
            }
            "withdraw" => {
                s.cancel_constrained_proposal(&b, &q.id, "size")
                    .unwrap_or_else(|e| panic!("{action}: {}", e.message));
            }
            _ => {
                s.configure_writing_policy(&b, &c, Some(0), "paused", "暂停核对", vec![m], &e)
                    .unwrap_or_else(|e| panic!("{action}: {}", e.message));
            }
        }
        let stored = s.get_constrained_proposal(&b, &q.id).unwrap();
        assert_eq!(stored.decision, decision);
        assert_eq!(stored.author_review, Some(saved.clone()));
        assert_eq!(stored.input, q.input);
        let dir = TestDir::new();
        let db = dir.path().join("exact-terminal.sqlite");
        s.recovery_snapshot(&db).unwrap();
        prepare_restore(&db, |_| Ok(())).unwrap();
        let json = s.backup().unwrap();
        if action == "accept" {
            let mut invalid: serde_json::Value = serde_json::from_str(&json).unwrap();
            let proposal = invalid["writing_evidence"]["proposals"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|p| p["id"] == q.id)
                .unwrap();
            proposal["input"]["new_body"] = json!(q.input.new_body.clone() + "x");
            assert!(s
                .restore_with_writing_ack(&invalid.to_string(), true)
                .is_err());
            assert_eq!(s.get_constrained_proposal(&b, &q.id).unwrap(), stored);
        }
        s.restore_with_writing_ack(&json, true).unwrap();
        let archived = s.get_constrained_proposal(&b, &q.id).unwrap();
        assert_eq!(archived.decision, decision);
        assert!(archived.live_epoch.is_none());
        assert_eq!(archived.author_review, Some(saved));
    }
}
