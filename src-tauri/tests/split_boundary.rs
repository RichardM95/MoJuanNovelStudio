use novel_core::{agent::AgentBroker, agent_stdio, Command, Service};
use serde_json::json;
use std::sync::{Arc, Mutex};
#[test]
fn chapter_split_is_not_an_external_agent_capability() {
    let mut store = Service::open(":memory:").unwrap();
    let state = store
        .apply(Command::CreateBook {
            title: "隔离测试".into(),
            author: "".into(),
        })
        .unwrap();
    let broker = Arc::new(AgentBroker::new(Arc::new(Mutex::new(store))));
    let c = broker.connect().unwrap();
    c.call(json!({"method":"request_access","params":{"client_name":"isolated split test"}}));
    broker.grant_session(c.id(), &state.books[0].id).unwrap();
    let names = [
        "prepare_chapter_split",
        "get_split_detail",
        "commit_chapter_split",
        "discard_chapter_split",
        "prepareChapterSplit",
        "getSplitDetail",
        "commitChapterSplit",
        "discardChapterSplit",
    ];
    let mut lines = vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    ];
    for (i, name) in names.iter().enumerate() {
        assert_eq!(
            c.call(json!({"method":name,"params":{}}))["error"]["code"],
            "method_not_found"
        );
        lines.push(json!({"jsonrpc":"2.0","id":i+2,"method":"tools/call","params":{"name":name,"arguments":{}}}));
    }
    let input = lines
        .into_iter()
        .map(|v| format!("{v}\n"))
        .collect::<String>();
    let mut output = vec![];
    agent_stdio::run(std::io::Cursor::new(input), &mut output, true, |_| {
        panic!("split crossed external tool boundary")
    })
    .unwrap();
    let responses = String::from_utf8(output).unwrap();
    assert_eq!(responses.lines().count(), 9);
    for line in responses.lines().skip(1) {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(v.get("error").is_some());
    }
}
