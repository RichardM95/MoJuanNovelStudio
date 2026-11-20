use novel_core::*;
use std::path::PathBuf;
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let d =
            std::env::temp_dir().join(format!("history-browser-fixture-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&d).unwrap();
        Self(d)
    }
    fn db(&self) -> PathBuf {
        self.0.join("library.sqlite")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn setup(f: &Fixture) -> (Service, LibraryState) {
    let mut s = Service::open(f.db()).unwrap();
    let a = s
        .apply(Command::CreateBook {
            title: "版本浏览虚构书".into(),
            author: "".into(),
        })
        .unwrap();
    (s, a)
}
fn save(s: &mut Service, c: &str, revision: i64) {
    s.apply(Command::SaveChapter {
        id: c.into(),
        body: format!("虚构版本正文{revision}"),
        expected_revision: revision,
    })
    .unwrap();
}
#[test]
fn metadata_paging_is_lazy_stable_and_scope_bound() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = &a.books[0].id;
    let c = &a.chapters[0].id;
    for rev in 0..5 {
        save(&mut s, c, rev);
    }
    let first = s.list_chapter_versions(b, c, None, 2, false).unwrap();
    assert_eq!(
        first.items.iter().map(|v| v.revision).collect::<Vec<_>>(),
        vec![5, 4]
    );
    assert!(serde_json::to_value(&first.items).unwrap()[0]
        .get("body")
        .is_none());
    save(&mut s, c, 5);
    let second = s
        .list_chapter_versions(b, c, first.next_cursor.as_deref(), 2, false)
        .unwrap();
    assert_eq!(
        second.items.iter().map(|v| v.revision).collect::<Vec<_>>(),
        vec![3, 2]
    );
    let detail = s.get_chapter_version(b, c, &first.items[0].id).unwrap();
    assert_eq!(detail.version.body, "虚构版本正文4");
    assert_eq!(detail.epoch, first.epoch);
    assert!(s
        .list_chapter_versions(b, c, first.next_cursor.as_deref(), 2, true)
        .is_err());
    assert!(s
        .list_chapter_versions(b, c, Some("invalid cursor"), 2, false)
        .is_err());
    for limit in [0, 101] {
        assert!(s.list_chapter_versions(b, c, None, limit, false).is_err());
    }
    let other = s
        .apply(Command::CreateBook {
            title: "别书".into(),
            author: "".into(),
        })
        .unwrap();
    let ob = &other.books.iter().find(|x| x.id != *b).unwrap().id;
    assert!(s.get_chapter_version(ob, c, &first.items[0].id).is_err());
}
#[test]
fn names_have_independent_cas_tombstones_and_do_not_change_manuscript_or_task_input() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = &a.books[0].id;
    let c = &a.chapters[0].id;
    save(&mut s, c, 0);
    let p = s.list_chapter_versions(b, c, None, 50, false).unwrap();
    let v = &p.items[0];
    let before = s.get_chapter(c).unwrap();
    let task = s.start_context_task(b, "name-only").unwrap();
    let named = s
        .set_chapter_version_label(b, c, &v.id, Some("  关键转折  ".into()), 0, &p.epoch)
        .unwrap();
    assert_eq!(named.label.as_deref(), Some("关键转折"));
    assert_eq!(named.label_revision, 1);
    assert!(s
        .set_chapter_version_label(b, c, &v.id, Some("旧请求".into()), 0, &p.epoch)
        .is_err());
    let cleared = s
        .set_chapter_version_label(b, c, &v.id, None, 1, &p.epoch)
        .unwrap();
    assert_eq!(cleared.label_revision, 2);
    assert!(cleared.label.is_none());
    assert!(s
        .set_chapter_version_label(b, c, &v.id, Some("ABA旧请求".into()), 0, &p.epoch)
        .is_err());
    assert!(s
        .list_chapter_versions(b, c, None, 50, true)
        .unwrap()
        .items
        .is_empty());
    assert_eq!(s.get_chapter(c).unwrap(), before);
    while s.step_next_task().unwrap().is_some() {}
    assert_eq!(
        s.get_context_task(b, &task.id).unwrap().task.status,
        "completed"
    );
}
#[test]
fn body_is_not_selected_or_decoded_for_metadata_and_label_operations() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = &a.books[0].id;
    let c = &a.chapters[0].id;
    save(&mut s, c, 0);
    let conn = rusqlite::Connection::open(f.db()).unwrap();
    conn.execute(
        "UPDATE chapter_versions SET body=zeroblob(10485760) WHERE chapter_id=?1",
        [c],
    )
    .unwrap();
    let p = s.list_chapter_versions(b, c, None, 50, false).unwrap();
    assert_eq!(p.items.len(), 2);
    s.set_chapter_version_label(
        b,
        c,
        &p.items[0].id,
        Some("元数据不读取大正文".into()),
        0,
        &p.epoch,
    )
    .unwrap();
    assert!(s.get_chapter_version(b, c, &p.items[0].id).is_err());
}
#[test]
fn epochs_scope_and_scoped_restore_prevent_old_or_wrong_selection() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = &a.books[0].id;
    let c = &a.chapters[0].id;
    save(&mut s, c, 0);
    save(&mut s, c, 1);
    let p = s.list_chapter_versions(b, c, None, 1, false).unwrap();
    let older = s
        .list_chapter_versions(b, c, p.next_cursor.as_deref(), 50, false)
        .unwrap();
    let id = &older.items[0].id;
    assert!(s
        .restore_chapter_version_scoped(b, c, id, 1, &p.epoch)
        .is_err());
    s.restore_chapter_version_scoped(b, c, id, 2, &p.epoch)
        .unwrap();
    assert_eq!(s.get_chapter(c).unwrap().revision, 3);
    let backup = s.backup().unwrap();
    s.restore(&backup).unwrap();
    assert!(s
        .list_chapter_versions(b, c, p.next_cursor.as_deref(), 50, false)
        .is_err());
    assert!(s
        .set_chapter_version_label(b, c, id, Some("过期时代".into()), 0, &p.epoch)
        .is_err());
    assert!(s
        .restore_chapter_version_scoped(b, c, id, 3, &p.epoch)
        .is_err());
}
#[test]
fn v7_label_roundtrip_and_old_import_reset_names_and_invalid_labels_do_not_mutate() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = &a.books[0].id;
    let c = &a.chapters[0].id;
    save(&mut s, c, 0);
    let p = s.list_chapter_versions(b, c, None, 50, false).unwrap();
    s.set_chapter_version_label(b, c, &p.items[0].id, Some("命名版本".into()), 0, &p.epoch)
        .unwrap();
    let before = s.backup().unwrap();
    let base: serde_json::Value = serde_json::from_str(&before).unwrap();
    assert_eq!(base["version"], 13);
    let epoch = s.epoch().unwrap();
    let mut cases = Vec::new();
    let mut missing = base.clone();
    missing.as_object_mut().unwrap().remove("version_labels");
    cases.push(missing);
    let mut dup = base.clone();
    let entry = dup["version_labels"][0].clone();
    dup["version_labels"].as_array_mut().unwrap().push(entry);
    cases.push(dup);
    for (key, value) in [
        ("version_id", serde_json::json!("missing")),
        ("label_revision", serde_json::json!(-1)),
        ("label_revision", serde_json::json!(9007199254740992i64)),
        ("label", serde_json::json!("bad\nname")),
    ] {
        let mut x = base.clone();
        x["version_labels"][0][key] = value;
        cases.push(x);
    }
    for x in cases {
        assert!(s.restore(&x.to_string()).is_err());
        assert_eq!(s.backup().unwrap(), before);
        assert_eq!(s.epoch().unwrap(), epoch);
    }
    s.restore(&before).unwrap();
    assert_eq!(
        s.list_chapter_versions(b, c, None, 50, true).unwrap().items[0]
            .label
            .as_deref(),
        Some("命名版本")
    );
    let mut old = base;
    old["version"] = serde_json::json!(6);
    old.as_object_mut().unwrap().remove("foreshadow_proposals");
    old.as_object_mut().unwrap().remove("version_labels");
    s.restore(&old.to_string()).unwrap();
    assert!(s
        .list_chapter_versions(b, c, None, 50, true)
        .unwrap()
        .items
        .is_empty());
}
#[test]
fn label_limits_concurrent_writers_and_clearing_preserve_all_versions() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = a.books[0].id.clone();
    let c = a.chapters[0].id.clone();
    save(&mut s, &c, 0);
    let page = s.list_chapter_versions(&b, &c, None, 50, false).unwrap();
    let id = page.items[0].id.clone();
    for label in [
        "x".repeat(121),
        "🙂".repeat(121),
        "bad\tname".into(),
        "bad\nname".into(),
    ] {
        assert!(s
            .set_chapter_version_label(&b, &c, &id, Some(label), 0, &page.epoch)
            .is_err());
    }
    let max_name = "🙂".repeat(120);
    let named = s
        .set_chapter_version_label(&b, &c, &id, Some(max_name.clone()), 0, &page.epoch)
        .unwrap();
    assert_eq!(named.label, Some(max_name));
    let shared = std::sync::Arc::new(std::sync::Mutex::new(s));
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let spawn = |label: &str| {
        let shared = shared.clone();
        let barrier = barrier.clone();
        let (b, c, id, epoch, label) = (
            b.clone(),
            c.clone(),
            id.clone(),
            page.epoch.clone(),
            label.to_string(),
        );
        std::thread::spawn(move || {
            barrier.wait();
            shared
                .lock()
                .unwrap()
                .set_chapter_version_label(&b, &c, &id, Some(label), 1, &epoch)
        })
    };
    let first = spawn("分支A");
    let second = spawn("分支B");
    barrier.wait();
    let outcomes = [first.join().unwrap(), second.join().unwrap()];
    assert_eq!(outcomes.iter().filter(|r| r.is_ok()).count(), 1);
    assert!(outcomes.iter().any(|r| r
        .as_ref()
        .err()
        .is_some_and(|e| e.code == "revision_conflict")));
    let mut s = shared.lock().unwrap();
    let cleared = s
        .set_chapter_version_label(&b, &c, &id, Some("   ".into()), 2, &page.epoch)
        .unwrap();
    assert_eq!(cleared.label_revision, 3);
    assert!(cleared.label.is_none());
    assert_eq!(s.chapter_history(&c).unwrap().len(), 2);
}
#[test]
fn unsafe_counter_and_failed_audit_cannot_partially_save_names() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = &a.books[0].id;
    let c = &a.chapters[0].id;
    save(&mut s, c, 0);
    let p = s.list_chapter_versions(b, c, None, 50, false).unwrap();
    let id = &p.items[0].id;
    let observer = rusqlite::Connection::open(f.db()).unwrap();
    observer.execute_batch("CREATE TRIGGER fail_label_audit BEFORE INSERT ON workflow_audit WHEN NEW.action='label_version' BEGIN SELECT RAISE(ABORT,'fixture audit failure'); END;").unwrap();
    assert!(s
        .set_chapter_version_label(b, c, id, Some("不能部分保存".into()), 0, &p.epoch)
        .is_err());
    let current = s.get_chapter_version(b, c, id).unwrap();
    assert!(current.label.is_none());
    assert_eq!(current.label_revision, 0);
    observer
        .execute_batch("DROP TRIGGER fail_label_audit;")
        .unwrap();
    let m = s
        .set_chapter_version_label(b, c, id, Some("保留名称".into()), 0, &p.epoch)
        .unwrap();
    assert_eq!(m.label_revision, 1);
    observer
        .execute(
            "UPDATE chapter_version_labels SET label_revision=?1 WHERE version_id=?2",
            rusqlite::params![9007199254740991i64, id],
        )
        .unwrap();
    assert!(s
        .set_chapter_version_label(
            b,
            c,
            id,
            Some("不能溢出".into()),
            9007199254740991,
            &p.epoch
        )
        .is_err());
    let current = s.get_chapter_version(b, c, id).unwrap();
    assert_eq!(current.label.as_deref(), Some("保留名称"));
    assert_eq!(current.label_revision, 9007199254740991);
}
#[test]
fn scope_and_recycle_refuse_all_new_history_actions_without_loading_foreign_body() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = &a.books[0].id;
    let c = &a.chapters[0].id;
    save(&mut s, c, 0);
    let first = s.list_chapter_versions(b, c, None, 50, false).unwrap();
    let all = s
        .apply(Command::CreateChapter {
            volume_id: a.volumes[0].id.clone(),
            title: "别章".into(),
        })
        .unwrap();
    let other = &all.chapters.iter().find(|x| x.id != *c).unwrap().id;
    assert!(s.get_chapter_version(b, other, &first.items[0].id).is_err());
    assert!(s
        .set_chapter_version_label(
            b,
            other,
            &first.items[0].id,
            Some("错章".into()),
            0,
            &first.epoch
        )
        .is_err());
    assert!(s
        .restore_chapter_version_scoped(b, other, &first.items[0].id, 0, &first.epoch)
        .is_err());
    s.apply(Command::TrashVolume {
        id: a.volumes[0].id.clone(),
    })
    .unwrap();
    assert!(s.list_chapter_versions(b, c, None, 50, false).is_err());
    assert!(s.get_chapter_version(b, c, &first.items[0].id).is_err());
    assert!(s
        .set_chapter_version_label(
            b,
            c,
            &first.items[0].id,
            Some("回收章".into()),
            0,
            &first.epoch
        )
        .is_err());
    assert!(s
        .restore_chapter_version_scoped(b, c, &first.items[0].id, 1, &first.epoch)
        .is_err());
}
#[test]
fn sqlite_snapshot_preserves_label_tombstone_and_purge_cascades_labels() {
    let f = Fixture::new();
    let (mut s, a) = setup(&f);
    let b = &a.books[0].id;
    let c = &a.chapters[0].id;
    save(&mut s, c, 0);
    let p = s.list_chapter_versions(b, c, None, 50, false).unwrap();
    let id = &p.items[0].id;
    s.set_chapter_version_label(b, c, id, Some("快照关键版本".into()), 0, &p.epoch)
        .unwrap();
    s.set_chapter_version_label(b, c, id, None, 1, &p.epoch)
        .unwrap();
    let path = s.recovery_snapshot(f.0.join("recovery.sqlite")).unwrap();
    let recovered = Service::open(path).unwrap();
    let copied = recovered.get_chapter_version(b, c, id).unwrap();
    assert!(copied.label.is_none());
    assert_eq!(copied.label_revision, 2);
    drop(recovered);
    s.apply(Command::TrashChapter { id: c.clone() }).unwrap();
    s.apply(Command::PurgeChapter { id: c.clone() }).unwrap();
    let observer = rusqlite::Connection::open(f.db()).unwrap();
    assert_eq!(
        observer
            .query_row("SELECT count(*) FROM chapter_version_labels", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}
#[test]
fn no_history_means_empty_not_an_implicit_checkpoint() {
    let f = Fixture::new();
    let (s, a) = setup(&f);
    let before = s.get_chapter(&a.chapters[0].id).unwrap();
    assert!(s
        .list_chapter_versions(&a.books[0].id, &a.chapters[0].id, None, 50, false)
        .unwrap()
        .items
        .is_empty());
    assert_eq!(s.get_chapter(&a.chapters[0].id).unwrap(), before);
    assert!(s.chapter_history(&a.chapters[0].id).unwrap().is_empty());
}
