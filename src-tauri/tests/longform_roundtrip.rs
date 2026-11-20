//! One connected acceptance scenario, using only original fictional text.
use novel_core::{
    agent::{AgentBroker, AgentConnection},
    files::write_new_file,
    prepare_restore, AuthorReviewItem, AuthorReviewVerdict, Command, CreateForeshadowOccurrence,
    CreateMaterialInput, ExportOptions, ForeshadowRole, Service,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc, Mutex},
};

struct TestDir(PathBuf);
impl TestDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("mojuan-roundtrip-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const STORY: &str = include_str!("../../qa/acceptance/longrange-foreshadow.txt");
const VOLUMES: [&str; 2] = ["启程", "冬闸"];
const QUOTE: &str = "守闸人把一枚缺了右角的方铜牌推给凌汐。背面刻着：「两短一长，走背潮。」";
const DRAFT: &str = "闸墙内传出铃声。凌汐拢袖，把船往岸边挪。";
const LATER: &str = "后来改稿：她另寻借来的渡船。";

fn agent_call(session: &AgentConnection, method: &str, params: Value) -> Value {
    session.call(json!({"method":method,"params":params}))
}
fn agent_result(session: &AgentConnection, method: &str, params: Value) -> Value {
    let reply = agent_call(session, method, params);
    assert!(reply.get("error").is_none(), "{method}: {reply}");
    reply["result"].clone()
}

#[test]
fn two_volume_txt_import_review_adoption_and_sqlite_restore_round_trip_to_exact_txt() {
    let scenes: Vec<_> = STORY.lines().map(|s| s.split_once('\t').unwrap()).collect();
    assert_eq!(scenes.len(), 21);

    let mut input = String::from("\u{feff}");
    let mut bodies = Vec::new();
    for (i, &(title, text)) in scenes.iter().enumerate() {
        if i == 0 || i == 12 {
            input.push_str(&format!(
                "第{}卷 {}\r\n",
                1 + usize::from(i == 12),
                VOLUMES[usize::from(i == 12)]
            ));
        }
        let body = if i == 20 {
            DRAFT.to_owned()
        } else {
            format!("{text}\r\n")
        };
        input.push_str(&format!("第{}章 {title}\r\n{body}", i + 1));
        bodies.push(body);
    }

    let dir = TestDir::new();
    let live_path = dir.0.join("live.sqlite");
    let backup_path = dir.0.join("saved.sqlite");
    let recovery_path = dir.0.join("before-restore.sqlite");
    let export_path = dir.0.join("苇门背潮.txt");
    let mut s = Service::open(&live_path).unwrap();
    let import_path = dir.0.join("incoming.txt");
    write_new_file(&import_path, input.as_bytes()).unwrap();
    let import_bytes = fs::read(import_path).unwrap();
    let p = s.prepare_txt_import(&import_bytes, "苇门背潮.txt").unwrap();
    assert_eq!(p.total_chapters, 21);
    assert!(s.load().unwrap().books.is_empty());
    for (i, body) in bodies.iter().enumerate() {
        let detail = s.get_txt_import_chapter(&p.id, i as u32).unwrap();
        assert_eq!(detail.body.as_bytes(), body.as_bytes());
    }
    let imported = s.commit_txt_import(&p.id).unwrap();
    let book = imported.book_id;
    let outline = s.get_outline(&book).unwrap();
    assert_eq!(
        outline
            .volumes
            .iter()
            .map(|v| v.title.as_str())
            .collect::<Vec<_>>(),
        VOLUMES
    );
    let ids = outline
        .chapters
        .iter()
        .map(|c| c.id.clone())
        .collect::<Vec<_>>();
    assert_eq!(ids.len(), scenes.len());
    for (i, c) in outline.chapters.iter().enumerate() {
        let chapter = s.get_chapter(&c.id).unwrap();
        assert_eq!(
            (
                c.title.as_str(),
                &c.volume_id,
                chapter.revision,
                chapter.body.as_bytes()
            ),
            (
                scenes[i].0,
                &outline.volumes[usize::from(i >= 12)].id,
                0,
                bodies[i].as_bytes()
            )
        );
    }
    let (plant, foil, target) = (&ids[1], &ids[11], &ids[20]);
    let imported_target = s.get_chapter(target).unwrap();
    let human_edit = format!("{DRAFT}\r\n她还没有认出水门。");
    s.apply(Command::SaveChapter {
        id: target.clone(),
        body: human_edit.clone(),
        expected_revision: imported_target.revision,
    })
    .unwrap();
    let edited = s.get_chapter(target).unwrap();
    assert_eq!(
        (edited.revision, edited.body.as_bytes()),
        (1, human_edit.as_bytes())
    );
    let history_before = s.chapter_history(target).unwrap();

    let epoch = s.epoch().unwrap();
    let card = s
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
    let byte = bodies[1].find(QUOTE).unwrap();
    assert_eq!(bodies[1].matches(QUOTE).count(), 1);
    assert!(!bodies[11].contains(QUOTE));
    assert!(bodies[11].contains("圆锡牌") && bodies[11].contains("两短一长"));
    let start = bodies[1][..byte].chars().count() as u32;
    let len = QUOTE.chars().count() as u32;
    let occurrence = s
        .create_foreshadow_occurrence(CreateForeshadowOccurrence {
            book_id: book.clone(),
            material_id: card.id.clone(),
            chapter_id: plant.clone(),
            chapter_revision: 0,
            start,
            end: start + len,
            quote: QUOTE.into(),
            role: ForeshadowRole::Plant,
            note: "第2场景由作者登记为铺设".into(),
            epoch: epoch.clone(),
            request_key: "roundtrip-plant".into(),
        })
        .unwrap()
        .occurrence;
    let policy = s
        .configure_writing_policy_with_context(
            &book,
            target,
            None,
            "active",
            "让孤舟绕开主航道，与西仓货铃区分",
            vec![card.id.clone()],
            vec![foil.clone()],
            &epoch,
        )
        .unwrap();
    let pack = s
        .prepare_writing_pack(&book, target, policy.revision, edited.revision, &epoch)
        .unwrap();
    let frozen = s.get_writing_pack(&book, &pack.id).unwrap();
    assert_eq!(frozen.foreshadow_occurrences, [occurrence.clone()]);
    assert_eq!(frozen.foreshadow_occurrences[0].source.chapter_id, *plant);
    assert_eq!(
        frozen.foreshadow_occurrences[0].source.volume_id,
        outline.volumes[0].id
    );
    assert_ne!(frozen.chapter.volume_id, occurrence.source.volume_id);
    assert_eq!(frozen.context_chapters.len(), 1);
    assert_eq!(frozen.context_chapters[0].chapter.id, *foil);
    assert_eq!(frozen.chapter, edited);

    let shared = Arc::new(Mutex::new(s));
    let broker = Arc::new(AgentBroker::new(shared.clone()));
    let session = broker.connect().unwrap();
    assert_eq!(
        agent_result(
            &session,
            "request_access",
            json!({"client_name":"roundtrip test"})
        )["status"],
        "pending"
    );
    broker.grant_session(session.id(), &book).unwrap();
    let entry = format!("material:{}", card.id);
    let field = format!("occurrence:{}", occurrence.id);
    let read = |entry: &str, field: &str| {
        agent_result(
            &session,
            "read_writing_pack_entry",
            json!({"pack_id":pack.id,"entry_id":entry,"field_key":field,"cursor":0}),
        )
    };
    assert_eq!(
        read(&entry, &field)["text"].as_str().unwrap().as_bytes(),
        QUOTE.as_bytes()
    );
    assert_eq!(
        read(&format!("context:{foil}"), "body")["text"]
            .as_str()
            .unwrap()
            .as_bytes(),
        bodies[11].as_bytes()
    );
    let candidate = scenes[20].1;
    assert!(candidate.contains("缺了右角的方铜牌") && !candidate.contains("圆锡牌"));
    let proposal = agent_result(
        &session,
        "propose_constrained_edit",
        json!({
            "pack_id":pack.id,"new_body":candidate,"idempotency_key":"roundtrip-candidate","conflicts":[],
            "citations":[
                {"entry_id":entry,"field_key":field,"start":0,"end":len,
                 "explanation":"第2场景明确把缺角方铜牌交给凌汐并刻有背潮暗号"},
                {"entry_id":format!("context:{foil}"),"field_key":"body","start":0,
                 "end":scenes[11].1.chars().count(),"explanation":"第12场景同节奏却是西仓收回的圆锡牌"}
            ]
        }),
    );
    let proposal_id = proposal["id"].as_str().unwrap();
    let accept = |s: &mut Service, r| {
        s.accept_reviewed_constrained_proposal(
            &book,
            proposal_id,
            policy.revision,
            edited.revision,
            &epoch,
            r,
            false,
        )
    };
    let (accepted, saved, saved_history) = {
        let mut s = shared.lock().unwrap();
        let pending = s.get_constrained_proposal(&book, proposal_id).unwrap();
        assert_eq!(pending.decision, "pending");
        assert!(pending.author_review.is_none());
        let err = accept(&mut s, 0).unwrap_err();
        assert_eq!(err.message, "请先保存逐项作者审阅");
        assert_eq!(s.get_chapter(target).unwrap(), edited);
        assert_eq!(s.chapter_history(target).unwrap(), history_before);
        assert_eq!(
            s.get_constrained_proposal(&book, proposal_id).unwrap(),
            pending
        );
        let review = s
            .save_constrained_review(
                &book,
                proposal_id,
                None,
                &epoch,
                vec![AuthorReviewItem {
                    material_id: card.id.clone(),
                    verdict: AuthorReviewVerdict::Conforms,
                    note: "脚本模拟作者：第2场景铺设；第12场景仅同节奏，不能互换".into(),
                }],
            )
            .unwrap();
        assert_eq!(s.get_chapter(target).unwrap(), edited);
        assert_eq!(s.chapter_history(target).unwrap(), history_before);
        let accepted = accept(&mut s, review.revision).unwrap();
        let saved = s.get_chapter(target).unwrap();
        assert_eq!(
            (accepted.decision.as_str(), accepted.accepted_revision),
            ("accepted", Some(2))
        );
        assert_eq!(
            (saved.revision, saved.body.as_bytes()),
            (2, candidate.as_bytes())
        );
        let saved_history = s.chapter_history(target).unwrap();
        assert_eq!(saved_history.len(), history_before.len() + 1);
        assert!(history_before.iter().all(|v| saved_history.contains(v)));
        assert!(saved_history.iter().any(|v| v.revision == 2
            && v.body == candidate
            && v.source == format!("strict:{proposal_id}")));
        (accepted, saved, saved_history)
    };
    let saved_library = shared.lock().unwrap().load().unwrap();
    let (later, later_history) = {
        let mut s = shared.lock().unwrap();
        s.export_sqlite_backup(&backup_path, |_| Ok(())).unwrap();
        assert!(fs::metadata(&backup_path).unwrap().len() > 0);
        s.apply(Command::SaveChapter {
            id: target.clone(),
            body: LATER.into(),
            expected_revision: saved.revision,
        })
        .unwrap();
        let later = s.get_chapter(target).unwrap();
        let later_history = s.chapter_history(target).unwrap();
        assert_eq!((later.revision, later.body.as_str()), (3, LATER));
        assert_eq!(later_history.len(), saved_history.len() + 1);
        (later, later_history)
    };
    assert_eq!(
        agent_result(&session, "get_chapter", json!({"chapter_id":target}))["body"],
        LATER
    );
    {
        let mut s = shared.lock().unwrap();
        let prepared = prepare_restore(&backup_path, |_| Ok(())).unwrap();
        let guard = s.restore_guard().unwrap();
        let denied = s
            .commit_prepared_restore(prepared, &guard, &recovery_path, |_| Ok(()))
            .unwrap_err();
        assert_eq!(
            denied,
            "writing_constraint: 恢复会替换当前写作约束，请明确确认后重试"
        );
        assert!(!recovery_path.exists());
        assert_eq!(s.get_chapter(target).unwrap(), later);
        assert_eq!(s.chapter_history(target).unwrap(), later_history);
        assert_eq!(s.restore_guard().unwrap(), guard);
        let prepared = prepare_restore(&backup_path, |_| Ok(())).unwrap();
        let restored = s
            .commit_prepared_restore_with_writing_ack(
                prepared,
                &guard,
                &recovery_path,
                Arc::new(AtomicBool::new(false)),
                true,
                |_| Ok(()),
            )
            .unwrap();
        assert_eq!(restored.recovery_path, recovery_path);
        assert_ne!(s.epoch().unwrap(), epoch);
    }
    // Keep the same process connection across restore. It was valid just before restore.
    let denied_write = agent_call(
        &session,
        "propose_constrained_edit",
        json!({"pack_id":pack.id,"new_body":LATER,"idempotency_key":"roundtrip-expired-write","conflicts":[],
          "citations":[{"entry_id":entry,"field_key":field,"start":0,"end":len,"explanation":"旧授权不能重用"}]}),
    );
    assert_eq!(
        denied_write["error"]["code"], "invalid_session",
        "{denied_write}"
    );
    assert!(denied_write.get("result").is_none());
    assert_eq!(shared.lock().unwrap().load().unwrap(), saved_library);
    assert_eq!(
        shared.lock().unwrap().chapter_history(target).unwrap(),
        saved_history
    );
    {
        let protected = Service::open(&recovery_path).unwrap();
        assert_eq!(protected.epoch().unwrap(), epoch);
        assert_eq!(protected.get_chapter(target).unwrap(), later);
        assert_eq!(protected.chapter_history(target).unwrap(), later_history);
    }
    drop((session, broker, shared));

    let mut s = Service::open(&live_path).unwrap();
    assert_ne!(s.epoch().unwrap(), epoch);
    assert_eq!(s.load().unwrap(), saved_library);
    assert_eq!(s.get_chapter(target).unwrap(), saved);
    assert_eq!(s.chapter_history(target).unwrap(), saved_history);
    let archived = s.get_writing_state(&book, target).unwrap();
    assert_eq!((archived.packs.len(), archived.proposals.len()), (1, 1));
    assert!(archived.packs.iter().any(|p| p.id == pack.id && p.archived));
    assert!(archived
        .proposals
        .iter()
        .any(|p| p.id == accepted.id && p.archived && p.decision == "accepted"));
    let archived_policy = archived.policy.as_ref().unwrap();
    assert_eq!(
        (archived_policy.mode.as_str(), archived_policy.revision),
        ("paused", policy.revision + 1)
    );
    let old_proposal = s.get_constrained_proposal(&book, proposal_id).unwrap();
    assert_eq!(old_proposal.author_review, accepted.author_review);
    assert_eq!(old_proposal.accepted_revision, accepted.accepted_revision);
    assert!(old_proposal.live_epoch.is_none());
    assert_eq!(s.get_writing_pack(&book, &pack.id).unwrap(), frozen);
    // Trusted write with the old policy/epoch also fails closed in the paused library.
    let stale = s
        .prepare_writing_pack(&book, target, policy.revision, saved.revision, &epoch)
        .unwrap_err();
    assert_eq!(
        (stale.code.as_str(), stale.message.as_str()),
        ("writing_constraint", "约束配置失效或暂停")
    );
    assert_eq!(
        json!(s.get_writing_state(&book, target).unwrap()),
        json!(archived)
    );
    assert_eq!(s.load().unwrap(), saved_library);
    assert_eq!(s.chapter_history(target).unwrap(), saved_history);

    // Independent text fixture oracle: no production renderer, SQL or preview-derived expected bytes.
    let mut expected = String::from("\u{feff}");
    for (i, &(title, _)) in scenes.iter().enumerate() {
        if i == 0 {
            expected.push_str("第1卷 启程\r\n\r\n");
        } else if i == 12 {
            expected.push_str("第2卷 冬闸\r\n\r\n");
        }
        expected.push_str(&format!("第{}章 {title}\r\n\r\n", i + 1));
        expected.push_str(if i == 20 { candidate } else { &bodies[i] });
        expected.push_str("\r\n\r\n");
    }
    let export = s
        .prepare_txt_export(
            &book,
            ExportOptions {
                scope: "book".into(),
                target_id: None,
                include_metadata: false,
                include_headings: true,
                newline: "crlf".into(),
                bom: true,
            },
        )
        .unwrap();
    assert_eq!(
        (export.volumes, export.chapters, export.bytes),
        (2, 21, expected.len())
    );
    assert!(!export.truncated);
    assert_eq!(export.text, expected.strip_prefix('\u{feff}').unwrap());
    let bytes = s.take_txt_export(&export.id).unwrap();
    write_new_file(&export_path, &bytes).unwrap();
    let exported = fs::read(&export_path).unwrap();
    assert_eq!(exported, expected.as_bytes());
    assert_ne!(exported, import_bytes);
    let exported_text = std::str::from_utf8(&exported).unwrap();
    assert!(!exported_text.contains(LATER) && !exported_text.contains(DRAFT));
    eprintln!(
        "fixture: {} input bytes, {} SQLite backup bytes, {} export bytes",
        input.len(),
        fs::metadata(&backup_path).unwrap().len(),
        expected.len()
    );
}
