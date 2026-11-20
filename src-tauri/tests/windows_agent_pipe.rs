#![cfg(windows)]
use novel_core::{agent::AgentBroker, ipc, Command, Service};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Command as ProcessCommand, Stdio},
    sync::{Arc, Mutex},
};

#[test]
#[ignore = "requires ordinary Windows named pipe access outside restricted test sandboxes"]
fn windows_pipe_uses_the_same_book_grant_as_the_desktop() {
    let mut service = Service::open(":memory:").unwrap();
    let library = service
        .apply(Command::CreateBook {
            title: "验收".into(),
            author: String::new(),
        })
        .unwrap();
    let broker = Arc::new(AgentBroker::new(Arc::new(Mutex::new(service))));
    let endpoint = ipc::desktop_endpoint(&format!("mojuan-test-{}", uuid::Uuid::new_v4())).unwrap();
    let server = ipc::start_server(Arc::clone(&broker), &endpoint).unwrap();
    assert!(ipc::start_server(Arc::clone(&broker), &endpoint).is_err());
    let mut client = ipc::connect(&endpoint).unwrap();
    let access = client
        .call(&json!({"method":"request_access","params":{"client_name":"fixture"}}))
        .unwrap();
    let session = access["result"]["id"].as_str().unwrap();
    let list = json!({"method":"list_books","params":{}});
    assert_eq!(
        client.call(&list).unwrap()["error"]["code"],
        "access_pending"
    );
    broker.grant_session(session, &library.books[0].id).unwrap();
    assert_eq!(
        client.call(&list).unwrap()["result"][0]["id"],
        library.books[0].id
    );
    broker.revoke_session(session).unwrap();
    assert!(client.call(&list).unwrap().get("error").is_some());
    drop(client);

    let mut process = ProcessCommand::new(env!("CARGO_BIN_EXE_mojuan-agent"))
        .args([
            "--socket",
            endpoint.to_str().unwrap(),
            "--mcp",
            "--name",
            "mcp-fixture",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = process.stdin.take().unwrap();
    let mut stdout = BufReader::new(process.stdout.take().unwrap());
    fn send(out: &mut impl Write, message: Value) {
        writeln!(out, "{message}").unwrap();
        out.flush().unwrap();
    }
    fn response(input: &mut impl BufRead) -> Value {
        let mut line = String::new();
        assert_ne!(input.read_line(&mut line).unwrap(), 0);
        serde_json::from_str(&line).unwrap()
    }
    let call = |id, name, args| {
        json!({
            "jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}
        })
    };
    let parsed = |result: Value| -> Value {
        serde_json::from_str(result["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
    };
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}),
    );
    assert_eq!(
        response(&mut stdout)["result"]["protocolVersion"],
        "2025-11-25"
    );
    send(
        &mut stdin,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    );
    send(&mut stdin, call(2, "session_status", json!({})));
    let pending = parsed(response(&mut stdout));
    assert_eq!(pending["result"]["status"], "pending");
    let session = pending["result"]["id"].as_str().unwrap();
    broker.grant_session(session, &library.books[0].id).unwrap();
    send(
        &mut stdin,
        call(
            3,
            "retrieve_writing_context",
            json!({"chapter_id":library.chapters[0].id,"query":"文字"}),
        ),
    );
    assert!(parsed(response(&mut stdout))["result"]["items"].is_array());
    drop(stdin);
    assert!(process.wait().unwrap().success());
    drop(server);
    assert!(ipc::connect(&endpoint).is_err());
}
