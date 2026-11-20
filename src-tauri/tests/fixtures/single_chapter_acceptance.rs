//! Original acceptance story within the harness-free stdio_process artifact.
//! Only the trusted parent test code reviews and decides. The child receives
//! real MCP over OS pipes and uses the unmodified production broker.
#[path = "single_chapter_provenance.rs"]
mod provenance;
#[path = "single_chapter_rework.rs"]
mod rework;

use super::Pipe;
use novel_core::{
    agent::AgentBroker, files::write_new_file, AuthorReviewItem, AuthorReviewVerdict, BackupJob,
    BackupJobManager, Chapter, ChapterVersion, Command, ConstrainedEditInput, ConstrainedProposal,
    CreateForeshadowOccurrence, CreateMaterialInput, ExportOptions, ForeshadowRole, Service,
    WritingCitation, WritingPack, WritingPolicy,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command as Process,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

#[derive(Deserialize)]
struct Story {
    schema_version: u8,
    work: Work,
    volume: Title,
    prior: Prior,
    target: Target,
    goal: String,
    materials: Vec<Card>,
    occurrence: Occurrence,
    citations: Vec<Citation>,
    accepted: Candidate,
    rejected: Candidate,
    export: Export,
    rework: Rework,
    provenance: Provenance,
}
#[derive(Deserialize)]
struct Work {
    title: String,
    author: String,
}
#[derive(Deserialize)]
struct Title {
    title: String,
}
#[derive(Deserialize)]
struct Prior {
    title: String,
    body: String,
}
#[derive(Deserialize)]
struct Target {
    title: String,
    original_body: String,
    dirty_body: String,
}
#[derive(Deserialize)]
struct Card {
    key: String,
    kind: String,
    title: String,
    fields: BTreeMap<String, String>,
    chapter: Option<String>,
    source: Option<String>,
    tracking_status: Option<String>,
}
#[derive(Deserialize)]
struct Occurrence {
    material: String,
    start: u32,
    end: u32,
    quote: String,
    role: ForeshadowRole,
    note: String,
}
#[derive(Deserialize)]
struct Citation {
    entry: String,
    field_key: String,
    start: u32,
    end: u32,
    quote: String,
    explanation: String,
}
#[derive(Deserialize)]
struct Candidate {
    body: String,
    request_key: String,
    reviews: Vec<Review>,
}
#[derive(Deserialize)]
struct Review {
    material: String,
    verdict: AuthorReviewVerdict,
    note: String,
}
#[derive(Deserialize)]
struct Rework {
    pending_body: String,
    fresh_body: String,
}
#[derive(Deserialize)]
struct Provenance {
    revised_rule: String,
    prior_append: String,
    later_volume: String,
    ordinary_card_title: String,
}
#[derive(Deserialize)]
struct Export {
    visible_text: String,
    bom: bool,
    newline: String,
}
struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("mojuan-chapter-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn receipt(pipe: &mut Pipe, method: &str, input: Value) -> Value {
    let response = pipe.call(method, input);
    assert!(response.get("error").is_none(), "{method}: {response}");
    response["result"].clone()
}

pub(super) fn single_chapter_acceptance() {
    let f: Story =
        serde_json::from_str(include_str!("../../../qa/acceptance/single-chapter.json")).unwrap();
    assert_eq!(f.schema_version, 1);
    assert_eq!((f.materials.len(), f.citations.len()), (4, 5));
    let dir = TempDir::new();
    let path = dir.0.join("writing.sqlite");
    let mut store = Service::open(&path).unwrap();
    let library = store
        .apply(Command::CreateBook {
            title: f.work.title.clone(),
            author: f.work.author.clone(),
        })
        .unwrap();
    let book = library.books[0].id.clone();
    let volume = library.volumes[0].id.clone();
    let prior = library.chapters[0].id.clone();
    store
        .apply(Command::RenameVolume {
            id: volume.clone(),
            title: f.volume.title.clone(),
        })
        .unwrap();
    store
        .apply(Command::RenameChapter {
            id: prior.clone(),
            title: f.prior.title.clone(),
        })
        .unwrap();
    let target = store
        .apply(Command::CreateChapter {
            volume_id: volume,
            title: f.target.title.clone(),
        })
        .unwrap()
        .chapters
        .into_iter()
        .find(|c| c.title == f.target.title)
        .unwrap()
        .id;
    for (id, body) in [(&prior, &f.prior.body), (&target, &f.target.original_body)] {
        store
            .apply(Command::SaveChapter {
                id: id.clone(),
                body: body.clone(),
                expected_revision: 0,
            })
            .unwrap();
    }
    let cards: BTreeMap<_, _> = f
        .materials
        .iter()
        .map(|c| {
            let card = store
                .create_material(CreateMaterialInput {
                    book_id: book.clone(),
                    kind: c.kind.clone(),
                    title: c.title.clone(),
                    fields: c.fields.clone(),
                    volume_id: None,
                    chapter_id: c.chapter.as_ref().map(|slot| {
                        assert_eq!(slot, "target");
                        target.clone()
                    }),
                    source_chapter_id: c.source.as_ref().map(|slot| {
                        assert_eq!(slot, "prior");
                        prior.clone()
                    }),
                    source_revision: c.source.as_ref().map(|_| 1),
                    story_order: 0,
                    story_time: None,
                    tracking_status: c.tracking_status.clone(),
                    payoff_chapter_id: None,
                    payoff_revision: None,
                })
                .unwrap();
            (c.key.as_str(), card)
        })
        .collect();
    let epoch = store.epoch().unwrap();
    let occurrence = store
        .create_foreshadow_occurrence(CreateForeshadowOccurrence {
            book_id: book.clone(),
            material_id: cards[f.occurrence.material.as_str()].id.clone(),
            chapter_id: prior.clone(),
            chapter_revision: 1,
            start: f.occurrence.start,
            end: f.occurrence.end,
            quote: f.occurrence.quote.clone(),
            role: f.occurrence.role,
            note: f.occurrence.note.clone(),
            epoch: epoch.clone(),
            request_key: "original-fixture-occurrence".into(),
        })
        .unwrap()
        .occurrence;
    let policy = store
        .configure_writing_policy_with_context(
            &book,
            &target,
            None,
            "active",
            &f.goal,
            f.materials
                .iter()
                .map(|c| cards[c.key.as_str()].id.clone())
                .collect(),
            vec![prior.clone()],
            &epoch,
        )
        .unwrap();
    assert_eq!(policy.revision, 0);
    let pack = store
        .prepare_writing_pack(&book, &target, 0, 1, &epoch)
        .unwrap()
        .id;

    // The foreign book is a blank authorization sentinel, not part of the story.
    let foreign_state = store
        .apply(Command::CreateBook {
            title: "隔离权限哨兵".into(),
            author: String::new(),
        })
        .unwrap();
    let foreign = foreign_state
        .chapters
        .into_iter()
        .find(|c| c.id != prior && c.id != target)
        .unwrap()
        .id;
    drop(store);
    let store = Service::open(&path).unwrap();
    assert_eq!(store.epoch().unwrap(), epoch);
    let original = store.get_chapter(&target).unwrap();
    let prior_saved = store.get_chapter(&prior).unwrap();
    assert_eq!(
        (original.revision, original.body.as_bytes()),
        (1, f.target.original_body.as_bytes())
    );
    assert_eq!(
        (prior_saved.revision, prior_saved.body.as_bytes()),
        (1, f.prior.body.as_bytes())
    );
    let original_history = store.chapter_history(&target).unwrap();
    assert_eq!(
        original_history
            .iter()
            .map(|v| (v.revision, v.body.as_str(), v.source.as_str()))
            .collect::<Vec<_>>(),
        [
            (0, "", "baseline"),
            (1, f.target.original_body.as_str(), "gui")
        ],
    );
    let original_unchanged = |s: &Service| {
        assert_eq!(s.get_chapter(&target).unwrap(), original);
        assert_eq!(s.chapter_history(&target).unwrap(), original_history);
    };

    // Service enforces one writer process. Release the trusted author handle
    // while the child owns the same isolated file.
    drop(store);
    let mut process = Process::new(std::env::current_exe().unwrap());
    process
        .env("MOJUAN_STDIO_TEST_FIXTURE", "yes")
        .env("FIXTURE_MCP", "yes")
        .env("FIXTURE_DB", &path)
        .env("FIXTURE_BOOK", &book)
        .env("FIXTURE_CHAPTER", &target)
        .env("FIXTURE_REPORT", dir.0.join("fixture-child.json"))
        .env("FIXTURE_GRANT", "yes")
        .env("FIXTURE_STRICT", "no")
        .env("FIXTURE_HUMAN_AFTER_TWO", "no")
        .env("FIXTURE_REVIEW_MATERIAL", "");
    let mut pipe = Pipe::spawn(process, true);
    pipe.initialize();
    let books = receipt(&mut pipe, "list_books", json!({}));
    assert_eq!(books.as_array().unwrap().len(), 1);
    assert_eq!(books[0]["id"], book);
    let foreign_response = pipe.call("get_writing_policy", json!({"chapter_id": foreign}));
    assert_eq!(foreign_response["error"]["code"], "not_found");
    assert!(foreign_response.get("result").is_none());
    let state = receipt(
        &mut pipe,
        "get_writing_policy",
        json!({"chapter_id": target}),
    );
    assert_eq!(state["epoch"], epoch);
    assert_eq!(state["policy"]["context_chapter_ids"], json!([prior]));
    assert_eq!(state["packs"][0]["id"], pack);
    let manifest = receipt(
        &mut pipe,
        "get_writing_pack_manifest",
        json!({"pack_id": pack}),
    );
    assert_eq!(manifest["chapter_revision"], 1);
    assert_eq!(manifest["policy_revision"], 0);
    assert_eq!(manifest["goal"], f.goal);
    let citations: Vec<_> = f
        .citations
        .iter()
        .map(|c| {
            let entry_id = if c.entry == "prior" {
                format!("context:{prior}")
            } else {
                format!("material:{}", cards[c.entry.as_str()].id)
            };
            let field_key = if c.field_key == "occurrence" {
                format!("occurrence:{}", occurrence.id)
            } else {
                c.field_key.clone()
            };
            let entry = manifest["entries"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["id"] == entry_id)
                .unwrap();
            assert!(entry["fields"]
                .as_array()
                .unwrap()
                .contains(&json!(field_key)));
            let data = receipt(
                &mut pipe,
                "read_writing_pack_entry",
                json!({
                    "pack_id": pack, "entry_id": entry_id, "field_key": field_key, "cursor": 0
                }),
            );
            let text = data["text"].as_str().unwrap();
            assert!(data["next_cursor"].is_null());
            assert_eq!(data["total_bytes"], text.len());
            if c.entry == "prior" {
                assert_eq!(entry["revision"], 1);
                assert_eq!(text.as_bytes(), f.prior.body.as_bytes());
            }
            let quote: String = text
                .chars()
                .skip(c.start as usize)
                .take((c.end - c.start) as usize)
                .collect();
            assert_eq!(
                quote.as_bytes(),
                c.quote.as_bytes(),
                "{entry_id}/{field_key}"
            );
            WritingCitation {
                entry_id,
                field_key,
                start: c.start,
                end: c.end,
                explanation: c.explanation.clone(),
            }
        })
        .collect();
    let candidate = |c: &Candidate| ConstrainedEditInput {
        pack_id: pack.clone(),
        new_body: c.body.clone(),
        citations: citations.clone(),
        conflicts: vec![],
        idempotency_key: c.request_key.clone(),
    };
    let good = candidate(&f.accepted);
    let bad = candidate(&f.rejected);
    let submit = |pipe: &mut Pipe, i: &ConstrainedEditInput| {
        receipt(pipe, "propose_constrained_edit", json!(i))
    };
    let mut malformed = good.clone();
    malformed.idempotency_key = "outside-frozen-quote".into();
    malformed.citations[0].end = 999;
    assert_eq!(
        pipe.call("propose_constrained_edit", json!(malformed))["error"]["code"],
        "writing_constraint"
    );
    let good_wire = submit(&mut pipe, &good);
    let bad_wire = submit(&mut pipe, &bad);
    let good_id = good_wire["id"].as_str().unwrap();
    let bad_id = bad_wire["id"].as_str().unwrap();
    assert_ne!(good_id, bad_id);
    assert_eq!(submit(&mut pipe, &good), good_wire);
    let mut altered = good.clone();
    altered.new_body = f.target.dirty_body.clone();
    assert_eq!(
        pipe.call("propose_constrained_edit", json!(altered))["error"]["message"],
        "同一幂等键不能用于不同载荷"
    );
    for (wire, input) in [(&good_wire, &good), (&bad_wire, &bad)] {
        assert_eq!(wire["base_revision"], 1);
        assert_eq!(wire["decision"], "pending");
        assert_eq!(wire["new_body_bytes"], input.new_body.len());
        assert_eq!(wire["citation_count"], f.citations.len());
        assert_eq!(wire["conflict_count"], 0);
        assert_eq!(wire["live_epoch"], epoch);
        assert!(wire["accepted_revision"].is_null());
        assert!(wire.get("input").is_none());
        assert!(wire.get("author_review").is_none());
        assert_eq!(
            receipt(
                &mut pipe,
                "get_constrained_proposal",
                json!({"proposal_id": wire["id"]})
            ),
            *wire,
        );
    }
    for method in [
        "save_constrained_review",
        "accept_reviewed_constrained_proposal",
        "decide_constrained_proposal",
    ] {
        let denied = pipe.call(
            method,
            json!({"proposal_id":good_id,"approve":true,"expected_review_revision":1}),
        );
        assert_eq!(
            denied["error"]["code"], -32602,
            "agent tool exposed: {method}"
        );
        assert!(denied.get("result").is_none());
    }
    pipe.finish();
    drop(pipe);
    let mut store = Service::open(&path).unwrap();
    for (wire, input) in [(&good_wire, &good), (&bad_wire, &bad)] {
        let saved = store
            .get_constrained_proposal(&book, wire["id"].as_str().unwrap())
            .unwrap();
        assert_eq!(&saved.input, input);
        assert_eq!(saved.chapter_id, target);
        assert_eq!(saved.book_id, book);
        assert!(saved.author_review.is_none());
    }
    assert_eq!(
        store
            .get_writing_state(&book, &target)
            .unwrap()
            .proposals
            .len(),
        2
    );
    original_unchanged(&store);

    // The author compares the exact frozen baseline against both candidate drafts,
    // then enters the fixture's independently scripted per-card verdicts.
    let frozen = store.get_writing_pack(&book, &pack).unwrap();
    assert_eq!(frozen.chapter, original);
    assert_eq!(frozen.context_chapters[0].chapter, prior_saved);
    assert_eq!(frozen.foreshadow_occurrences, [occurrence]);
    assert!(store
        .get_constrained_proposal(&book, good_id)
        .unwrap()
        .author_review
        .is_none());
    assert!(store
        .get_constrained_proposal(&book, bad_id)
        .unwrap()
        .author_review
        .is_none());
    original_unchanged(&store);

    let items = |c: &Candidate| {
        c.reviews
            .iter()
            .map(|r| AuthorReviewItem {
                material_id: cards[r.material.as_str()].id.clone(),
                verdict: r.verdict,
                note: r.note.clone(),
            })
            .collect::<Vec<_>>()
    };
    let bad_review = store
        .save_constrained_review(&book, bad_id, None, &epoch, items(&f.rejected))
        .unwrap();
    assert_eq!(
        (
            bad_review.revision,
            bad_review.policy_revision,
            bad_review.chapter_revision
        ),
        (1, 0, 1)
    );
    assert_eq!(bad_review.items, items(&f.rejected));
    let blocked = store
        .accept_reviewed_constrained_proposal(&book, bad_id, 0, 1, &epoch, 1, false)
        .unwrap_err();
    assert_eq!(blocked.message, "所有必需资料均须由作者明确判断为符合");
    let still_pending = store.get_constrained_proposal(&book, bad_id).unwrap();
    assert_eq!(still_pending.decision, "pending");
    assert_eq!(still_pending.author_review.as_ref(), Some(&bad_review));
    original_unchanged(&store);
    let rejection = store
        .decide_constrained_proposal(&book, bad_id, false, 0, 1, &epoch, false)
        .unwrap();
    assert_eq!(rejection.decision, "rejected");
    assert_eq!(rejection.accepted_revision, None);
    assert_eq!(rejection.author_review, Some(bad_review));
    original_unchanged(&store);

    let without_review = store
        .accept_reviewed_constrained_proposal(&book, good_id, 0, 1, &epoch, 1, false)
        .unwrap_err();
    assert_eq!(without_review.message, "请先保存逐项作者审阅");
    original_unchanged(&store);

    let good_review = store
        .save_constrained_review(&book, good_id, None, &epoch, items(&f.accepted))
        .unwrap();
    assert_eq!(
        (
            good_review.revision,
            good_review.policy_revision,
            good_review.chapter_revision
        ),
        (1, 0, 1)
    );
    assert_eq!(good_review.items, items(&f.accepted));
    let before_fault = store.get_constrained_proposal(&book, good_id).unwrap();
    assert_eq!(before_fault.decision, "pending");
    assert_eq!(before_fault.decided_at, None);
    original_unchanged(&store);

    // Abort only at the final decision write and only when the real transaction
    // has already written both the revised chapter and its corresponding history.
    let sql = rusqlite::Connection::open(&path).unwrap();
    sql.execute_batch(
        "CREATE TRIGGER fixture_fail_decision BEFORE UPDATE ON writing_proposals
         WHEN NEW.decision='accepted'
           AND EXISTS (SELECT 1 FROM chapters WHERE id=NEW.chapter_id AND revision=2)
           AND EXISTS (SELECT 1 FROM chapter_versions WHERE chapter_id=NEW.chapter_id AND revision=2)
         BEGIN SELECT RAISE(ABORT, 'isolated acceptance fixture'); END;"
    ).unwrap();
    let failure = store
        .accept_reviewed_constrained_proposal(&book, good_id, 0, 1, &epoch, 1, false)
        .unwrap_err();
    assert_eq!(failure.message, "写作约束数据操作失败，未确认写入");
    original_unchanged(&store);
    assert_eq!(
        store.get_constrained_proposal(&book, good_id).unwrap(),
        before_fault
    );
    assert_eq!(
        store.get_constrained_proposal(&book, bad_id).unwrap(),
        rejection
    );
    sql.execute_batch("DROP TRIGGER fixture_fail_decision")
        .unwrap();
    drop(sql);

    // The test author intentionally confirms a new attempt after reading the
    // pending state and original history; this is not an automatic resend.
    let acceptance = store
        .accept_reviewed_constrained_proposal(&book, good_id, 0, 1, &epoch, 1, false)
        .unwrap();
    assert_eq!(acceptance.decision, "accepted");
    assert_eq!(acceptance.accepted_revision, Some(2));
    assert_eq!(acceptance.author_review, Some(good_review));
    let verify_final = |s: &Service| -> Vec<ChapterVersion> {
        let current = s.get_chapter(&target).unwrap();
        assert_eq!(
            (current.revision, current.body.as_bytes()),
            (2, f.accepted.body.as_bytes())
        );
        assert_eq!(s.get_chapter(&prior).unwrap(), prior_saved);
        assert_eq!(
            s.get_constrained_proposal(&book, good_id).unwrap(),
            acceptance
        );
        assert_eq!(
            s.get_constrained_proposal(&book, bad_id).unwrap(),
            rejection
        );
        let h = s.chapter_history(&target).unwrap();
        assert_eq!(h.len(), 3);
        assert_eq!(h[..2], original_history);
        assert_eq!(
            (h[2].revision, h[2].body.as_bytes()),
            (2, f.accepted.body.as_bytes())
        );
        assert_eq!(h[2].source, format!("strict:{good_id}"));
        assert_eq!(
            s.get_writing_state(&book, &target).unwrap().proposals.len(),
            2
        );
        h
    };
    let final_history = verify_final(&store);
    drop(store);
    let mut store = Service::open(&path).unwrap();
    assert_eq!(store.epoch().unwrap(), epoch);
    assert_eq!(verify_final(&store), final_history);
    let replay = store
        .accept_reviewed_constrained_proposal(&book, good_id, 0, 1, &epoch, 1, false)
        .unwrap_err();
    assert_eq!(replay.message, "提案已处理、归档或书库已恢复");
    assert_eq!(verify_final(&store), final_history);

    write_story_txt(&mut store, &book, &target, &f, &dir.0.join("chapter.txt"));
    eprintln!("stdio_process::single_chapter_acceptance: real MCP pipe, SQLite rollback/reopen and TXT file passed");
    let accepted_chapter = store.get_chapter(&target).unwrap();
    let accepted_audit = store.audit_log().unwrap();
    let accepted_story = AcceptedChapter {
        chapter: &accepted_chapter,
        prior: &prior_saved,
        history: &final_history,
        policy: &policy,
        pack: &frozen,
        proposals: [&acceptance, &rejection],
    };
    let exported_v2 = accepted_story.backup_and_recover(store, &path, &dir.0, &foreign, &epoch, &f);
    eprintln!("stdio_process::single_chapter_acceptance: real backup jobs, restored v2/TXT and recoverable pre-restore v3 passed");

    // Continue the pre-restore v2 lineage on an isolated copy of the production
    // snapshot, not the already-restored (new epoch) live library or its backup.
    let rework_db = dir.0.join("chapter-rework.sqlite");
    std::fs::copy(&exported_v2, &rework_db).unwrap();
    accepted_story.rework_history(&rework_db, &dir.0, &epoch, &f, &accepted_audit);
    accepted_story.rebuild_provenance(&exported_v2, &dir.0, &epoch, &f);
}

fn write_story_txt(store: &mut Service, book: &str, target: &str, f: &Story, output: &Path) {
    assert_eq!(
        f.export.visible_text,
        format!("{}\r\n\r\n", f.accepted.body)
    );
    write_chapter_txt(store, book, target, f, &f.export.visible_text, output, 157);
}

fn write_chapter_txt(
    store: &mut Service,
    book: &str,
    target: &str,
    f: &Story,
    visible_text: &str,
    output: &Path,
    expected_bytes: usize,
) {
    let preview = store
        .prepare_txt_export(
            book,
            ExportOptions {
                scope: "chapter".into(),
                target_id: Some(target.into()),
                include_metadata: false,
                include_headings: false,
                newline: f.export.newline.clone(),
                bom: f.export.bom,
            },
        )
        .unwrap();
    assert_eq!(
        (preview.volumes, preview.chapters, preview.truncated),
        (1, 1, false)
    );
    assert!(preview.bom);
    assert_eq!(preview.newline, "crlf");
    assert_eq!(preview.text.as_bytes(), visible_text.as_bytes());
    let mut expected = b"\xef\xbb\xbf".to_vec();
    expected.extend_from_slice(visible_text.as_bytes());
    assert_eq!(expected.len(), expected_bytes);
    assert_eq!(preview.bytes, expected.len());
    let bytes = store.take_txt_export(&preview.id).unwrap();
    assert!(store.take_txt_export(&preview.id).is_err());
    write_new_file(output, &bytes).unwrap();
    assert_eq!(std::fs::read(output).unwrap(), expected);
    assert!(write_new_file(output, f.target.dirty_body.as_bytes()).is_err());
    assert_eq!(std::fs::read(output).unwrap(), expected);
}

fn wait_for_job(manager: &BackupJobManager, job: &BackupJob, expected: &str) -> BackupJob {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let state = manager.status(&job.id).unwrap();
        if !matches!(state.status.as_str(), "running" | "committing") {
            assert_eq!(
                state.status, expected,
                "{} job finished with {:?} in {}",
                job.kind, state.error_code, state.progress.phase,
            );
            return state;
        }
        assert!(
            Instant::now() < deadline,
            "{} job never reached {expected}: {} / {}",
            job.kind,
            state.status,
            state.progress.phase,
        );
        thread::sleep(Duration::from_millis(2));
    }
}

fn prepare_candidate(manager: &BackupJobManager, source: &Path) -> BackupJob {
    let job = manager.start_prepare_restore(source.to_owned()).unwrap();
    let ready = wait_for_job(manager, &job, "prepared");
    assert!(ready.result.is_none());
    assert!(ready.confirmation_token.is_some());
    let summary = ready.preview.as_ref().unwrap();
    assert_eq!(summary.format, "sqlite");
    assert_eq!(
        (
            summary.writing_policies,
            summary.writing_packs,
            summary.writing_proposals
        ),
        (1, 1, 2)
    );
    ready
}

fn cancel_candidate(manager: &BackupJobManager, ready: &BackupJob) {
    let token = ready.confirmation_token.as_deref().unwrap();
    assert!(manager.cancel(&ready.id).unwrap().cancel_requested);
    let cancelled = wait_for_job(manager, ready, "cancelled");
    assert!(cancelled.result.is_none());
    assert!(cancelled.confirmation_token.is_none());
    assert!(cancelled.error_code.is_none());
    assert!(!manager.has_pending());
    assert!(manager
        .confirm_restore_with_writing_ack(&ready.id, token, true)
        .is_err());
}

struct AcceptedChapter<'a> {
    chapter: &'a Chapter,
    prior: &'a Chapter,
    history: &'a [ChapterVersion],
    policy: &'a WritingPolicy,
    pack: &'a WritingPack,
    proposals: [&'a ConstrainedProposal; 2],
}

impl AcceptedChapter<'_> {
    fn assert_writing_evidence(&self, store: &Service, epoch: &str, archived: bool) {
        let book = &self.policy.book_id;
        let state = store.get_writing_state(book, &self.chapter.id).unwrap();
        assert_eq!(state.epoch, epoch);
        let mut policy = self.policy.clone();
        if archived {
            policy.mode = "paused".into();
            policy.revision += 1;
        }
        assert_eq!(state.policy, Some(policy));
        assert_eq!(state.packs.len(), 1);
        assert_eq!(state.packs[0].id, self.pack.id);
        assert_eq!(state.packs[0].archived, archived);
        assert_eq!(
            store.get_writing_pack(book, &self.pack.id).unwrap(),
            *self.pack
        );
        assert_eq!(state.proposals.len(), 2);
        for before in self.proposals {
            let summary = state.proposals.iter().find(|p| p.id == before.id).unwrap();
            assert_eq!(summary.archived, archived);
            assert_eq!(summary.decision, before.decision);
            let mut expected = before.clone();
            if archived {
                expected.live_epoch = None;
            }
            // Only the live epoch changes. The full receipt includes decision,
            // time and accepted revision, candidate, citations and author review.
            assert_eq!(
                store.get_constrained_proposal(book, &before.id).unwrap(),
                expected
            );
        }
    }

    fn backup_and_recover(
        &self,
        store: Service,
        database: &Path,
        dir: &Path,
        foreign: &str,
        old_epoch: &str,
        story: &Story,
    ) -> PathBuf {
        let book = &self.policy.book_id;
        let target = &self.chapter.id;
        let recovery_dir = dir.join("recovery");
        let backup = dir.join("accepted-v2.sqlite");
        let store = Arc::new(Mutex::new(store));
        let manager = BackupJobManager::new(store.clone(), recovery_dir.clone());

        let export = manager.start_export(backup.clone()).unwrap();
        let exported = wait_for_job(&manager, &export, "done").result.unwrap();
        assert_eq!(exported.output_path.as_deref(), Some(backup.as_path()));
        assert!(exported.recovery_path.is_none());
        let v3_body = format!("{}\r\n她又在页边添了一道铅笔记号。", story.accepted.body);
        let (v3, v3_history) = {
            let mut live = store.lock().unwrap();
            live.apply(Command::SaveChapter {
                id: target.clone(),
                body: v3_body.clone(),
                expected_revision: 2,
            })
            .unwrap();
            let chapter = live.get_chapter(target).unwrap();
            let history = live.chapter_history(target).unwrap();
            assert_eq!(
                (chapter.revision, chapter.body.as_bytes()),
                (3, v3_body.as_bytes())
            );
            assert_eq!(history.len(), 4);
            assert_eq!(&history[..3], self.history);
            assert_eq!(
                (
                    history[3].revision,
                    history[3].body.as_bytes(),
                    history[3].source.as_str()
                ),
                (3, v3_body.as_bytes(), "gui")
            );
            (chapter, history)
        };
        let still_v3 = |live: &Service| {
            assert_eq!(live.get_chapter(target).unwrap(), v3);
            assert_eq!(live.get_chapter(&self.prior.id).unwrap(), *self.prior);
            assert_eq!(live.chapter_history(target).unwrap(), v3_history);
            self.assert_writing_evidence(live, old_epoch, false);
        };

        // This is a separate in-process connection on the same live Service,
        // not the finished MCP child or a production socket.
        let broker = Arc::new(AgentBroker::new(store.clone()));
        let session = broker.connect().unwrap();
        let request_access =
            json!({"method":"request_access","params":{"client_name":"restore fixture"}});
        assert_eq!(
            session.call(request_access.clone())["result"]["status"],
            "pending"
        );
        broker.grant_session(session.id(), book).unwrap();
        let read = json!({"method":"get_chapter","params":{"chapter_id":target}});
        let allowed = session.call(read.clone());
        assert_eq!(allowed["result"]["revision"], 3);
        assert_eq!(allowed["result"]["body"], v3_body);

        let ready = prepare_candidate(&manager, &backup);
        cancel_candidate(&manager, &ready);
        still_v3(&store.lock().unwrap());

        let ready = prepare_candidate(&manager, &backup);
        // Change only the blank authorization sentinel after guard capture.
        // The confirmed worker cannot acquire the live mutex until that write
        // has happened. The story's v3 body/history are not the staleness input.
        let sentinel_original = {
            let mut live = store.lock().unwrap();
            let sentinel_original = live.get_chapter(foreign).unwrap();
            live.apply(Command::RenameChapter {
                id: foreign.into(),
                title: "预览后更名的隔离哨兵".into(),
            })
            .unwrap();
            manager
                .confirm_restore_with_writing_ack(
                    &ready.id,
                    ready.confirmation_token.as_deref().unwrap(),
                    true,
                )
                .unwrap();
            sentinel_original
        };
        let stale = wait_for_job(&manager, &ready, "failed");
        assert_eq!(stale.error_code.as_deref(), Some("restore_conflict"));
        assert!(stale.result.is_none());
        assert!(stale.confirmation_token.is_none());
        let sentinel_v3 = {
            let live = store.lock().unwrap();
            still_v3(&live);
            let sentinel = live.get_chapter(foreign).unwrap();
            assert_eq!(sentinel.title, "预览后更名的隔离哨兵");
            sentinel
        };

        let ready = prepare_candidate(&manager, &backup);
        let queued = manager
            .confirm_restore(&ready.id, ready.confirmation_token.as_deref().unwrap())
            .unwrap();
        assert_eq!(queued.status, "running");
        assert!(queued.result.is_none());
        let no_ack = wait_for_job(&manager, &ready, "failed");
        assert_eq!(no_ack.error_code.as_deref(), Some("operation_failed"));
        assert!(no_ack.result.is_none());
        assert!(no_ack.confirmation_token.is_none());
        still_v3(&store.lock().unwrap());
        assert!(
            !recovery_dir.exists() || std::fs::read_dir(&recovery_dir).unwrap().next().is_none()
        );
        assert_eq!(session.call(read.clone()), allowed);

        let ready = prepare_candidate(&manager, &backup);
        manager
            .confirm_restore_with_writing_ack(
                &ready.id,
                ready.confirmation_token.as_deref().unwrap(),
                true,
            )
            .unwrap();
        let committed = wait_for_job(&manager, &ready, "done");
        assert!(committed.confirmation_token.is_none());
        assert!(committed.error_code.is_none());
        let restored = committed.result.unwrap();
        assert!(restored.output_path.is_none());
        assert_eq!(json!(restored.summary), json!(ready.preview));
        let v2_versions = ready.preview.unwrap().history;
        let recovery = restored.recovery_path.unwrap();
        assert_eq!(recovery.parent(), Some(recovery_dir.as_path()));
        assert!(recovery.is_file());
        let new_epoch = store.lock().unwrap().epoch().unwrap();
        assert_ne!(new_epoch, old_epoch);
        let assert_restored = |live: &Service| {
            assert_eq!(live.get_chapter(target).unwrap(), *self.chapter);
            assert_eq!(live.get_chapter(&self.prior.id).unwrap(), *self.prior);
            assert_eq!(live.chapter_history(target).unwrap(), self.history);
            assert_eq!(live.get_chapter(foreign).unwrap(), sentinel_original);
            self.assert_writing_evidence(live, &new_epoch, true);
        };
        assert_restored(&store.lock().unwrap());
        let expired = session.call(read.clone());
        assert_eq!(expired["error"]["code"], "invalid_session");
        assert!(expired.get("result").is_none());
        let fresh = broker.connect().unwrap();
        assert_eq!(fresh.call(request_access)["result"]["status"], "pending");
        let unapproved = fresh.call(read);
        assert_eq!(unapproved["error"]["code"], "access_pending");
        assert!(unapproved.get("result").is_none());

        manager.shutdown();
        drop(manager);
        drop(fresh);
        drop(session);
        drop(broker);
        drop(store);

        let mut live = Service::open(database).unwrap();
        assert_restored(&live);
        let [accepted, rejected] = self.proposals;
        for epoch in [old_epoch, new_epoch.as_str()] {
            let accept_error = live
                .accept_reviewed_constrained_proposal(book, &accepted.id, 0, 1, epoch, 1, false)
                .unwrap_err();
            let reject_error = live
                .decide_constrained_proposal(book, &rejected.id, false, 0, 1, epoch, false)
                .unwrap_err();
            for error in [accept_error, reject_error] {
                assert_eq!(error.code, "writing_constraint");
                assert_eq!(error.message, "提案已处理、归档或书库已恢复");
            }
        }
        assert_restored(&live);
        write_story_txt(
            &mut live,
            book,
            target,
            story,
            &dir.join("restored-chapter.txt"),
        );

        // Validate the recovery file as published. A read/write Service open
        // can checkpoint WAL on close and otherwise hide a non-standalone file.
        let live = Arc::new(Mutex::new(live));
        let manager = BackupJobManager::new(live.clone(), recovery_dir);
        let ready = prepare_candidate(&manager, &recovery);
        assert_eq!(ready.preview.as_ref().unwrap().history, v2_versions + 1);
        cancel_candidate(&manager, &ready);
        assert_restored(&live.lock().unwrap());
        manager.shutdown();

        let snapshot = Service::open(&recovery).unwrap();
        still_v3(&snapshot);
        assert_eq!(snapshot.get_chapter(foreign).unwrap(), sentinel_v3);
        backup
    }
}
