//! Test-artifact subprocesses use real OS stdin/stdout pipes and isolated SQLite.
//! Production agent_stdio::run -> AgentBroker -> Service execute unchanged.
//! Trusted fixture grants and GUI saves are simulated only inside this test artifact.
//! This does NOT run the production mojuan-agent binary, IPC socket host, or Windows.
use novel_core::{AuthorReviewItem, AuthorReviewVerdict, Command, Service};
const PRIVATE_REVIEW_NOTE: &str = "AUTHOR-ONLY REVIEW NOTE FROM ISOLATED PIPE FIXTURE";
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command as Process, Stdio},
    sync::{mpsc, Arc, Barrier, Mutex},
    thread,
    time::{Duration, Instant},
};
#[path = "fixtures/single_chapter_acceptance.rs"]
mod single_chapter_acceptance;
#[path = "fixtures/manuscript_instruction_boundary.rs"]
mod manuscript_instruction_boundary;

struct Pipe {
    child: Child,
    input: Option<ChildStdin>,
    output: mpsc::Receiver<Result<Value, String>>,
    reader: Option<thread::JoinHandle<()>>,
    mcp: bool,
    next: u64,
}
struct Harness {
    pipe: Pipe,
    dir: PathBuf,
    db: PathBuf,
    book: String,
    chapter: String,
    other: String,
    prior: Option<String>,
    foreign_pack: Option<String>,
    foreshadow_cards: Option<(String, String, String)>,
    material: String,
    other_material: String,
    deleted_material: String,
}
impl Harness {
    fn new(mcp: bool, human_after_two: bool, grant: bool) -> Self {
        Self::new_mode(mcp, human_after_two, grant, false)
    }
    fn new_mode(mcp: bool, human_after_two: bool, grant: bool, strict: bool) -> Self {
        Self::new_modes(mcp, human_after_two, grant, strict, false)
    }
    fn new_modes(
        mcp: bool,
        human_after_two: bool,
        grant: bool,
        strict: bool,
        foreshadow: bool,
    ) -> Self {
        let executable = std::env::current_exe().unwrap();
        let dir = std::env::temp_dir().join(format!("mojuan-stdio-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let db = dir.join("fixture.db");
        let mut store = Service::open(&db).unwrap();
        let state = store
            .apply(Command::CreateBook {
                title: "Allowed fixture".into(),
                author: "test".into(),
            })
            .unwrap();
        let book = state.books[0].id.clone();
        let mut chapter = state.chapters[0].id.clone();
        let state = store
            .apply(Command::CreateBook {
                title: "Other secret fixture".into(),
                author: "private".into(),
            })
            .unwrap();
        let other = state
            .chapters
            .iter()
            .find(|c| c.id != chapter)
            .unwrap()
            .id
            .clone();
        let other_book = state
            .books
            .iter()
            .find(|b| b.id != book)
            .unwrap()
            .id
            .clone();
        let make = |store: &mut Service, book: &str, title: &str| {
            store
                .create_material(novel_core::CreateMaterialInput {
                    book_id: book.into(),
                    kind: "outline".into(),
                    title: title.into(),
                    fields: std::collections::BTreeMap::from([(
                        "content".into(),
                        "PROCESS PRIVATE MATERIAL".into(),
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
                })
                .unwrap()
        };
        store
            .create_material(novel_core::CreateMaterialInput {
                book_id: book.clone(),
                kind: "summary".into(),
                title: "Fixture chapter summary".into(),
                fields: std::collections::BTreeMap::from([(
                    "content".into(),
                    "SUMMARY PIPE TEXT".into(),
                )]),
                volume_id: None,
                chapter_id: None,
                source_chapter_id: Some(chapter.clone()),
                source_revision: Some(0),
                story_order: 0,
                story_time: None,
                tracking_status: None,
                payoff_chapter_id: None,
                payoff_revision: None,
            })
            .unwrap();
        let material = make(&mut store, &book, "Allowed material").id;
        make(&mut store, &book, "Second material");
        let other_material = make(&mut store, &other_book, "Other private material").id;
        let deleted_material = make(&mut store, &book, "Recycled material").id;
        store.trash_material(&deleted_material, 0).unwrap();
        for kind in ["timeline", "foreshadow"] {
            planning_fixture(&mut store, &book, kind, &format!("Allowed {kind}"));
            planning_fixture(&mut store, &other_book, kind, &format!("Foreign {kind}"));
        }
        let foreshadow_cards = foreshadow.then(|| {
            let allowed = store
                .list_materials(&book, false)
                .unwrap()
                .into_iter()
                .find(|m| m.kind == "foreshadow")
                .unwrap()
                .id;
            let foreign = store
                .list_materials(&other_book, false)
                .unwrap()
                .into_iter()
                .find(|m| m.kind == "foreshadow")
                .unwrap()
                .id;
            let volume = store.get_chapter(&chapter).unwrap().volume_id;
            let stub = store
                .apply(Command::CreateChapter {
                    volume_id: volume,
                    title: "Purged clue source".into(),
                })
                .unwrap()
                .chapters
                .into_iter()
                .find(|c| c.title == "Purged clue source")
                .unwrap()
                .id;
            let epoch = store.epoch().unwrap();
            for id in [&chapter, &stub] {
                store
                    .apply(Command::SaveChapter {
                        id: id.clone(),
                        body: "X😀\r\nCLUE body-not-quoted".into(),
                        expected_revision: 0,
                    })
                    .unwrap();
            }
            let mut add = |id: &str, key: &str| {
                store
                    .create_foreshadow_occurrence(novel_core::CreateForeshadowOccurrence {
                        book_id: book.clone(),
                        material_id: allowed.clone(),
                        chapter_id: id.into(),
                        chapter_revision: 1,
                        start: 1,
                        end: 8,
                        quote: "😀\r\nCLUE".into(),
                        role: novel_core::ForeshadowRole::Hint,
                        note: "fixture annotation".into(),
                        epoch: epoch.clone(),
                        request_key: key.into(),
                    })
                    .unwrap()
                    .occurrence
            };
            add(&chapter, "pipe-current");
            add(&stub, "pipe-frozen");
            let trash = add(&chapter, "pipe-trash");
            store
                .set_foreshadow_occurrence_deleted(&book, &allowed, &trash.id, 0, &epoch, true)
                .unwrap();
            store
                .apply(Command::TrashChapter { id: stub.clone() })
                .unwrap();
            store.apply(Command::PurgeChapter { id: stub }).unwrap();
            let submitted = store.propose_foreshadow_occurrence(&book, "separate-connection-fixture", &epoch, novel_core::ForeshadowProposalInput {
                material_id: allowed.clone(), material_revision: 4, chapter_id: chapter.clone(), chapter_revision: 1,
                start: 1, end: 8, quote: "😀\r\nCLUE".into(), role: novel_core::ForeshadowRole::Hint,
                note: "".into(), reason: "foreign owner".into(), idempotency_key: "separate-owner".into(),
            }).unwrap();
            (allowed, foreign, submitted.id)
        });
        let mut prior = None;
        let mut foreign_pack = None;
        if strict {
            let epoch = store.epoch().unwrap();
            let id = chapter.clone();
            store
                .apply(Command::SaveChapter {
                    id: id.clone(),
                    body: format!("{}旧🚣\r\n岸", "x".repeat(32765)),
                    expected_revision: 0,
                })
                .unwrap();
            let volume_id = store.get_chapter(&id).unwrap().volume_id;
            let state = store
                .apply(Command::CreateChapter {
                    volume_id,
                    title: "Fixture target".into(),
                })
                .unwrap();
            chapter = state
                .chapters
                .iter()
                .find(|c| c.title == "Fixture target")
                .unwrap()
                .id
                .clone();
            let policy = store
                .configure_writing_policy_with_context(
                    &book,
                    &chapter,
                    None,
                    "active",
                    "Write consistently",
                    vec![material.clone()],
                    vec![id.clone()],
                    &epoch,
                )
                .unwrap();
            store
                .prepare_writing_pack(&book, &chapter, policy.revision, 0, &epoch)
                .unwrap();
            prior = Some(id);
            store
                .configure_writing_policy(
                    &other_book,
                    &other,
                    None,
                    "active",
                    "Foreign",
                    vec![other_material.clone()],
                    &epoch,
                )
                .unwrap();
            foreign_pack = Some(
                store
                    .prepare_writing_pack(&other_book, &other, 0, 0, &epoch)
                    .unwrap()
                    .id,
            );
        }
        drop(store);
        let mut process = Process::new(executable);
        process
            .env("MOJUAN_STDIO_TEST_FIXTURE", "yes")
            .env("FIXTURE_DB", &db)
            .env("FIXTURE_GRANT", if grant { "yes" } else { "no" })
            .env("FIXTURE_BOOK", &book)
            .env("FIXTURE_CHAPTER", &chapter)
            .env("FIXTURE_REPORT", dir.join("report.json"))
            .env("FIXTURE_MCP", if mcp { "yes" } else { "no" })
            .env("FIXTURE_STRICT", if strict { "yes" } else { "no" })
            .env("FIXTURE_REVIEW_MATERIAL", &material)
            .env(
                "FIXTURE_HUMAN_AFTER_TWO",
                if human_after_two { "yes" } else { "no" },
            );
        Self {
            pipe: Pipe::spawn(process, mcp),
            dir,
            db,
            book,
            chapter,
            other,
            prior,
            foreign_pack,
            foreshadow_cards,
            material,
            other_material,
            deleted_material,
        }
    }
    fn send(&mut self, value: Value) {
        self.pipe.send(value);
    }
    fn receive(&self) -> Value {
        self.pipe.receive()
    }
    fn initialize(&mut self) {
        self.pipe.initialize();
    }
    fn call(&mut self, method: &str, params: Value) -> Value {
        self.pipe.call(method, params)
    }
    fn finish(&mut self) {
        self.pipe.finish();
    }
    fn edit(&mut self, key: &str, body: &str) -> Value {
        self.call("propose_edit",json!({"chapter_id":self.chapter,"base_revision":0,"new_body":body,"reason":"test fixture","idempotency_key":key}))
    }
}
impl Drop for Harness {
    fn drop(&mut self) {
        self.pipe.stop();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
impl Pipe {
    fn spawn(mut process: Process, mcp: bool) -> Self {
        let mut child = process
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = child.stdin.take();
        let stdout = child.stdout.take().unwrap();
        let (tx, output) = mpsc::channel();
        let reader = thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let value = line.map_err(|e| e.to_string()).and_then(|line| {
                    assert!(
                        line.len() + 1 <= novel_core::agent_stdio::MAX_FRAME,
                        "oversized subprocess output frame"
                    );
                    serde_json::from_str(&line).map_err(|e| format!("non-JSON stdout: {e}: {line}"))
                });
                if tx.send(value).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            input,
            output,
            reader: Some(reader),
            mcp,
            next: 1,
        }
    }
    fn send(&mut self, value: Value) {
        writeln!(self.input.as_mut().unwrap(), "{value}").unwrap();
        self.input.as_mut().unwrap().flush().unwrap();
    }
    fn receive(&self) -> Value {
        self.output
            .recv_timeout(Duration::from_secs(10))
            .expect("child response timeout or closed stdout")
            .expect("protocol stdout must contain only JSON")
    }
    fn initialize(&mut self) {
        self.send(json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"fixture","version":"1"}}}));
        assert_eq!(self.receive()["result"]["protocolVersion"], "2025-11-25");
        self.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    }
    fn call(&mut self, method: &str, params: Value) -> Value {
        self.call_with_wire_check(method, params, |_| {})
    }
    // Test-only observer sees the complete parsed response before MCP unwrapping.
    // Existing callers retain exactly the same normalized return value.
    fn call_with_wire_check(
        &mut self,
        method: &str,
        params: Value,
        inspect: impl FnOnce(&Value),
    ) -> Value {
        if self.mcp {
            let id = self.next;
            self.next += 1;
            self.send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":method,"arguments":params}}));
            let reply = self.receive();
            assert_eq!(reply["id"], id);
            inspect(&reply);
            if reply.get("error").is_some() {
                return reply;
            }
            serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
        } else {
            self.send(json!({"method":method,"params":params}));
            let reply = self.receive();
            inspect(&reply);
            reply
        }
    }
    fn finish(&mut self) {
        self.input.take();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(Instant::now() < deadline, "fixture exit timeout");
            thread::sleep(Duration::from_millis(10));
        }
        self.reader.take().unwrap().join().unwrap();
        assert!(
            self.output.try_iter().next().is_none(),
            "unexpected stdout after protocol"
        );
    }
    fn stop(&mut self) {
        self.input.take();
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
impl Drop for Pipe {
    fn drop(&mut self) {
        self.stop();
    }
}
fn json_process_scope_replay_and_forbidden_accept_preserve_database() {
    let mut h = Harness::new(false, false, true);
    let books = h.call("list_books", json!({}));
    assert_eq!(books["result"].as_array().unwrap().len(), 1);
    assert_eq!(books["result"][0]["id"], h.book);
    assert!(!books.to_string().contains("Other secret"));
    let other = h.other.clone();
    assert!(h
        .call("get_chapter", json!({"chapter_id":other}))
        .get("error")
        .is_some());
    let first = h.edit("same", "suggested only");
    assert_eq!(first["result"]["status"], "pending");
    assert_eq!(h.edit("same", "suggested only"), first);
    assert_eq!(
        h.edit("same", "changed")["error"]["code"],
        "idempotency_conflict"
    );
    assert!(h
        .call("approve", json!({"proposal_id":first["result"]["id"]}))
        .get("error")
        .is_some());
    h.finish();
    let store = Service::open(&h.db).unwrap();
    assert_eq!(store.get_chapter(&h.chapter).unwrap().body, "");
    assert_eq!(store.get_chapter(&h.chapter).unwrap().revision, 0);
    assert_eq!(store.list_proposals(&h.book).unwrap().len(), 1);
}
fn two_pending_proposals_survive_competing_human_saves() {
    let mut h = Harness::new(true, true, true);
    h.initialize();
    let a = h.edit("a", "proposal A");
    let b = h.edit("b", "proposal B");
    assert_ne!(a["result"]["id"], b["result"]["id"]);
    assert_eq!(
        h.edit("stale", "proposal C")["error"]["code"],
        "revision_conflict"
    );
    assert!(h
        .call("approve", json!({"proposal_id":a["result"]["id"]}))
        .get("error")
        .is_some());
    let chapter = h.chapter.clone();
    assert_eq!(
        h.call("get_chapter", json!({"chapter_id":chapter}))["result"]["body"],
        "HUMAN FIXTURE SAVE"
    );
    h.finish();
    let report: Value =
        serde_json::from_slice(&std::fs::read(h.dir.join("report.json")).unwrap()).unwrap();
    assert_eq!(
        report["accept_errors"],
        json!(["revision_conflict", "revision_conflict"])
    );
    assert_eq!(report["human_race"], json!(["ok", "revision_conflict"]));
    let store = Service::open(&h.db).unwrap();
    assert_eq!(
        store.get_chapter(&h.chapter).unwrap().body,
        "HUMAN FIXTURE SAVE"
    );
    assert_eq!(store.get_chapter(&h.chapter).unwrap().revision, 1);
    assert_eq!(store.list_proposals(&h.book).unwrap().len(), 2);
    assert!(store
        .list_proposals(&h.book)
        .unwrap()
        .iter()
        .all(|p| p.status == "pending"));
}

// This is a harness=false test artifact, never a shipped production entry point.
fn main() {
    if std::env::var("MOJUAN_STDIO_TEST_FIXTURE").as_deref() == Ok("yes") {
        fixture_child();
        return;
    }
    if std::env::args().any(|arg| arg == "single_chapter_acceptance") {
        single_chapter_acceptance::single_chapter_acceptance();
        return;
    }
    if std::env::args().any(|arg| arg == "manuscript_instruction_boundary") {
        manuscript_instruction_boundary::manuscript_instruction_boundary();
        return;
    }
    foreshadow_process(false);
    foreshadow_process(true);
    strict_writing_process(false);
    strict_writing_process(true);
    materials_process_readonly(false);
    materials_process_readonly(true);
    planning_process_readonly(false);
    planning_process_readonly(true);
    context_process_proposals(false);
    context_process_proposals(true);
    pending_process_cannot_read_or_propose();
    json_process_scope_replay_and_forbidden_accept_preserve_database();
    two_pending_proposals_survive_competing_human_saves();
    single_chapter_acceptance::single_chapter_acceptance();
    manuscript_instruction_boundary::manuscript_instruction_boundary();
    eprintln!(
        "stdio_process: 15 real pipe + SQLite scenarios passed (not socket or Windows validation)"
    );
}
fn fixture_child() {
    let db = std::env::var("FIXTURE_DB").unwrap();
    let book = std::env::var("FIXTURE_BOOK").unwrap();
    let chapter = std::env::var("FIXTURE_CHAPTER").unwrap();
    let report_path = std::env::var("FIXTURE_REPORT").unwrap();
    let mcp = std::env::var("FIXTURE_MCP").as_deref() == Ok("yes");
    let race = std::env::var("FIXTURE_HUMAN_AFTER_TWO").as_deref() == Ok("yes");
    let strict = std::env::var("FIXTURE_STRICT").as_deref() == Ok("yes");
    let review_material = std::env::var("FIXTURE_REVIEW_MATERIAL").unwrap();
    let service = Arc::new(Mutex::new(Service::open(db).unwrap()));
    let broker = Arc::new(novel_core::agent::AgentBroker::new(service.clone()));
    let connection = broker.connect().unwrap();
    assert_eq!(
        connection.call(
            json!({"method":"request_access","params":{"client_name":"isolated stdio fixture"}})
        )["result"]["status"],
        "pending"
    );
    if std::env::var("FIXTURE_GRANT").as_deref() == Ok("yes") {
        broker.grant_session(connection.id(), &book).unwrap();
    }
    let mut count = 0;
    let mut human_race = Vec::new();
    novel_core::agent_stdio::run(
        std::io::stdin().lock(),
        std::io::stdout().lock(),
        mcp,
        |request| {
            let response = connection.call(request.clone());
            if strict
                && request["method"] == "propose_constrained_edit"
                && request["params"]["idempotency_key"] == "strict"
            {
                if let Some(id) = response["result"]["id"].as_str() {
                    // Simulate an independent trusted author saving the first review.
                    // This test artifact exposes no wire method that performs this write.
                    let mut store = service.lock().unwrap();
                    if store
                        .get_constrained_proposal(&book, id)
                        .unwrap()
                        .author_review
                        .is_none()
                    {
                        let e = store.epoch().unwrap();
                        store
                            .save_constrained_review(
                                &book,
                                id,
                                None,
                                &e,
                                vec![AuthorReviewItem {
                                    material_id: review_material.clone(),
                                    verdict: AuthorReviewVerdict::Conforms,
                                    note: PRIVATE_REVIEW_NOTE.into(),
                                }],
                            )
                            .unwrap();
                    }
                }
            }
            count += 1;
            if race && count == 2 {
                // Trusted fixture independently models two GUI saves racing at revision zero.
                // There is no external save/approve tool and no fixture command in the protocol.
                let barrier = Arc::new(Barrier::new(3));
                let mut workers = Vec::new();
                for _ in 0..2 {
                    let service = service.clone();
                    let barrier = barrier.clone();
                    let chapter = chapter.clone();
                    workers.push(thread::spawn(move || {
                        barrier.wait();
                        service
                            .lock()
                            .unwrap()
                            .apply(Command::SaveChapter {
                                id: chapter,
                                body: "HUMAN FIXTURE SAVE".into(),
                                expected_revision: 0,
                            })
                            .map(|_| "ok".to_owned())
                            .unwrap_or_else(|error| {
                                assert!(
                                    error.starts_with("revision_conflict:"),
                                    "unexpected save error: {error}"
                                );
                                "revision_conflict".to_owned()
                            })
                    }));
                }
                barrier.wait();
                human_race = workers.into_iter().map(|w| w.join().unwrap()).collect();
                human_race.sort();
                assert_eq!(human_race, vec!["ok", "revision_conflict"]);
            }
            Ok(response)
        },
    )
    .unwrap();
    let mut errors = Vec::new();
    if race {
        let mut store = service.lock().unwrap();
        let proposals = store.list_proposals(&book).unwrap();
        for p in proposals {
            errors.push(store.decide_proposal(&p.id, true, 1).unwrap_err().code)
        }
    }
    std::fs::write(
        report_path,
        serde_json::to_vec(&json!({"accept_errors":errors,"human_race":human_race})).unwrap(),
    )
    .unwrap();
}

fn pending_process_cannot_read_or_propose() {
    let mut h = Harness::new(false, false, false);
    assert_eq!(
        h.call("session_status", json!({}))["result"]["status"],
        "pending"
    );
    assert_eq!(
        h.call("list_books", json!({}))["error"]["code"],
        "access_pending"
    );
    assert_eq!(
        h.call("list_materials", json!({}))["error"]["code"],
        "access_pending"
    );
    assert_eq!(
        h.call("search_book", json!({"query":"private"}))["error"]["code"],
        "access_pending"
    );
    let material = h.material.clone();
    assert_eq!(h.call("propose_material_edit",json!({"material_id":material,"base_revision":0,"new_title":"Denied","new_fields":{"content":"Denied"},"reason":"Denied","idempotency_key":"Denied"}))["error"]["code"],"access_pending");
    assert_eq!(
        h.call("get_material", json!({"material_id":material}))["error"]["code"],
        "access_pending"
    );
    let chapter = h.chapter.clone();
    assert_eq!(
        h.call("get_chapter", json!({"chapter_id":chapter}))["error"]["code"],
        "access_pending"
    );
    assert_eq!(
        h.edit("denied", "must never be stored")["error"]["code"],
        "access_pending"
    );
    h.finish();
    let store = Service::open(&h.db).unwrap();
    assert!(store.list_proposals(&h.book).unwrap().is_empty());
    assert_eq!(store.get_chapter(&h.chapter).unwrap().body, "");
}

fn materials_process_readonly(mcp: bool) {
    let mut h = Harness::new(mcp, false, true);
    if mcp {
        h.initialize();
    }
    let list = h.call("list_materials", json!({"limit":1,"kind":"outline"}));
    assert_eq!(list["result"]["items"].as_array().unwrap().len(), 1);
    assert!(list["result"]["items"][0].get("fields").is_none());
    assert!(!list.to_string().contains("PROCESS PRIVATE MATERIAL"));
    let cursor = list["result"]["next_cursor"].clone();
    assert!(cursor.is_string());
    let page2 = h.call(
        "list_materials",
        json!({"limit":1,"after_id":cursor,"kind":"outline"}),
    );
    assert_eq!(page2["result"]["items"].as_array().unwrap().len(), 1);
    assert!(page2["result"]["next_cursor"].is_null());
    assert_ne!(
        list["result"]["items"][0]["id"],
        page2["result"]["items"][0]["id"]
    );
    let id = h.material.clone();
    assert_eq!(
        h.call("get_material", json!({"material_id":id}))["result"]["fields"]["content"],
        "PROCESS PRIVATE MATERIAL"
    );
    for id in [h.other_material.clone(), h.deleted_material.clone()] {
        assert_eq!(
            h.call("get_material", json!({"material_id":id}))["error"]["code"],
            "not_found"
        );
    }
    for method in [
        "create_material",
        "save_material",
        "trash_material",
        "restore_material",
    ] {
        assert!(h
            .call(method, json!({"material_id":h.material}))
            .get("error")
            .is_some());
    }
    assert_eq!(
        h.call("list_materials", json!({"limit":101}))["error"]["code"],
        "invalid_input"
    );
    h.finish();
    let store = Service::open(&h.db).unwrap();
    let material = store.get_material(&h.material, false).unwrap();
    assert_eq!(material.revision, 0);
    assert_eq!(
        material.fields.get("content").unwrap(),
        "PROCESS PRIVATE MATERIAL"
    );
    assert_eq!(store.list_materials(&h.book, false).unwrap().len(), 5);
}

fn context_process_proposals(mcp: bool) {
    let mut h = Harness::new(mcp, false, true);
    if mcp {
        h.initialize();
    }
    let found = h.call("search_book", json!({"query":"Allowed material","limit":1}));
    assert!(found.get("error").is_none(), "{found}");
    assert!(found.to_string().contains("Allowed material"));
    let hidden = h.call("search_book", json!({"query":"Other private material"}));
    assert!(hidden.get("error").is_none(), "{hidden}");
    assert!(!hidden.to_string().contains("Other private material"));
    let args = json!({"material_id":h.material,"base_revision":0,"new_title":"Suggested title","new_fields":{"content":"AGENT PROPOSAL ONLY"},"new_source_revision":null,"reason":"fixture","idempotency_key":"material-pipe"});
    let proposed = h.call("propose_material_edit", args.clone());
    assert_eq!(proposed["result"]["status"], "pending", "{proposed}");
    assert_eq!(h.call("propose_material_edit", args), proposed);
    let id = proposed["result"]["id"].clone();
    assert_eq!(
        h.call("get_material_proposal", json!({"proposal_id":id})),
        proposed
    );
    assert!(h
        .call("approve_material_proposal", json!({"proposal_id":id}))
        .get("error")
        .is_some());
    let foreign = h.other_material.clone();
    assert_eq!(h.call("propose_material_edit",json!({"material_id":foreign,"base_revision":0,"new_title":"No","new_fields":{"content":"No"},"reason":"No","idempotency_key":"foreign"}))["error"]["code"],"not_found");
    let summaries = h.call("list_materials", json!({"kind":"summary"}));
    assert_eq!(summaries["result"]["items"].as_array().unwrap().len(), 1);
    let summary_id = summaries["result"]["items"][0]["id"].clone();
    assert_eq!(
        h.call("get_material", json!({"material_id":summary_id}))["result"]["fields"]["content"],
        "SUMMARY PIPE TEXT"
    );
    h.finish();
    let store = Service::open(&h.db).unwrap();
    let unchanged = store.get_material(&h.material, false).unwrap();
    assert_eq!(unchanged.revision, 0);
    assert_eq!(unchanged.title, "Allowed material");
    assert_eq!(unchanged.fields["content"], "PROCESS PRIVATE MATERIAL");
}

fn planning_fixture(s: &mut Service, book: &str, kind: &str, title: &str) -> novel_core::Material {
    let fields = if kind == "timeline" {
        json!({"content":"PLANNING PIPE TEXT"})
    } else {
        json!({"setup":"PLANNING PIPE TEXT","resolution":""})
    };
    s.create_material(serde_json::from_value(json!({"book_id":book,"kind":kind,"title":title,"fields":fields,"volume_id":null,"chapter_id":null,"source_chapter_id":null,"source_revision":null,"story_order":1,"story_time":"Day one","tracking_status":"planned","payoff_chapter_id":null,"payoff_revision":null})).unwrap()).unwrap()
}
fn planning_process_readonly(mcp: bool) {
    let mut h = Harness::new(mcp, false, true);
    if mcp {
        h.initialize();
    }
    let mut originals = Vec::new();
    for kind in ["timeline", "foreshadow"] {
        let page = h.call("list_materials", json!({"kind":kind,"limit":1}));
        assert_eq!(page["result"]["items"].as_array().unwrap().len(), 1);
        let meta = &page["result"]["items"][0];
        assert_eq!(meta["title"], format!("Allowed {kind}"));
        assert!(meta.get("fields").is_none());
        assert!(page["result"]["next_cursor"].is_null());
        let id = meta["id"].clone();
        let read = h.call("get_material", json!({"material_id":id}));
        assert_eq!(read["result"]["tracking_status"], "planned");
        originals.push(read["result"].clone());
        assert_eq!(h.call("propose_material_edit",json!({"material_id":id,"base_revision":0,"new_title":"Denied","new_fields":read["result"]["fields"],"reason":"fixture","idempotency_key":kind}))["error"]["code"],"unsupported_operation");
    }
    let search = h.call("search_book", json!({"query":"PLANNING PIPE TEXT"}));
    assert!(search.get("error").is_none(), "{search}");
    assert!(search.to_string().contains("Allowed timeline"));
    assert!(search.to_string().contains("Allowed foreshadow"));
    assert!(!search.to_string().contains("Foreign"));
    h.finish();
    let store = Service::open(&h.db).unwrap();
    for original in originals {
        let current = serde_json::to_value(
            store
                .get_material(original["id"].as_str().unwrap(), false)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(current, original);
    }
}

fn strict_writing_process(mcp: bool) {
    let mut h = Harness::new_mode(mcp, false, true, true);
    if mcp {
        h.initialize();
    }
    let state = h.call("get_writing_policy", json!({"chapter_id":h.chapter}));
    let pack = state["result"]["packs"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(h.edit("ordinary", "bad").get("error").is_some());
    assert!(h
        .call("get_writing_policy", json!({"chapter_id":h.other}))
        .get("error")
        .is_some());
    let m = h.call("get_writing_pack_manifest", json!({"pack_id":pack}));
    assert!(m["result"].get("materials").is_none());
    let prior = h.prior.clone().unwrap();
    let handle = format!("context:{prior}");
    assert_eq!(
        state["result"]["policy"]["context_chapter_ids"],
        json!([prior])
    );
    let context = m["result"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "context_chapter")
        .unwrap();
    assert_eq!(context["id"], handle);
    assert_eq!(context["fields"], json!(["body", "$title"]));
    assert_eq!(context["metadata"]["chapter_id"], prior);
    assert_eq!(context["revision"], 1);
    assert!(context.get("body").is_none());
    let chunk=h.call("read_writing_pack_entry",json!({"pack_id":pack,"entry_id":format!("material:{}",h.material),"field_key":"content","cursor":0}));
    assert_eq!(chunk["result"]["text"], "PROCESS PRIVATE MATERIAL");
    let part = |h: &mut Harness, cursor| {
        h.call(
            "read_writing_pack_entry",
            json!({"pack_id":pack,"entry_id":handle,"field_key":"body","cursor":cursor}),
        )
    };
    let first = part(&mut h, 0);
    assert_eq!(
        first["result"]["text"].as_str().unwrap(),
        format!("{}旧", "x".repeat(32765))
    );
    assert_eq!(first["result"]["next_cursor"], 32768);
    let last = part(&mut h, 32768);
    assert_eq!(last["result"]["text"], "🚣\r\n岸");
    assert!(last["result"]["next_cursor"].is_null());
    assert!(part(&mut h, 32766).get("error").is_some());
    let foreign = h.foreign_pack.clone().unwrap();
    assert!(h
        .call("get_writing_pack_manifest", json!({"pack_id":foreign}))
        .get("error")
        .is_some());
    assert!(h
        .call(
            "read_writing_pack_entry",
            json!({"pack_id":foreign,"entry_id":handle,"field_key":"body","cursor":0})
        )
        .get("error")
        .is_some());
    let input = json!({"pack_id":pack,"new_body":"DRAFT ONLY","citations":[
        {"entry_id":format!("material:{}",h.material),"field_key":"content","start":0,"end":7,"explanation":"Follows outline"},
        {"entry_id":handle,"field_key":"body","start":32766,"end":32767,"explanation":"Boat continuity"}
    ],"conflicts":[],"idempotency_key":"strict"});
    let q = h.call("propose_constrained_edit", input.clone());
    let id = q["result"]["id"].as_str().unwrap().to_owned();
    assert_eq!(
        h.call("propose_constrained_edit", input.clone())["result"]["id"],
        id
    );
    assert_eq!(q["result"]["pack_id"], pack);
    assert!(q["result"].get("input").is_none());
    assert_eq!(h.call("propose_constrained_edit", input.clone()), q);
    let reread = h.call("get_constrained_proposal", json!({"proposal_id":id}));
    assert_eq!(reread, q);
    assert!(reread["result"].get("author_review").is_none());
    assert!(!reread.to_string().contains(PRIVATE_REVIEW_NOTE));
    let mut heavy = input.clone();
    heavy["new_body"] = json!("\"".repeat(700_000));
    heavy["idempotency_key"] = json!("heavy");
    let large = h.call("propose_constrained_edit", heavy.clone());
    let large_id = large["result"]["id"].as_str().unwrap().to_owned();
    assert_eq!(large["result"]["new_body_bytes"], 700_000);
    assert!(large["result"].to_string().len() < 4096);
    assert_eq!(
        h.call("propose_constrained_edit", heavy.clone())["result"]["id"],
        large_id
    );
    assert_eq!(
        h.call("get_constrained_proposal", json!({"proposal_id":large_id}))["result"]
            ["new_body_bytes"],
        700_000
    );
    let mut changed = heavy.clone();
    changed["new_body"] = json!("different same key");
    assert!(h
        .call("propose_constrained_edit", changed)
        .get("error")
        .is_some());
    if mcp {
        heavy["idempotency_key"] = json!("must-not-run");
        h.send(json!({"jsonrpc":"2.0","id":"x".repeat(129),"method":"tools/call","params":{"name":"propose_constrained_edit","arguments":heavy}}));
        let rejected = h.receive();
        assert!(rejected.get("error").is_some());
        assert!(rejected["id"].is_null());
    }
    let mut spoof_review = input.clone();
    spoof_review["author_review"] = json!({"revision":1,"items":[]});
    assert!(h
        .call("propose_constrained_edit", spoof_review)
        .get("error")
        .is_some());
    let mut spoof = input;
    spoof["client_id"] = json!("forged");
    assert!(h
        .call("propose_constrained_edit", spoof)
        .get("error")
        .is_some());
    assert!(h
        .call(
            "decide_constrained_proposal",
            json!({"proposal_id":id,"approve":true})
        )
        .get("error")
        .is_some());
    for method in [
        "save_constrained_review",
        "accept_reviewed_constrained_proposal",
        "configure_writing_policy_with_context",
        "get_writing_context_directory",
        "prepare_writing_pack",
    ] {
        assert!(h.call(method, json!({"proposal_id":id,"expected_review_revision":1,"items":[{"material_id":h.material,"verdict":"conflict","note":"spoof"}]}))
            .get("error").is_some());
    }
    assert!(h
        .call("configure_writing_policy", json!({"chapter_id":h.chapter}))
        .get("error")
        .is_some());
    assert_eq!(
        h.call("get_constrained_proposal", json!({"proposal_id":id}))["result"]["decision"],
        "pending"
    );
    let cancellation = h.call("cancel_constrained_proposal", json!({"proposal_id":id}));
    assert_eq!(cancellation["result"]["decision"], "cancelled");
    assert!(!cancellation.to_string().contains(PRIVATE_REVIEW_NOTE));
    assert!(cancellation["result"].get("author_review").is_none());
    h.finish();
    let store = Service::open(&h.db).unwrap();
    assert_eq!(store.get_chapter(&h.chapter).unwrap().body, "");
    let review = store
        .get_constrained_proposal(&h.book, &id)
        .unwrap()
        .author_review
        .unwrap();
    assert_eq!(review.revision, 1);
    assert_eq!(review.items[0].verdict, AuthorReviewVerdict::Conforms);
    assert_eq!(review.items[0].note, PRIVATE_REVIEW_NOTE);
    assert_eq!(
        store
            .get_writing_state(&h.book, &h.chapter)
            .unwrap()
            .proposals
            .len(),
        2
    );
}

fn foreshadow_process(mcp: bool) {
    let mut h = Harness::new_modes(mcp, false, true, false, true);
    if mcp {
        h.initialize();
        h.send(json!({"jsonrpc":"2.0","id":"list","method":"tools/list"}));
        let tool_list = h.receive();
        let tools = tool_list["result"]["tools"].as_array().unwrap();
        let f = tools
            .iter()
            .find(|v| v["name"] == "list_foreshadow_occurrences")
            .expect("declared MCP read tool");
        assert_eq!(f["inputSchema"]["properties"]["limit"]["maximum"], 20);
        assert_eq!(f["inputSchema"]["additionalProperties"], false);
        assert_eq!(
            tools
                .iter()
                .filter(|v| v["name"].as_str().unwrap().contains("foreshadow"))
                .count(),
            4
        );
    }
    let (allowed, foreign, foreign_owned) = h.foreshadow_cards.clone().unwrap();
    let method = "list_foreshadow_occurrences";
    let one = h.call(method, json!({"material_id":allowed,"limit":1}));
    assert!(one.get("error").is_none(), "{one}");
    let first = &one["result"]["items"][0];
    let cursor = one["result"]["next_after_id"].as_str().unwrap();
    assert_eq!(first["occurrence"]["id"], cursor);
    let two = h.call(
        method,
        json!({"material_id":allowed,"limit":1,"after_id":cursor}),
    );
    let second = &two["result"]["items"][0];
    assert_ne!(first["occurrence"]["id"], second["occurrence"]["id"]);
    assert!(two["result"]["next_after_id"].is_null());
    for record in [first, second] {
        assert_eq!(record["occurrence"]["quote"], "😀\r\nCLUE");
        assert_eq!(
            record["current"].is_null(),
            record["source_status"] == "unavailable"
        );
        let json = record.to_string();
        assert!(
            !json.contains("request_key")
                && !json.contains("request_epoch")
                && !json.contains("body-not-quoted")
        );
    }
    assert!([first, second]
        .iter()
        .any(|row| row["source_status"] == "unavailable"));
    assert!([first, second]
        .iter()
        .any(|row| row["source_status"] == "current"));
    for args in [
        json!({"material_id":foreign}),
        json!({"material_id":allowed,"book_id":"try-override"}),
        json!({"material_id":allowed,"limit":21}),
    ] {
        assert!(h.call(method, args).get("error").is_some());
    }
    assert!(h
        .call("create_foreshadow_occurrence", json!({}))
        .get("error")
        .is_some());
    assert!(h
        .call("set_foreshadow_occurrence_deleted", json!({}))
        .get("error")
        .is_some());
    let method = "propose_foreshadow_occurrence";
    let proposal = json!({"material_id":allowed,"material_revision":4,"chapter_id":h.chapter,"chapter_revision":1,
        "start":1,"end":8,"quote":"😀\r\nCLUE","role":"hint","note":"PRIVATE PAYLOAD NOTE","reason":"PRIVATE PAYLOAD REASON","idempotency_key":"private-request-key"});
    for (field, value) in [
        ("material_id",json!(foreign)), ("chapter_id",json!(h.other)), ("client_id",json!("forged")),
        ("book_id",json!(h.book)), ("epoch",json!("forged")), ("status",json!("accepted")),
        ("quote",json!("😀\nCLUE")),
    ] {
        let mut forged = proposal.clone(); forged[field] = value;
        assert!(h.call(method, forged).get("error").is_some(), "{field}");
    }
    let first = h.call(method, proposal.clone());
    assert_eq!(first["result"]["status"], "pending", "{first}");
    let id = first["result"]["id"].as_str().unwrap();
    for text in ["PRIVATE PAYLOAD", "private-request-key", "originating_epoch", "client_id", "CLUE"] {
        assert!(!first.to_string().contains(text));
    }
    assert_eq!(h.call(method, proposal.clone()), first);
    let mut conflict = proposal.clone(); conflict["role"] = json!("payoff");
    assert_eq!(h.call(method, conflict)["error"]["code"], "idempotency_conflict");
    for method in ["get_foreshadow_proposal", "cancel_foreshadow_proposal"] {
        assert!(h.call(method,json!({"proposal_id":foreign_owned})).get("error").is_some());
        assert!(h.call(method,json!({"proposal_id":id,"client_id":"spoof"})).get("error").is_some());
    }
    assert_eq!(h.call("get_foreshadow_proposal",json!({"proposal_id":id})), first);
    assert!(h.call("decide_foreshadow_proposal",json!({"proposal_id":id,"action":"accept"})).get("error").is_some());
    let cancelled = h.call("cancel_foreshadow_proposal",json!({"proposal_id":id}));
    assert_eq!(cancelled["result"]["status"], "cancelled");
    assert_eq!(h.call("cancel_foreshadow_proposal",json!({"proposal_id":id})), cancelled);
    assert_eq!(h.call(method,proposal), cancelled);
    h.finish();
    let s = Service::open(&h.db).unwrap();
    assert_eq!(
        s.list_foreshadow_occurrences(&h.book, &allowed, &s.epoch().unwrap())
            .unwrap()
            .len(),
        3
    );
}
