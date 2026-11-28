use novel_core::*;
use std::{
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
fn fixture() -> (Arc<Mutex<Service>>, String, String) {
    let mut s = Service::open(":memory:").unwrap();
    let state = s
        .apply(Command::CreateBook {
            title: "isolated runner fixture".into(),
            author: "".into(),
        })
        .unwrap();
    for n in 0..12 {
        s.apply(Command::CreateChapter {
            volume_id: state.volumes[0].id.clone(),
            title: format!("Chapter {n}"),
        })
        .unwrap();
    }
    (
        Arc::new(Mutex::new(s)),
        state.books[0].id.clone(),
        state.chapters[0].id.clone(),
    )
}
fn terminal(store: &Arc<Mutex<Service>>, book: &str, id: &str) -> TaskDetail {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let detail = store.lock().unwrap().get_context_task(book, id).unwrap();
        if !["queued", "running"].contains(&detail.task.status.as_str()) {
            return detail;
        }
        assert!(
            Instant::now() < end,
            "runner failed to reach terminal state"
        );
        thread::sleep(Duration::from_millis(2));
    }
}
#[test]
fn runner_completes_real_task_and_wakes_from_idle() {
    let (store, b, _) = fixture();
    let runner = TaskRunner::start(store.clone());
    for key in ["first", "second"] {
        let task = store.lock().unwrap().start_context_task(&b, key).unwrap();
        runner.notify();
        let d = terminal(&store, &b, &task.id);
        assert_eq!(d.task.status, "completed");
        assert_eq!(d.task.done, d.task.total);
        assert_eq!(d.items.len(), d.task.done as usize);
    }
    runner.shutdown();
    runner.shutdown();
    assert_eq!(runner.failure_code(), None);
}
#[test]
fn two_runners_share_atomic_steps_without_duplicate_progress() {
    let (store, b, _) = fixture();
    let task = store
        .lock()
        .unwrap()
        .start_context_task(&b, "race")
        .unwrap();
    let a = TaskRunner::start(store.clone());
    let c = TaskRunner::start(store.clone());
    let d = terminal(&store, &b, &task.id);
    assert_eq!(d.task.status, "completed");
    assert_eq!(d.items.len(), d.task.total as usize);
    assert_eq!(d.task.done, d.task.total);
    a.shutdown();
    c.shutdown();
}
#[test]
fn cancellation_at_locked_boundary_cannot_be_overwritten() {
    let (store, b, _) = fixture();
    let mut s = store.lock().unwrap();
    let task = s.start_context_task(&b, "cancel").unwrap();
    let runner = TaskRunner::start(store.clone());
    s.cancel_context_task(&b, &task.id).unwrap();
    drop(s);
    runner.notify();
    runner.shutdown();
    let d = store
        .lock()
        .unwrap()
        .get_context_task(&b, &task.id)
        .unwrap();
    assert_eq!(d.task.status, "cancelled");
    assert_eq!(d.task.done, 0);
}
#[test]
fn edit_at_boundary_invalidates_frozen_input() {
    let (store, b, c) = fixture();
    let mut s = store.lock().unwrap();
    let task = s.start_context_task(&b, "edit").unwrap();
    let runner = TaskRunner::start(store.clone());
    s.apply(Command::RenameChapter {
        id: c,
        title: "changed after task snapshot".into(),
    })
    .unwrap();
    drop(s);
    runner.notify();
    let d = terminal(&store, &b, &task.id);
    assert_ne!(d.task.status, "completed");
    assert!(d.task.reason_code.is_some());
    runner.shutdown();
}
#[test]
fn restore_while_runner_waits_does_not_recreate_old_task() {
    let (store, b, _) = fixture();
    let mut s = store.lock().unwrap();
    let backup = s.backup().unwrap();
    s.start_context_task(&b, "restore").unwrap();
    let runner = TaskRunner::start(store.clone());
    s.restore(&backup).unwrap();
    drop(s);
    runner.notify();
    runner.shutdown();
    assert!(store
        .lock()
        .unwrap()
        .list_context_tasks(&b)
        .unwrap()
        .is_empty());
}
#[test]
fn shutdown_is_bounded_even_when_another_thread_holds_service() {
    let (store, _, _) = fixture();
    let guard = store.lock().unwrap();
    let runner = TaskRunner::start(store.clone());
    let (tx, rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        runner.shutdown();
        runner.shutdown();
        tx.send(()).unwrap();
    });
    assert!(
        rx.recv_timeout(Duration::from_secs(2)).is_ok(),
        "shutdown waited on Service mutex"
    );
    drop(guard);
    worker.join().unwrap();
}
#[test]
fn poisoned_service_stops_runner_without_retry_loop_or_message_leak() {
    let (store, _, _) = fixture();
    let other = store.clone();
    let _ = thread::spawn(move || {
        let _g = other.lock().unwrap();
        panic!("synthetic private fixture content");
    })
    .join();
    let runner = TaskRunner::start(store);
    let end = Instant::now() + Duration::from_secs(2);
    while runner.failure_code().is_none() {
        assert!(Instant::now() < end);
        thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        runner.failure_code().as_deref(),
        Some("service_lock_poisoned")
    );
    assert_eq!(
        runner.check_health().unwrap_err(),
        "后台检查已停止，请重开应用恢复"
    );
    runner.shutdown();
}

#[test]
fn sql_error_stops_worker_and_notify_does_not_retry() {
    let dir = std::env::temp_dir().join(format!("mojuan-runner-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("fixture.db");
    let mut service = Service::open(&path).unwrap();
    let state = service
        .apply(Command::CreateBook {
            title: "isolated".into(),
            author: "".into(),
        })
        .unwrap();
    let book = &state.books[0].id;
    let task = service.start_context_task(book, "sql-failure").unwrap();
    let inspector = rusqlite::Connection::open(&path).unwrap();
    inspector.execute_batch("CREATE TRIGGER fixture_fail_step BEFORE INSERT ON context_task_items BEGIN SELECT RAISE(ABORT,'private fixture error text'); END;").unwrap();
    let store = Arc::new(Mutex::new(service));
    let runner = TaskRunner::start(store.clone());
    let end = Instant::now() + Duration::from_secs(3);
    while runner.failure_code().is_none() {
        assert!(Instant::now() < end);
        thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(runner.failure_code().as_deref(), Some("task_step_failed"));
    assert_eq!(
        runner.check_health().unwrap_err(),
        "后台检查已停止，请重开应用恢复"
    );
    inspector
        .execute_batch("DROP TRIGGER fixture_fail_step;")
        .unwrap();
    runner.notify();
    runner.shutdown();
    let d = store
        .lock()
        .unwrap()
        .get_context_task(book, &task.id)
        .unwrap();
    assert_eq!(d.task.done, 0);
    assert!(d.items.is_empty());
    drop(runner);
    drop(store);
    drop(inspector);
    // Startup recovers the durable unfinished task, but a fresh worker must not run it.
    let reopened = Arc::new(Mutex::new(Service::open(&path).unwrap()));
    assert_eq!(
        reopened
            .lock()
            .unwrap()
            .get_context_task(book, &task.id)
            .unwrap()
            .task
            .status,
        "interrupted"
    );
    let next = TaskRunner::start(reopened.clone());
    next.notify();
    next.shutdown();
    assert_eq!(
        reopened
            .lock()
            .unwrap()
            .get_context_task(book, &task.id)
            .unwrap()
            .task
            .status,
        "interrupted"
    );
    drop(next);
    drop(reopened);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn task_management_is_not_exposed_to_external_agents() {
    use serde_json::json;
    let (store, book, _) = fixture();
    let broker = Arc::new(novel_core::agent::AgentBroker::new(store.clone()));
    let connection = broker.connect().unwrap();
    connection
        .call(json!({"method":"request_access","params":{"client_name":"isolated task boundary"}}));
    broker.grant_session(connection.id(), &book).unwrap();
    let forbidden = [
        "start_context_task",
        "cancel_context_task",
        "resume_context_task",
        "retry_context_task",
        "step_next_task",
        "list_context_tasks",
        "get_context_task",
    ];
    for method in forbidden {
        assert_eq!(
            connection.call(json!({"method":method,"params":{"book_id":book}}))["error"]["code"],
            "method_not_found"
        );
    }
    let mut lines = vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
    ];
    for (i, method) in forbidden.iter().enumerate() {
        lines.push(json!({"jsonrpc":"2.0","id":i+3,"method":"tools/call","params":{"name":method,"arguments":{"book_id":book}}}));
    }
    let input = lines
        .into_iter()
        .map(|v| format!("{v}\n"))
        .collect::<String>();
    let mut output = Vec::new();
    novel_core::agent_stdio::run(std::io::Cursor::new(input), &mut output, true, |_| {
        panic!("forbidden task tool reached transport")
    })
    .unwrap();
    let responses = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        responses[1]["result"]["tools"].as_array().unwrap().len(),
        26
    );
    for tool in responses[1]["result"]["tools"].as_array().unwrap() {
        assert!(!forbidden.contains(&tool["name"].as_str().unwrap()));
    }
    for response in &responses[2..] {
        assert_eq!(response["error"]["code"], -32602);
    }
    assert!(store
        .lock()
        .unwrap()
        .list_context_tasks(&book)
        .unwrap()
        .is_empty());
}
