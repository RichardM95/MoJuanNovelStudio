use novel_core::*;
fn setup() -> (Service, String, String) {
    let mut s = Service::open(":memory:").unwrap();
    let a = s
        .apply(Command::CreateBook {
            title: "虚构书".into(),
            author: "测试".into(),
        })
        .unwrap();
    (s, a.books[0].id.clone(), a.chapters[0].id.clone())
}
fn input(c: &str, key: &str, rev: i64, body: &str) -> ProposalInput {
    ProposalInput {
        client_id: "fixture-agent".into(),
        idempotency_key: key.into(),
        chapter_id: c.into(),
        base_revision: rev,
        new_body: body.into(),
        reason: "测试修改建议".into(),
    }
}
#[test]
fn proposals_are_immutable_idempotent_and_revision_bound() {
    let (mut s, b, c) = setup();
    let p = s.propose_edit(input(&c, "one", 0, "建议")).unwrap();
    assert_eq!(s.get_chapter(&c).unwrap().body, "");
    assert_eq!(s.propose_edit(input(&c, "one", 0, "建议")).unwrap(), p);
    assert_eq!(
        s.propose_edit(input(&c, "one", 0, "不同"))
            .unwrap_err()
            .code,
        "idempotency_conflict"
    );
    s.apply(Command::SaveChapter {
        id: c.clone(),
        body: "人工新稿".into(),
        expected_revision: 0,
    })
    .unwrap();
    assert_eq!(
        s.decide_proposal(&p.id, true, 1).unwrap_err().code,
        "revision_conflict"
    );
    assert_eq!(s.get_chapter(&c).unwrap().body, "人工新稿");
    assert_eq!(s.list_proposals(&b).unwrap()[0].status, "pending");
    s.decide_proposal(&p.id, false, 1).unwrap();
    assert_eq!(s.list_proposals(&b).unwrap()[0].status, "rejected");
    assert_eq!(
        s.propose_edit(input(&c, "one", 0, "建议")).unwrap().status,
        "rejected"
    );
}
#[test]
fn approval_and_history_restore_are_new_atomic_revisions() {
    let (mut s, b, c) = setup();
    s.apply(Command::SaveChapter {
        id: c.clone(),
        body: "人工".into(),
        expected_revision: 0,
    })
    .unwrap();
    let p = s.propose_edit(input(&c, "accept", 1, "采纳稿")).unwrap();
    s.decide_proposal(&p.id, true, 1).unwrap();
    let h = s.chapter_history(&c).unwrap();
    assert!(h.iter().any(|v| v.revision == 0 && v.body.is_empty()));
    assert!(h.iter().any(|v| v.revision == 1 && v.body == "人工"));
    assert!(h.iter().any(|v| v.revision == 2 && v.body == "采纳稿"));
    assert_eq!(s.list_proposals(&b).unwrap()[0].status, "accepted");
    let old = h.iter().find(|v| v.revision == 1).unwrap();
    assert_eq!(
        s.restore_chapter_version(&old.id, 1).unwrap_err().code,
        "revision_conflict"
    );
    s.restore_chapter_version(&old.id, 2).unwrap();
    assert_eq!(s.get_chapter(&c).unwrap().revision, 3);
    assert_eq!(s.get_chapter(&c).unwrap().body, "人工");
    assert!(s.decide_proposal(&p.id, true, 3).is_err());
}
#[test]
fn save_failure_rolls_back_text_history_and_audit() {
    let path = std::env::temp_dir().join(format!("workflow-{}.db", uuid::Uuid::new_v4()));
    let mut s = Service::open(&path).unwrap();
    let a = s
        .apply(Command::CreateBook {
            title: "测试".into(),
            author: "".into(),
        })
        .unwrap();
    let c = &a.chapters[0].id;
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_audit BEFORE INSERT ON workflow_audit BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
    assert!(s
        .apply(Command::SaveChapter {
            id: c.clone(),
            body: "不得保存".into(),
            expected_revision: 0
        })
        .is_err());
    assert_eq!(s.get_chapter(c).unwrap().revision, 0);
    assert!(s.chapter_history(c).unwrap().is_empty());
    drop(conn);
    drop(s);
    std::fs::remove_file(&path).unwrap();
}
#[test]
fn metadata_reads_do_not_include_bodies_and_recycle_blocks_workflow() {
    let (mut s, b, c) = setup();
    let outline = serde_json::to_value(s.get_outline(&b).unwrap()).unwrap();
    assert!(outline["chapters"][0].get("body").is_none());
    let p = s.propose_edit(input(&c, "trash", 0, "建议")).unwrap();
    s.apply(Command::TrashChapter { id: c.clone() }).unwrap();
    assert_eq!(s.get_chapter(&c).unwrap_err().code, "deleted");
    assert!(s.propose_edit(input(&c, "new", 0, "建议")).is_err());
    assert!(s.decide_proposal(&p.id, true, 0).is_err());
    s.apply(Command::RestoreChapter { id: c.clone() }).unwrap();
    assert_eq!(s.list_proposals(&b).unwrap().len(), 1);
    s.apply(Command::TrashBook { id: b.clone() }).unwrap();
    assert!(s.get_outline(&b).is_err());
    s.apply(Command::PurgeBook { id: b }).unwrap();
    assert!(s.chapter_history(&c).is_err());
}
#[test]
fn full_restore_rotates_epoch_clears_proposals_preserves_history() {
    let (mut s, b, c) = setup();
    s.apply(Command::SaveChapter {
        id: c.clone(),
        body: "保存稿".into(),
        expected_revision: 0,
    })
    .unwrap();
    let p = s.propose_edit(input(&c, "restore", 1, "建议")).unwrap();
    let epoch = s.epoch().unwrap();
    let backup = s.backup().unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&backup).unwrap()["version"],
        13
    );
    let before = s.chapter_history(&c).unwrap();
    s.restore(&backup).unwrap();
    assert_ne!(s.epoch().unwrap(), epoch);
    assert!(s.list_proposals(&b).unwrap().is_empty());
    assert_eq!(s.chapter_history(&c).unwrap(), before);
    assert!(s.decide_proposal(&p.id, true, 1).is_err());
    assert_ne!(
        s.propose_edit(input(&c, "restore", 1, "建议")).unwrap().id,
        p.id
    );
    let epoch = s.epoch().unwrap();
    let mut malformed: serde_json::Value = serde_json::from_str(&backup).unwrap();
    malformed["history"][0]["chapter_id"] = serde_json::json!("missing");
    assert!(s.restore(&malformed.to_string()).is_err());
    assert_eq!(s.epoch().unwrap(), epoch);
}
#[test]
fn second_writer_is_rejected_and_reopen_releases_lock() {
    let p = std::env::temp_dir().join(format!("writer-{}.db", uuid::Uuid::new_v4()));
    let s = Service::open(&p).unwrap();
    assert!(Service::open(&p).is_err());
    drop(s);
    let s = Service::open(&p).unwrap();
    drop(s);
    std::fs::remove_file(p).unwrap();
}
#[test]
fn valid_restore_can_reparent_existing_ids_without_losing_content() {
    let (mut s, oldbook, c) = setup();
    let next = s
        .apply(Command::CreateBook {
            title: "目的书".into(),
            author: "".into(),
        })
        .unwrap();
    let target = next
        .volumes
        .iter()
        .find(|v| v.book_id != oldbook)
        .unwrap()
        .id
        .clone();
    let mut json: serde_json::Value = serde_json::from_str(&s.backup().unwrap()).unwrap();
    json["books"]
        .as_array_mut()
        .unwrap()
        .retain(|b| b["id"] != oldbook);
    json["volumes"]
        .as_array_mut()
        .unwrap()
        .retain(|v| v["book_id"] != oldbook);
    for item in json["chapters"].as_array_mut().unwrap() {
        if item["id"] == c {
            item["volume_id"] = serde_json::json!(target);
            item["sort_order"] = serde_json::json!(1)
        }
    }
    s.restore(&json.to_string()).unwrap();
    assert_eq!(s.get_chapter(&c).unwrap().volume_id, target);
}
#[test]
fn metadata_changes_keep_proposals_and_purge_cascades_workflow_rows() {
    let path = std::env::temp_dir().join(format!("workflow-purge-{}.db", uuid::Uuid::new_v4()));
    let mut s = Service::open(&path).unwrap();
    let a = s
        .apply(Command::CreateBook {
            title: "fixture".into(),
            author: "".into(),
        })
        .unwrap();
    let c = &a.chapters[0].id;
    let b = &a.books[0].id;
    let p = s.propose_edit(input(c, "keep", 0, "建议")).unwrap();
    s.apply(Command::SaveChapter {
        id: c.clone(),
        body: "gui".into(),
        expected_revision: 0,
    })
    .unwrap();
    s.apply(Command::RenameChapter {
        id: c.clone(),
        title: "rename".into(),
    })
    .unwrap();
    s.apply(Command::TrashChapter { id: c.clone() }).unwrap();
    s.apply(Command::RestoreChapter { id: c.clone() }).unwrap();
    assert_eq!(s.list_proposals(b).unwrap()[0], p);
    assert_eq!(s.chapter_history(c).unwrap().len(), 2);
    s.apply(Command::TrashChapter { id: c.clone() }).unwrap();
    s.apply(Command::PurgeChapter { id: c.clone() }).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    for table in ["edit_proposals", "chapter_versions", "workflow_audit"] {
        assert_eq!(
            conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    drop(conn);
    drop(s);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn failed_acceptance_rolls_back_proposal_body_history_and_audit() {
    let path = std::env::temp_dir().join(format!("workflow-atomic-{}.db", uuid::Uuid::new_v4()));
    let mut s = Service::open(&path).unwrap();
    let a = s
        .apply(Command::CreateBook {
            title: "fixture".into(),
            author: "".into(),
        })
        .unwrap();
    let c = &a.chapters[0].id;
    let p = s.propose_edit(input(c, "atomic", 0, "建议")).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_accept BEFORE INSERT ON workflow_audit WHEN NEW.action='accept_proposal' BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
    assert!(s.decide_proposal(&p.id, true, 0).is_err());
    assert_eq!(s.get_chapter(c).unwrap().revision, 0);
    assert_eq!(
        s.list_proposals(&a.books[0].id).unwrap()[0].status,
        "pending"
    );
    assert!(s.chapter_history(c).unwrap().is_empty());
    assert_eq!(
        conn.query_row("SELECT count(*) FROM workflow_audit", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    drop(conn);
    drop(s);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn malformed_history_rejected_without_changing_epoch_or_state() {
    let (mut s, _, c) = setup();
    s.apply(Command::SaveChapter {
        id: c,
        body: "x".into(),
        expected_revision: 0,
    })
    .unwrap();
    let backup = s.backup().unwrap();
    let epoch = s.epoch().unwrap();
    for (field, value) in [
        ("revision", serde_json::json!(999)),
        ("source", serde_json::json!("")),
        ("id", serde_json::json!("")),
        ("created_at", serde_json::json!(-1)),
    ] {
        let mut json: serde_json::Value = serde_json::from_str(&backup).unwrap();
        json["history"][0][field] = value;
        assert!(s.restore(&json.to_string()).is_err());
        assert_eq!(s.backup().unwrap(), backup);
        assert_eq!(s.epoch().unwrap(), epoch);
    }
}
#[test]
fn audit_read_contains_no_body_and_tracks_human_decision() {
    let (mut s, _, c) = setup();
    let p = s
        .propose_edit(input(&c, "audit", 0, "正文秘密fixture"))
        .unwrap();
    s.decide_proposal(&p.id, true, 0).unwrap();
    let log = s.audit_log().unwrap();
    assert!(log.iter().any(|e| e.action == "accept_proposal"
        && e.source == "human"
        && e.chapter_id.as_deref() == Some(&c)
        && e.revision == Some(1)));
    let json = serde_json::to_string(&log).unwrap();
    assert!(!json.contains("正文秘密fixture"));
    assert!(!json.contains("body"));
}
#[test]
fn independent_process_writer_lock() {
    if let Ok(path) = std::env::var("MOJUAN_FIXTURE_LOCK_PATH") {
        let result = Service::open(path);
        assert_eq!(
            result.is_ok(),
            std::env::var("MOJUAN_FIXTURE_LOCK_EXPECT_OPEN").unwrap() == "yes"
        );
        return;
    }
    let path = std::env::temp_dir().join(format!("workflow-process-{}.db", uuid::Uuid::new_v4()));
    let s = Service::open(&path).unwrap();
    let probe = |expected: &str| {
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "independent_process_writer_lock"])
            .env("MOJUAN_FIXTURE_LOCK_PATH", &path)
            .env("MOJUAN_FIXTURE_LOCK_EXPECT_OPEN", expected)
            .output()
            .unwrap()
    };
    let locked = probe("no");
    assert!(
        locked.status.success(),
        "{}",
        String::from_utf8_lossy(&locked.stdout)
    );
    drop(s);
    let released = probe("yes");
    assert!(
        released.status.success(),
        "{}",
        String::from_utf8_lossy(&released.stdout)
    );
    std::fs::remove_file(path).unwrap();
}
#[test]
fn concurrent_approval_of_same_base_accepts_exactly_one() {
    let (mut s, b, c) = setup();
    let p = s.propose_edit(input(&c, "race-a", 0, "稿件A")).unwrap();
    let q = s.propose_edit(input(&c, "race-b", 0, "稿件B")).unwrap();
    let store = std::sync::Arc::new(std::sync::Mutex::new(s));
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let spawn = |id: String| {
        let store = store.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            store.lock().unwrap().decide_proposal(&id, true, 0)
        })
    };
    let first = spawn(p.id);
    let second = spawn(q.id);
    barrier.wait();
    let outcomes = [first.join().unwrap(), second.join().unwrap()];
    assert_eq!(outcomes.iter().filter(|r| r.is_ok()).count(), 1);
    assert!(outcomes.iter().any(|r| r
        .as_ref()
        .err()
        .is_some_and(|e| e.code == "revision_conflict")));
    let s = store.lock().unwrap();
    assert_eq!(s.get_chapter(&c).unwrap().revision, 1);
    let proposals = s.list_proposals(&b).unwrap();
    assert_eq!(
        proposals.iter().filter(|p| p.status == "accepted").count(),
        1
    );
    assert_eq!(
        proposals.iter().filter(|p| p.status == "pending").count(),
        1
    );
    assert_eq!(s.chapter_history(&c).unwrap().len(), 2);
}
#[test]
fn invalid_and_stale_proposals_never_create_rows() {
    let (mut s, b, c) = setup();
    let mut bad = input(&c, "bad", 0, "x");
    bad.reason = "x".repeat(4097);
    assert_eq!(s.propose_edit(bad).unwrap_err().code, "invalid_input");
    assert_eq!(
        s.propose_edit(input(&c, "stale", 1, "x")).unwrap_err().code,
        "revision_conflict"
    );
    assert!(s.list_proposals(&b).unwrap().is_empty());
    assert!(s.audit_log().unwrap().is_empty());
}
#[test]
fn cancel_only_own_pending_proposal_and_preserve_idempotency() {
    let (mut s, b, c) = setup();
    let p = s.propose_edit(input(&c, "cancel-own", 0, "建议")).unwrap();
    assert_eq!(
        s.cancel_own_proposal(&p.id, "other-agent")
            .unwrap_err()
            .code,
        "forbidden"
    );
    assert_eq!(s.list_proposals(&b).unwrap()[0].status, "pending");
    let cancelled = s.cancel_own_proposal(&p.id, "fixture-agent").unwrap();
    assert_eq!(cancelled.status, "cancelled");
    assert_eq!(s.get_chapter(&c).unwrap().revision, 0);
    assert!(s.chapter_history(&c).unwrap().is_empty());
    assert_eq!(
        s.propose_edit(input(&c, "cancel-own", 0, "建议")).unwrap(),
        cancelled
    );
    assert_eq!(
        s.cancel_own_proposal(&p.id, "fixture-agent")
            .unwrap_err()
            .code,
        "proposal_decided"
    );
    assert!(s.decide_proposal(&p.id, true, 0).is_err());
    let log = s.audit_log().unwrap();
    assert!(log
        .iter()
        .any(|e| e.action == "cancel_proposal" && e.source == "fixture-agent"));
    let accepted = s
        .propose_edit(input(&c, "already-accepted", 0, "采纳"))
        .unwrap();
    s.decide_proposal(&accepted.id, true, 0).unwrap();
    assert_eq!(
        s.cancel_own_proposal(&accepted.id, "fixture-agent")
            .unwrap_err()
            .code,
        "proposal_decided"
    );
}
#[test]
fn cancellation_audit_failure_rolls_back_status() {
    let path = std::env::temp_dir().join(format!("workflow-cancel-{}.db", uuid::Uuid::new_v4()));
    let mut s = Service::open(&path).unwrap();
    let a = s
        .apply(Command::CreateBook {
            title: "fixture".into(),
            author: "".into(),
        })
        .unwrap();
    let p = s
        .propose_edit(input(&a.chapters[0].id, "cancel-fail", 0, "建议"))
        .unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TRIGGER cancel_fail BEFORE INSERT ON workflow_audit WHEN NEW.action='cancel_proposal' BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
    assert!(s.cancel_own_proposal(&p.id, "fixture-agent").is_err());
    assert_eq!(
        s.list_proposals(&a.books[0].id).unwrap()[0].status,
        "pending"
    );
    drop(conn);
    drop(s);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn old_proposal_status_constraint_migrates_without_losing_rows_or_keys() {
    let path = std::env::temp_dir().join(format!(
        "workflow-status-migration-{}.db",
        uuid::Uuid::new_v4()
    ));
    let mut s = Service::open(&path).unwrap();
    let a = s
        .apply(Command::CreateBook {
            title: "fixture".into(),
            author: "".into(),
        })
        .unwrap();
    let c = &a.chapters[0].id;
    let p = s
        .propose_edit(input(c, "legacy-status", 0, "建议"))
        .unwrap();
    drop(s);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("ALTER TABLE edit_proposals RENAME TO edit_proposals_fixture;
 CREATE TABLE edit_proposals(id TEXT PRIMARY KEY,client_id TEXT NOT NULL,idempotency_key TEXT NOT NULL,payload TEXT NOT NULL,chapter_id TEXT NOT NULL REFERENCES chapters(id) ON DELETE CASCADE,base_revision INTEGER NOT NULL,old_body TEXT NOT NULL,new_body TEXT NOT NULL,reason TEXT NOT NULL,source TEXT NOT NULL,status TEXT NOT NULL CHECK(status IN ('pending','accepted','rejected')),created_at INTEGER NOT NULL,UNIQUE(client_id,idempotency_key));
 INSERT INTO edit_proposals SELECT * FROM edit_proposals_fixture;DROP TABLE edit_proposals_fixture;CREATE INDEX proposal_chapter ON edit_proposals(chapter_id);").unwrap();
    drop(conn);
    let mut s = Service::open(&path).unwrap();
    assert_eq!(
        s.propose_edit(input(c, "legacy-status", 0, "建议"))
            .unwrap(),
        p
    );
    assert_eq!(
        s.cancel_own_proposal(&p.id, "fixture-agent")
            .unwrap()
            .status,
        "cancelled"
    );
    drop(s);
    let s = Service::open(&path).unwrap();
    assert_eq!(
        s.list_proposals(&a.books[0].id).unwrap()[0].status,
        "cancelled"
    );
    drop(s);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn stale_proposal_can_be_rejected_without_stale_text_overwrite() {
    let (mut s, b, c) = setup();
    let p = s
        .propose_edit(input(&c, "stale-reject", 0, "建议"))
        .unwrap();
    s.apply(Command::SaveChapter {
        id: c.clone(),
        body: "最新人工正文".into(),
        expected_revision: 0,
    })
    .unwrap();
    s.decide_proposal(&p.id, false, 0).unwrap();
    assert_eq!(s.get_chapter(&c).unwrap().body, "最新人工正文");
    assert_eq!(s.get_chapter(&c).unwrap().revision, 1);
    assert_eq!(s.list_proposals(&b).unwrap()[0].status, "rejected");
}
#[test]
fn point_proposal_lookup_checks_client_identity() {
    let (mut s, _, c) = setup();
    let p = s.propose_edit(input(&c, "point", 0, "建议")).unwrap();
    assert_eq!(s.get_own_proposal(&p.id, "fixture-agent").unwrap(), p);
    assert_eq!(
        s.get_own_proposal(&p.id, "other").unwrap_err().code,
        "forbidden"
    );
    assert_eq!(
        s.get_own_proposal("missing", "fixture-agent")
            .unwrap_err()
            .code,
        "not_found"
    );
}
#[test]
fn pending_proposal_limits_and_retries_do_not_block_canonical_saves() {
    let (mut s, b, c) = setup();
    let outline = s.get_outline(&b).unwrap();
    let v = &outline.volumes[0].id;
    let mut chapters = vec![c.clone()];
    for n in 0..5 {
        let a = s
            .apply(Command::CreateChapter {
                volume_id: v.clone(),
                title: format!("章{n}"),
            })
            .unwrap();
        chapters.push(a.chapters.last().unwrap().id.clone());
    }
    for chapter in chapters.iter().take(5) {
        for n in 0..20 {
            s.propose_edit(input(chapter, &format!("{chapter}-{n}"), 0, "建议"))
                .unwrap();
        }
    }
    assert_eq!(
        s.propose_edit(input(&c, &format!("{c}-0"), 0, "建议"))
            .unwrap()
            .status,
        "pending"
    );
    assert_eq!(
        s.propose_edit(input(&c, "chapter-full", 0, "建议"))
            .unwrap_err()
            .code,
        "limit_exceeded"
    );
    assert_eq!(
        s.propose_edit(input(&chapters[5], "global-full", 0, "建议"))
            .unwrap_err()
            .code,
        "limit_exceeded"
    );
    s.apply(Command::SaveChapter {
        id: c.clone(),
        body: "仍可正常写作".into(),
        expected_revision: 0,
    })
    .unwrap();
    assert_eq!(s.get_chapter(&c).unwrap().revision, 1);
}
#[test]
fn total_proposal_count_is_bounded_even_after_cancellation() {
    let (mut s, _, c) = setup();
    for n in 0..1000 {
        let p = s
            .propose_edit(input(&c, &format!("quota-{n}"), 0, "x"))
            .unwrap();
        s.cancel_own_proposal(&p.id, "fixture-agent").unwrap();
    }
    assert_eq!(
        s.propose_edit(input(&c, "quota-0", 0, "x")).unwrap().status,
        "cancelled"
    );
    assert_eq!(
        s.propose_edit(input(&c, "quota-over", 0, "x"))
            .unwrap_err()
            .code,
        "limit_exceeded"
    );
}
#[test]
fn proposal_body_byte_quota_preserves_retries_and_never_deletes_canonical_text() {
    let (mut s, _, c) = setup();
    let body = "x".repeat(8 * 1024 * 1024);
    s.apply(Command::SaveChapter {
        id: c.clone(),
        body: body.clone(),
        expected_revision: 0,
    })
    .unwrap();
    let first = s.propose_edit(input(&c, "bytes-one", 1, &body)).unwrap();
    s.propose_edit(input(&c, "bytes-two", 1, &body)).unwrap();
    assert_eq!(
        s.propose_edit(input(&c, "bytes-one", 1, &body)).unwrap(),
        first
    );
    assert_eq!(
        s.propose_edit(input(&c, "bytes-over", 1, ""))
            .err()
            .map(|e| e.code)
            .as_deref(),
        Some("limit_exceeded")
    );
    s.cancel_own_proposal(&first.id, "fixture-agent").unwrap();
    s.apply(Command::SaveChapter {
        id: c.clone(),
        body: "仍可保存".into(),
        expected_revision: 1,
    })
    .unwrap();
    assert_eq!(s.get_chapter(&c).unwrap().body, "仍可保存");
    assert_eq!(
        s.propose_edit(input(&c, "bytes-after-cancel", 2, "x"))
            .err()
            .map(|e| e.code)
            .as_deref(),
        Some("limit_exceeded")
    );
}
