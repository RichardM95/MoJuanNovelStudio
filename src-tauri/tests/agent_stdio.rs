#[path = "../src/agent_stdio.rs"]
mod agent_stdio;
use serde_json::{json, Value};
use std::io::Cursor;
fn run(lines: Vec<Value>, mcp: bool) -> Vec<Value> {
    let data = lines
        .into_iter()
        .map(|x| x.to_string() + "\n")
        .collect::<String>();
    let mut out = Vec::new();
    agent_stdio::run(Cursor::new(data), &mut out, mcp, |_| {
        Ok(json!({"result":{"ok":true}}))
    })
    .unwrap();
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}
#[test]
fn mcp_requires_exact_version_and_initialization() {
    let out = run(
        vec![
            json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
            json!({"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"2026-07-28"}}),
        ],
        true,
    );
    assert_eq!(out[0]["error"]["code"], -32002);
    assert_eq!(out[1]["error"]["code"], -32602);
}
#[test]
fn negotiates_codex_tools_protocol_versions() {
    for version in ["2025-03-26","2025-06-18","2025-11-25"] {
        let response=run(vec![json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":version}})],true);
        assert_eq!(response[0]["result"]["protocolVersion"],version);
    }
}
#[test]
fn tools_include_explicitly_authorized_draft_jobs() {
    let out = run(
        vec![
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"approve","arguments":{}}}),
        ],
        true,
    );
    assert_eq!(out.len(), 3);
    assert_eq!(out[1]["result"]["tools"].as_array().unwrap().len(), 26);
    assert_eq!(out[2]["error"]["code"], -32602);
}
#[test]
fn malformed_and_oversized_frames_are_bounded() {
    let mut out = Vec::new();
    agent_stdio::run(Cursor::new(b"bad\n"), &mut out, false, |_| panic!()).unwrap();
    assert!(String::from_utf8(out).unwrap().contains("invalid_json"));
    let input = vec![b'x'; 2 * 1024 * 1024 + 1];
    assert!(agent_stdio::run(Cursor::new(input), Vec::new(), false, |_| panic!()).is_err());
}
#[test]
fn cli_rejects_unknown_methods_and_mcp_wraps_errors() {
    let out = run(vec![json!({"method":"delete","params":{}})], false);
    assert_eq!(out[0]["error"]["code"], "unknown_method");
}
#[test]
fn successful_tool_call_uses_only_selected_method_and_arguments() {
    let lines = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_chapter","arguments":{"chapter_id":"fixture"}}}),
    ];
    let input = lines
        .iter()
        .map(|v| v.to_string() + "\n")
        .collect::<String>();
    let mut output = Vec::new();
    let mut seen = Vec::new();
    agent_stdio::run(Cursor::new(input), &mut output, true, |req| {
        seen.push(req.clone());
        Ok(json!({"error":{"code":"pending","message":"Not authorized"}}))
    })
    .unwrap();
    assert_eq!(
        seen,
        vec![json!({"method":"get_chapter","params":{"chapter_id":"fixture"}})]
    );
    let responses: Vec<Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(responses[1]["result"]["isError"], true);
    assert!(responses[1]["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("pending"));
}
#[test]
fn notifications_never_run_agent_tools() {
    let input = json!({"jsonrpc":"2.0","method":"tools/call","params":{"name":"propose_edit"}})
        .to_string()
        + "\n";
    let mut output = Vec::new();
    agent_stdio::run(Cursor::new(input), &mut output, true, |_| {
        panic!("notification cannot invoke tool")
    })
    .unwrap();
    assert!(output.is_empty());
}
#[test]
fn cli_forwards_authorized_operation_but_not_request_access() {
    let out = run(
        vec![
            json!({"method":"session_status","params":{}}),
            json!({"method":"request_access","params":{"client_name":"spoof"}}),
        ],
        false,
    );
    assert_eq!(out[0]["result"]["ok"], true);
    assert_eq!(out[1]["error"]["code"], "unknown_method");
}
#[test]
fn transport_error_permanently_drops_connection_without_second_call() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let mut connection = agent_stdio::ConnectionGuard::new(calls.clone());
    assert!(connection
        .call(|c| {
            c.fetch_add(1, Ordering::SeqCst);
            Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "late response",
            ))
        })
        .is_err());
    assert!(connection
        .call(|c| {
            c.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"result":"late"}))
        })
        .is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn material_tools_only_expose_bounded_reads() {
    let out = run(
        vec![
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"save_material","arguments":{}}}),
        ],
        true,
    );
    let tools = out[1]["result"]["tools"].as_array().unwrap();
    let list = tools
        .iter()
        .find(|t| t["name"] == "list_materials")
        .unwrap();
    assert_eq!(list["inputSchema"]["properties"]["limit"]["maximum"], 100);
    assert!(tools.iter().any(|t| t["name"] == "get_material"));
    assert!(!tools.iter().any(|t| t["name"] == "save_material"));
    assert_eq!(out[2]["error"]["code"], -32602);
}

#[test]
fn context_tools_have_bounded_search_and_no_acceptance() {
    let out = run(
        vec![
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"approve_material_proposal","arguments":{}}}),
        ],
        true,
    );
    let tools = out[1]["result"]["tools"].as_array().unwrap();
    let search = tools.iter().find(|t| t["name"] == "search_book").unwrap();
    assert_eq!(search["inputSchema"]["properties"]["limit"]["maximum"], 50);
    assert!(tools.iter().any(|t| t["name"] == "propose_material_edit"));
    assert!(tools
        .iter()
        .any(|t| t["name"] == "cancel_material_proposal"));
    assert_eq!(out[2]["error"]["code"], -32602);
}
#[test]
fn planning_kinds_are_readonly_with_a_bounded_occurrence_listing() {
    let out = run(
        vec![
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"resolve_foreshadow","arguments":{}}}),
        ],
        true,
    );
    let tools = out[1]["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 26);
    let kinds = tools
        .iter()
        .find(|t| t["name"] == "list_materials")
        .unwrap()["inputSchema"]["properties"]["kind"]["enum"]
        .as_array()
        .unwrap();
    assert!(kinds.contains(&json!("timeline")));
    assert!(kinds.contains(&json!("foreshadow")));
    let listing=tools.iter().find(|t|t["name"]=="list_foreshadow_occurrences").unwrap();
    assert_eq!(listing["inputSchema"]["properties"]["limit"]["default"],10);
    assert_eq!(listing["inputSchema"]["properties"]["limit"]["maximum"],20);
    assert_eq!(listing["inputSchema"]["additionalProperties"],false);
    let proposed = tools.iter().find(|t| t["name"]=="propose_foreshadow_occurrence").unwrap();
    assert_eq!(proposed["inputSchema"]["properties"]["quote"]["maxLength"], 4096);
    assert_eq!(proposed["inputSchema"]["properties"]["note"]["maxLength"], 1024);
    assert_eq!(proposed["inputSchema"]["properties"]["reason"]["maxLength"], 2048);
    assert_eq!(proposed["inputSchema"]["additionalProperties"], false);
    assert!(proposed["inputSchema"]["properties"].get("client_id").is_none());
    assert!(proposed["inputSchema"]["properties"].get("book_id").is_none());
    assert!(!tools.iter().any(|t| t["name"] == "decide_foreshadow_proposal"));

    assert_eq!(out[2]["error"]["code"], -32602);
}
#[test]
fn oversized_mcp_id_is_rejected_before_dispatch() {
    let lines = vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":"x".repeat(129),"method":"tools/call","params":{"name":"propose_constrained_edit","arguments":{}}}),
    ];
    let data = lines
        .into_iter()
        .map(|x| x.to_string() + "\n")
        .collect::<String>();
    let mut out = vec![];
    let mut called = false;
    agent_stdio::run(Cursor::new(data), &mut out, true, |_| {
        called = true;
        Ok(json!({"result":{}}))
    })
    .unwrap();
    assert!(!called);
    let last: Value =
        serde_json::from_str(String::from_utf8(out).unwrap().lines().last().unwrap()).unwrap();
    assert!(last.get("error").is_some());
    assert!(last["id"].is_null());
}
#[test]
fn mcp_escaped_output_is_bounded_without_truncated_json() {
    let data=[json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}),json!({"jsonrpc":"2.0","method":"notifications/initialized"}),json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_outline","arguments":{}}})].into_iter().map(|x|x.to_string()+"\n").collect::<String>();
    let mut out = vec![];
    agent_stdio::run(Cursor::new(data), &mut out, true, |_| {
        Ok(json!({"result":{"body":"\"".repeat(700_000)}}))
    })
    .unwrap();
    for line in out.split(|b| *b == b'\n').filter(|x| !x.is_empty()) {
        assert!(line.len() + 1 <= agent_stdio::MAX_FRAME);
        let _: Value = serde_json::from_slice(line).unwrap();
    }
    let last: Value =
        serde_json::from_str(String::from_utf8(out).unwrap().lines().last().unwrap()).unwrap();
    assert_eq!(last["error"]["code"], -32001);
}
