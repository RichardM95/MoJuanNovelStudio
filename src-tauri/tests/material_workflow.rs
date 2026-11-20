use novel_core::*;
use std::collections::BTreeMap;
fn fixture() -> (Service, String, String, Material) {
    let mut s = Service::open(":memory:").unwrap();
    let a = s
        .apply(Command::CreateBook {
            title: "fixture".into(),
            author: "".into(),
        })
        .unwrap();
    let b = a.books[0].id.clone();
    let c = a.chapters[0].id.clone();
    let m = s
        .create_material(CreateMaterialInput {
            book_id: b.clone(),
            kind: "summary".into(),
            title: "summary".into(),
            fields: BTreeMap::from([("content".into(), "original summary".into())]),
            volume_id: None,
            chapter_id: None,
            source_chapter_id: Some(c.clone()),
            source_revision: Some(0),
            story_order: 0,
            story_time: None,
            tracking_status: None,
            payoff_chapter_id: None,
            payoff_revision: None,
        })
        .unwrap();
    (s, b, c, m)
}
fn input(m: &Material, key: &str) -> MaterialProposalInput {
    MaterialProposalInput {
        client_id: "fixture-agent".into(),
        idempotency_key: key.into(),
        material_id: m.id.clone(),
        base_revision: m.revision,
        new_title: "suggested title".into(),
        new_fields: BTreeMap::from([("content".into(), "suggested summary".into())]),
        new_source_revision: m.source_revision,
        reason: "fixture review".into(),
    }
}
#[test]
fn material_proposals_are_immutable_idempotent_and_review_only() {
    let (mut s, b, _, m) = fixture();
    let p = s.propose_material_edit(input(&m, "key")).unwrap();
    assert_eq!(s.get_material(&m.id, false).unwrap(), m);
    assert_eq!(p.old_snapshot, m);
    assert_eq!(s.propose_material_edit(input(&m, "key")).unwrap(), p);
    let mut changed = input(&m, "key");
    changed.new_title = "different".into();
    assert_eq!(
        s.propose_material_edit(changed).unwrap_err().code,
        "idempotency_conflict"
    );
    assert!(s.get_material_proposal(&p.id, "other").is_err());
    assert_eq!(s.list_material_proposals(&b).unwrap().len(), 1);
    let saved = s.decide_material_proposal(&p.id, true, 0).unwrap();
    assert_eq!(saved.title, "suggested title");
    assert_eq!(saved.revision, 1);
    assert!(s.list_material_proposals(&b).unwrap().is_empty());
    assert_eq!(
        s.get_material_proposal(&p.id, "fixture-agent")
            .unwrap()
            .status,
        "accepted"
    );
    assert_eq!(
        s.propose_material_edit(input(&m, "key")).unwrap().status,
        "accepted"
    );
    assert!(s.decide_material_proposal(&p.id, true, 1).is_err());
}
#[test]
fn source_cas_blocks_acceptance_but_stale_rejection_is_available() {
    let (mut s, _, c, m) = fixture();
    let p = s.propose_material_edit(input(&m, "old")).unwrap();
    s.apply(Command::SaveChapter {
        id: c,
        body: "new source".into(),
        expected_revision: 0,
    })
    .unwrap();
    assert_eq!(
        s.decide_material_proposal(&p.id, true, 0).unwrap_err().code,
        "revision_conflict"
    );
    assert_eq!(s.get_material(&m.id, false).unwrap().fields, m.fields);
    assert_eq!(
        s.decide_material_proposal(&p.id, false, 0)
            .unwrap()
            .revision,
        0
    );
    assert_eq!(
        s.get_material_proposal(&p.id, "fixture-agent")
            .unwrap()
            .status,
        "rejected"
    );
    let mut next = input(&m, "fresh");
    next.new_source_revision = Some(1);
    let p = s.propose_material_edit(next).unwrap();
    assert_eq!(
        s.decide_material_proposal(&p.id, true, 0)
            .unwrap()
            .source_revision,
        Some(1)
    );
}
#[test]
fn only_own_pending_can_cancel_and_bad_fields_are_rejected() {
    let (mut s, b, _, m) = fixture();
    let mut bad = input(&m, "bad");
    bad.new_fields.insert("unknown".into(), "x".into());
    assert!(s.propose_material_edit(bad).is_err());
    assert!(s.list_material_proposals(&b).unwrap().is_empty());
    let p = s.propose_material_edit(input(&m, "cancel")).unwrap();
    assert!(s.cancel_material_proposal(&p.id, "other").is_err());
    assert_eq!(
        s.cancel_material_proposal(&p.id, "fixture-agent")
            .unwrap()
            .status,
        "cancelled"
    );
    assert!(s.cancel_material_proposal(&p.id, "fixture-agent").is_err());
    assert_eq!(s.get_material(&m.id, false).unwrap().revision, 0);
}
#[test]
fn material_revision_conflict_does_not_prevent_rejecting_old_proposal() {
    let (mut s, _, _, m) = fixture();
    let p = s
        .propose_material_edit(input(&m, "stale-material"))
        .unwrap();
    s.save_material(SaveMaterialInput {
        id: m.id.clone(),
        title: "human title".into(),
        fields: m.fields.clone(),
        volume_id: m.volume_id.clone(),
        chapter_id: m.chapter_id.clone(),
        source_chapter_id: m.source_chapter_id.clone(),
        source_revision: m.source_revision,
        story_order: 0,
        story_time: None,
        tracking_status: None,
        payoff_chapter_id: None,
        payoff_revision: None,
        expected_revision: 0,
    })
    .unwrap();
    assert_eq!(
        s.decide_material_proposal(&p.id, true, 1).unwrap_err().code,
        "revision_conflict"
    );
    assert_eq!(
        s.decide_material_proposal(&p.id, false, 0).unwrap().title,
        "human title"
    );
}
#[test]
fn concurrent_material_accepts_have_exactly_one_winner() {
    use std::sync::{Arc, Barrier, Mutex};
    let (mut s, b, _, m) = fixture();
    let p = s.propose_material_edit(input(&m, "race1")).unwrap();
    let q = s.propose_material_edit(input(&m, "race2")).unwrap();
    let s = Arc::new(Mutex::new(s));
    let barrier = Arc::new(Barrier::new(3));
    let workers = [p.id, q.id].map(|id| {
        let s = s.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            s.lock().unwrap().decide_material_proposal(&id, true, 0)
        })
    });
    barrier.wait();
    let results = workers.map(|w| w.join().unwrap());
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert!(results.iter().any(|r| r
        .as_ref()
        .err()
        .is_some_and(|e| e.code == "revision_conflict")));
    assert_eq!(
        s.lock()
            .unwrap()
            .get_material(&m.id, false)
            .unwrap()
            .revision,
        1
    );
    assert_eq!(
        s.lock()
            .unwrap()
            .list_material_proposals(&b)
            .unwrap()
            .iter()
            .filter(|p| p.status == "pending")
            .count(),
        1
    );
}
#[test]
fn unavailable_source_blocks_acceptance_without_deleting_old_snapshot() {
    let (mut s, _, c, m) = fixture();
    let p = s.propose_material_edit(input(&m, "unavailable")).unwrap();
    s.apply(Command::TrashChapter { id: c.clone() }).unwrap();
    assert!(s.decide_material_proposal(&p.id, true, 0).is_err());
    s.apply(Command::PurgeChapter { id: c }).unwrap();
    assert!(s.decide_material_proposal(&p.id, true, 0).is_err());
    assert_eq!(
        s.get_material_proposal(&p.id, "fixture-agent")
            .unwrap()
            .old_snapshot
            .fields,
        m.fields
    );
    assert_eq!(
        s.decide_material_proposal(&p.id, false, 0)
            .unwrap()
            .revision,
        0
    );
}
#[test]
fn full_restore_clears_material_proposals_and_rotates_epoch() {
    let (mut s, b, _, m) = fixture();
    let p = s.propose_material_edit(input(&m, "restore")).unwrap();
    let before = s.epoch().unwrap();
    let backup = s.backup().unwrap();
    assert!(!backup.contains("fixture review"));
    s.restore(&backup).unwrap();
    assert_ne!(s.epoch().unwrap(), before);
    assert!(s.list_material_proposals(&b).unwrap().is_empty());
    assert!(s.get_material_proposal(&p.id, "fixture-agent").is_err());
    assert_eq!(s.get_material(&m.id, false).unwrap().fields, m.fields);
}
struct DiskFixture(std::path::PathBuf);
impl Drop for DiskFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn material_acceptance_audit_failure_rolls_back_every_change() {
    let f = DiskFixture(
        std::env::temp_dir().join(format!("material-workflow-{}", uuid::Uuid::new_v4())),
    );
    std::fs::create_dir(&f.0).unwrap();
    let path = f.0.join("fixture.sqlite");
    let (base, _, _, m) = fixture();
    let backup = base.backup().unwrap();
    let mut s = Service::open(&path).unwrap();
    s.restore(&backup).unwrap();
    let p = s.propose_material_edit(input(&m, "atomic")).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_material_audit BEFORE INSERT ON material_proposal_audit WHEN NEW.action='accept' BEGIN SELECT RAISE(ABORT,'fixture fail'); END;").unwrap();
    assert!(s.decide_material_proposal(&p.id, true, 0).is_err());
    assert_eq!(s.get_material(&m.id, false).unwrap(), m);
    assert_eq!(
        s.get_material_proposal(&p.id, "fixture-agent")
            .unwrap()
            .status,
        "pending"
    );
    assert_eq!(
        conn.query_row("SELECT count(*) FROM material_proposal_audit", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    drop(conn);
    drop(s);
}
#[test]
fn material_proposal_pending_and_bytes_quotas_preserve_idempotent_retries() {
    let (mut s, _, _, m) = fixture();
    let first = s.propose_material_edit(input(&m, "0")).unwrap();
    for i in 1..20 {
        s.propose_material_edit(input(&m, &i.to_string())).unwrap();
    }
    assert_eq!(
        s.propose_material_edit(input(&m, "limit"))
            .unwrap_err()
            .code,
        "limit_exceeded"
    );
    assert_eq!(s.propose_material_edit(input(&m, "0")).unwrap(), first);
    let (mut s, _, _, m) = fixture();
    let mut hit = false;
    for i in 0..100 {
        let mut p = input(&m, &format!("byte{i}"));
        p.new_fields
            .insert("content".into(), "x".repeat(256 * 1024));
        match s.propose_material_edit(p) {
            Ok(p) => {
                s.cancel_material_proposal(&p.id, "fixture-agent").unwrap();
            }
            Err(e) => {
                assert_eq!(e.code, "limit_exceeded");
                hit = true;
                break;
            }
        }
    }
    assert!(hit);
    assert_eq!(s.get_material(&m.id, false).unwrap().fields, m.fields);
}
#[test]
fn global_pending_and_all_status_record_limits_are_enforced() {
    let (mut s, b, c, m) = fixture();
    let mut cards = vec![m.clone()];
    for n in 0..5 {
        cards.push(
            s.create_material(CreateMaterialInput {
                book_id: b.clone(),
                kind: "summary".into(),
                title: format!("card{n}"),
                fields: BTreeMap::from([("content".into(), "text".into())]),
                volume_id: None,
                chapter_id: None,
                source_chapter_id: Some(c.clone()),
                source_revision: Some(0),
                story_order: 0,
                story_time: None,
                tracking_status: None,
                payoff_chapter_id: None,
                payoff_revision: None,
            })
            .unwrap(),
        );
    }
    for (n, card) in cards.iter().take(5).enumerate() {
        for i in 0..20 {
            s.propose_material_edit(input(card, &format!("pending{n}:{i}")))
                .unwrap();
        }
    }
    assert_eq!(
        s.propose_material_edit(input(&cards[5], "global-cap"))
            .unwrap_err()
            .code,
        "limit_exceeded"
    );
    let (mut s, _, _, m) = fixture();
    for n in 0..1000 {
        let p = s
            .propose_material_edit(input(&m, &format!("record{n}")))
            .unwrap();
        s.cancel_material_proposal(&p.id, "fixture-agent").unwrap();
    }
    assert_eq!(
        s.propose_material_edit(input(&m, "too-many"))
            .unwrap_err()
            .code,
        "limit_exceeded"
    );
    assert_eq!(s.get_material(&m.id, false).unwrap().fields, m.fields);
}
#[test]
fn review_list_contains_only_pending_proposals_for_active_cards() {
    let (mut s, b, _, m) = fixture();
    let p = s.propose_material_edit(input(&m, "listed")).unwrap();
    assert_eq!(s.list_material_proposals(&b).unwrap().len(), 1);
    s.cancel_material_proposal(&p.id, "fixture-agent").unwrap();
    assert!(s.list_material_proposals(&b).unwrap().is_empty());
    assert_eq!(
        s.get_material_proposal(&p.id, "fixture-agent")
            .unwrap()
            .status,
        "cancelled"
    );
    let p = s.propose_material_edit(input(&m, "hidden")).unwrap();
    s.trash_material(&m.id, 0).unwrap();
    assert!(s.list_material_proposals(&b).unwrap().is_empty());
    assert_eq!(
        s.get_material_proposal(&p.id, "fixture-agent")
            .unwrap()
            .status,
        "pending"
    );
}
#[test]
fn json_expansion_limit_rejects_before_proposal_or_audit_write() {
    let f =
        DiskFixture(std::env::temp_dir().join(format!("material-frame-{}", uuid::Uuid::new_v4())));
    std::fs::create_dir(&f.0).unwrap();
    let path = f.0.join("fixture.sqlite");
    let (base, b, _, m) = fixture();
    let mut s = Service::open(&path).unwrap();
    s.restore(&base.backup().unwrap()).unwrap();
    let mut request = input(&m, "encoded-too-large");
    request
        .new_fields
        .insert("content".into(), "\0".repeat(128 * 1024));
    let rejection = s
        .propose_material_edit(request)
        .err()
        .expect("oversized encoded proposal must fail before writing");
    assert_eq!(rejection.code, "limit_exceeded");
    assert!(s.list_material_proposals(&b).unwrap().is_empty());
    assert_eq!(s.get_material(&m.id, false).unwrap(), m);
    let conn = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        conn.query_row("SELECT count(*) FROM material_proposals", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row("SELECT count(*) FROM material_proposal_audit", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(conn);
    drop(s);
}

#[test]
fn accepted_control_character_proposal_fits_mcp_double_encoding() {
    let (mut s, _, _, m) = fixture();
    let mut request = input(&m, "encoded-valid");
    request
        .new_fields
        .insert("content".into(), "\0".repeat(64 * 1024));
    let proposal = s.propose_material_edit(request).unwrap();
    assert!(serde_json::to_vec(&proposal).unwrap().len() <= 512 * 1024);
    let lines = [
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_material_proposal","arguments":{"proposal_id":proposal.id}}}),
    ];
    let wire = lines
        .iter()
        .map(|v| v.to_string() + "\n")
        .collect::<String>();
    let mut output = Vec::new();
    novel_core::agent_stdio::run(std::io::Cursor::new(wire), &mut output, true, |_| {
        Ok(serde_json::json!({"result":proposal}))
    })
    .unwrap();
    for line in output
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
    {
        assert!(line.len() < novel_core::ipc::MAX_FRAME);
    }
    let responses = String::from_utf8(output).unwrap();
    let last: serde_json::Value = serde_json::from_str(responses.lines().last().unwrap()).unwrap();
    assert_eq!(last["result"]["isError"], false);
}

#[test]
fn tracking_kinds_reject_proposals_without_writes() {
    let path = std::env::temp_dir().join(format!("tracker-proposal-{}.db", uuid::Uuid::new_v4()));
    let mut s = Service::open(&path).unwrap();
    let state = s
        .apply(Command::CreateBook {
            title: "tracking fixture".into(),
            author: "".into(),
        })
        .unwrap();
    for kind in ["timeline", "foreshadow"] {
        let fields = if kind == "timeline" {
            serde_json::json!({"content":"event"})
        } else {
            serde_json::json!({"setup":"clue","resolution":""})
        };
        let m = s.create_material(serde_json::from_value(serde_json::json!({"book_id":state.books[0].id,"kind":kind,"title":kind,"fields":fields,"tracking_status":"planned","story_order":5})).unwrap()).unwrap();
        let mut request = input(&m, kind);
        request.new_fields = m.fields.clone();
        assert_eq!(
            s.propose_material_edit(request).unwrap_err().code,
            "unsupported_operation"
        );
        assert_eq!(s.get_material(&m.id, false).unwrap(), m);
    }
    let conn = rusqlite::Connection::open(&path).unwrap();
    for table in ["material_proposals", "material_proposal_audit"] {
        assert_eq!(
            conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    drop(conn);
    drop(s);
    let _ = std::fs::remove_file(path);
}

#[test]
fn forged_tracking_proposal_cannot_be_accepted_but_can_be_rejected() {
    // Isolated corruption fixture simulates a legacy/forged pending row, not a production write path.
    for kind in ["timeline", "foreshadow"] {
        let path = std::env::temp_dir().join(format!("tracker-review-{}.db", uuid::Uuid::new_v4()));
        let mut s = Service::open(&path).unwrap();
        let state = s
            .apply(Command::CreateBook {
                title: "fixture".into(),
                author: "".into(),
            })
            .unwrap();
        let m = s.create_material(serde_json::from_value(serde_json::json!({"book_id":state.books[0].id,"kind":"outline","title":"original","fields":{"content":"unchanged"}})).unwrap()).unwrap();
        let p = s.propose_material_edit(input(&m, "legacy")).unwrap();
        let conn = rusqlite::Connection::open(&path).unwrap();
        let fields = if kind == "timeline" {
            r#"{"content":"unchanged"}"#
        } else {
            r#"{"setup":"unchanged","resolution":""}"#
        };
        conn.execute(
            "UPDATE materials SET kind=?1,fields=?2,tracking_status='planned' WHERE id=?3",
            rusqlite::params![kind, fields, m.id],
        )
        .unwrap();
        assert_eq!(
            s.decide_material_proposal(&p.id, true, 0).unwrap_err().code,
            "unsupported_operation"
        );
        assert_eq!(
            s.get_material_proposal(&p.id, "fixture-agent")
                .unwrap()
                .status,
            "pending"
        );
        assert_eq!(s.get_material(&m.id, false).unwrap().title, "original");
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM material_proposal_audit", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        s.decide_material_proposal(&p.id, false, 0).unwrap();
        assert_eq!(
            s.get_material_proposal(&p.id, "fixture-agent")
                .unwrap()
                .status,
            "rejected"
        );
        drop(conn);
        drop(s);
        let _ = std::fs::remove_file(path);
    }
}
