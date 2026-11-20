// Standalone cross-engine evidence helper. Compile against the respective
// unmodified old/new novel_core and rusqlite artifacts; see SQLITE-MAINTENANCE.md.
use novel_core::*;
use std::{collections::BTreeMap, path::Path};
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = &args[1];
    let root = Path::new(&args[2]);
    println!(
        "engine={} source={}",
        rusqlite::version(),
        rusqlite::Connection::open_in_memory()
            .unwrap()
            .query_row("select sqlite_source_id()", [], |r| r.get::<_, String>(0))
            .unwrap()
    );
    if mode == "create" {
        std::fs::create_dir(root).unwrap();
        let mut s = Service::open(root.join("live.sqlite")).unwrap();
        let state = s
            .apply(Command::CreateBook {
                title: "旧引擎迁移虚构书".into(),
                author: "隔离测试".into(),
            })
            .unwrap();
        let b = &state.books[0].id;
        let c = &state.chapters[0].id;
        for (rev, body) in ["原稿\r\n月印🌙", "二稿\r\n保留线索🌙"]
            .into_iter()
            .enumerate()
        {
            s.apply(Command::SaveChapter {
                id: c.clone(),
                body: body.into(),
                expected_revision: rev as i64,
            })
            .unwrap();
        }
        s.create_material(CreateMaterialInput {
            book_id: b.clone(),
            kind: "setting".into(),
            title: "旧设定".into(),
            fields: BTreeMap::from([("synopsis".into(), "不可擅改原始证据".into())]),
            volume_id: None,
            chapter_id: None,
            source_chapter_id: None,
            source_revision: None,
            story_order: 0,
            story_time: None,
            tracking_status: None,
            payoff_chapter_id: None,
            payoff_revision: None,
        })
        .unwrap();
        let p = s
            .propose_edit(ProposalInput {
                client_id: "isolated-old-agent".into(),
                idempotency_key: "old-pending".into(),
                chapter_id: c.clone(),
                base_revision: 2,
                new_body: "未采纳候选".into(),
                reason: "保留待审".into(),
            })
            .unwrap();
        assert_eq!(p.status, "pending");
        let h = s.chapter_history(c).unwrap();
        s.set_chapter_version_label(
            b,
            c,
            &h[1].id,
            Some("旧版本标签".into()),
            0,
            &s.epoch().unwrap(),
        )
        .unwrap();
        std::fs::write(root.join("expected.json"), s.backup().unwrap()).unwrap();
        s.export_sqlite_backup(&root.join("old-backup.sqlite"), |_| Ok(()))
            .unwrap();
        drop(s);
    } else {
        let expected = std::fs::read_to_string(root.join("expected.json")).unwrap();
        let mut s = Service::open(root.join("live.sqlite")).unwrap();
        assert_eq!(
            s.backup().unwrap(),
            expected,
            "new engine must preserve exported old library data and IDs"
        );
        let a = s.load().unwrap();
        let c = a.chapters[0].id.clone();
        assert_eq!(
            s.list_proposals(&a.books[0].id).unwrap()[0].status,
            "pending"
        );
        let old = s.chapter_history(&c).unwrap();
        s.apply(Command::SaveChapter {
            id: c.clone(),
            body: "升级后续写\r\n🌙".into(),
            expected_revision: 2,
        })
        .unwrap();
        let h = s.chapter_history(&c).unwrap();
        assert_eq!(&h[..old.len()], old);
        assert_eq!(h.last().unwrap().revision, 3);
        drop(s);
        let s = Service::open(root.join("live.sqlite")).unwrap();
        assert_eq!(s.get_chapter(&c).unwrap().body, "升级后续写\r\n🌙");
        assert_eq!(s.chapter_history(&c).unwrap(), h);
        drop(s);
        let mut dest = Service::open(root.join("restored.sqlite")).unwrap();
        let prepared = prepare_restore(&root.join("old-backup.sqlite"), |_| Ok(())).unwrap();
        let guard = dest.restore_guard().unwrap();
        dest.commit_prepared_restore(
            prepared,
            &guard,
            &root.join("restore-before.sqlite"),
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(dest.get_chapter(&c).unwrap().body, "二稿\r\n保留线索🌙");
        assert_eq!(dest.chapter_history(&c).unwrap(), old);
        let state = dest.load().unwrap();
        assert_eq!(state.books[0].title, "旧引擎迁移虚构书");
        // Canonical restore deliberately excludes legacy edit proposals.
        assert!(dest.list_proposals(&state.books[0].id).unwrap().is_empty());
        drop(dest);
        println!("old live preserved, continued writing reopened, old backup restored");
    }
}
