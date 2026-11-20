//! External history boundary only: trusted history UI APIs must not expand agent tools.
//! All text and databases are isolated synthetic fixtures; no IPC sockets or Windows claims.
use novel_core::{agent::AgentBroker, agent_stdio, Command, Service};
use serde_json::{json, Value};
use std::{
    io::Cursor,
    sync::{Arc, Mutex},
};
struct Fixture {
    store: Arc<Mutex<Service>>,
    broker: Arc<AgentBroker>,
    book: String,
    chapter: String,
    foreign_chapter: String,
    old_version: String,
}
fn fixture() -> Fixture {
    let mut s = Service::open(":memory:").unwrap();
    let a = s
        .apply(Command::CreateBook {
            title: "History boundary A".into(),
            author: "".into(),
        })
        .unwrap();
    let book = a.books[0].id.clone();
    let chapter = a.chapters[0].id.clone();
    s.apply(Command::SaveChapter {
        id: chapter.clone(),
        body: "ARCHIVED_ONLY_A_FIXTURE".into(),
        expected_revision: 0,
    })
    .unwrap();
    s.apply(Command::SaveChapter {
        id: chapter.clone(),
        body: "CURRENT_A_FIXTURE".into(),
        expected_revision: 1,
    })
    .unwrap();
    let old_version = s
        .chapter_history(&chapter)
        .unwrap()
        .into_iter()
        .find(|v| v.body == "ARCHIVED_ONLY_A_FIXTURE")
        .unwrap()
        .id;
    let b = s
        .apply(Command::CreateBook {
            title: "Foreign history B".into(),
            author: "".into(),
        })
        .unwrap();
    let foreign_chapter = b
        .chapters
        .iter()
        .find(|c| c.id != chapter)
        .unwrap()
        .id
        .clone();
    s.apply(Command::SaveChapter {
        id: foreign_chapter.clone(),
        body: "FOREIGN_B_PRIVATE_FIXTURE".into(),
        expected_revision: 0,
    })
    .unwrap();
    let store = Arc::new(Mutex::new(s));
    let broker = Arc::new(AgentBroker::new(store.clone()));
    Fixture {
        store,
        broker,
        book,
        chapter,
        foreign_chapter,
        old_version,
    }
}
fn connection(f: &Fixture) -> novel_core::agent::AgentConnection {
    let c = f.broker.connect().unwrap();
    c.call(json!({"method":"request_access","params":{"client_name":"history boundary fixture"}}));
    f.broker.grant_session(c.id(), &f.book).unwrap();
    c
}
fn forbidden() -> Vec<&'static str> {
    vec![
        "chapter_history",
        "restore_chapter_version",
        "list_chapter_versions",
        "get_chapter_version",
        "name_chapter_version",
        "set_chapter_version_label",
        "listChapterVersions",
        "getChapterVersion",
        "setChapterVersionLabel",
        "restoreChapterVersionScoped",
        "chapterHistory",
        "restoreChapterVersion",
        "restore_chapter_version_scoped",
    ]
}
#[test]
fn broker_history_commands_are_not_public_and_cannot_mutate_current_text() {
    let f = fixture();
    let c = connection(&f);
    for method in forbidden() {
        let result=c.call(json!({"method":method,"params":{"book_id":f.book,"chapter_id":f.chapter,"version_id":f.old_version,"expected_revision":2,"name":"external forbidden"}}));
        assert_eq!(result["error"]["code"], "method_not_found", "{method}");
    }
    assert_eq!(
        f.store
            .lock()
            .unwrap()
            .get_chapter(&f.chapter)
            .unwrap()
            .body,
        "CURRENT_A_FIXTURE"
    );
}
#[test]
fn allowed_reads_do_not_offer_history_or_cross_book_escape() {
    let f = fixture();
    let c = connection(&f);
    for request in [
        json!({"method":"list_books"}),
        json!({"method":"get_outline"}),
        json!({"method":"get_chapter","params":{"chapter_id":f.chapter}}),
    ] {
        let result = c.call(request);
        assert!(result.get("result").is_some());
        let encoded = result.to_string();
        assert!(!encoded.contains("ARCHIVED_ONLY_A_FIXTURE"));
        assert!(!encoded.contains("FOREIGN_B_PRIVATE_FIXTURE"));
        assert!(!encoded.contains(&f.old_version));
    }
    for params in [
        json!({"chapter_id":f.chapter,"version_id":f.old_version}),
        json!({"chapter_id":f.chapter,"revision":1}),
        json!({"chapter_id":f.chapter,"include_history":true}),
    ] {
        let result = c.call(json!({"method":"get_chapter","params":params}));
        assert_eq!(result["error"]["code"], "invalid_input");
        assert!(!result.to_string().contains("ARCHIVED_ONLY_A_FIXTURE"));
    }
    let foreign = c.call(json!({"method":"get_chapter","params":{"chapter_id":f.foreign_chapter}}));
    assert_eq!(foreign["error"]["code"], "not_found");
}
#[test]
fn mcp_tool_catalog_and_dispatch_keep_history_management_private() {
    let f = fixture();
    let c = connection(&f);
    let mut input = vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
    ];
    for (i, name) in forbidden().iter().enumerate() {
        input.push(json!({"jsonrpc":"2.0","id":i+3,"method":"tools/call","params":{"name":name,"arguments":{"chapter_id":f.chapter,"version_id":f.old_version}}}));
    }
    let mut output = Vec::new();
    let mut transport_calls = 0;
    agent_stdio::run(
        Cursor::new(
            input
                .into_iter()
                .map(|v| format!("{v}\n"))
                .collect::<String>(),
        ),
        &mut output,
        true,
        |request| {
            transport_calls += 1;
            Ok(c.call(request.clone()))
        },
    )
    .unwrap();
    assert_eq!(
        transport_calls, 0,
        "unknown MCP tools must not reach Broker"
    );
    let rows = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let names = rows[1]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names.len(), 24);
    for name in forbidden() {
        assert!(!names.contains(&name));
    }
    for row in &rows[2..] {
        assert_eq!(row["error"]["code"], -32602);
    }
    assert_eq!(
        f.store
            .lock()
            .unwrap()
            .get_chapter(&f.chapter)
            .unwrap()
            .body,
        "CURRENT_A_FIXTURE"
    );
}
