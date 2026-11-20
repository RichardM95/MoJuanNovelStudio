use novel_core::{agent::AgentBroker, agent_stdio, Command, Service};
use serde_json::json;
use std::sync::{Arc, Mutex};
#[test]
fn batch_replacement_is_not_an_external_agent_capability() {
    let mut store = Service::open(":memory:").unwrap();
    let state = store
        .apply(Command::CreateBook {
            title: "隔离测试".into(),
            author: "".into(),
        })
        .unwrap();
    let broker = Arc::new(AgentBroker::new(Arc::new(Mutex::new(store))));
    let c = broker.connect().unwrap();
    c.call(json!({"method":"request_access","params":{"client_name":"isolated replacement test"}}));
    broker.grant_session(c.id(), &state.books[0].id).unwrap();
    let names = [
        "preview_book_replace",
        "get_replace_detail",
        "commit_book_replace",
        "discard_book_replace",
        "previewBookReplace",
        "getReplaceDetail",
        "commitBookReplace",
        "discardBookReplace",
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
        panic!("replacement crossed external tool boundary")
    })
    .unwrap();
    let responses = String::from_utf8(output).unwrap();
    assert_eq!(responses.lines().count(), 9);
    for line in responses.lines().skip(1) {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(v.get("error").is_some());
    }
}
#[test]
fn chapter_move_or_recycle_invalidates_the_whole_preview() {
    for recycle in [false, true] {
        let mut s = Service::open(":memory:").unwrap();
        let initial = s
            .apply(Command::CreateBook {
                title: "测试".into(),
                author: "".into(),
            })
            .unwrap();
        let b = initial.books[0].id.clone();
        let c = initial.chapters[0].id.clone();
        s.apply(Command::SaveChapter {
            id: c.clone(),
            body: "旧名".into(),
            expected_revision: 0,
        })
        .unwrap();
        let state = s
            .apply(Command::CreateVolume {
                book_id: b.clone(),
                title: "第二卷".into(),
            })
            .unwrap();
        let v = state
            .volumes
            .iter()
            .find(|v| v.id != initial.volumes[0].id)
            .unwrap()
            .id
            .clone();
        let p = s.preview_book_replace(&b, "旧名", "新名").unwrap();
        if recycle {
            s.apply(Command::TrashChapter { id: c.clone() }).unwrap();
        } else {
            s.apply(Command::TransferChapter {
                id: c.clone(),
                volume_id: v,
            })
            .unwrap();
        }
        assert!(s.commit_book_replace(&p.id).is_err());
        let final_state = s.load().unwrap();
        let chapter = final_state.chapters.iter().find(|x| x.id == c).unwrap();
        assert_eq!(chapter.body, "旧名");
        assert_eq!(chapter.revision, 1);
    }
}
