//! Original literary micro-fixture; candidate text is scripted, not generated.
use novel_core::{
    agent::{AgentBroker, AgentConnection},
    AuthorReviewItem, AuthorReviewVerdict, Command, CreateForeshadowOccurrence,
    CreateMaterialInput, ForeshadowRole, Service, WorkflowError,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

struct TestDir(PathBuf);
impl TestDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("mojuan-longrange-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const STORY: &str = include_str!("../../qa/acceptance/longrange-foreshadow.txt");
const QUOTE: &str = "守闸人把一枚缺了右角的方铜牌推给凌汐。背面刻着：「两短一长，走背潮。」";
const DRAFT: &str = "闸墙内传出铃声。凌汐拢袖，把船往岸边挪。";
const SOURCE_CHANGED: &str = "必需伏笔的出现来源已变化或不可用，请作者先处理";

fn call(session: &AgentConnection, method: &str, params: Value) -> Value {
    session.call(json!({"method":method,"params":params}))
}

fn result(session: &AgentConnection, method: &str, params: Value) -> Value {
    let reply = call(session, method, params);
    assert!(reply.get("error").is_none(), "{method}: {reply}");
    reply.get("result").unwrap().clone()
}

fn assert_wire_error(reply: Value, code: &str) {
    assert_eq!(reply["error"]["code"], code, "{reply}");
    assert!(reply.get("result").is_none(), "{reply}");
}

fn assert_source_changed(error: WorkflowError) {
    assert_eq!(
        (error.code.as_str(), error.message.as_str()),
        ("revision_conflict", SOURCE_CHANGED)
    );
}

#[test]
fn longrange_foreshadow_is_separate_from_eight_prior_chapters_and_version_bound() {
    let chapters: Vec<_> = STORY
        .lines()
        .map(|line| line.split_once('\t').unwrap())
        .collect();
    assert_eq!(chapters.len(), 21);
    let dir = TestDir::new();
    let path = dir.0.join("original.sqlite");
    let mut store = Service::open(&path).unwrap();
    let shelf = store
        .apply(Command::CreateBook {
            title: "苇门背潮".into(),
            author: "原创验收样本".into(),
        })
        .unwrap();
    let book = shelf.books[0].id.clone();
    let volume1 = shelf.volumes[0].id.clone();
    let first = shelf.chapters[0].id.clone();
    store
        .apply(Command::RenameChapter {
            id: first.clone(),
            title: chapters[0].0.into(),
        })
        .unwrap();
    let volume2 = store
        .apply(Command::CreateVolume {
            book_id: book.clone(),
            title: "冬闸".into(),
        })
        .unwrap()
        .volumes
        .into_iter()
        .find(|v| v.id != volume1)
        .unwrap()
        .id;
    let mut ids = Vec::new();
    for (idx, &(title, body)) in chapters.iter().enumerate() {
        let id = if idx == 0 {
            first.clone()
        } else {
            store
                .apply(Command::CreateChapter {
                    volume_id: if idx < 12 {
                        volume1.clone()
                    } else {
                        volume2.clone()
                    },
                    title: title.into(),
                })
                .unwrap()
                .chapters
                .into_iter()
                .find(|c| c.title == title)
                .unwrap()
                .id
        };
        store
            .apply(Command::SaveChapter {
                id: id.clone(),
                body: if idx == 20 { DRAFT } else { body }.into(),
                expected_revision: 0,
            })
            .unwrap();
        ids.push(id);
    }
    let outline = store.get_outline(&book).unwrap();
    assert_eq!(
        outline.chapters.iter().map(|c| &c.id).collect::<Vec<_>>(),
        ids.iter().collect::<Vec<_>>()
    );
    let (plant, echo, target) = (&ids[1], &ids[11], &ids[20]);
    let ordinary = &ids[12..20];
    let original = store.get_chapter(target).unwrap();
    let history = store.chapter_history(target).unwrap();
    let epoch = store.epoch().unwrap();
    // Keep the card unbound: chapter 2 provenance comes only from its occurrence.
    let card = store
        .create_material(CreateMaterialInput {
            book_id: book.clone(),
            kind: "foreshadow".into(),
            title: "方铜牌与背潮门".into(),
            fields: BTreeMap::from([("setup".into(), "守闸人的认门凭证".into())]),
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
    let old_plant = chapters[1].1;
    let offset = old_plant.find(QUOTE).unwrap();
    let start = old_plant[..offset].chars().count() as u32;
    let end = start + QUOTE.chars().count() as u32;
    let occurrence = store
        .create_foreshadow_occurrence(CreateForeshadowOccurrence {
            book_id: book.clone(),
            material_id: card.id.clone(),
            chapter_id: plant.clone(),
            chapter_revision: 1,
            start,
            end,
            quote: QUOTE.into(),
            role: ForeshadowRole::Plant,
            note: "作者登记，尚未回收".into(),
            epoch: epoch.clone(),
            request_key: "plant-chapter-2".into(),
        })
        .unwrap()
        .occurrence;
    let policy = store
        .configure_writing_policy_with_context(
            &book,
            target,
            None,
            "active",
            "让孤舟避开主航道；作者复核远距伏笔",
            vec![card.id.clone()],
            ordinary.to_vec(),
            &epoch,
        )
        .unwrap();
    let manifest = store
        .prepare_writing_pack(&book, target, policy.revision, original.revision, &epoch)
        .unwrap();
    let frozen = store.get_writing_pack(&book, &manifest.id).unwrap();
    assert_eq!(frozen.foreshadow_occurrences, [occurrence.clone()]);
    assert_eq!(
        frozen
            .context_chapters
            .iter()
            .map(|c| &c.chapter.id)
            .collect::<Vec<_>>(),
        ordinary.iter().collect::<Vec<_>>()
    );

    let foreign = store
        .apply(Command::CreateBook {
            title: "无关作品".into(),
            author: String::new(),
        })
        .unwrap()
        .books
        .into_iter()
        .find(|b| b.id != book)
        .unwrap()
        .id;
    let store = Arc::new(Mutex::new(store));
    let broker = Arc::new(AgentBroker::new(store.clone()));
    let session = broker.connect().unwrap();
    let other = broker.connect().unwrap();
    for (connection, scope) in [(&session, &book), (&other, &foreign)] {
        assert_eq!(
            result(
                connection,
                "request_access",
                json!({"client_name":"longrange test"})
            )["status"],
            "pending"
        );
        broker.grant_session(connection.id(), scope).unwrap();
    }
    let entry = format!("material:{}", card.id);
    let field = format!("occurrence:{}", occurrence.id);
    let params = json!({"pack_id":manifest.id,"entry_id":entry,"field_key":field,"cursor":0});
    assert_wire_error(
        call(&other, "read_writing_pack_entry", params.clone()),
        "writing_constraint",
    );
    assert_wire_error(
        call(
            &other,
            "list_foreshadow_occurrences",
            json!({"material_id":card.id}),
        ),
        "not_found",
    );

    let wire = result(
        &session,
        "get_writing_pack_manifest",
        json!({"pack_id":manifest.id}),
    );
    assert_eq!(wire["chapter_id"], *target);
    assert_eq!(wire["book_id"], book);
    let entries = wire["entries"].as_array().unwrap();
    let context = entries
        .iter()
        .filter(|e| e["kind"] == "context_chapter")
        .map(|e| e["metadata"]["chapter_id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        context,
        ordinary.iter().map(String::as_str).collect::<Vec<_>>()
    );
    for prior in [plant, echo] {
        assert_wire_error(
            call(
                &session,
                "read_writing_pack_entry",
                json!({
                    "pack_id":manifest.id,"entry_id":format!("context:{prior}"),"field_key":"body","cursor":0
                }),
            ),
            "writing_constraint",
        );
    }
    for (idx, prior) in (12..20).zip(ordinary) {
        let chunk = result(
            &session,
            "read_writing_pack_entry",
            json!({
                "pack_id":manifest.id,"entry_id":format!("context:{prior}"),"field_key":"body","cursor":0
            }),
        );
        assert_eq!(chunk["text"], chapters[idx].1);
        assert!(chunk["next_cursor"].is_null());
    }
    let chunk = result(&session, "read_writing_pack_entry", params.clone());
    assert_eq!(chunk["text"].as_str().unwrap().as_bytes(), QUOTE.as_bytes());
    assert_eq!(chunk["total_bytes"], QUOTE.len());
    assert!(chunk["next_cursor"].is_null());
    let live_params = json!({"material_id":card.id});
    let live = result(&session, "list_foreshadow_occurrences", live_params.clone());
    assert_eq!(live["items"].as_array().unwrap().len(), 1);
    assert_eq!(live["items"][0]["occurrence"], json!(occurrence));
    assert_eq!(live["items"][0]["source_status"], "current");
    assert_eq!(live["items"][0]["current"]["chapter_id"], *plant);
    assert_eq!(live["items"][0]["current"]["chapter_revision"], 1);
    assert_eq!(live["items"][0]["current"]["volume_id"], volume1);
    assert_ne!(original.volume_id, volume1);

    let mut input = json!({
        "pack_id":manifest.id,"new_body":chapters[20].1,"idempotency_key":"ch21-original",
        "citations":[{"entry_id":entry,"field_key":field,"start":0,"end":QUOTE.chars().count(),
            "explanation":"对应守闸人亲给的方铜牌，不是西仓的圆锡牌"}],"conflicts":[]
    });
    let proposal = result(&session, "propose_constrained_edit", input.clone());
    assert_eq!(proposal["decision"], "pending");
    let proposal_id = proposal["id"].as_str().unwrap();
    let mut edited_plant = old_plant.to_owned();
    edited_plant.push_str("她还记得守闸人把量水尺立在门后。");
    assert_ne!(edited_plant, old_plant);
    assert_eq!(
        &edited_plant.as_bytes()[offset..offset + QUOTE.len()],
        QUOTE.as_bytes()
    );
    let review_revision;
    {
        let mut store = store.lock().unwrap();
        let review = store
            .save_constrained_review(
                &book,
                proposal_id,
                None,
                &epoch,
                vec![AuthorReviewItem {
                    material_id: card.id.clone(),
                    verdict: AuthorReviewVerdict::Conforms,
                    note: "脚本模拟作者，此处仅验证审阅后仍会重新核对来源".into(),
                }],
            )
            .unwrap();
        review_revision = review.revision;
        store
            .apply(Command::SaveChapter {
                id: plant.clone(),
                body: edited_plant.clone(),
                expected_revision: 1,
            })
            .unwrap();
        let stored_plant = store.get_chapter(plant).unwrap();
        assert_eq!(stored_plant.revision, 2);
        assert_eq!(stored_plant.body, edited_plant);
        assert_eq!(
            &stored_plant.body.as_bytes()[offset..offset + QUOTE.len()],
            QUOTE.as_bytes()
        );
        assert_source_changed(
            store
                .save_constrained_review(
                    &book,
                    proposal_id,
                    Some(review_revision),
                    &epoch,
                    vec![AuthorReviewItem {
                        material_id: card.id.clone(),
                        verdict: AuthorReviewVerdict::Question,
                        note: "第12章只是同节奏，未回收。".into(),
                    }],
                )
                .unwrap_err(),
        );
        assert_eq!(
            store
                .get_constrained_proposal(&book, proposal_id)
                .unwrap()
                .author_review,
            Some(review)
        );
        assert_source_changed(
            store
                .prepare_writing_pack(&book, target, policy.revision, original.revision, &epoch)
                .unwrap_err(),
        );
        assert_source_changed(
            store
                .accept_reviewed_constrained_proposal(
                    &book,
                    proposal_id,
                    policy.revision,
                    original.revision,
                    &epoch,
                    review_revision,
                    false,
                )
                .unwrap_err(),
        );
        assert_eq!(store.get_writing_pack(&book, &manifest.id).unwrap(), frozen);
        assert_eq!(store.get_chapter(target).unwrap(), original);
        assert_eq!(store.chapter_history(target).unwrap(), history);
        let state = store.get_writing_state(&book, target).unwrap();
        assert_eq!(state.packs.len(), 1);
        assert_eq!(state.proposals.len(), 1);
        assert_eq!(state.proposals[0].decision, "pending");
    }
    let stale = result(&session, "list_foreshadow_occurrences", live_params);
    assert_eq!(stale["items"][0]["source_status"], "stale");
    assert_eq!(stale["items"][0]["occurrence"], json!(occurrence));
    assert_eq!(stale["items"][0]["current"]["chapter_revision"], 2);
    assert_eq!(result(&session, "read_writing_pack_entry", params), chunk);
    // New key validates a fresh write attempt; replaying the old key is a receipt lookup.
    input["idempotency_key"] = json!("ch21-retry-stale");
    let refused = call(&session, "propose_constrained_edit", input);
    assert_eq!(refused["error"]["message"], SOURCE_CHANGED);
    assert_wire_error(refused, "revision_conflict");
    drop((session, other, broker, store));
    let reopened = Service::open(&path).unwrap();
    assert_eq!(reopened.get_chapter(target).unwrap(), original);
    assert_eq!(reopened.chapter_history(target).unwrap(), history);
    assert_eq!(
        reopened.get_writing_pack(&book, &manifest.id).unwrap(),
        frozen
    );
    let state = reopened.get_writing_state(&book, target).unwrap();
    assert_eq!(state.proposals.len(), 1);
    assert_eq!(
        (
            state.proposals[0].id.as_str(),
            state.proposals[0].decision.as_str()
        ),
        (proposal_id, "pending")
    );
}

const SPLIT_QUOTE: &str = "背面刻着：「两短一长，走背潮。」";

#[test]
fn verbatim_split_requires_explicit_foreshadow_registration_and_new_author_review() {
    for split in [false, true] {
        let story: Vec<_> = STORY
            .lines()
            .map(|line| line.split_once('\t').unwrap())
            .collect();
        let (plant_title, plant_text) = story[1];
        // Give the split cursor a supplementary-plane scalar before the citation.
        let original_source = format!("她提灯🕯沿阶而下。{plant_text}");
        let body = original_source.as_str();
        let (target_title, candidate) = story[20];
        let byte = body.find(SPLIT_QUOTE).unwrap();
        assert_eq!(body.matches(SPLIT_QUOTE).count(), 1);
        let (left_body, right_body) = body.split_at(byte);
        assert!(!left_body.is_empty());
        assert!(right_body.starts_with(SPLIT_QUOTE));
        let start = left_body.chars().count() as u32;
        let utf16_cursor = left_body.encode_utf16().count() as u32;
        assert_eq!(utf16_cursor, start + 1);
        let quote_len = SPLIT_QUOTE.chars().count() as u32;
        let dir = TestDir::new();
        let path = dir.0.join("split-provenance.sqlite");
        let mut store = Service::open(&path).unwrap();
        let shelf = store
            .apply(Command::CreateBook {
                title: "苇门背潮".into(),
                author: "原创验收样本".into(),
            })
            .unwrap();
        let book = shelf.books[0].id.clone();
        let source = shelf.chapters[0].id.clone();
        store
            .apply(Command::RenameChapter {
                id: source.clone(),
                title: plant_title.into(),
            })
            .unwrap();
        let volume2 = store
            .apply(Command::CreateVolume {
                book_id: book.clone(),
                title: "冬闸".into(),
            })
            .unwrap()
            .volumes
            .into_iter()
            .find(|v| v.id != shelf.volumes[0].id)
            .unwrap()
            .id;
        let target = store
            .apply(Command::CreateChapter {
                volume_id: volume2,
                title: target_title.into(),
            })
            .unwrap()
            .chapters
            .into_iter()
            .find(|c| c.title == target_title)
            .unwrap()
            .id;
        for (id, text) in [(&source, body), (&target, DRAFT)] {
            store
                .apply(Command::SaveChapter {
                    id: id.clone(),
                    body: text.into(),
                    expected_revision: 0,
                })
                .unwrap();
        }
        let original = store.get_chapter(&target).unwrap();
        let history = store.chapter_history(&target).unwrap();
        let source_before = store.get_chapter(&source).unwrap();
        let epoch = store.epoch().unwrap();
        let card = store
            .create_material(CreateMaterialInput {
                book_id: book.clone(),
                kind: "foreshadow".into(),
                title: "方铜牌与背潮门".into(),
                fields: BTreeMap::from([("setup".into(), "守闸人的认门凭证".into())]),
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
        let register = |chapter_id: &str, chapter_revision, start, request_key: &str| {
            CreateForeshadowOccurrence {
                book_id: book.clone(),
                material_id: card.id.clone(),
                chapter_id: chapter_id.into(),
                chapter_revision,
                start,
                end: start + quote_len,
                quote: SPLIT_QUOTE.into(),
                role: ForeshadowRole::Plant,
                note: "作者明确登记原文".into(),
                epoch: epoch.clone(),
                request_key: request_key.into(),
            }
        };
        let old = store
            .create_foreshadow_occurrence(register(
                &source,
                source_before.revision,
                start,
                "split-old-occurrence",
            ))
            .unwrap()
            .occurrence;
        assert_eq!(
            (
                old.source.chapter_id.as_str(),
                old.source.chapter_revision,
                old.start,
                old.end
            ),
            (
                source.as_str(),
                source_before.revision,
                start,
                start + quote_len
            )
        );
        let policy = store
            .configure_writing_policy_with_context(
                &book,
                &target,
                None,
                "active",
                "让孤舟绕开主流",
                vec![card.id.clone()],
                vec![],
                &epoch,
            )
            .unwrap();
        let old_pack = store
            .prepare_writing_pack(&book, &target, policy.revision, original.revision, &epoch)
            .unwrap();
        let old_frozen = store.get_writing_pack(&book, &old_pack.id).unwrap();
        assert_eq!(old_frozen.foreshadow_occurrences, [old.clone()]);
        let entry = format!("material:{}", card.id);
        let old_field = format!("occurrence:{}", old.id);
        let read_params = |pack: &str, field: &str| {
            json!({
                "pack_id":pack, "entry_id":entry, "field_key":field, "cursor":0
            })
        };
        let input = |pack: &str, field: &str, key: &str| {
            json!({
                "pack_id":pack, "new_body":candidate, "idempotency_key":key, "conflicts":[],
                "citations":[{"entry_id":entry,"field_key":field,"start":0,"end":quote_len,
                    "explanation":"对应原文铜牌暗号"}]
            })
        };
        let review = |note: &str| {
            vec![AuthorReviewItem {
                material_id: card.id.clone(),
                verdict: AuthorReviewVerdict::Conforms,
                note: note.into(),
            }]
        };
        let grant = |broker: &Arc<AgentBroker>| {
            let session = broker.connect().unwrap();
            assert_eq!(
                result(
                    &session,
                    "request_access",
                    json!({"client_name":"split provenance"})
                )["status"],
                "pending"
            );
            broker.grant_session(session.id(), &book).unwrap();
            session
        };

        let store = Arc::new(Mutex::new(store));
        let broker = Arc::new(AgentBroker::new(store.clone()));
        let session = grant(&broker);
        let old_quote = result(
            &session,
            "read_writing_pack_entry",
            read_params(&old_pack.id, &old_field),
        );
        assert_eq!(
            old_quote["text"].as_str().unwrap().as_bytes(),
            SPLIT_QUOTE.as_bytes()
        );
        assert!(old_quote["next_cursor"].is_null());
        let old_id = result(
            &session,
            "propose_constrained_edit",
            input(&old_pack.id, &old_field, "split-old-draft"),
        )["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let old_review = store
            .lock()
            .unwrap()
            .save_constrained_review(
                &book,
                &old_id,
                None,
                &epoch,
                review("拆分前认可，仅适用旧依据"),
            )
            .unwrap();
        let approved = store
            .lock()
            .unwrap()
            .get_constrained_proposal(&book, &old_id)
            .unwrap();
        assert_eq!(approved.decision, "pending");
        assert_eq!(approved.author_review, Some(old_review.clone()));
        let accept = |s: &mut Service, id: &str, revision| {
            s.accept_reviewed_constrained_proposal(
                &book,
                id,
                policy.revision,
                original.revision,
                &epoch,
                revision,
                false,
            )
        };
        let unchanged = |s: &Service| {
            assert_eq!(s.get_chapter(&target).unwrap(), original);
            assert_eq!(s.chapter_history(&target).unwrap(), history);
        };
        let accepted_once = |s: &Service, proposal_id: &str| {
            let chapter = s.get_chapter(&target).unwrap();
            assert_eq!(chapter.body.as_bytes(), candidate.as_bytes());
            assert_eq!(chapter.revision, original.revision + 1);
            let current = s.chapter_history(&target).unwrap();
            assert_eq!(current.len(), history.len() + 1);
            assert!(history.iter().all(|v| current.contains(v)));
            assert!(current.iter().any(|v| v.revision == chapter.revision
                && v.body.as_bytes() == candidate.as_bytes()
                && v.source == format!("strict:{proposal_id}")));
        };
        unchanged(&store.lock().unwrap());
        if !split {
            let accepted =
                accept(&mut store.lock().unwrap(), &old_id, old_review.revision).unwrap();
            assert_eq!(
                (accepted.decision.as_str(), accepted.accepted_revision),
                ("accepted", Some(original.revision + 1))
            );
            drop((session, broker, store));
            let reopened = Service::open(&path).unwrap();
            accepted_once(&reopened, &old_id);
            assert_eq!(
                reopened.get_constrained_proposal(&book, &old_id).unwrap(),
                accepted
            );
            continue;
        }

        let right = {
            let mut s = store.lock().unwrap();
            let preview = s
                .prepare_chapter_split(
                    &book,
                    &source,
                    source_before.revision,
                    utf16_cursor,
                    "背潮铜牌的刻文",
                )
                .unwrap();
            assert_eq!(
                (preview.before.as_bytes(), preview.after.as_bytes()),
                (left_body.as_bytes(), right_body.as_bytes())
            );
            unchanged(&s);
            let split = s.commit_chapter_split(&preview.id).unwrap();
            let left = s.get_chapter(&source).unwrap();
            let right = s.get_chapter(&split.new_chapter_id).unwrap();
            assert_ne!(left.id, right.id);
            assert_eq!(
                (&left.id, left.revision, left.body.as_bytes()),
                (&source, source_before.revision + 1, left_body.as_bytes())
            );
            assert_eq!(
                (right.revision, right.sort_order, right.body.as_bytes()),
                (0, left.sort_order + 1, right_body.as_bytes())
            );
            assert!(!left.body.contains(SPLIT_QUOTE));
            assert_eq!(right.body.matches(SPLIT_QUOTE).count(), 1);
            assert!(s
                .chapter_history(&source)
                .unwrap()
                .iter()
                .any(|v| v.revision == source_before.revision
                    && v.body.as_bytes() == body.as_bytes()));
            assert_source_changed(accept(&mut s, &old_id, old_review.revision).unwrap_err());
            assert_source_changed(
                s.prepare_writing_pack(&book, &target, policy.revision, original.revision, &epoch)
                    .unwrap_err(),
            );
            assert_eq!(
                s.get_constrained_proposal(&book, &old_id).unwrap(),
                approved
            );
            unchanged(&s);
            right
        };
        let live = result(
            &session,
            "list_foreshadow_occurrences",
            json!({"material_id":card.id}),
        );
        assert_eq!(live["items"].as_array().unwrap().len(), 1);
        assert_eq!(live["items"][0]["occurrence"], json!(old));
        assert_eq!(live["items"][0]["source_status"], "stale");
        assert_eq!(live["items"][0]["current"]["chapter_id"], source);
        assert_eq!(
            live["items"][0]["current"]["chapter_revision"],
            source_before.revision + 1
        );
        assert_eq!(
            result(
                &session,
                "read_writing_pack_entry",
                read_params(&old_pack.id, &old_field)
            ),
            old_quote
        );
        let refused = call(
            &session,
            "propose_constrained_edit",
            input(&old_pack.id, &old_field, "split-stale-new-key"),
        );
        assert_eq!(refused["error"]["message"], SOURCE_CHANGED);
        assert_wire_error(refused, "revision_conflict");
        drop((session, broker, store));

        let mut s = Service::open(&path).unwrap();
        assert_eq!(s.epoch().unwrap(), epoch);
        assert_eq!(s.get_writing_pack(&book, &old_pack.id).unwrap(), old_frozen);
        assert_eq!(
            s.get_constrained_proposal(&book, &old_id).unwrap(),
            approved
        );
        assert_eq!(
            s.get_writing_state(&book, &target).unwrap().proposals.len(),
            1
        );
        assert_eq!(s.get_chapter(&right.id).unwrap(), right);
        unchanged(&s);
        assert_source_changed(accept(&mut s, &old_id, old_review.revision).unwrap_err());
        let retired = s
            .set_foreshadow_occurrence_deleted(&book, &card.id, &old.id, old.revision, &epoch, true)
            .unwrap()
            .occurrence;
        assert!(retired.deleted_at.is_some());
        assert_eq!(
            (&retired.source, retired.start, retired.end, &retired.quote),
            (&old.source, old.start, old.end, &old.quote)
        );
        let registered = s
            .create_foreshadow_occurrence(register(
                &right.id,
                right.revision,
                0,
                "split-new-occurrence",
            ))
            .unwrap();
        assert_eq!(
            serde_json::to_value(registered.source_status).unwrap(),
            "current"
        );
        let new = registered.occurrence;
        assert_ne!(new.id, old.id);
        assert_eq!(
            (
                new.source.chapter_id.as_str(),
                new.source.chapter_revision,
                new.start,
                new.end,
                new.quote.as_bytes()
            ),
            (
                right.id.as_str(),
                right.revision,
                0,
                quote_len,
                old.quote.as_bytes()
            )
        );
        let corrected = s
            .prepare_writing_pack(&book, &target, policy.revision, original.revision, &epoch)
            .unwrap();
        assert_ne!(corrected.id, old_pack.id);
        assert_eq!(
            s.get_writing_pack(&book, &corrected.id)
                .unwrap()
                .foreshadow_occurrences,
            [new.clone()]
        );
        let old_refused = accept(&mut s, &old_id, old_review.revision).unwrap_err();
        assert_eq!(
            old_refused.message,
            "章节、前文、必需资料或来源已改变，旧建议不可采纳"
        );
        assert_eq!(
            s.get_constrained_proposal(&book, &old_id).unwrap(),
            approved
        );
        unchanged(&s);
        let store = Arc::new(Mutex::new(s));
        let broker = Arc::new(AgentBroker::new(store.clone()));
        let session = grant(&broker);
        assert_wire_error(
            call(
                &session,
                "read_writing_pack_entry",
                read_params(&corrected.id, &old_field),
            ),
            "writing_constraint",
        );
        let field = format!("occurrence:{}", new.id);
        let quote = result(
            &session,
            "read_writing_pack_entry",
            read_params(&corrected.id, &field),
        );
        assert_eq!(
            quote["text"].as_str().unwrap().as_bytes(),
            SPLIT_QUOTE.as_bytes()
        );
        let new_id = result(
            &session,
            "propose_constrained_edit",
            input(&corrected.id, &field, "split-corrected-draft"),
        )["id"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_ne!(new_id, old_id);
        let accepted;
        {
            let mut s = store.lock().unwrap();
            let pending = s.get_constrained_proposal(&book, &new_id).unwrap();
            assert_eq!(pending.decision, "pending");
            assert_eq!(pending.input.pack_id, corrected.id);
            assert!(pending.author_review.is_none());
            let error = accept(&mut s, &new_id, old_review.revision).unwrap_err();
            assert_eq!(
                (error.code.as_str(), error.message.as_str()),
                ("writing_constraint", "请先保存逐项作者审阅")
            );
            assert_eq!(s.get_constrained_proposal(&book, &new_id).unwrap(), pending);
            unchanged(&s);
            let fresh_review = s
                .save_constrained_review(
                    &book,
                    &new_id,
                    None,
                    &epoch,
                    review("拆章后按新章与新依据重新审核"),
                )
                .unwrap();
            assert_eq!(fresh_review.proposal_id, new_id);
            unchanged(&s);
            accepted = accept(&mut s, &new_id, fresh_review.revision).unwrap();
            assert_eq!(
                (
                    accepted.decision.as_str(),
                    accepted.accepted_revision,
                    accepted.author_review.as_ref()
                ),
                ("accepted", Some(original.revision + 1), Some(&fresh_review))
            );
            accepted_once(&s, &new_id);
        }
        drop((session, broker, store));
        let reopened = Service::open(&path).unwrap();
        accepted_once(&reopened, &new_id);
        assert_eq!(
            reopened.get_writing_pack(&book, &old_pack.id).unwrap(),
            old_frozen
        );
        assert_eq!(
            reopened.get_constrained_proposal(&book, &old_id).unwrap(),
            approved
        );
        assert_eq!(
            reopened.get_constrained_proposal(&book, &new_id).unwrap(),
            accepted
        );
        assert_eq!(
            reopened
                .get_foreshadow_occurrence(&book, &card.id, &old.id, &epoch)
                .unwrap()
                .occurrence,
            retired
        );
        let rows = reopened
            .list_foreshadow_occurrences(&book, &card.id, &epoch)
            .unwrap();
        assert_eq!(rows.len(), 2);
        let active: Vec<_> = rows
            .iter()
            .filter(|row| row.occurrence.deleted_at.is_none())
            .collect();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].occurrence, new);
        assert_eq!(
            serde_json::to_value(active[0].source_status).unwrap(),
            "current"
        );
    }
}
