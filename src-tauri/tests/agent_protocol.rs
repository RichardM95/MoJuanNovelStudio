use novel_core::{agent::AgentBroker, Command, Service};
use serde_json::json;
use std::sync::{Arc, Mutex};
fn setup() -> (
    Arc<Mutex<Service>>,
    Arc<AgentBroker>,
    String,
    String,
    String,
) {
    let mut s = Service::open(":memory:").unwrap();
    let a = s
        .apply(Command::CreateBook {
            title: "A".into(),
            author: "".into(),
        })
        .unwrap();
    let b = a.books[0].id.clone();
    let c = a.chapters[0].id.clone();
    let other = s
        .apply(Command::CreateBook {
            title: "secret".into(),
            author: "".into(),
        })
        .unwrap()
        .chapters
        .iter()
        .find(|x| x.id != c)
        .unwrap()
        .id
        .clone();
    let s = Arc::new(Mutex::new(s));
    let broker = Arc::new(AgentBroker::new(s.clone()));
    (s, broker, b, c, other)
}
#[test]
fn pending_scope_and_forbidden_methods() {
    let (_, broker, b, c, other) = setup();
    let conn = broker.connect().unwrap();
    assert!(conn
        .call(json!({"method":"list_books"}))
        .get("error")
        .is_some());
    conn.call(json!({"method":"request_access","params":{"client_name":"fixture"}}));
    broker.grant_session(conn.id(), &b).unwrap();
    assert_eq!(
        conn.call(json!({"method":"list_books"}))["result"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        conn.call(json!({"method":"get_chapter","params":{"chapter_id":c}}))["result"]["body"],
        ""
    );
    assert!(conn
        .call(json!({"method":"get_chapter","params":{"chapter_id":other}}))
        .get("error")
        .is_some());
    for method in [
        "approve",
        "restore_history",
        "apply_command",
        "delete",
        "sql",
    ] {
        assert!(conn.call(json!({"method":method})).get("error").is_some());
    }
    drop(conn);
    assert!(broker.list_sessions().is_empty());
}
#[test]
fn proposals_idempotency_identity_revoke_and_epoch() {
    let (s, broker, b, c, _) = setup();
    let a = broker.connect().unwrap();
    a.call(json!({"method":"request_access","params":{"client_name":"fixture"}}));
    broker.grant_session(a.id(), &b).unwrap();
    let req = json!({"method":"propose_edit","params":{"chapter_id":c,"base_revision":0,"new_body":"proposal","reason":"test","idempotency_key":"key"}});
    let p = a.call(req.clone());
    assert!(p.get("result").is_some(), "{p}");
    assert_eq!(a.call(req), p);
    assert_eq!(s.lock().unwrap().get_chapter(&c).unwrap().body, "");
    assert_eq!(p["result"]["client_id"], a.id());
    let q = broker.connect().unwrap();
    q.call(json!({"method":"request_access","params":{"client_name":"other"}}));
    broker.grant_session(q.id(), &b).unwrap();
    assert!(q
        .call(json!({"method":"get_proposal","params":{"proposal_id":p["result"]["id"]}}))
        .get("error")
        .is_some());
    broker.revoke_session(a.id()).unwrap();
    assert!(a
        .call(json!({"method":"list_books"}))
        .get("error")
        .is_some());
    let backup = s.lock().unwrap().backup().unwrap();
    s.lock().unwrap().restore(&backup).unwrap();
    assert!(q
        .call(json!({"method":"list_books"}))
        .get("error")
        .is_some());
}
#[cfg(unix)]
#[test]
#[ignore = "requires Unix socket bind; cloud sandbox returns EPERM even after approved escalation"]
fn real_unix_ipc_private_modes_and_disconnect() {
    use std::{
        io::{Read, Write},
        os::unix::fs::PermissionsExt,
    };
    let (_, broker, _, _, _) = setup();
    let dir = std::env::temp_dir().join(format!("mojuan-ipc-{}", uuid::Uuid::new_v4()));
    let path = dir.join("agent.sock");
    let server = novel_core::ipc::start_server(broker.clone(), &path).unwrap();
    assert_eq!(
        std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let mut c = novel_core::ipc::connect(&path).unwrap();
    assert_eq!(
        c.call(&json!({"method":"request_access","params":{"client_name":"ipc"}}))
            .unwrap()["result"]["status"],
        "pending"
    );
    drop(c);
    for _ in 0..30 {
        if broker.list_sessions().is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(broker.list_sessions().is_empty());
    let mut raw = std::os::unix::net::UnixStream::connect(&path).unwrap();
    raw.write_all(b"{bad}\n").unwrap();
    let mut r = [0; 256];
    let n = raw.read(&mut r).unwrap();
    assert!(String::from_utf8_lossy(&r[..n]).contains("invalid_json"));
    drop(raw);
    drop(server);
    assert!(!path.exists());
    std::fs::remove_dir(dir).unwrap();
}
#[test]
fn own_cancel_and_injected_identity_denied() {
    let (_, broker, b, c, _) = setup();
    let a = broker.connect().unwrap();
    a.call(json!({"method":"request_access","params":{"client_name":"agent"}}));
    broker.grant_session(a.id(), &b).unwrap();
    let mut req = json!({"method":"propose_edit","params":{"chapter_id":c,"base_revision":0,"new_body":"x","reason":"r","idempotency_key":"key"}});
    req["params"]["client_id"] = json!("human");
    assert_eq!(a.call(req.clone())["error"]["code"], "invalid_input");
    req["params"].as_object_mut().unwrap().remove("client_id");
    let p = a.call(req);
    assert!(a
        .call(json!({"method":"cancel_proposal","params":{"proposal_id":p["result"]["id"]}}))
        .get("result")
        .is_some());
}
#[test]
fn bounded_sessions_frames_body_and_rate() {
    let (_, broker, b, c, _) = setup();
    let connections = (0..16)
        .map(|_| broker.connect().unwrap())
        .collect::<Vec<_>>();
    assert!(broker.connect().is_err());
    let a = &connections[0];
    a.call(json!({"method":"request_access","params":{"client_name":"limits"}}));
    broker.grant_session(a.id(), &b).unwrap();
    assert_eq!(a.call(json!({"method":"propose_edit","params":{"chapter_id":c,"base_revision":0,"new_body":"x".repeat(1024*1024+1),"reason":"r","idempotency_key":"k"}}))["error"]["code"],"limit_exceeded");
    let mut rate_limited = false;
    for _ in 0..121 {
        rate_limited |=
            a.call(json!({"method":"session_status"}))["error"]["code"] == "rate_limited";
    }
    assert!(rate_limited);
    let mut large = std::io::Cursor::new(vec![b'x'; novel_core::ipc::MAX_FRAME + 1]);
    assert!(novel_core::ipc::read_frame(&mut large).is_err());
}
#[cfg(unix)]
#[test]
fn ipc_refuses_unsafe_existing_directory_and_symlink() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let (_, broker, _, _, _) = setup();
    let dir = std::env::temp_dir().join(format!("mojuan-insecure-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(novel_core::ipc::start_server(broker.clone(), &dir.join("a.sock")).is_err());
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let alias = dir.with_extension("link");
    symlink(&dir, &alias).unwrap();
    assert!(novel_core::ipc::start_server(broker, &alias.join("a.sock")).is_err());
    std::fs::remove_file(alias).unwrap();
    std::fs::remove_dir(dir).unwrap();
}
#[cfg(unix)]
#[test]
#[ignore = "requires Unix socket bind; cloud sandbox returns EPERM even after approved escalation"]
fn host_shutdown_revokes_live_connection() {
    let (_, broker, _, _, _) = setup();
    let dir = std::env::temp_dir().join(format!("mojuan-shutdown-{}", uuid::Uuid::new_v4()));
    let path = dir.join("a.sock");
    let server = novel_core::ipc::start_server(broker.clone(), &path).unwrap();
    let mut client = novel_core::ipc::connect(&path).unwrap();
    client
        .call(&json!({"method":"request_access","params":{"client_name":"fixture"}}))
        .unwrap();
    drop(server);
    assert!(broker.list_sessions().is_empty());
    assert!(client.call(&json!({"method":"session_status"})).is_err());
    std::fs::remove_dir(dir).unwrap();
}
#[test]
fn unknown_fields_and_regrant_do_not_expand_scope() {
    let (_, broker, b, c, _) = setup();
    let a = broker.connect().unwrap();
    assert_eq!(
        a.call(
            json!({"method":"request_access","params":{"client_name":"a","client_id":"stolen"}})
        )["error"]["code"],
        "invalid_input"
    );
    a.call(json!({"method":"request_access","params":{"client_name":"a"}}));
    broker.grant_session(a.id(), &b).unwrap();
    assert!(broker.grant_session(a.id(), &b).is_err());
    assert_eq!(
        a.call(json!({"method":"get_chapter","params":{"chapter_id":c},"session_id":"stolen"}))
            ["error"]["code"],
        "invalid_input"
    );
    assert_eq!(
        a.call(json!({"method":"get_outline","params":{"book_id":"other"}}))["error"]["code"],
        "invalid_input"
    );
    assert_eq!(
        a.call(json!({"method":"approve"}))["error"]["code"],
        "method_not_found"
    );
}
#[cfg(unix)]
#[test]
fn ipc_rejects_replaceable_ancestor_before_creating_directory() {
    use std::os::unix::fs::PermissionsExt;
    let (_, broker, _, _, _) = setup();
    let root = std::env::temp_dir().join(format!("mojuan-parent-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o777)).unwrap();
    let dir = root.join("private");
    assert!(novel_core::ipc::start_server(broker, &dir.join("a.sock")).is_err());
    assert!(
        !dir.exists(),
        "must reject unsafe ancestor before side effects"
    );
    std::fs::remove_dir(root).unwrap();
}
#[test]
fn material_list_is_readonly_and_rejects_invalid_pagination() {
    let (_, broker, b, _, _) = setup();
    let a = broker.connect().unwrap();
    a.call(json!({"method":"request_access","params":{"client_name":"materials"}}));
    broker.grant_session(a.id(), &b).unwrap();
    assert_eq!(
        a.call(json!({"method":"list_materials","params":{}}))["result"],
        json!({"items":[],"next_cursor":null})
    );
    for params in [
        json!({"limit":0}),
        json!({"limit":101}),
        json!({"limit":-1}),
        json!({"kind":"unknown"}),
        json!({"book_id":b}),
        json!({"after_id":"x".repeat(129)}),
    ] {
        assert_eq!(
            a.call(json!({"method":"list_materials","params":params}))["error"]["code"],
            "invalid_input"
        );
    }
    for method in [
        "create_material",
        "save_material",
        "trash_material",
        "restore_material",
    ] {
        assert_eq!(
            a.call(json!({"method":method,"params":{}}))["error"]["code"],
            "method_not_found"
        );
    }
}
fn card(s: &mut Service, book: &str, title: &str, chapter: Option<String>) -> novel_core::Material {
    s.create_material(novel_core::CreateMaterialInput {
        book_id: book.into(),
        kind: "outline".into(),
        title: title.into(),
        fields: std::collections::BTreeMap::from([(
            "content".into(),
            format!("private fields {title}"),
        )]),
        volume_id: None,
        chapter_id: chapter,
        source_chapter_id: None,
        source_revision: None,
        story_order: 0,
        story_time: None,
        tracking_status: None,
        payoff_chapter_id: None,
        payoff_revision: None,
    })
    .unwrap()
}
#[test]
fn material_metadata_pages_scope_and_recycle_chain() {
    let (store, broker, b, c, _) = setup();
    let (mut ids, foreign, trashed, linked) = {
        let mut s = store.lock().unwrap();
        let other = s
            .load()
            .unwrap()
            .books
            .into_iter()
            .find(|x| x.id != b)
            .unwrap()
            .id;
        let ids = (0..3)
            .map(|i| card(&mut s, &b, &format!("card{i}"), None).id)
            .collect::<Vec<_>>();
        let foreign = card(&mut s, &other, "foreign", None).id;
        let trashed = card(&mut s, &b, "trashed", None).id;
        s.trash_material(&trashed, 0).unwrap();
        let linked = card(&mut s, &b, "chapter-linked", Some(c.clone())).id;
        s.apply(Command::TrashChapter { id: c }).unwrap();
        (ids, foreign, trashed, linked)
    };
    ids.sort();
    let a = broker.connect().unwrap();
    a.call(json!({"method":"request_access","params":{"client_name":"card-read"}}));
    broker.grant_session(a.id(), &b).unwrap();
    let first = a.call(json!({"method":"list_materials","params":{"limit":2}}));
    assert_eq!(first["result"]["items"].as_array().unwrap().len(), 2);
    assert_eq!(first["result"]["items"][0]["id"], ids[0]);
    assert_eq!(first["result"]["next_cursor"], ids[1]);
    assert!(!first.to_string().contains("private fields"));
    assert!(first["result"]["items"][0].get("fields").is_none());
    let last = a.call(
        json!({"method":"list_materials","params":{"limit":2,"after_id":ids[1],"kind":"outline"}}),
    );
    assert_eq!(last["result"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(last["result"]["items"][0]["id"], ids[2]);
    assert!(last["result"]["next_cursor"].is_null());
    assert_eq!(
        a.call(json!({"method":"list_materials","params":{"kind":"character"}}))["result"]["items"],
        json!([])
    );
    for id in [foreign, trashed, linked, "absent".into()] {
        assert_eq!(
            a.call(json!({"method":"get_material","params":{"material_id":id}}))["error"]["code"],
            "not_found"
        );
    }
    assert!(
        a.call(json!({"method":"get_material","params":{"material_id":ids[0]}}))["result"]
            ["fields"]["content"]
            .as_str()
            .unwrap()
            .starts_with("private fields")
    );
    broker.revoke_session(a.id()).unwrap();
    assert_eq!(
        a.call(json!({"method":"list_materials"}))["error"]["code"],
        "invalid_session"
    );
    assert_eq!(
        a.call(json!({"method":"get_material","params":{"material_id":ids[0]}}))["error"]["code"],
        "invalid_session"
    );
}
#[test]
fn materials_default_page_limit_and_epoch_are_enforced() {
    let (store, broker, b, _, _) = setup();
    for n in 0..51 {
        card(&mut store.lock().unwrap(), &b, &format!("item{n}"), None);
    }
    let a = broker.connect().unwrap();
    a.call(json!({"method":"request_access","params":{"client_name":"paging"}}));
    broker.grant_session(a.id(), &b).unwrap();
    let page = a.call(json!({"method":"list_materials"}));
    assert_eq!(page["result"]["items"].as_array().unwrap().len(), 50);
    assert!(page["result"]["next_cursor"].is_string());
    let mut s = store.lock().unwrap();
    let backup = s.backup().unwrap();
    s.restore(&backup).unwrap();
    drop(s);
    assert_eq!(
        a.call(json!({"method":"list_materials"}))["error"]["code"],
        "invalid_session"
    );
}
#[test]
fn material_volume_parent_recycle_blocks_and_restore_reenables_read() {
    let (store, broker, b, c, _) = setup();
    let (volume, material) = {
        let mut s = store.lock().unwrap();
        let volume = s.get_chapter(&c).unwrap().volume_id;
        let material = s
            .create_material(novel_core::CreateMaterialInput {
                book_id: b.clone(),
                kind: "outline".into(),
                title: "volume scope".into(),
                fields: std::collections::BTreeMap::from([(
                    "content".into(),
                    "volume outline".into(),
                )]),
                volume_id: Some(volume.clone()),
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
            .id;
        (volume, material)
    };
    let a = broker.connect().unwrap();
    a.call(json!({"method":"request_access","params":{"client_name":"volume materials"}}));
    broker.grant_session(a.id(), &b).unwrap();
    store
        .lock()
        .unwrap()
        .apply(Command::TrashVolume { id: volume.clone() })
        .unwrap();
    assert_eq!(
        a.call(json!({"method":"list_materials"}))["result"]["items"],
        json!([])
    );
    assert_eq!(
        a.call(json!({"method":"get_material","params":{"material_id":material}}))["error"]["code"],
        "not_found"
    );
    store
        .lock()
        .unwrap()
        .apply(Command::RestoreVolume { id: volume })
        .unwrap();
    assert_eq!(
        a.call(json!({"method":"get_material","params":{"material_id":material}}))["result"]
            ["fields"]["content"],
        "volume outline"
    );
}
#[test]
fn material_reads_reject_scope_injection_and_recycled_authorized_book() {
    let (store, broker, b, _, _) = setup();
    let m = card(&mut store.lock().unwrap(), &b, "scope", None).id;
    let a = broker.connect().unwrap();
    a.call(json!({"method":"request_access","params":{"client_name":"scope guard"}}));
    broker.grant_session(a.id(), &b).unwrap();
    for (method, params) in [
        ("get_material", json!({"material_id":m,"book_id":b})),
        ("get_material", json!({"material_id":m,"client_id":"other"})),
        ("list_materials", json!({"include_deleted":true})),
    ] {
        assert_eq!(
            a.call(json!({"method":method,"params":params}))["error"]["code"],
            "invalid_input"
        );
    }
    store
        .lock()
        .unwrap()
        .apply(Command::TrashBook { id: b })
        .unwrap();
    assert!(a
        .call(json!({"method":"get_material","params":{"material_id":m}}))
        .get("error")
        .is_some());
    assert!(a
        .call(json!({"method":"list_materials"}))
        .get("error")
        .is_some());
}
#[test]
fn context_search_and_material_proposals_are_scoped_and_never_applied() {
    let (store, broker, book, _, _) = setup();
    let material = card(&mut store.lock().unwrap(), &book, "needle-local", None);
    let a = broker.connect().unwrap();
    a.call(json!({"method":"request_access","params":{"client_name":"context"}}));
    broker.grant_session(a.id(), &book).unwrap();
    let found = a.call(json!({"method":"search_book","params":{"query":"needle-local","limit":1}}));
    assert!(found.get("error").is_none(), "{found}");
    assert!(found.to_string().contains("needle-local"));
    for params in [
        json!({"query":""}),
        json!({"query":"x".repeat(257)}),
        json!({"query":"x","limit":0}),
        json!({"query":"x","limit":51}),
        json!({"query":"x","book_id":book}),
    ] {
        assert_eq!(
            a.call(json!({"method":"search_book","params":params}))["error"]["code"],
            "invalid_input"
        );
    }
    let args = json!({"material_id":material.id,"base_revision":0,"new_title":"suggested","new_fields":{"content":"suggested only"},"new_source_revision":null,"reason":"fixture","idempotency_key":"material-one"});
    let proposal = a.call(json!({"method":"propose_material_edit","params":args}));
    assert_eq!(proposal["result"]["status"], "pending", "{proposal}");
    assert_eq!(
        a.call(json!({"method":"propose_material_edit","params":args})),
        proposal
    );
    let id = proposal["result"]["id"].clone();
    assert_eq!(
        a.call(json!({"method":"get_material_proposal","params":{"proposal_id":id}})),
        proposal
    );
    let second = broker.connect().unwrap();
    second.call(json!({"method":"request_access","params":{"client_name":"other"}}));
    broker.grant_session(second.id(), &book).unwrap();
    assert_eq!(
        second.call(json!({"method":"get_material_proposal","params":{"proposal_id":id}}))["error"]
            ["code"],
        "not_found"
    );
    assert!(a
        .call(json!({"method":"approve_material_proposal","params":{"proposal_id":id}}))
        .get("error")
        .is_some());
    let unchanged = store
        .lock()
        .unwrap()
        .get_material(&material.id, false)
        .unwrap();
    assert_eq!(unchanged.revision, 0);
    assert_eq!(unchanged.title, "needle-local");
    assert_eq!(
        a.call(json!({"method":"cancel_material_proposal","params":{"proposal_id":id}}))["result"]
            ["status"],
        "cancelled"
    );
}

#[test]
fn summary_context_is_readable_only_in_granted_book() {
    let (store, broker, book, chapter, _) = setup();
    let summary = store
        .lock()
        .unwrap()
        .create_material(novel_core::CreateMaterialInput {
            book_id: book.clone(),
            kind: "summary".into(),
            title: "Chapter summary".into(),
            fields: std::collections::BTreeMap::from([("content".into(), "summary-text".into())]),
            volume_id: None,
            chapter_id: None,
            source_chapter_id: Some(chapter),
            source_revision: Some(0),
            story_order: 0,
            story_time: None,
            tracking_status: None,
            payoff_chapter_id: None,
            payoff_revision: None,
        })
        .unwrap();
    let a = broker.connect().unwrap();
    a.call(json!({"method":"request_access","params":{"client_name":"summary-reader"}}));
    assert_eq!(
        a.call(json!({"method":"search_book","params":{"query":"summary"}}))["error"]["code"],
        "access_pending"
    );
    broker.grant_session(a.id(), &book).unwrap();
    let list = a.call(json!({"method":"list_materials","params":{"kind":"summary"}}));
    assert_eq!(list["result"]["items"][0]["id"], summary.id);
    assert!(list["result"]["items"][0].get("fields").is_none());
    let args = json!({"material_id":summary.id,"base_revision":0,"new_title":"Suggested summary","new_fields":{"content":"suggested summary"},"new_source_revision":0,"reason":"Review source","idempotency_key":"summary-update"});
    assert_eq!(
        a.call(json!({"method":"propose_material_edit","params":args}))["result"]["status"],
        "pending"
    );
    let mut spoof = args;
    spoof["client_id"] = json!("someone-else");
    assert_eq!(
        a.call(json!({"method":"propose_material_edit","params":spoof}))["error"]["code"],
        "invalid_input"
    );
    assert_eq!(
        a.call(json!({"method":"search_book","params":{"query":"汉".repeat(86)}}))["error"]["code"],
        "invalid_input"
    );
}
fn planning_card(s: &mut Service, book: &str, kind: &str, title: &str) -> novel_core::Material {
    let fields = if kind == "timeline" {
        json!({"content":"planning secret"})
    } else {
        json!({"setup":"planning secret","resolution":""})
    };
    s.create_material(serde_json::from_value(json!({"book_id":book,"kind":kind,"title":title,"fields":fields,"volume_id":null,"chapter_id":null,"source_chapter_id":null,"source_revision":null,"story_order":4,"story_time":"Dawn","tracking_status":"planned","payoff_chapter_id":null,"payoff_revision":null})).unwrap()).unwrap()
}
#[test]
fn timeline_and_foreshadow_are_scoped_readonly_even_for_proposals() {
    let (store, broker, book, _, _) = setup();
    let (timeline, foreshadow, foreign) = {
        let mut s = store.lock().unwrap();
        let other = s
            .load()
            .unwrap()
            .books
            .into_iter()
            .find(|b| b.id != book)
            .unwrap()
            .id;
        (
            planning_card(&mut s, &book, "timeline", "Timeline fixture"),
            planning_card(&mut s, &book, "foreshadow", "Foreshadow fixture"),
            planning_card(&mut s, &other, "timeline", "Foreign planning fixture"),
        )
    };
    let a = broker.connect().unwrap();
    a.call(json!({"method":"request_access","params":{"client_name":"planning"}}));
    broker.grant_session(a.id(), &book).unwrap();
    for card in [&timeline, &foreshadow] {
        let page = a.call(json!({"method":"list_materials","params":{"kind":card.kind,"limit":1}}));
        assert_eq!(page["result"]["items"][0]["id"], card.id);
        assert!(page["result"]["items"][0].get("fields").is_none());
        let read = a.call(json!({"method":"get_material","params":{"material_id":card.id}}));
        assert_eq!(read["result"]["title"], card.title);
        // Broker must reject this type before core proposal validation/creation is reached.
        assert_eq!(a.call(json!({"method":"propose_material_edit","params":{"material_id":card.id,"base_revision":-1,"new_title":"Forbidden","new_fields":card.fields,"reason":"fixture","idempotency_key":"bad-revision"}}))["error"]["code"],"unsupported_operation");
        assert_eq!(a.call(json!({"method":"propose_material_edit","params":{"material_id":card.id,"base_revision":0,"new_title":"Forbidden","new_fields":card.fields,"reason":"fixture","idempotency_key":card.id}}))["error"]["code"],"unsupported_operation");
    }
    assert_eq!(a.call(json!({"method":"propose_material_edit","params":{"material_id":foreign.id,"base_revision":0,"new_title":"Forbidden","new_fields":foreign.fields,"reason":"fixture","idempotency_key":"foreign"}}))["error"]["code"],"not_found");
    let search = a.call(json!({"method":"search_book","params":{"query":"planning secret"}}));
    assert!(search.get("error").is_none());
    assert!(!search.to_string().contains("Foreign planning fixture"));
    let s = store.lock().unwrap();
    assert_eq!(s.get_material(&timeline.id, false).unwrap(), timeline);
    assert_eq!(s.get_material(&foreshadow.id, false).unwrap(), foreshadow);
    drop(s);
    broker.revoke_session(a.id()).unwrap();
    assert_eq!(
        a.call(json!({"method":"list_materials","params":{"kind":"timeline"}}))["error"]["code"],
        "invalid_session"
    );
    assert_eq!(
        a.call(json!({"method":"get_material","params":{"material_id":foreshadow.id}}))["error"]
            ["code"],
        "invalid_session"
    );
}
