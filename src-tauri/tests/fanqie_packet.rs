use novel_core::{Command, Service};

fn seed(s: &mut Service, name: &str) -> String {
    let p = s
        .prepare_txt_import(
            "第一卷 归途\n第一章 初见\n原稿\n第二章 再见\n下章".as_bytes(),
            name,
        )
        .unwrap();
    s.commit_txt_import(&p.id).unwrap().book_id
}

#[test]
fn exact_body_and_active_book_only() {
    let mut s = Service::open(":memory:").unwrap();
    let b = seed(&mut s, "灯塔.txt");
    let other = seed(&mut s, "船坞.txt");
    let index = s.fanqie_directory(&b).unwrap();
    let c = &index.chapters[0];
    assert_eq!(c.ordinal, 1);
    let body = "「灯亮了。」🙂\r\n\r\n雨停了。";
    s.apply(Command::SaveChapter {
        id: c.chapter_id.clone(),
        body: body.into(),
        expected_revision: c.revision,
    })
    .unwrap();
    assert!(s.fanqie_packet(&b, &index.epoch, c).is_err());
    let fresh = s.fanqie_directory(&b).unwrap();
    let current = &fresh.chapters[0];
    assert_eq!(
        s.fanqie_packet(&b, &fresh.epoch, current).unwrap().body,
        body
    );
    assert!(s.fanqie_packet(&b, "old-epoch", current).is_err());
    assert!(s.fanqie_packet(&other, &fresh.epoch, current).is_err());
}

#[test]
fn rechecks_rename_order_and_recycle_even_when_body_revision_is_unchanged() {
    let mut s = Service::open(":memory:").unwrap();
    let b = seed(&mut s, "灯塔.txt");
    let start = s.fanqie_directory(&b).unwrap();
    let c = &start.chapters[0];
    s.apply(Command::RenameChapter {
        id: c.chapter_id.clone(),
        title: "新题".into(),
    })
    .unwrap();
    assert!(s.fanqie_packet(&b, &start.epoch, c).is_err());
    let renamed = s.fanqie_directory(&b).unwrap();
    assert_eq!(renamed.chapters[0].revision, c.revision);
    s.apply(Command::PlaceChapter {
        id: c.chapter_id.clone(),
        volume_id: c.volume_id.clone(),
        before_id: None,
    })
    .unwrap();
    assert!(s
        .fanqie_packet(&b, &renamed.epoch, &renamed.chapters[0])
        .is_err());
    let moved = s.fanqie_directory(&b).unwrap();
    let c = &moved.chapters[1];
    assert_eq!(c.ordinal, 2);
    s.apply(Command::RenameVolume {
        id: c.volume_id.clone(),
        title: "改卷名".into(),
    })
    .unwrap();
    assert!(s.fanqie_packet(&b, &moved.epoch, c).is_err());
    let fresh = s.fanqie_directory(&b).unwrap();
    let c = &fresh.chapters[1];
    s.apply(Command::TrashChapter {
        id: c.chapter_id.clone(),
    })
    .unwrap();
    assert!(s.fanqie_packet(&b, &fresh.epoch, c).is_err());
}

#[test]
fn restoring_the_same_saved_text_still_invalidates_the_old_queue() {
    let dir = std::env::temp_dir().join(format!("fanqie-restore-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let mut s = Service::open(":memory:").unwrap();
    let b = seed(&mut s, "灯塔.txt");
    let captured = s.fanqie_directory(&b).unwrap();
    let path = dir.join("snapshot.sqlite");
    s.recovery_snapshot(&path).unwrap();
    let guard = s.restore_guard().unwrap();
    s.commit_prepared_restore(
        novel_core::prepare_restore(&path, |_| Ok(())).unwrap(),
        &guard,
        &dir.join("before.sqlite"),
        |_| Ok(()),
    )
    .unwrap();
    let current = s.fanqie_directory(&b).unwrap();
    assert_eq!(captured.chapters, current.chapters);
    assert_ne!(captured.epoch, current.epoch);
    assert!(s
        .fanqie_packet(&b, &captured.epoch, &captured.chapters[0])
        .is_err());
    assert!(s
        .fanqie_packet(&b, &current.epoch, &current.chapters[0])
        .is_ok());
    drop(s);
    std::fs::remove_dir_all(dir).unwrap();
}
