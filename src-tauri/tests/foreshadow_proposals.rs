use novel_core::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn fixture(mut s: Service) -> (Service, String, ForeshadowProposalInput, String) {
    let tree = s
        .apply(Command::CreateBook {
            title: "虚构谜案".into(),
            author: "".into(),
        })
        .unwrap();
    let (book, chapter) = (tree.books[0].id.clone(), tree.chapters[0].id.clone());
    s.apply(Command::SaveChapter {
        id: chapter.clone(),
        body: "甲😀\r\n线索乙".into(),
        expected_revision: 0,
    })
    .unwrap();
    let card = create_card(&mut s, &book);
    let epoch = s.epoch().unwrap();
    (
        s,
        book,
        ForeshadowProposalInput {
            material_id: card,
            material_revision: 0,
            chapter_id: chapter,
            chapter_revision: 1,
            start: 1,
            end: 6,
            quote: "😀\r\n线索".into(),
            role: ForeshadowRole::Payoff,
            note: "虚构备注".into(),
            reason: "也许是呼应".into(),
            idempotency_key: "fake-key".into(),
        },
        epoch,
    )
}

fn create_card(s: &mut Service, book: &str) -> String {
    s.create_material(CreateMaterialInput {
        book_id: book.into(),
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
    .unwrap()
    .id
}

fn decide(
    s: &mut Service,
    book: &str,
    input: &ForeshadowProposalInput,
    id: &str,
    epoch: &str,
    accept: bool,
) -> WorkflowResult<ForeshadowProposal> {
    s.decide_foreshadow_proposal(DecideForeshadowProposal {
        book_id: book.into(),
        material_id: input.material_id.clone(),
        proposal_id: id.into(),
        expected_epoch: epoch.into(),
        expected_material_revision: input.material_revision,
        action: if accept {
            ForeshadowProposalAction::Accept
        } else {
            ForeshadowProposalAction::Reject
        },
    })
}

#[test]
fn exact_scalar_submit_is_immutable_owned_and_idempotent() {
    let (mut s, book, input, epoch) = fixture(Service::open(":memory:").unwrap());
    let card = &input.material_id;
    let before = s.load().unwrap();
    let material = s.get_material(card, false).unwrap();
    let propose = |s: &mut Service, i: ForeshadowProposalInput| {
        s.propose_foreshadow_occurrence(&book, "client", &epoch, i)
    };
    let mut bad = input.clone();
    bad.quote = "😀\n线索".into();
    assert_eq!(propose(&mut s, bad).unwrap_err().code, "invalid_input");
    let mut bad = input.clone();
    bad.material_revision += 1;
    assert_eq!(propose(&mut s, bad).unwrap_err().code, "revision_conflict");
    let mut bad = input.clone();
    bad.chapter_revision -= 1;
    assert_eq!(propose(&mut s, bad).unwrap_err().code, "revision_conflict");
    let mut bad = input.clone();
    bad.reason = "中".repeat(683);
    assert!(propose(&mut s, bad).is_err());
    let mut bad = input.clone();
    bad.note = "😀".repeat(257);
    assert!(propose(&mut s, bad).is_err());
    let mut bad = input.clone();
    bad.quote = "😀".repeat(1025);
    assert!(propose(&mut s, bad).is_err());
    assert!(s
        .propose_foreshadow_occurrence("other-book", "client", &epoch, input.clone())
        .is_err());
    assert!(s
        .propose_foreshadow_occurrence(&book, "client", "old-epoch", input.clone())
        .is_err());
    let first = propose(&mut s, input.clone()).unwrap();
    assert_eq!(first.status, ForeshadowProposalStatus::Pending);
    assert_eq!(propose(&mut s, input.clone()).unwrap(), first);
    assert_eq!(
        s.get_own_foreshadow_proposal(&book, "client", &epoch, &first.id)
            .unwrap(),
        first
    );
    assert_eq!(
        s.get_own_foreshadow_proposal(&book, "other-client", &epoch, &first.id)
            .unwrap_err()
            .code,
        "not_found"
    );
    assert!(s
        .get_own_foreshadow_proposal("other-book", "client", &epoch, &first.id)
        .is_err());
    let mut bad = input.clone();
    bad.role = ForeshadowRole::Hint;
    assert_eq!(
        propose(&mut s, bad).unwrap_err().code,
        "idempotency_conflict"
    );
    assert_eq!(s.load().unwrap(), before);
    assert_eq!(
        s.get_material(card, false).unwrap().revision,
        material.revision
    );
    assert!(s
        .list_foreshadow_occurrences(&book, card, &epoch)
        .unwrap()
        .is_empty());
    assert_eq!(
        s.list_foreshadow_proposals(&book, None, true, None, None, &epoch)
            .unwrap()
            .pending_count,
        1
    );
    let cancelled = s
        .cancel_own_foreshadow_proposal(&book, "client", &epoch, &first.id)
        .unwrap();
    assert_eq!(cancelled.status, ForeshadowProposalStatus::Cancelled);
    assert_eq!(propose(&mut s, input.clone()).unwrap(), cancelled);
}

#[test]
fn acceptance_changes_one_revision_once_and_keeps_other_suggestions_stale() {
    let (mut s, book, input, epoch) = fixture(Service::open(":memory:").unwrap());
    let card = &input.material_id;
    let first = s
        .propose_foreshadow_occurrence(&book, "client", &epoch, input.clone())
        .unwrap();
    let second = s
        .propose_foreshadow_occurrence(&book, "other-client", &epoch, input.clone())
        .unwrap();
    let before = s.load().unwrap();
    let accepted = decide(&mut s, &book, &input, &first.id, &epoch, true).unwrap();
    let occurrence = accepted.accepted_occurrence.as_ref().unwrap();
    assert_eq!(occurrence.occurrence.quote, input.quote);
    assert_eq!(accepted.receipt.status, ForeshadowProposalStatus::Accepted);
    assert_eq!(
        decide(&mut s, &book, &input, &first.id, &epoch, true)
            .unwrap()
            .receipt,
        accepted.receipt
    );
    assert!(decide(&mut s, &book, &input, &first.id, &epoch, false).is_err());
    let mut wrong_revision = input.clone();
    wrong_revision.material_revision = 1;
    assert!(decide(&mut s, &book, &wrong_revision, &first.id, &epoch, true).is_err());
    assert_eq!(
        decide(&mut s, &book, &input, &second.id, &epoch, true)
            .unwrap_err()
            .code,
        "revision_conflict"
    );
    assert_eq!(s.load().unwrap(), before);
    let material = s.get_material(card, false).unwrap();
    assert_eq!(material.revision, 1);
    assert_eq!(material.tracking_status.as_deref(), Some("planned"));
    assert_eq!(
        s.list_foreshadow_occurrences(&book, card, &epoch)
            .unwrap()
            .len(),
        1
    );
    let o = &occurrence.occurrence;
    s.set_foreshadow_occurrence_deleted(&book, card, &o.id, 0, &epoch, true)
        .unwrap();
    let replay = decide(&mut s, &book, &input, &first.id, &epoch, true).unwrap();
    assert!(replay
        .accepted_occurrence
        .unwrap()
        .occurrence
        .deleted_at
        .is_some());
    assert_eq!(s.get_material(card, false).unwrap().revision, 2);
}

#[test]
fn resave_rename_reorder_move_and_purge_block_acceptance_but_keep_evidence() {
    for change in 0..6 {
        let (mut s, book, input, epoch) = fixture(Service::open(":memory:").unwrap());
        let tree = s.load().unwrap();
        let v = tree.volumes[0].id.clone();
        let v2 = s
            .apply(Command::CreateVolume {
                book_id: book.clone(),
                title: "虚构第二卷".into(),
            })
            .unwrap()
            .volumes[1]
            .id
            .clone();
        s.apply(Command::CreateChapter {
            volume_id: v.clone(),
            title: "虚构下章".into(),
        })
        .unwrap();
        let row = s
            .propose_foreshadow_occurrence(&book, "client", &epoch, input.clone())
            .unwrap();
        let ch = input.chapter_id.clone();
        match change {
            0 => {
                s.apply(Command::SaveChapter {
                    id: ch,
                    body: "丙😀\r\n线索乙".into(),
                    expected_revision: 1,
                })
                .unwrap();
            }
            1 => {
                s.apply(Command::RenameChapter {
                    id: ch,
                    title: "虚构新标题".into(),
                })
                .unwrap();
            }
            2 => {
                s.apply(Command::MoveVolume {
                    id: v2,
                    direction: -1,
                })
                .unwrap();
            }
            3 => {
                s.apply(Command::TransferChapter {
                    id: ch,
                    volume_id: v2,
                })
                .unwrap();
            }
            4 => {
                s.apply(Command::MoveChapter {
                    id: ch,
                    direction: 1,
                })
                .unwrap();
            }
            _ => {
                s.apply(Command::TrashVolume { id: v.clone() }).unwrap();
                assert!(decide(&mut s, &book, &input, &row.id, &epoch, true).is_err());
                s.apply(Command::PurgeVolume { id: v }).unwrap();
            }
        }
        assert!(decide(&mut s, &book, &input, &row.id, &epoch, true).is_err());
        let rejected = decide(&mut s, &book, &input, &row.id, &epoch, false).unwrap();
        assert_eq!(rejected.receipt.status, ForeshadowProposalStatus::Rejected);
        assert_eq!(rejected.quote, input.quote);
        assert_eq!(rejected.source.chapter_revision, 1);
        assert!(s
            .list_foreshadow_occurrences(&book, &input.material_id, &epoch)
            .unwrap()
            .is_empty());
    }
}

#[test]
fn pending_caps_and_decision_rollback_leave_no_formal_record() {
    let temp = Scratch::new();
    let path = temp.0.join("fictional.sqlite");
    let (mut s, book, mut input, epoch) = fixture(Service::open(&path).unwrap());
    let mut row = None;
    for n in 0..20 {
        input.idempotency_key = format!("fixture-{n}");
        row = Some(
            s.propose_foreshadow_occurrence(&book, "client", &epoch, input.clone())
                .unwrap(),
        );
    }
    let row = row.unwrap();
    input.idempotency_key = "excess".into();
    assert_eq!(
        s.propose_foreshadow_occurrence(&book, "client", &epoch, input.clone())
            .unwrap_err()
            .code,
        "limit_exceeded"
    );
    let sql = rusqlite::Connection::open(path).unwrap();
    sql.execute_batch("CREATE TRIGGER fixture_abort BEFORE UPDATE OF status ON foreshadow_proposals BEGIN SELECT RAISE(ABORT, 'fictional error'); END;").unwrap();
    assert!(decide(&mut s, &book, &input, &row.id, &epoch, true).is_err());
    assert_eq!(
        s.get_own_foreshadow_proposal(&book, "client", &epoch, &row.id)
            .unwrap(),
        row
    );
    assert_eq!(
        s.get_material(&input.material_id, false).unwrap().revision,
        0
    );
    assert!(s
        .list_foreshadow_occurrences(&book, &input.material_id, &epoch)
        .unwrap()
        .is_empty());
    sql.execute_batch("DROP TRIGGER fixture_abort").unwrap();
    assert!(decide(&mut s, &book, &input, &row.id, &epoch, true).is_ok());
}

struct Scratch(std::path::PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "mojuan-foreshadow-fixture-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn json13_restores_history_and_rejects_bad_status_missing_or_forged_links() {
    use serde_json::{json, Value};
    let (mut s, book, mut input, epoch) = fixture(Service::open(":memory:").unwrap());
    let first = s
        .propose_foreshadow_occurrence(&book, "client", &epoch, input.clone())
        .unwrap();
    input.idempotency_key = "other-fake-key".into();
    let pending = s
        .propose_foreshadow_occurrence(&book, "client", &epoch, input.clone())
        .unwrap();
    let accepted = decide(&mut s, &book, &input, &first.id, &epoch, true).unwrap();
    s.apply(Command::TrashChapter {
        id: input.chapter_id.clone(),
    })
    .unwrap();
    s.apply(Command::PurgeChapter {
        id: input.chapter_id.clone(),
    })
    .unwrap();
    let backup: Value = serde_json::from_str(&s.backup().unwrap()).unwrap();
    assert_eq!(backup["version"], 13);
    let mut target = Service::open(":memory:").unwrap();
    target.restore(&backup.to_string()).unwrap();
    let new_epoch = target.epoch().unwrap();
    assert_ne!(new_epoch, epoch);
    let found = target
        .get_foreshadow_proposal(&book, &input.material_id, &first.id, &new_epoch)
        .unwrap();
    assert!(found.archived);
    assert_eq!(found.receipt, accepted.receipt);
    assert_eq!(found.source_status, ForeshadowSourceStatus::Unavailable);
    assert_eq!(found.quote, input.quote);
    assert_eq!(
        target
            .list_foreshadow_proposals(&book, None, true, None, None, &new_epoch)
            .unwrap()
            .pending_count,
        0
    );
    assert!(decide(&mut target, &book, &input, &pending.id, &new_epoch, false).is_err());
    assert!(target
        .get_own_foreshadow_proposal(&book, "client", &epoch, &pending.id)
        .is_err());
    assert!(target
        .get_own_foreshadow_proposal(&book, "client", &new_epoch, &pending.id)
        .is_err());
    assert!(target
        .cancel_own_foreshadow_proposal(&book, "client", &new_epoch, &pending.id)
        .is_err());
    let before = target.backup().unwrap();
    let original = &backup["foreshadow_proposals"]["rows"];
    let accepted_pos = original
        .as_array()
        .unwrap()
        .iter()
        .position(|row| row["decision"]["status"] == "accepted")
        .unwrap();
    let mut variants = vec![];
    let mut old = backup.clone();
    old["version"] = json!(12);
    variants.push(old.clone());
    old["foreshadow_proposals"] = Value::Null;
    variants.push(old.clone());
    old.as_object_mut().unwrap().remove("foreshadow_proposals");
    let mut missing_field = backup.clone();
    missing_field
        .as_object_mut()
        .unwrap()
        .remove("foreshadow_proposals");
    variants.push(missing_field);
    let mut dangling = backup.clone();
    dangling["foreshadow_occurrences"] = json!([]);
    variants.push(dangling);
    let mut bad_projection = backup.clone();
    bad_projection["foreshadow_occurrences"][0]["occurrence"]["role"] = json!("hint");
    variants.push(bad_projection);
    let mut bad_key = backup.clone();
    bad_key["foreshadow_occurrences"][0]["request_key"] = json!("not-the-proposal");
    variants.push(bad_key);
    let mut bad_status = backup.clone();
    bad_status["foreshadow_proposals"]["rows"][accepted_pos]["decision"]["status"] =
        json!("pending");
    variants.push(bad_status);
    let mut bad_scope = backup.clone();
    bad_scope["foreshadow_proposals"]["rows"][0]["record"]["book_id"] = json!("other-fiction");
    variants.push(bad_scope);
    let mut bad_reverse = backup.clone();
    bad_reverse["foreshadow_proposals"]["rows"][accepted_pos]["decision"] =
        json!({"status":"pending","decided_at":null,"accepted_occurrence_id":null});
    variants.push(bad_reverse);
    let pending_template = original
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["decision"]["status"] == "pending")
        .unwrap();
    let mut over_card_limit = backup.clone();
    over_card_limit["foreshadow_proposals"]["rows"] = Value::Array(
        (0..21)
            .map(|n| {
                let mut row = pending_template.clone();
                row["record"]["id"] = json!(uuid::Uuid::new_v4().to_string());
                row["payload"]["idempotency_key"] = json!(format!("backup-fixture-{n}"));
                row
            })
            .collect(),
    );
    variants.push(over_card_limit);
    for malformed in variants {
        assert!(
            target.restore(&malformed.to_string()).is_err(),
            "{malformed}"
        );
        assert_eq!(target.backup().unwrap(), before);
    }
    // An already archived association may retain evidence after the formal row
    // is gone, but the history must never claim it still exists or re-create it.
    let mut archived_history: Value = serde_json::from_str(&before).unwrap();
    archived_history["foreshadow_occurrences"] = json!([]);
    target.restore(&archived_history.to_string()).unwrap();
    let now = target.epoch().unwrap();
    let retained = target
        .get_foreshadow_proposal(&book, &input.material_id, &first.id, &now)
        .unwrap();
    assert!(retained.archived);
    assert_eq!(retained.receipt, accepted.receipt);
    assert!(retained.accepted_occurrence.is_none());
    assert!(decide(&mut target, &book, &input, &first.id, &now, true).is_err());
    old["foreshadow_occurrences"] = json!([]);
    target.restore(&old.to_string()).unwrap();
    assert!(target
        .list_foreshadow_proposals(&book, None, false, None, None, &target.epoch().unwrap())
        .unwrap()
        .items
        .is_empty());
}

#[test]
fn current_sqlite_requires_table_and_exact_aliases_and_keeps_archived_evidence() {
    let scratch = Scratch::new();
    let (mut s, book, input, epoch) = fixture(Service::open(":memory:").unwrap());
    let row = s
        .propose_foreshadow_occurrence(&book, "client", &epoch, input.clone())
        .unwrap();
    assert!(
        s.create_foreshadow_occurrence(CreateForeshadowOccurrence {
            book_id: book.clone(),
            material_id: input.material_id.clone(),
            chapter_id: input.chapter_id.clone(),
            chapter_revision: input.chapter_revision,
            start: input.start,
            end: input.end,
            quote: input.quote.clone(),
            note: input.note.clone(),
            role: input.role,
            epoch: epoch.clone(),
            request_key: format!("\u{1f}foreshadow-proposal:{}", row.id),
        })
        .is_err(),
        "ordinary author creation cannot forge the internal approval marker"
    );
    let approved = decide(&mut s, &book, &input, &row.id, &epoch, true).unwrap();
    let mut target = Service::open(":memory:").unwrap();
    let good = scratch.0.join("good.sqlite");
    s.recovery_snapshot(&good).unwrap();
    let prepared = prepare_restore(&good, |_| Ok(())).unwrap();
    let guard = target.restore_guard().unwrap();
    target
        .commit_prepared_restore(prepared, &guard, &scratch.0.join("recovery.sqlite"), |_| {
            Ok(())
        })
        .unwrap();
    let restored = target
        .get_foreshadow_proposal(&book, &input.material_id, &row.id, &target.epoch().unwrap())
        .unwrap();
    assert!(restored.archived);
    assert_eq!(restored.receipt, approved.receipt);
    assert_eq!(
        restored.accepted_occurrence.unwrap().occurrence,
        approved.accepted_occurrence.unwrap().occurrence
    );
    for (index, bad) in [
        "DROP TABLE foreshadow_proposals",
        "UPDATE foreshadow_proposals SET client_id='fictional-impostor'",
        "UPDATE foreshadow_proposals SET byte_count=byte_count+1",
        "UPDATE foreshadow_proposals SET status='cancelled'",
        "UPDATE foreshadow_occurrences SET request_key='not-bound'",
        "DELETE FROM foreshadow_occurrences",
    ]
    .into_iter()
    .enumerate()
    {
        let file = scratch.0.join(format!("bad-{index}.sqlite"));
        std::fs::copy(&good, &file).unwrap();
        let db = rusqlite::Connection::open(&file).unwrap();
        db.execute_batch(bad).unwrap();
        drop(db);
        assert!(prepare_restore(&file, |_| Ok(())).is_err(), "{bad}");
        if index == 0 {
            assert!(Service::open(&file).is_err());
        }
    }
}

#[test]
fn only_acceptance_invalidates_existing_foreshadow_writing_pack() {
    use serde_json::json;
    let (mut service, book, input, epoch) = fixture(Service::open(":memory:").unwrap());
    let volume = service.get_chapter(&input.chapter_id).unwrap().volume_id;
    let target = service
        .apply(Command::CreateChapter {
            volume_id: volume,
            title: "另一个虚构章节".into(),
        })
        .unwrap()
        .chapters
        .into_iter()
        .find(|c| c.id != input.chapter_id)
        .unwrap()
        .id;
    service
        .configure_writing_policy(
            &book,
            &target,
            None,
            "active",
            "虚构约束",
            vec![input.material_id.clone()],
            &epoch,
        )
        .unwrap();
    let pack = service
        .prepare_writing_pack(&book, &target, 0, 0, &epoch)
        .unwrap();
    let writing = |key: &str| {
        serde_json::from_value::<ConstrainedEditInput>(json!({
        "pack_id":pack.id,"new_body":"虚构段落","idempotency_key":key,"conflicts":[],
        "citations":[{"entry_id":format!("material:{}",input.material_id),"field_key":"setup","start":0,"end":2,"explanation":"核对"}]
    })).unwrap()
    };
    let suggestion = service
        .propose_foreshadow_occurrence(&book, "client", &epoch, input.clone())
        .unwrap();
    let proposed = service
        .propose_constrained_edit(&book, "fictional-writer", writing("first"))
        .unwrap();
    let review = vec![AuthorReviewItem {
        material_id: input.material_id.clone(),
        verdict: AuthorReviewVerdict::Conforms,
        note: "".into(),
    }];
    let before = service
        .save_constrained_review(&book, &proposed.id, None, &epoch, review.clone())
        .unwrap();
    decide(&mut service, &book, &input, &suggestion.id, &epoch, true).unwrap();
    assert!(service
        .propose_constrained_edit(&book, "fictional-writer", writing("second"))
        .is_err());
    assert!(service
        .save_constrained_review(&book, &proposed.id, Some(before.revision), &epoch, review)
        .is_err());
    assert!(service
        .accept_reviewed_constrained_proposal(
            &book,
            &proposed.id,
            0,
            0,
            &epoch,
            before.revision,
            false
        )
        .is_err());
}

#[test]
fn trashed_or_review_required_cards_cannot_be_used_but_their_suggestions_can_be_rejected() {
    let scratch = Scratch::new();
    let path = scratch.0.join("review-required.sqlite");
    let (mut service, book, mut input, epoch) = fixture(Service::open(&path).unwrap());
    let saved = service
        .propose_foreshadow_occurrence(&book, "client", &epoch, input.clone())
        .unwrap();
    let sql = rusqlite::Connection::open(path).unwrap();
    sql.execute(
        "UPDATE materials SET content_review_required=1 WHERE id=?1",
        [&input.material_id],
    )
    .unwrap();
    input.idempotency_key = "another-key".into();
    assert!(service
        .propose_foreshadow_occurrence(&book, "client", &epoch, input.clone())
        .is_err());
    assert!(decide(&mut service, &book, &input, &saved.id, &epoch, true).is_err());
    service.trash_material(&input.material_id, 0).unwrap();
    let queued = service
        .list_foreshadow_proposals(&book, None, true, None, None, &epoch)
        .unwrap();
    assert_eq!(queued.pending_count, 1);
    assert_eq!(queued.items[0].receipt, saved);
    let detail = service
        .get_foreshadow_proposal(&book, &input.material_id, &saved.id, &epoch)
        .unwrap();
    assert!(!detail.card_current);
    assert_eq!(
        decide(&mut service, &book, &input, &saved.id, &epoch, false)
            .unwrap()
            .receipt
            .status,
        ForeshadowProposalStatus::Rejected
    );
    service
        .apply(Command::TrashBook { id: book.clone() })
        .unwrap();
    service.apply(Command::PurgeBook { id: book }).unwrap();
    let backup: serde_json::Value = serde_json::from_str(&service.backup().unwrap()).unwrap();
    assert_eq!(
        backup["foreshadow_proposals"]["rows"],
        serde_json::json!([])
    );
}

#[test]
fn global_active_and_cumulative_quotas_are_independent_and_terminal_decisions_stay_available() {
    let (mut service, book, mut input, epoch) = fixture(Service::open(":memory:").unwrap());
    let mut ids = vec![];
    for n in 0..100 {
        if n > 0 && n % 20 == 0 {
            input.material_id = create_card(&mut service, &book);
        }
        input.idempotency_key = format!("fictional-{n}");
        ids.push(
            service
                .propose_foreshadow_occurrence(&book, "client", &epoch, input.clone())
                .unwrap()
                .id,
        );
    }
    input.material_id = create_card(&mut service, &book);
    input.idempotency_key = "excess-active".into();
    assert_eq!(
        service
            .propose_foreshadow_occurrence(&book, "client", &epoch, input.clone())
            .unwrap_err()
            .code,
        "limit_exceeded"
    );
    let first_page = service
        .list_foreshadow_proposals(&book, None, true, None, Some(20), &epoch)
        .unwrap();
    let cursor = first_page.next_after_id.as_ref().unwrap();
    let second_page = service
        .list_foreshadow_proposals(&book, None, true, Some(cursor), Some(20), &epoch)
        .unwrap();
    assert_eq!(second_page.pending_count, 100);
    assert!(second_page
        .items
        .iter()
        .all(|i| i.receipt.id.as_str() > cursor.as_str()));
    for id in ids {
        service
            .cancel_own_foreshadow_proposal(&book, "client", &epoch, &id)
            .unwrap();
    }
    for n in 100..1000 {
        input.idempotency_key = format!("fictional-{n}");
        let row = service
            .propose_foreshadow_occurrence(&book, "client", &epoch, input.clone())
            .unwrap();
        if n < 999 {
            service
                .cancel_own_foreshadow_proposal(&book, "client", &epoch, &row.id)
                .unwrap();
        } else {
            let mut extra = input.clone();
            extra.idempotency_key = "excess-total".into();
            assert_eq!(
                service
                    .propose_foreshadow_occurrence(&book, "client", &epoch, extra)
                    .unwrap_err()
                    .code,
                "limit_exceeded"
            );
            assert_eq!(
                decide(&mut service, &book, &input, &row.id, &epoch, false)
                    .unwrap()
                    .receipt
                    .status,
                ForeshadowProposalStatus::Rejected
            );
        }
    }
}

#[test]
fn sqlite_generation_one_and_zero_migrate_without_losing_occurrences_or_creating_suggestions() {
    let scratch = Scratch::new();
    let gen_zero = scratch.0.join("gen-zero.sqlite");
    let db = rusqlite::Connection::open(&gen_zero).unwrap();
    db.execute_batch(include_str!("fixtures/pre-foreshadow-v0.sql"))
        .unwrap();
    drop(db);
    let migrated = Service::open(&gen_zero).unwrap();
    assert_eq!(
        migrated.load().unwrap().books[0].title,
        "Pre-feature fictional fixture"
    );
    drop(migrated);
    let good = scratch.0.join("gen-one.sqlite");
    let (mut service, book, input, epoch) = fixture(Service::open(":memory:").unwrap());
    let saved = service
        .create_foreshadow_occurrence(CreateForeshadowOccurrence {
            book_id: book.clone(),
            material_id: input.material_id.clone(),
            chapter_id: input.chapter_id.clone(),
            chapter_revision: input.chapter_revision,
            start: input.start,
            end: input.end,
            quote: input.quote.clone(),
            note: input.note.clone(),
            role: input.role,
            epoch: epoch.clone(),
            request_key: format!("foreshadow-proposal:{}", uuid::Uuid::new_v4()),
        })
        .unwrap();
    let mut json12: Value = serde_json::from_str(&service.backup().unwrap()).unwrap();
    json12["version"] = json!(12);
    json12
        .as_object_mut()
        .unwrap()
        .remove("foreshadow_proposals");
    let mut old_json = Service::open(":memory:").unwrap();
    old_json.restore(&json12.to_string()).unwrap();
    let restored = old_json
        .list_foreshadow_occurrences(&book, &input.material_id, &old_json.epoch().unwrap())
        .unwrap();
    assert_eq!(restored, vec![saved.clone()]);
    service.recovery_snapshot(&good).unwrap();
    let db = rusqlite::Connection::open(&good).unwrap();
    db.execute_batch("DROP TABLE foreshadow_proposals; PRAGMA user_version=1;")
        .unwrap();
    drop(db);
    assert!(prepare_restore(&good, |_| Ok(())).is_ok());
    let migrated = Service::open(&good).unwrap();
    assert_eq!(
        migrated
            .list_foreshadow_occurrences(&book, &input.material_id, &epoch)
            .unwrap(),
        vec![saved]
    );
    assert!(migrated
        .list_foreshadow_proposals(&book, None, false, None, None, &epoch)
        .unwrap()
        .items
        .is_empty());
    let db = rusqlite::Connection::open(good).unwrap();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn escaped_json_uses_exact_row_and_aggregate_budgets() {
    let scratch = Scratch::new();
    let path = scratch.0.join("byte-budget.sqlite");
    let (mut service, book, mut input, epoch) = fixture(Service::open(&path).unwrap());
    let chapter = input.chapter_id.clone();
    let volume = service.get_chapter(&chapter).unwrap().volume_id;
    let dense = "\u{0001}";
    service
        .apply(Command::SaveChapter {
            id: chapter.clone(),
            body: dense.repeat(4096),
            expected_revision: 1,
        })
        .unwrap();
    input.chapter_revision = 2;
    input.start = 0;
    input.end = 4096;
    input.quote = dense.repeat(4096);
    input.note = dense.repeat(1024);
    input.reason = dense.repeat(2048);
    for (id, is_chapter) in [(chapter.clone(), true), (volume.clone(), false)] {
        let title = format!("章{}", dense.repeat(2000));
        service
            .apply(if is_chapter {
                Command::RenameChapter { id, title }
            } else {
                Command::RenameVolume { id, title }
            })
            .unwrap();
    }
    assert_eq!(
        service
            .propose_foreshadow_occurrence(&book, "client", &epoch, input.clone())
            .unwrap_err()
            .code,
        "limit_exceeded"
    );
    service
        .apply(Command::RenameVolume {
            id: volume,
            title: "虚构卷".into(),
        })
        .unwrap();
    let first = service
        .propose_foreshadow_occurrence(&book, "client", &epoch, input.clone())
        .unwrap();
    let sql = rusqlite::Connection::open(path).unwrap();
    let (raw_record, raw_payload): (String, String) = sql
        .query_row(
            "SELECT record,payload FROM foreshadow_proposals WHERE id=?1",
            [&first.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let bytes = raw_record.len() + raw_payload.len();
    assert!((50000..65536).contains(&bytes));
    // Duplicate exact byte-weight fixtures as otherwise scoped, distinct cancelled
    // rows so reaching the real cumulative cap does not require hundreds of UI calls.
    let n = (32 * 1024 * 1024 / bytes) - 1;
    // Keep original serialized order and alias sizes by replacing same-length fields.
    let fixture_key = &input.idempotency_key;
    let tx = sql.unchecked_transaction().unwrap();
    for i in 0..n {
        let id = uuid::Uuid::new_v4().to_string();
        let key = format!("fake-{i:03}");
        assert_eq!(key.len(), fixture_key.len());
        let r = raw_record.replacen(&first.id, &id, 1);
        let p = raw_payload.replacen(fixture_key, &key, 1);
        tx.execute("INSERT INTO foreshadow_proposals (id,book_id,material_id,client_id,originating_epoch,request_key,status,decided_at,accepted_occurrence_id,record,payload,byte_count)
            SELECT ?1,book_id,material_id,client_id,originating_epoch,?2,'cancelled',?3,NULL,?4,?5,byte_count FROM foreshadow_proposals WHERE id=?6",
            rusqlite::params![id,key,first.created_at,r,p,first.id]).unwrap();
    }
    tx.commit().unwrap();
    input.idempotency_key = "fake-new".into();
    assert_eq!(
        service
            .propose_foreshadow_occurrence(&book, "client", &epoch, input.clone())
            .unwrap_err()
            .code,
        "limit_exceeded"
    );
    assert_eq!(
        decide(&mut service, &book, &input, &first.id, &epoch, false)
            .unwrap()
            .receipt
            .status,
        ForeshadowProposalStatus::Rejected
    );
}

fn accepted_fixture() -> (Service, String, ForeshadowProposalInput, String, String) {
    let (mut s, book, input, epoch) = fixture(Service::open(":memory:").unwrap());
    let row = s
        .propose_foreshadow_occurrence(&book, "fixture", &epoch, input.clone())
        .unwrap();
    let o = decide(&mut s, &book, &input, &row.id, &epoch, true)
        .unwrap()
        .accepted_occurrence
        .unwrap()
        .occurrence
        .id;
    let volume = s.get_chapter(&input.chapter_id).unwrap().volume_id;
    let target = s
        .apply(Command::CreateChapter {
            volume_id: volume,
            title: "虚构目标".into(),
        })
        .unwrap()
        .chapters
        .into_iter()
        .find(|c| c.id != input.chapter_id)
        .unwrap()
        .id;
    s.configure_writing_policy(
        &book,
        &target,
        None,
        "active",
        "虚构规则",
        vec![input.material_id.clone()],
        &epoch,
    )
    .unwrap();
    let _pack = s
        .prepare_writing_pack(&book, &target, 0, 0, &epoch)
        .unwrap();
    (s, book, input, row.id, o)
}

#[test]
fn archived_suggestion_must_agree_with_existing_pack_copy() {
    let (s, _, _, _, o) = accepted_fixture();
    let mut archive = Service::open(":memory:").unwrap();
    archive.restore(&s.backup().unwrap()).unwrap();
    let mut backup: Value = serde_json::from_str(&archive.backup().unwrap()).unwrap();
    assert_ne!(
        backup["foreshadow_proposals"]["epoch"],
        backup["foreshadow_proposals"]["rows"][0]["record"]["originating_epoch"]
    );
    assert_eq!(
        backup["writing_evidence"]["packs"][0]["foreshadow_occurrences"][0]["id"],
        o
    );
    backup["foreshadow_occurrences"] = json!([]);
    // Without a current row, a genuinely agreeing pack copy is fine.
    let mut target = Service::open(":memory:").unwrap();
    target.restore(&backup.to_string()).unwrap();
    let before = target.backup().unwrap();
    backup["foreshadow_proposals"]["rows"][0]["payload"]["note"] = json!("与包矛盾的虚构备注");
    let actual = target.restore_with_writing_ack(&backup.to_string(), true);
    assert!(
        actual.is_err(),
        "contradictory same-ID pack/suggestion backup was imported"
    );
    assert_eq!(before, target.backup().unwrap());
}

#[test]
fn current_backup_cannot_keep_formal_row_if_its_only_audit_row_is_lost() {
    let (s, _, _, _, _) = accepted_fixture();
    let mut target = Service::open(":memory:").unwrap();
    let before = target.backup().unwrap();
    let mut backup: Value = serde_json::from_str(&s.backup().unwrap()).unwrap();
    let mut downgraded = backup.clone();
    downgraded["version"] = json!(12);
    downgraded
        .as_object_mut()
        .unwrap()
        .remove("foreshadow_proposals");
    assert!(
        target.restore(&downgraded.to_string()).is_err(),
        "v12 imported new author approval without audit"
    );
    backup["foreshadow_proposals"]["rows"] = json!([]);
    assert!(
        target.restore(&backup.to_string()).is_err(),
        "imported a reserved-key formal occurrence after acceptance audit row removed"
    );
    assert_eq!(before, target.backup().unwrap());
}

#[test]
fn sqlite_archived_suggestion_must_agree_with_existing_pack_copy() {
    let (s, _, _, _, _) = accepted_fixture();
    let mut archive = Service::open(":memory:").unwrap();
    archive.restore(&s.backup().unwrap()).unwrap();
    let dir = std::env::temp_dir().join(format!("foreshadow-review-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let file = dir.join("archive.sqlite");
    archive.recovery_snapshot(&file).unwrap();
    let c = rusqlite::Connection::open(&file).unwrap();
    c.execute_batch("DELETE FROM foreshadow_occurrences;")
        .unwrap();
    assert!(
        prepare_restore(&file, |_| Ok(())).is_ok(),
        "agreeing frozen copy must stay supported"
    );
    c.execute_batch("UPDATE foreshadow_proposals SET payload=json_set(payload,'$.note','contradicting fictional note');
        UPDATE foreshadow_proposals SET byte_count=length(CAST(record AS BLOB))+length(CAST(payload AS BLOB));").unwrap();
    drop(c);
    let actual = prepare_restore(&file, |_| Ok(()));
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(
        actual.is_err(),
        "SQLite prepared contradictory same-ID pack/suggestion backup"
    );
}

#[test]
fn sqlite_current_backup_cannot_keep_reserved_formal_row_if_only_audit_is_lost() {
    let (s, _, _, _, _) = accepted_fixture();
    let dir = std::env::temp_dir().join(format!("foreshadow-review-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let file = dir.join("current.sqlite");
    s.recovery_snapshot(&file).unwrap();
    let c = rusqlite::Connection::open(&file).unwrap();
    c.execute_batch("DELETE FROM foreshadow_proposals;")
        .unwrap();
    drop(c);
    let actual = prepare_restore(&file, |_| Ok(()));
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(
        actual.is_err(),
        "SQLite prepared reserved-key formal occurrence without audit row"
    );
}

#[test]
fn live_generation_two_open_does_not_silently_replace_missing_epoch() {
    let dir = std::env::temp_dir().join(format!("foreshadow-review-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let file = dir.join("live.sqlite");
    let (mut s, book, input, epoch) = fixture(Service::open(&file).unwrap());
    let proposal = s
        .propose_foreshadow_occurrence(&book, "fixture", &epoch, input.clone())
        .unwrap();
    drop(s);
    let c = rusqlite::Connection::open(&file).unwrap();
    c.execute_batch("DELETE FROM workflow_meta WHERE key='epoch';")
        .unwrap();
    drop(c);
    assert!(
        prepare_restore(&file, |_| Ok(())).is_err(),
        "prepared restore must reject absent epoch"
    );
    let before = std::fs::read(&file).unwrap();
    assert!(Service::open(&file).is_err(),"opened a generation 2 SQLite file with a lost epoch and silently archived the pending proposal");
    assert_eq!(before, std::fs::read(&file).unwrap());
    let c = rusqlite::Connection::open(&file).unwrap();
    let missing: bool = c
        .query_row(
            "SELECT count(*)=0 FROM workflow_meta WHERE key='epoch'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(missing);
    assert_eq!(
        c.query_row(
            "SELECT status FROM foreshadow_proposals WHERE id=?1",
            [proposal.id],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "pending"
    );
    drop(c);
    std::fs::remove_dir_all(&dir).unwrap();
}

fn two_accepted() -> (Service, String, String) {
    let (mut service, book, mut input, epoch) = fixture(Service::open(":memory:").unwrap());
    let a = service
        .propose_foreshadow_occurrence(&book, "fixture", &epoch, input.clone())
        .unwrap();
    input.idempotency_key = "other-pending-same-revision".into();
    service
        .propose_foreshadow_occurrence(&book, "fixture", &epoch, input.clone())
        .unwrap();
    input.idempotency_key = "reject-at-same-revision".into();
    let rejected = service
        .propose_foreshadow_occurrence(&book, "fixture", &epoch, input.clone())
        .unwrap();
    decide(&mut service, &book, &input, &rejected.id, &epoch, false).unwrap();
    decide(&mut service, &book, &input, &a.id, &epoch, true).unwrap();
    input.material_revision = 1;
    input.idempotency_key = "accepted-from-next-revision".into();
    let b = service
        .propose_foreshadow_occurrence(&book, "fixture", &epoch, input.clone())
        .unwrap();
    let decision = decide(&mut service, &book, &input, &b.id, &epoch, true).unwrap();
    (
        service,
        b.id,
        decision.receipt.accepted_occurrence_id.unwrap(),
    )
}

#[test]
fn json13_rejects_two_acceptances_from_one_card_revision_in_the_same_epoch() {
    let (service, second_id, occurrence_id) = two_accepted();
    let original = service.backup().unwrap();
    let mut target = Service::open(":memory:").unwrap();
    // v0+v1 acceptances, including unrelated v0 pending/rejected rows, are valid.
    target.restore(&original).unwrap();
    let before = target.backup().unwrap();
    let mut duplicate: Value = serde_json::from_str(&original).unwrap();
    let rows = duplicate["foreshadow_proposals"]["rows"]
        .as_array_mut()
        .unwrap();
    let index = rows
        .iter()
        .position(|row| row["record"]["id"] == second_id)
        .unwrap();
    assert_eq!(rows[index]["payload"]["material_revision"], 1);
    rows[index]["payload"]["material_revision"] = json!(0);
    assert!(target.restore(&duplicate.to_string()).is_err());
    assert_eq!(target.backup().unwrap(), before);

    // An old epoch may independently describe an identical base revision.
    let archive_epoch = uuid::Uuid::new_v4().to_string();
    duplicate["foreshadow_proposals"]["rows"][index]["record"]["originating_epoch"] =
        json!(archive_epoch);
    let o = duplicate["foreshadow_occurrences"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|o| o["occurrence"]["id"] == occurrence_id)
        .unwrap();
    o["request_epoch"] = json!(archive_epoch);
    target.restore(&duplicate.to_string()).unwrap();
}

#[test]
fn sqlite_rejects_two_acceptances_from_one_card_revision_before_creating_a_restore_candidate() {
    let (service, second_id, occurrence_id) = two_accepted();
    let scratch = Scratch::new();
    let path = scratch.0.join("same-base.sqlite");
    service.recovery_snapshot(&path).unwrap();
    assert!(prepare_restore(&path, |_| Ok(())).is_ok());
    let mut target = Service::open(":memory:").unwrap();
    target.restore(&service.backup().unwrap()).unwrap();
    let before = target.backup().unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(db.execute("UPDATE foreshadow_proposals SET payload=json_set(payload,'$.material_revision',0) WHERE id=?1", [&second_id]).unwrap(),1);
    assert!(prepare_restore(&path, |_| Ok(())).is_err());
    assert_eq!(target.backup().unwrap(), before);

    let archived_epoch = uuid::Uuid::new_v4().to_string();
    db.execute("UPDATE foreshadow_proposals SET originating_epoch=?1, record=json_set(record,'$.originating_epoch',?1) WHERE id=?2",
        rusqlite::params![archived_epoch,second_id]).unwrap();
    db.execute("UPDATE foreshadow_occurrences SET request_epoch=?1, record=json_set(record,'$.request_epoch',?1) WHERE id=?2",
        rusqlite::params![archived_epoch,occurrence_id]).unwrap();
    assert!(prepare_restore(&path, |_| Ok(())).is_ok());
}
