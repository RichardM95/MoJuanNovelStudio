use novel_core::{
    agent::{AgentBroker, AgentConnection},
    Command, Service,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

fn call(c: &AgentConnection, method: &str, params: Value) -> Value {
    c.call(json!({"method":method,"params":params}))
}
fn ok(c: &AgentConnection, m: &str, p: Value) -> Value {
    let reply = call(c, m, p);
    assert!(reply.get("error").is_none(), "{m}: {reply}");
    reply["result"].clone()
}
#[test]
fn delivery_requires_two_grants_and_reconciles_interrupted_chapters_in_order() {
    let mut s = Service::open(":memory:").unwrap();
    let imported = s
        .prepare_txt_import(
            "第一卷 归途\n第一章 初见\n正文甲\n第二章 再见\n正文乙".as_bytes(),
            "灯塔.txt",
        )
        .unwrap();
    let book = s.commit_txt_import(&imported.id).unwrap().book_id;
    let dir = s.fanqie_directory(&book).unwrap();
    let ids = dir
        .chapters
        .iter()
        .map(|c| &c.chapter_id)
        .collect::<Vec<_>>();
    assert_eq!(ids.len(), 2);
    let store = Arc::new(Mutex::new(s));
    let broker = Arc::new(AgentBroker::new(store.clone()));
    let connect = |method: &str, send: bool| {
        let c = broker.connect().unwrap();
        ok(&c, method, json!({"client_name":"fixture"}));
        broker.grant_access(c.id(), &book, send).unwrap();
        c
    };
    let normal = connect("request_access", false);
    let agent = connect("request_access", true);
    let browser = connect("request_browser_access", false);
    let request = json!({"chapter_ids":ids,"idempotency_key":"batch-1"});
    let create = "create_fanqie_draft_job";
    assert!(call(&normal, create, request.clone())
        .get("error")
        .is_some());
    assert!(call(&agent, "browser_claim", json!({}))
        .get("error")
        .is_some());
    assert!(call(&browser, "get_chapter", json!({"chapter_id":ids[0]}))
        .get("error")
        .is_some());
    assert!(call(&agent, create, request.clone()).get("error").is_some());
    ok(
        &browser,
        "browser_bind",
        json!({"volume_id":dir.chapters[0].volume_id,
        "list_url":"https://fanqienovel.com/main/writer/fixture","book":"灯塔","volume":"归途","first_number":12}),
    );
    assert!(call(
        &agent,
        create,
        json!({"chapter_ids":["foreign"],"idempotency_key":"invalid"})
    )
    .get("error")
    .is_some());
    let job = ok(&agent, create, request.clone());
    assert_eq!(job, ok(&agent, create, request.clone()));
    let claim = ok(&browser, "browser_claim", json!({}));
    assert_eq!(claim["number"], "12");
    assert_eq!(claim["verify_only"], false);
    let replay = ok(&browser, "browser_claim", json!({}));
    assert_eq!(replay["verify_only"], true);
    assert_ne!(replay["claim"], claim["claim"]);
    let receipt = |c: &Value| json!({"job_id":job["job_id"],"claim":c["claim"],"state":"exists","detail":"回读一致"});
    assert!(call(&browser, "browser_finish", receipt(&claim))
        .get("error")
        .is_some());
    ok(&browser, "browser_finish", receipt(&replay));
    broker.revoke_session(agent.id()).unwrap();
    assert!(ok(&browser, "browser_claim", json!({})).is_null());
    let next_agent = connect("request_access", true);
    assert_eq!(ok(&next_agent, create, request)["job_id"], job["job_id"]);
    let next = ok(&browser, "browser_claim", json!({}));
    assert_eq!(next["number"], "13");
    let check = json!({"job_id":job["job_id"],"claim":next["claim"]});
    ok(&browser, "browser_validate", check.clone());
    let c = &dir.chapters[1];
    store
        .lock()
        .unwrap()
        .apply(Command::SaveChapter {
            id: c.chapter_id.clone(),
            expected_revision: c.revision,
            body: "新正文".into(),
        })
        .unwrap();
    assert!(call(&browser, "browser_validate", check)
        .get("error")
        .is_some());
    assert!(call(&browser, "browser_claim", json!({}))
        .get("error")
        .is_some());
    let report = ok(
        &next_agent,
        "get_fanqie_draft_job",
        json!({"job_id":job["job_id"]}),
    );
    assert_eq!(report["chapters"][0]["state"], "exists");
    assert_eq!(report["chapters"][1]["state"], "review");
}
