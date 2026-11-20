//! Original controlled pair. The trusted author verdicts below are scripted, not inferred.
use novel_core::{
    agent::{AgentBroker, AgentConnection},
    AuthorReviewItem, AuthorReviewVerdict, Command, Service,
};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

struct TestDir(PathBuf);
impl TestDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("mojuan-disclosure-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        Self(dir)
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn result(session: &AgentConnection, method: &str, params: Value) -> Value {
    let reply = session.call(json!({"method":method,"params":params}));
    assert!(reply.get("error").is_none(), "{method}: {reply}");
    reply["result"].clone()
}

#[test]
fn same_exact_setting_citation_does_not_substitute_for_author_disclosure_review() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../qa/acceptance/setting-disclosure.json")).unwrap();
    let text = |key: &str| fixture[key].as_str().unwrap();
    let statement = "苏苇诚实地作证：“我亲眼看见匣底有三道青线。”";
    assert_eq!(
        text("early_candidate").split_once('，').unwrap().1,
        text("late_candidate").split_once('，').unwrap().1
    );
    for key in ["early_candidate", "late_candidate"] {
        assert!(text(key).ends_with(statement));
    }

    let dir = TestDir::new();
    let path = dir.0.join("disclosure.sqlite");
    let mut store = Service::open(&path).unwrap();
    let shelf = store
        .apply(Command::CreateBook {
            title: "汐灯阁".into(),
            author: "原创验收样本".into(),
        })
        .unwrap();
    let book = shelf.books[0].id.clone();
    let volume = shelf.volumes[0].id.clone();
    let prior = shelf.chapters[0].id.clone();
    let mut ids = vec![prior.clone()];
    for title in ["门廊候匣", "当面开匣", "次晨作证"] {
        let state = store
            .apply(Command::CreateChapter {
                volume_id: volume.clone(),
                title: title.into(),
            })
            .unwrap();
        ids.push(
            state
                .chapters
                .iter()
                .find(|c| c.title == title)
                .unwrap()
                .id
                .clone(),
        );
    }
    for (id, key) in ids
        .iter()
        .zip(["prior", "early_draft", "disclosure", "late_draft"])
    {
        store
            .apply(Command::SaveChapter {
                id: id.clone(),
                body: text(key).into(),
                expected_revision: 0,
            })
            .unwrap();
    }
    assert_eq!(
        store
            .get_outline(&book)
            .unwrap()
            .chapters
            .iter()
            .map(|c| &c.id)
            .collect::<Vec<_>>(),
        ids.iter().collect::<Vec<_>>()
    );
    let [prior, early, disclosure, late] = ids.as_slice() else {
        panic!("four chapters")
    };
    let epoch = store.epoch().unwrap();
    let material = store
        .create_material(
            serde_json::from_value(json!({
                "book_id":book,"kind":"setting","title":"苏苇对匣底印记的获知规则",
                "fields":{"synopsis":text("rule")}
            }))
            .unwrap(),
        )
        .unwrap();
    let targets = [early, late];
    let context = [vec![prior.clone()], vec![prior.clone(), disclosure.clone()]];
    let mut manifests = Vec::new();
    for (target, context) in targets.iter().zip(&context) {
        let policy = store
            .configure_writing_policy_with_context(
                &book,
                target,
                None,
                "active",
                "核对苏苇在本场景的获知范围",
                vec![material.id.clone()],
                context.clone(),
                &epoch,
            )
            .unwrap();
        manifests.push(
            store
                .prepare_writing_pack(&book, target, policy.revision, 1, &epoch)
                .unwrap(),
        );
    }

    let store = Arc::new(Mutex::new(store));
    let broker = Arc::new(AgentBroker::new(store.clone()));
    let agent = broker.connect().unwrap();
    assert_eq!(
        result(
            &agent,
            "request_access",
            json!({"client_name":"disclosure test"})
        )["status"],
        "pending"
    );
    broker.grant_session(agent.id(), &book).unwrap();
    let entry = format!("material:{}", material.id);
    let citation = json!({
        "entry_id":entry,"field_key":"synopsis","start":0,"end":text("rule").chars().count(),
        "explanation":"依据这条设定核对苏苇何时能作第一手确认"
    });

    // The approved early pack lacks this future chapter; direct use as a pack
    // handle fails. This is a freeze-scope check, not a semantic timeline parser.
    let outside = agent.call(json!({"method":"read_writing_pack_entry","params":{
        "pack_id":manifests[0].id,"entry_id":format!("context:{disclosure}"),
        "field_key":"body","cursor":0
    }}));
    assert_eq!(outside["error"]["code"], "writing_constraint");
    assert!(outside.get("result").is_none());
    let (early_base, early_history, late_base, late_history) = {
        let s = store.lock().unwrap();
        (
            s.get_chapter(early).unwrap(),
            s.chapter_history(early).unwrap(),
            s.get_chapter(late).unwrap(),
            s.chapter_history(late).unwrap(),
        )
    };
    assert_eq!(early_base.body, text("early_draft"));
    assert_eq!(late_base.body, text("late_draft"));
    let assert_manuscripts_unchanged = |s: &Service| {
        assert_eq!(s.get_chapter(early).unwrap(), early_base);
        assert_eq!(s.chapter_history(early).unwrap(), early_history);
        assert_eq!(s.get_chapter(late).unwrap(), late_base);
        assert_eq!(s.chapter_history(late).unwrap(), late_history);
    };
    let mut proposals = Vec::new();
    for ((manifest, key), expected_context) in manifests
        .iter()
        .zip(["early_candidate", "late_candidate"])
        .zip(&context)
    {
        let wire = result(
            &agent,
            "get_writing_pack_manifest",
            json!({"pack_id":manifest.id}),
        );
        let ids = wire["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["kind"] == "context_chapter")
            .map(|e| e["metadata"]["chapter_id"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            expected_context
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
        );
        for id in expected_context {
            let chunk = result(
                &agent,
                "read_writing_pack_entry",
                json!({
                    "pack_id":manifest.id,"entry_id":format!("context:{id}"),"field_key":"body","cursor":0
                }),
            );
            assert_eq!(
                chunk["text"],
                text(if id == prior { "prior" } else { "disclosure" })
            );
            assert!(chunk["next_cursor"].is_null());
        }
        let chunk = result(
            &agent,
            "read_writing_pack_entry",
            json!({
                "pack_id":manifest.id,"entry_id":entry,"field_key":"synopsis","cursor":0
            }),
        );
        assert_eq!(
            chunk["text"].as_str().unwrap().as_bytes(),
            text("rule").as_bytes()
        );
        assert!(chunk["next_cursor"].is_null());
        let p = result(
            &agent,
            "propose_constrained_edit",
            json!({
                "pack_id":manifest.id,"new_body":text(key),"idempotency_key":key,
                "citations":[citation.clone()],"conflicts":[]
            }),
        );
        assert_eq!(p["decision"], "pending");
        proposals.push(p["id"].as_str().unwrap().to_owned());
        assert_manuscripts_unchanged(&store.lock().unwrap());
    }

    let (early_review, late_review);
    {
        let mut s = store.lock().unwrap();
        for id in &proposals {
            let p = s.get_constrained_proposal(&book, id).unwrap();
            assert_eq!(json!(p.input.citations), json!([citation]));
            assert!(p.input.conflicts.is_empty());
            assert!(p.author_review.is_none());
        }

        early_review = s.save_constrained_review(&book, &proposals[0], None, &epoch, vec![
            AuthorReviewItem {
                material_id: material.id.clone(), verdict: AuthorReviewVerdict::Conflict,
                note: "候选说在开匣前，苏苇却诚实声称自己亲眼看见。她先前在门廊抄表、无钥匙；唯一当面开匣发生在下一场。此处无传闻、诈唬或替代接触途径。".into(),
            }
        ]).unwrap();
        assert_manuscripts_unchanged(&s);
        let err = s
            .accept_reviewed_constrained_proposal(
                &book,
                &proposals[0],
                manifests[0].policy_revision,
                early_base.revision,
                &epoch,
                early_review.revision,
                true,
            )
            .unwrap_err();
        assert_eq!(err.code, "writing_constraint");
        assert_eq!(err.message, "所有必需资料均须由作者明确判断为符合");
        assert_eq!(
            s.get_constrained_proposal(&book, &proposals[0])
                .unwrap()
                .decision,
            "pending"
        );
        assert_eq!(s.get_chapter(early).unwrap(), early_base);
        assert_eq!(s.chapter_history(early).unwrap(), early_history);
        assert_eq!(
            s.decide_constrained_proposal(
                &book,
                &proposals[0],
                false,
                manifests[0].policy_revision,
                early_base.revision,
                &epoch,
                false,
            )
            .unwrap()
            .decision,
            "rejected"
        );

        let unreviewed = s
            .accept_reviewed_constrained_proposal(
                &book,
                &proposals[1],
                manifests[1].policy_revision,
                late_base.revision,
                &epoch,
                1,
                false,
            )
            .unwrap_err();
        assert_eq!(unreviewed.message, "请先保存逐项作者审阅");
        assert_eq!(s.get_chapter(late).unwrap(), late_base);
        assert_eq!(s.chapter_history(late).unwrap(), late_history);
        late_review = s.save_constrained_review(&book, &proposals[1], None, &epoch, vec![
            AuthorReviewItem {
                material_id: material.id, verdict: AuthorReviewVerdict::Conforms,
                note: "候选在次晨；上一场值守人已当面开匣，苏苇亲眼看到三道青线。就本设定项而言，第一手陈述有依据。".into(),
            }
        ]).unwrap();
        assert_manuscripts_unchanged(&s);
        let accepted = s
            .accept_reviewed_constrained_proposal(
                &book,
                &proposals[1],
                manifests[1].policy_revision,
                late_base.revision,
                &epoch,
                late_review.revision,
                false,
            )
            .unwrap();
        assert_eq!(accepted.decision, "accepted");
        assert_eq!(accepted.accepted_revision, Some(late_base.revision + 1));
    }
    drop((agent, broker, store));

    let reopened = Service::open(&path).unwrap();
    let refused = reopened
        .get_constrained_proposal(&book, &proposals[0])
        .unwrap();
    assert_eq!(refused.decision, "rejected");
    assert_eq!(refused.author_review, Some(early_review));
    assert_eq!(reopened.get_chapter(early).unwrap(), early_base);
    assert_eq!(reopened.chapter_history(early).unwrap(), early_history);
    let accepted = reopened
        .get_constrained_proposal(&book, &proposals[1])
        .unwrap();
    assert_eq!(accepted.decision, "accepted");
    assert_eq!(accepted.author_review, Some(late_review));
    let late_after = reopened.get_chapter(late).unwrap();
    assert_eq!(late_after.revision, late_base.revision + 1);
    assert_eq!(
        late_after.body.as_bytes(),
        text("late_candidate").as_bytes()
    );
    let mut history = reopened.chapter_history(late).unwrap();
    assert_eq!(history.len(), late_history.len() + 1);
    let pos = history
        .iter()
        .position(|v| v.revision == late_base.revision + 1)
        .unwrap();
    let version = history.remove(pos);
    assert_eq!(version.body.as_bytes(), text("late_candidate").as_bytes());
    assert_eq!(history, late_history);
}

#[test]
fn moving_the_frozen_disclosure_past_the_target_requires_a_new_review() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../qa/acceptance/setting-disclosure.json")).unwrap();
    let text = |key: &str| fixture[key].as_str().unwrap();
    for moved in [false, true] {
        let dir = TestDir::new();
        let path = dir.0.join("move-disclosure.sqlite");
        let mut store = Service::open(&path).unwrap();
        let shelf = store
            .apply(Command::CreateBook {
                title: "汐灯阁".into(),
                author: "原创验收样本".into(),
            })
            .unwrap();
        let book = shelf.books[0].id.clone();
        let volume = &shelf.volumes[0].id;
        let prior = shelf.chapters[0].id.clone();
        let mut add_chapter = |title: &str| {
            store
                .apply(Command::CreateChapter {
                    volume_id: volume.clone(),
                    title: title.into(),
                })
                .unwrap()
                .chapters
                .into_iter()
                .find(|c| c.title == title)
                .unwrap()
                .id
        };
        let disclosure = add_chapter("当面开匣");
        let target = add_chapter("次晨作证");
        for (id, key) in [
            (&prior, "prior"),
            (&disclosure, "disclosure"),
            (&target, "late_draft"),
        ] {
            store
                .apply(Command::SaveChapter {
                    id: id.clone(),
                    body: text(key).into(),
                    expected_revision: 0,
                })
                .unwrap();
        }
        let epoch = store.epoch().unwrap();
        let material = store
            .create_material(
                serde_json::from_value(json!({
                    "book_id":book,"kind":"setting","title":"苏苇对匣底印记的获知规则",
                    "fields":{"synopsis":text("rule")}
                }))
                .unwrap(),
            )
            .unwrap();
        let goal = "比较目录前文是否仍包含苏苇当面开匣的依据";
        let policy = store
            .configure_writing_policy_with_context(
                &book,
                &target,
                None,
                "active",
                goal,
                vec![material.id.clone()],
                vec![prior.clone(), disclosure.clone()],
                &epoch,
            )
            .unwrap();
        let original_target = store.get_chapter(&target).unwrap();
        let original_disclosure = store.get_chapter(&disclosure).unwrap();
        let target_history = store.chapter_history(&target).unwrap();
        let disclosure_history = store.chapter_history(&disclosure).unwrap();
        let pack = store
            .prepare_writing_pack(
                &book,
                &target,
                policy.revision,
                original_target.revision,
                &epoch,
            )
            .unwrap();
        let handle = format!("context:{disclosure}");
        let context_ids = |pack: &novel_core::WritingPackManifest| {
            pack.entries
                .iter()
                .filter(|e| e.kind == "context_chapter")
                .map(|e| e.metadata["chapter_id"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(context_ids(&pack), vec![prior.clone(), disclosure.clone()]);
        let read_disclosure = |s: &Service| {
            let chunk = s
                .read_writing_pack_entry(&book, &pack.id, &handle, "body", 0)
                .unwrap();
            assert_eq!(chunk.text.as_bytes(), text("disclosure").as_bytes());
            assert_eq!(chunk.next_cursor, None);
        };
        read_disclosure(&store);
        let submit = |s: &mut Service, pack_id: &str, key: &str| {
            s.propose_constrained_edit(
                &book,
                "disclosure-test",
                serde_json::from_value(json!({
                    "pack_id":pack_id,"new_body":text("late_candidate"),"idempotency_key":key,
                    "citations":[{
                        "entry_id":format!("material:{}",material.id),"field_key":"synopsis",
                        "start":0,"end":text("rule").chars().count(),
                        "explanation":"核对第一手证词与开匣依据"
                    }],
                    "conflicts":[]
                }))
                .unwrap(),
            )
            .unwrap()
        };
        let proposal = submit(&mut store, &pack.id, "reviewed");
        assert!(proposal.author_review.is_none());
        let review = store
            .save_constrained_review(
                &book,
                &proposal.id,
                None,
                &epoch,
                vec![AuthorReviewItem {
                    material_id: material.id.clone(),
                    verdict: AuthorReviewVerdict::Conforms,
                    note: "冻结前文可读到苏苇当面开匣，次晨证词有依据。".into(),
                }],
            )
            .unwrap();
        let reviewed = store.get_constrained_proposal(&book, &proposal.id).unwrap();
        assert_eq!(reviewed.author_review.as_ref(), Some(&review));
        assert_eq!(reviewed.decision, "pending");
        assert_eq!(store.get_chapter(&target).unwrap(), original_target);
        assert_eq!(store.chapter_history(&target).unwrap(), target_history);
        assert_eq!(store.get_chapter(&disclosure).unwrap(), original_disclosure);
        assert_eq!(
            store.chapter_history(&disclosure).unwrap(),
            disclosure_history
        );
        if moved {
            store
                .apply(Command::MoveChapter {
                    id: disclosure.clone(),
                    direction: 1,
                })
                .unwrap();
        }
        use novel_core::WritingPredecessorPosition;
        let observed = store
            .diagnose_writing_predecessors(&book, &target, &pack.id, &epoch)
            .unwrap();
        assert_eq!(observed.epoch, epoch);
        assert_eq!(observed.book_id, book);
        assert_eq!(observed.chapter_id, target);
        assert_eq!(observed.pack_id, pack.id);
        assert_eq!(observed.target.id, target);
        assert_eq!(observed.entries.len(), 2);
        assert_eq!(observed.entries[0].chapter_id, prior);
        assert_eq!(
            observed.entries[0].position,
            WritingPredecessorPosition::Before
        );
        assert_eq!(observed.entries[1].chapter_id, disclosure);
        assert_eq!(
            observed.entries[1].current.as_ref().unwrap().title,
            "当面开匣"
        );
        assert_eq!(
            observed.entries[1].position,
            if moved {
                WritingPredecessorPosition::After
            } else {
                WritingPredecessorPosition::Before
            }
        );
        for (b, c, p, e) in [
            (
                "other-book",
                target.as_str(),
                pack.id.as_str(),
                epoch.as_str(),
            ),
            (
                book.as_str(),
                disclosure.as_str(),
                pack.id.as_str(),
                epoch.as_str(),
            ),
            (book.as_str(), target.as_str(), "other-pack", epoch.as_str()),
            (
                book.as_str(),
                target.as_str(),
                pack.id.as_str(),
                "old-epoch",
            ),
        ] {
            assert!(store.diagnose_writing_predecessors(b, c, p, e).is_err());
        }
        let directory = store.get_writing_context_directory(&book, &target).unwrap();
        assert_eq!(
            directory
                .chapters
                .into_iter()
                .map(|c| c.id)
                .collect::<Vec<_>>(),
            if moved {
                vec![prior.clone()]
            } else {
                vec![prior.clone(), disclosure.clone()]
            }
        );
        assert_eq!(store.epoch().unwrap(), epoch);
        let mut expected_target = original_target.clone();
        let mut expected_disclosure = original_disclosure.clone();
        if moved {
            std::mem::swap(
                &mut expected_target.sort_order,
                &mut expected_disclosure.sort_order,
            );
        }
        let assert_untouched = |s: &Service| {
            assert_eq!(s.get_chapter(&target).unwrap(), expected_target);
            assert_eq!(s.get_chapter(&disclosure).unwrap(), expected_disclosure);
            assert_eq!(s.chapter_history(&target).unwrap(), target_history);
            assert_eq!(s.chapter_history(&disclosure).unwrap(), disclosure_history);
        };
        assert_untouched(&store);
        assert_eq!(
            store.get_constrained_proposal(&book, &proposal.id).unwrap(),
            reviewed
        );
        let attempt = store.accept_reviewed_constrained_proposal(
            &book,
            &proposal.id,
            policy.revision,
            original_target.revision,
            &epoch,
            review.revision,
            false,
        );
        if !moved {
            let accepted = attempt.unwrap();
            assert_eq!(accepted.decision, "accepted");
            assert_eq!(
                accepted.accepted_revision,
                Some(original_target.revision + 1)
            );
            drop(store);
            let reopened = Service::open(&path).unwrap();
            assert_eq!(
                reopened.get_chapter(&target).unwrap().body.as_bytes(),
                text("late_candidate").as_bytes()
            );
            assert_eq!(
                reopened
                    .get_constrained_proposal(&book, &proposal.id)
                    .unwrap(),
                accepted
            );
            continue;
        }
        let why = "所选前文章节必须在目标章之前，请调整配置";
        let error = attempt.unwrap_err();
        assert_eq!(error.code, "writing_constraint");
        assert_eq!(error.message, why);
        assert_eq!(
            store.get_constrained_proposal(&book, &proposal.id).unwrap(),
            reviewed
        );
        assert_untouched(&store);
        let error = store
            .prepare_writing_pack(
                &book,
                &target,
                policy.revision,
                original_target.revision,
                &epoch,
            )
            .unwrap_err();
        assert_eq!(error.code, "writing_constraint");
        assert_eq!(error.message, why);
        read_disclosure(&store);
        let revised = store
            .configure_writing_policy_with_context(
                &book,
                &target,
                Some(policy.revision),
                "active",
                goal,
                vec![material.id.clone()],
                vec![prior.clone()],
                &epoch,
            )
            .unwrap();
        let cancelled = store.get_constrained_proposal(&book, &proposal.id).unwrap();
        assert_eq!(cancelled.decision, "cancelled");
        assert_eq!(cancelled.author_review.as_ref(), Some(&review));
        let fresh = store
            .prepare_writing_pack(
                &book,
                &target,
                revised.revision,
                original_target.revision,
                &epoch,
            )
            .unwrap();
        assert_eq!(context_ids(&fresh), vec![prior.clone()]);
        assert_eq!(
            store
                .read_writing_pack_entry(&book, &fresh.id, &handle, "body", 0)
                .unwrap_err()
                .code,
            "writing_constraint",
        );
        let next = submit(&mut store, &fresh.id, "unreviewed");
        assert_eq!(next.decision, "pending");
        assert!(next.author_review.is_none());
        let error = store
            .accept_reviewed_constrained_proposal(
                &book,
                &next.id,
                revised.revision,
                original_target.revision,
                &epoch,
                review.revision,
                false,
            )
            .unwrap_err();
        assert_eq!(error.message, "请先保存逐项作者审阅");
        assert_untouched(&store);
        drop(store);
        let mut reopened = Service::open(&path).unwrap();
        assert_untouched(&reopened);
        read_disclosure(&reopened);
        let old = reopened
            .get_constrained_proposal(&book, &proposal.id)
            .unwrap();
        assert_eq!(old.decision, "cancelled");
        assert_eq!(old.author_review.as_ref(), Some(&review));
        assert_eq!(
            reopened.get_constrained_proposal(&book, &next.id).unwrap(),
            next
        );
        let mut replaced = Service::open(":memory:").unwrap();
        replaced
            .restore_with_writing_ack(&reopened.backup().unwrap(), true)
            .unwrap();
        assert!(replaced.get_chapter(&target).is_ok());
        let replacement_epoch = replaced.epoch().unwrap();
        assert_ne!(replacement_epoch, epoch);
        assert!(replaced
            .diagnose_writing_predecessors(&book, &target, &pack.id, &replacement_epoch)
            .unwrap_err()
            .message
            .contains("已归档"));
        reopened
            .apply(Command::TrashChapter {
                id: disclosure.clone(),
            })
            .unwrap();
        let absent = reopened
            .diagnose_writing_predecessors(&book, &target, &pack.id, &epoch)
            .unwrap();
        assert_eq!(absent.entries[1].chapter_id, disclosure);
        assert_eq!(
            absent.entries[1].position,
            WritingPredecessorPosition::NotVisible
        );
        assert!(absent.entries[1].current.is_none());
        reopened
            .apply(Command::TrashChapter { id: target.clone() })
            .unwrap();
        assert!(reopened
            .diagnose_writing_predecessors(&book, &target, &pack.id, &epoch)
            .is_err());
    }
}


#[test]
fn outline_and_written_predecessor_conflict_requires_author_judgment() {
    let f: Value =
        serde_json::from_str(include_str!("../../qa/acceptance/outline-conflict.json")).unwrap();
    let text = |key: &str| f[key].as_str().unwrap();
    let dir = TestDir::new();
    let path = dir.0.join("outline-conflict.sqlite");
    let mut store = Service::open(&path).unwrap();
    let shelf = store
        .apply(Command::CreateBook {
            title: text("title").into(),
            author: "原创验收样本".into(),
        })
        .unwrap();
    let book = shelf.books[0].id.clone();
    let volume = shelf.volumes[0].id.clone();
    let prior = shelf.chapters[0].id.clone();
    store
        .apply(Command::RenameChapter {
            id: prior.clone(),
            title: text("prior_title").into(),
        })
        .unwrap();
    let target = store
        .apply(Command::CreateChapter {
            volume_id: volume,
            title: text("chapter").into(),
        })
        .unwrap()
        .chapters
        .into_iter()
        .find(|c| c.title == text("chapter"))
        .unwrap()
        .id;
    for (id, value) in [(&prior, text("prior_body")), (&target, text("original"))] {
        store
            .apply(Command::SaveChapter {
                id: id.clone(),
                body: value.into(),
                expected_revision: 0,
            })
            .unwrap();
    }
    let outline = store
        .create_material(
            serde_json::from_value(json!({
                "book_id":book,"kind":"outline","title":text("outline_title"),
                "fields":{"content":text("outline_body")}
            }))
            .unwrap(),
        )
        .unwrap();
    let base = store.get_chapter(&target).unwrap();
    let history = store.chapter_history(&target).unwrap();
    let unchanged = |s: &Service| {
        assert_eq!(s.get_chapter(&target).unwrap(), base);
        assert_eq!(s.chapter_history(&target).unwrap(), history);
    };
    let epoch = store.epoch().unwrap();
    let policy = store
        .configure_writing_policy_with_context(
            &book,
            &target,
            None,
            "active",
            "核对次晨过桥的依据",
            vec![outline.id.clone()],
            vec![prior.clone()],
            &epoch,
        )
        .unwrap();
    let pack = store
        .prepare_writing_pack(&book, &target, policy.revision, base.revision, &epoch)
        .unwrap();
    let store = Arc::new(Mutex::new(store));
    let broker = Arc::new(AgentBroker::new(store.clone()));
    let agent = broker.connect().unwrap();
    assert_eq!(
        result(
            &agent,
            "request_access",
            json!({"client_name":"outline conflict fixture"})
        )["status"],
        "pending"
    );
    broker.grant_session(agent.id(), &book).unwrap();
    let cited = |entry: String, field: &str, body_key: &str, quote_key: &str| {
        let frozen = result(
            &agent,
            "read_writing_pack_entry",
            json!({"pack_id":pack.id,"entry_id":entry,"field_key":field,"cursor":0}),
        );
        assert_eq!(frozen["text"], text(body_key));
        assert!(frozen["next_cursor"].is_null());
        let full = frozen["text"].as_str().unwrap();
        let quote = text(quote_key);
        assert_eq!(full.matches(quote).count(), 1);
        let start = full[..full.find(quote).unwrap()].chars().count();
        let end = start + quote.chars().count();
        assert_eq!(
            full.chars()
                .skip(start)
                .take(end - start)
                .collect::<String>(),
            quote
        );
        json!({"entry_id":entry,"field_key":field,"start":start,"end":end,"explanation":"冻结引文，供作者核对"})
    };
    let outline_cite = cited(
        format!("material:{}", outline.id),
        "content",
        "outline_body",
        "outline_quote",
    );
    let prior_cite = cited(
        format!("context:{prior}"),
        "body",
        "prior_body",
        "prior_quote",
    );
    let proposals = [
        ("flagged",json!([outline_cite,prior_cite]), json!([{"entry_ids":[outline.id],"explanation":text("conflict")}])),
        ("foil",json!([outline_cite]),json!([])),
    ].map(|(name,citations,conflicts)| {
        let p = result(&agent,"propose_constrained_edit",json!({"pack_id":pack.id,"new_body":text(name),"idempotency_key":name,"citations":citations,"conflicts":conflicts}));
        let s=store.lock().unwrap();
        let q=s.get_constrained_proposal(&book,p["id"].as_str().unwrap()).unwrap();
        assert_eq!(q.decision,"pending");
        assert_eq!(q.input.pack_id,pack.id);
        assert_eq!(q.input.new_body,text(name));
        assert!(q.author_review.is_none());
        unchanged(&s);
        q
    });
    assert_ne!(proposals[0].id, proposals[1].id);
    assert_eq!(proposals[0].input.citations.len(), 2);
    assert_eq!(
        proposals[0].input.conflicts[0].explanation,
        text("conflict")
    );
    assert_eq!(proposals[1].input.citations.len(), 1);
    assert!(proposals[1].input.conflicts.is_empty());
    let mut persisted = Vec::new();
    {
        let mut s = store.lock().unwrap();
        for p in &proposals {
            let review = s
                .save_constrained_review(
                    &book,
                    &p.id,
                    None,
                    &epoch,
                    vec![AuthorReviewItem {
                        material_id: outline.id.clone(),
                        verdict: AuthorReviewVerdict::Question,
                        note: text("note").into(),
                    }],
                )
                .unwrap();
            unchanged(&s);
            let error = s
                .accept_reviewed_constrained_proposal(
                    &book,
                    &p.id,
                    policy.revision,
                    base.revision,
                    &epoch,
                    review.revision,
                    true,
                )
                .unwrap_err();
            assert_eq!(
                (error.code.as_str(), error.message.as_str()),
                ("writing_constraint", "所有必需资料均须由作者明确判断为符合")
            );
            unchanged(&s);
            let q = s.get_constrained_proposal(&book, &p.id).unwrap();
            assert_eq!(q.decision, "pending");
            assert_eq!(q.author_review, Some(review));
            persisted.push(q);
        }
    }
    drop((agent, broker, store));
    let reopened = Service::open(&path).unwrap();
    unchanged(&reopened);
    for p in persisted {
        assert_eq!(reopened.get_constrained_proposal(&book, &p.id).unwrap(), p);
    }
}
