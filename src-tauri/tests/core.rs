use novel_core::*;
fn new_book(s: &mut Service) -> LibraryState {
    s.apply(Command::CreateBook {
        title: "长夜".into(),
        author: "作者".into(),
    })
    .unwrap()
}
#[test]
fn lifecycle_revision_backup_export() {
    let mut s = Service::open(":memory:").unwrap();
    let a = new_book(&mut s);
    let b = &a.books[0].id;
    let c = &a.chapters[0].id;
    s.apply(Command::SaveChapter {
        id: c.clone(),
        body: "你好\n世界".into(),
        expected_revision: 0,
    })
    .unwrap();
    assert!(s
        .apply(Command::SaveChapter {
            id: c.clone(),
            body: "旧文".into(),
            expected_revision: 0
        })
        .is_err());
    let txt = s.export_book(b).unwrap();
    assert!(txt.contains("你好\r\n世界"));
    let backup = s.backup().unwrap();
    s.apply(Command::TrashBook { id: b.clone() }).unwrap();
    assert!(s.load().unwrap().books[0].deleted_at.is_some());
    s.apply(Command::RestoreBook { id: b.clone() }).unwrap();
    assert!(s.load().unwrap().books[0].deleted_at.is_none());
    let before = s.backup().unwrap();
    assert!(s.restore("{\"version\":1,\"books\":[],\"volumes\":[],\"chapters\":[{\"id\":\"x\",\"volume_id\":\"missing\",\"title\":\"x\",\"body\":\"\",\"revision\":0,\"sort_order\":0}]}").is_err());
    assert_eq!(before, s.backup().unwrap());
    s.restore(&backup).unwrap();
    assert_eq!(backup, s.backup().unwrap());
}
#[test]
fn reopen_and_order() {
    let p = std::env::temp_dir().join(format!("novel-test-{}.sqlite", uuid::Uuid::new_v4()));
    let mut s = Service::open(&p).unwrap();
    let a = new_book(&mut s);
    let v = a.volumes[0].id.clone();
    s.apply(Command::CreateChapter {
        volume_id: v,
        title: "二".into(),
    })
    .unwrap();
    let a = s.load().unwrap();
    let id = a.chapters[1].id.clone();
    s.apply(Command::MoveChapter {
        id: id.clone(),
        direction: -1,
    })
    .unwrap();
    drop(s);
    let s = Service::open(&p).unwrap();
    assert_eq!(s.load().unwrap().chapters[0].id, id);
    drop(s);
    std::fs::remove_file(p).unwrap();
}
#[test]
fn edit_and_volume_order_and_duplicate_restore() {
    let mut s = Service::open(":memory:").unwrap();
    let a = new_book(&mut s);
    let b = a.books[0].id.clone();
    s.apply(Command::UpdateBook {
        id: b.clone(),
        title: "新书".into(),
        author: "新作者".into(),
    })
    .unwrap();
    s.apply(Command::CreateVolume {
        book_id: b,
        title: "后卷".into(),
    })
    .unwrap();
    let a = s.load().unwrap();
    let v = a.volumes[1].id.clone();
    s.apply(Command::RenameVolume {
        id: v.clone(),
        title: "前卷".into(),
    })
    .unwrap();
    s.apply(Command::MoveVolume {
        id: v.clone(),
        direction: -1,
    })
    .unwrap();
    assert_eq!(s.load().unwrap().volumes[0].id, v);
    let a = s.load().unwrap();
    let c = a.chapters[0].id.clone();
    s.apply(Command::RenameChapter {
        id: c,
        title: "改章".into(),
    })
    .unwrap();
    let backup = s.backup().unwrap();
    let mut json: serde_json::Value = serde_json::from_str(&backup).unwrap();
    let duplicate = json["books"][0].clone();
    json["books"].as_array_mut().unwrap().push(duplicate);
    assert!(s.restore(&json.to_string()).is_err());
    assert_eq!(s.backup().unwrap(), backup);
}
#[test]
fn transfer_chapter_between_volumes() {
    let mut s = Service::open(":memory:").unwrap();
    let a = new_book(&mut s);
    let source = a.volumes[0].id.clone();
    let chapter = a.chapters[0].id.clone();
    s.apply(Command::CreateChapter {
        volume_id: source.clone(),
        title: "留在原卷".into(),
    })
    .unwrap();
    s.apply(Command::CreateVolume {
        book_id: a.books[0].id.clone(),
        title: "目的卷".into(),
    })
    .unwrap();
    let a = s.load().unwrap();
    let target = a
        .volumes
        .iter()
        .find(|v| v.id != source)
        .unwrap()
        .id
        .clone();
    s.apply(Command::CreateChapter {
        volume_id: target.clone(),
        title: "目的卷已有章".into(),
    })
    .unwrap();
    let before = s.backup().unwrap();
    assert!(s
        .apply(Command::TransferChapter {
            id: chapter.clone(),
            volume_id: "missing".into()
        })
        .is_err());
    assert_eq!(before, s.backup().unwrap());
    let a = s
        .apply(Command::TransferChapter {
            id: chapter.clone(),
            volume_id: target.clone(),
        })
        .unwrap();
    let c = a.chapters.iter().find(|c| c.id == chapter).unwrap();
    assert_eq!(c.volume_id, target);
    assert_eq!(c.sort_order, 1);
    assert_eq!(
        a.chapters
            .iter()
            .find(|c| c.volume_id == source)
            .unwrap()
            .sort_order,
        0
    );
    let a = s
        .apply(Command::TransferChapter {
            id: chapter.clone(),
            volume_id: target,
        })
        .unwrap();
    assert_eq!(
        a.chapters
            .iter()
            .find(|c| c.id == chapter)
            .unwrap()
            .sort_order,
        1
    );
}
#[test]
fn saving_does_not_rewrite_other_records() {
    let p = std::env::temp_dir().join(format!("novel-save-test-{}.sqlite", uuid::Uuid::new_v4()));
    let mut s = Service::open(&p).unwrap();
    let a = new_book(&mut s);
    let observer = rusqlite::Connection::open(&p).unwrap();
    observer.execute_batch("CREATE TRIGGER reject_book_delete BEFORE DELETE ON books BEGIN SELECT RAISE(ABORT,'must not rewrite books'); END; CREATE TRIGGER reject_chapter_delete BEFORE DELETE ON chapters BEGIN SELECT RAISE(ABORT,'must not rewrite chapters'); END;").unwrap();
    let c = a.chapters[0].id.clone();
    let a = s
        .apply(Command::SaveChapter {
            id: c.clone(),
            body: "只更新正文".into(),
            expected_revision: 0,
        })
        .unwrap();
    assert_eq!(a.chapters[0].body, "只更新正文");
    assert_eq!(a.chapters[0].revision, 1);
    assert!(s
        .apply(Command::SaveChapter {
            id: c,
            body: "过期".into(),
            expected_revision: 0
        })
        .is_err());
    assert_eq!(s.load().unwrap().chapters[0].body, "只更新正文");
    drop(observer);
    drop(s);
    std::fs::remove_file(p).unwrap();
}
#[test]
fn restored_maximum_order_returns_error_without_panicking() {
    let mut s = Service::open(":memory:").unwrap();
    let a = new_book(&mut s);
    let mut backup: serde_json::Value = serde_json::from_str(&s.backup().unwrap()).unwrap();
    backup["volumes"][0]["sort_order"] = serde_json::json!(i64::MAX);
    backup["chapters"][0]["sort_order"] = serde_json::json!(i64::MAX);
    s.restore(&backup.to_string()).unwrap();
    let before = s.backup().unwrap();
    assert!(s
        .apply(Command::CreateVolume {
            book_id: a.books[0].id.clone(),
            title: "溢出卷".into()
        })
        .is_err());
    assert!(s
        .apply(Command::CreateChapter {
            volume_id: a.volumes[0].id.clone(),
            title: "溢出章".into()
        })
        .is_err());
    assert_eq!(s.backup().unwrap(), before);
}
#[test]
fn oversized_backup_is_rejected() {
    let p = std::env::temp_dir().join(format!("novel-size-test-{}.sqlite", uuid::Uuid::new_v4()));
    let mut s = Service::open(&p).unwrap();
    let a = new_book(&mut s);
    let conn = rusqlite::Connection::open(&p).unwrap();
    // JSON escaping expands each legal 6MiB body to 36MiB, exceeding the
    // portable-backup cap with just two individually valid chapters.
    let body = "\u{0001}".repeat(6 * 1024 * 1024);
    conn.execute("UPDATE chapters SET body=?1", [&body])
        .unwrap();
    conn.execute(
        "INSERT INTO chapters(id,volume_id,title,body,revision,sort_order) VALUES('second',?1,'second',?2,0,1)",
        rusqlite::params![a.volumes[0].id, body],
    )
    .unwrap();
    match s.backup() {
        Err(e) => assert!(e.contains("64")),
        Ok(_) => panic!("oversized backup must be rejected"),
    };
    drop(conn);
    drop(s);
    std::fs::remove_file(p).unwrap();
}
#[test]
fn place_chapter_reorders_within_volume() {
    let mut s = Service::open(":memory:").unwrap();
    let a = new_book(&mut s);
    let v = a.volumes[0].id.clone();
    let first = a.chapters[0].id.clone();
    for title in ["二", "三"] {
        s.apply(Command::CreateChapter {
            volume_id: v.clone(),
            title: title.into(),
        })
        .unwrap();
    }
    let a = s.load().unwrap();
    let third = a
        .chapters
        .iter()
        .find(|c| c.title == "三")
        .unwrap()
        .id
        .clone();
    let a = s
        .apply(Command::PlaceChapter {
            id: third.clone(),
            volume_id: v.clone(),
            before_id: Some(first),
        })
        .unwrap();
    let ordered: Vec<_> = a
        .chapters
        .iter()
        .filter(|c| c.volume_id == v)
        .collect();
    let mut ordered = ordered;
    ordered.sort_by_key(|c| c.sort_order);
    assert_eq!(
        ordered.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(),
        vec!["三", "初章", "二"]
    );
    assert_eq!(
        ordered.iter().map(|c| c.sort_order).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    let a = s
        .apply(Command::PlaceChapter {
            id: third,
            volume_id: v.clone(),
            before_id: None,
        })
        .unwrap();
    let mut ordered: Vec<_> = a.chapters.iter().filter(|c| c.volume_id == v).collect();
    ordered.sort_by_key(|c| c.sort_order);
    assert_eq!(
        ordered.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(),
        vec!["初章", "二", "三"]
    );
}

#[test]
fn place_chapter_moves_across_volumes() {
    let mut s = Service::open(":memory:").unwrap();
    let a = new_book(&mut s);
    let v1 = a.volumes[0].id.clone();
    let first = a.chapters[0].id.clone();
    s.apply(Command::CreateVolume {
        book_id: a.books[0].id.clone(),
        title: "远方".into(),
    })
    .unwrap();
    let a = s.load().unwrap();
    let v2 = a
        .volumes
        .iter()
        .find(|v| v.id != v1)
        .unwrap()
        .id
        .clone();
    for title in ["甲", "乙"] {
        s.apply(Command::CreateChapter {
            volume_id: v2.clone(),
            title: title.into(),
        })
        .unwrap();
    }
    let a = s.load().unwrap();
    let second = a
        .chapters
        .iter()
        .find(|c| c.title == "乙")
        .unwrap()
        .id
        .clone();
    let a = s
        .apply(Command::PlaceChapter {
            id: first.clone(),
            volume_id: v2.clone(),
            before_id: Some(second),
        })
        .unwrap();
    let mut target: Vec<_> = a.chapters.iter().filter(|c| c.volume_id == v2).collect();
    target.sort_by_key(|c| c.sort_order);
    assert_eq!(
        target.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(),
        vec!["甲", "初章", "乙"]
    );
    assert_eq!(
        target.iter().map(|c| c.sort_order).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert!(a.chapters.iter().all(|c| c.volume_id != v1));
    let a = s
        .apply(Command::PlaceChapter {
            id: first,
            volume_id: v2.clone(),
            before_id: None,
        })
        .unwrap();
    let mut target: Vec<_> = a.chapters.iter().filter(|c| c.volume_id == v2).collect();
    target.sort_by_key(|c| c.sort_order);
    assert_eq!(
        target.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(),
        vec!["甲", "乙", "初章"]
    );
}

#[test]
fn place_chapter_rejects_invalid_targets() {
    let mut s = Service::open(":memory:").unwrap();
    let a = new_book(&mut s);
    let book = a.books[0].id.clone();
    let v1 = a.volumes[0].id.clone();
    let first = a.chapters[0].id.clone();
    s.apply(Command::CreateVolume {
        book_id: book.clone(),
        title: "远方".into(),
    })
    .unwrap();
    let a = s.load().unwrap();
    let v2 = a
        .volumes
        .iter()
        .find(|v| v.id != v1)
        .unwrap()
        .id
        .clone();
    s.apply(Command::CreateChapter {
        volume_id: v2.clone(),
        title: "甲".into(),
    })
    .unwrap();
    s.apply(Command::CreateChapter {
        volume_id: v1.clone(),
        title: "删除".into(),
    })
    .unwrap();
    let a = s.load().unwrap();
    let hidden = a
        .chapters
        .iter()
        .find(|c| c.title == "删除")
        .unwrap()
        .id
        .clone();
    let other = a
        .chapters
        .iter()
        .find(|c| c.title == "甲")
        .unwrap()
        .id
        .clone();
    s.apply(Command::TrashChapter {
        id: hidden.clone(),
    })
    .unwrap();
    let before = s.backup().unwrap();
    for (id, volume_id, before_id) in [
        (first.clone(), v1.clone(), Some(other.clone())),
        (first.clone(), v2.clone(), Some(hidden.clone())),
        (first.clone(), v2.clone(), Some("缺失".into())),
    ] {
        assert!(s
            .apply(Command::PlaceChapter {
                id,
                volume_id,
                before_id,
            })
            .is_err());
        assert_eq!(before, s.backup().unwrap());
    }
    let b = s
        .apply(Command::CreateBook {
            title: "另一本".into(),
            author: "".into(),
        })
        .unwrap();
    let foreign_volume = b
        .volumes
        .iter()
        .find(|v| v.book_id != book)
        .unwrap()
        .id
        .clone();
    let foreign = b
        .chapters
        .iter()
        .find(|c| c.volume_id == foreign_volume)
        .unwrap()
        .id
        .clone();
    let before = s.backup().unwrap();
    assert!(s
        .apply(Command::PlaceChapter {
            id: foreign,
            volume_id: v1,
            before_id: None,
        })
        .is_err());
    assert_eq!(before, s.backup().unwrap());
}

#[test]
fn place_chapter_keeps_deleted_unique() {
    let mut s = Service::open(":memory:").unwrap();
    let a = new_book(&mut s);
    let v = a.volumes[0].id.clone();
    let first = a.chapters[0].id.clone();
    s.apply(Command::CreateChapter {
        volume_id: v.clone(),
        title: "二".into(),
    })
    .unwrap();
    let a = s.load().unwrap();
    let second = a
        .chapters
        .iter()
        .find(|c| c.title == "二")
        .unwrap()
        .id
        .clone();
    s.apply(Command::TrashChapter { id: second.clone() })
        .unwrap();
    s.apply(Command::PlaceChapter {
        id: first,
        volume_id: v.clone(),
        before_id: None,
    })
    .unwrap();
    s.apply(Command::RestoreChapter { id: second }).unwrap();
    let a = s.load().unwrap();
    let mut ordered: Vec<_> = a.chapters.iter().filter(|c| c.volume_id == v).collect();
    ordered.sort_by_key(|c| (c.sort_order, c.id.clone()));
    assert_eq!(
        ordered.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(),
        vec!["二", "初章"]
    );
    assert_eq!(
        ordered.iter().map(|c| c.sort_order).collect::<Vec<_>>(),
        vec![0, 1]
    );
}

#[test]
fn place_volume_reorders_and_rejects_foreign() {
    let mut s = Service::open(":memory:").unwrap();
    let a = new_book(&mut s);
    let book = a.books[0].id.clone();
    let v1 = a.volumes[0].id.clone();
    for title in ["二卷", "三卷"] {
        s.apply(Command::CreateVolume {
            book_id: book.clone(),
            title: title.into(),
        })
        .unwrap();
    }
    let a = s.load().unwrap();
    let third = a
        .volumes
        .iter()
        .find(|v| v.title == "三卷")
        .unwrap()
        .id
        .clone();
    let a = s
        .apply(Command::PlaceVolume {
            id: third.clone(),
            before_id: Some(v1.clone()),
        })
        .unwrap();
    let mut ordered: Vec<_> = a.volumes.iter().filter(|v| v.book_id == book).collect();
    ordered.sort_by_key(|v| v.sort_order);
    assert_eq!(
        ordered.iter().map(|v| v.title.as_str()).collect::<Vec<_>>(),
        vec!["三卷", "启程", "二卷"]
    );
    assert_eq!(
        ordered.iter().map(|v| v.sort_order).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    let b = s
        .apply(Command::CreateBook {
            title: "另一本".into(),
            author: "".into(),
        })
        .unwrap();
    let foreign = b
        .volumes
        .iter()
        .find(|v| v.book_id != book)
        .unwrap()
        .id
        .clone();
    let before = s.backup().unwrap();
    assert!(s
        .apply(Command::PlaceVolume {
            id: v1.clone(),
            before_id: Some(foreign),
        })
        .is_err());
    assert_eq!(before, s.backup().unwrap());
    assert!(s
        .apply(Command::PlaceVolume {
            id: v1.clone(),
            before_id: Some(v1),
        })
        .is_ok());
    assert_eq!(before, s.backup().unwrap());
}

#[test]
fn nested_recycle_restore_and_purge() {
    let p = std::env::temp_dir().join(format!("novel-recycle-{}.sqlite", uuid::Uuid::new_v4()));
    let mut s = Service::open(&p).unwrap();
    let a = new_book(&mut s);
    let b = a.books[0].id.clone();
    let v = a.volumes[0].id.clone();
    let c = a.chapters[0].id.clone();
    let before = s.backup().unwrap();
    assert!(s.apply(Command::PurgeBook { id: b.clone() }).is_err());
    assert!(s.apply(Command::PurgeVolume { id: v.clone() }).is_err());
    assert!(s.apply(Command::PurgeChapter { id: c.clone() }).is_err());
    assert_eq!(before, s.backup().unwrap());
    s.apply(Command::TrashChapter { id: c.clone() }).unwrap();
    assert!(s
        .apply(Command::SaveChapter {
            id: c.clone(),
            body: "forbidden".into(),
            expected_revision: 0
        })
        .is_err());
    assert!(!s.export_book(&b).unwrap().contains("初章"));
    s.apply(Command::TrashVolume { id: v.clone() }).unwrap();
    assert!(s
        .apply(Command::RestoreChapter { id: c.clone() })
        .unwrap_err()
        .contains("恢复"));
    s.apply(Command::TrashBook { id: b.clone() }).unwrap();
    assert!(s.apply(Command::RestoreVolume { id: v.clone() }).is_err());
    let backup = s.backup().unwrap();
    drop(s);
    let mut s = Service::open(&p).unwrap();
    assert_eq!(backup, s.backup().unwrap());
    s.apply(Command::RestoreBook { id: b.clone() }).unwrap();
    assert!(s.load().unwrap().volumes[0].deleted_at.is_some());
    s.apply(Command::RestoreVolume { id: v.clone() }).unwrap();
    assert!(s.load().unwrap().chapters[0].deleted_at.is_some());
    s.apply(Command::RestoreChapter { id: c.clone() }).unwrap();
    assert!(s.export_book(&b).unwrap().contains("初章"));
    s.apply(Command::TrashVolume { id: v.clone() }).unwrap();
    s.apply(Command::PurgeVolume { id: v.clone() }).unwrap();
    let a = s.load().unwrap();
    assert!(a.volumes.is_empty() && a.chapters.is_empty());
    s.restore(&backup).unwrap();
    s.apply(Command::PurgeBook { id: b }).unwrap();
    let a = s.load().unwrap();
    assert!(a.books.is_empty() && a.volumes.is_empty() && a.chapters.is_empty());
    drop(s);
    std::fs::remove_file(p).unwrap();
}
#[test]
fn recycled_parent_forbids_mutations_and_does_not_mark_children() {
    let mut s = Service::open(":memory:").unwrap();
    let a = new_book(&mut s);
    let b = a.books[0].id.clone();
    let v = a.volumes[0].id.clone();
    let c = a.chapters[0].id.clone();
    s.apply(Command::CreateVolume {
        book_id: b.clone(),
        title: "target".into(),
    })
    .unwrap();
    let target = s
        .load()
        .unwrap()
        .volumes
        .iter()
        .find(|x| x.id != v)
        .unwrap()
        .id
        .clone();
    s.apply(Command::TrashVolume { id: v.clone() }).unwrap();
    let a = s.load().unwrap();
    assert!(a.chapters[0].deleted_at.is_none());
    let before = s.backup().unwrap();
    for command in [
        Command::RenameVolume {
            id: v.clone(),
            title: "x".into(),
        },
        Command::MoveVolume {
            id: v.clone(),
            direction: 1,
        },
        Command::CreateChapter {
            volume_id: v.clone(),
            title: "x".into(),
        },
        Command::RenameChapter {
            id: c.clone(),
            title: "x".into(),
        },
        Command::MoveChapter {
            id: c.clone(),
            direction: 1,
        },
        Command::TransferChapter {
            id: c.clone(),
            volume_id: target.clone(),
        },
        Command::SaveChapter {
            id: c.clone(),
            body: "x".into(),
            expected_revision: 0,
        },
        Command::PurgeChapter { id: c.clone() },
    ] {
        assert!(s.apply(command).is_err());
        assert_eq!(before, s.backup().unwrap());
    }
    s.apply(Command::RestoreVolume { id: v.clone() }).unwrap();
    s.apply(Command::TrashChapter { id: c.clone() }).unwrap();
    s.apply(Command::PurgeChapter { id: c }).unwrap();
    assert!(s.load().unwrap().chapters.is_empty());
    s.apply(Command::TrashBook { id: b.clone() }).unwrap();
    let before = s.backup().unwrap();
    for command in [
        Command::UpdateBook {
            id: b.clone(),
            title: "x".into(),
            author: "".into(),
        },
        Command::CreateVolume {
            book_id: b.clone(),
            title: "x".into(),
        },
        Command::RenameVolume {
            id: target,
            title: "x".into(),
        },
    ] {
        assert!(s.apply(command).is_err());
        assert_eq!(before, s.backup().unwrap());
    }
    assert!(s.export_book(&b).is_err());
}
#[test]
fn old_database_migrates_and_old_json_remains_compatible() {
    let p = std::env::temp_dir().join(format!("novel-legacy-{}.sqlite", uuid::Uuid::new_v4()));
    let conn = rusqlite::Connection::open(&p).unwrap();
    conn.execute_batch("CREATE TABLE books(id TEXT PRIMARY KEY,title TEXT NOT NULL,author TEXT NOT NULL,deleted_at INTEGER);CREATE TABLE volumes(id TEXT PRIMARY KEY,book_id TEXT NOT NULL REFERENCES books(id),title TEXT NOT NULL,sort_order INTEGER NOT NULL);CREATE TABLE chapters(id TEXT PRIMARY KEY,volume_id TEXT NOT NULL REFERENCES volumes(id),title TEXT NOT NULL,body TEXT NOT NULL,revision INTEGER NOT NULL,sort_order INTEGER NOT NULL);INSERT INTO books VALUES('b','legacy','author',NULL);INSERT INTO volumes VALUES('v','b','v',0);INSERT INTO chapters VALUES('c','v','c','body',3,0);").unwrap();
    drop(conn);
    let mut s = Service::open(&p).unwrap();
    let a = s.load().unwrap();
    assert_eq!(a.chapters[0].body, "body");
    assert_eq!(a.chapters[0].revision, 3);
    assert!(a.chapters[0].deleted_at.is_none());
    assert!(a.volumes[0].deleted_at.is_none());
    let mut json: serde_json::Value = serde_json::from_str(&s.backup().unwrap()).unwrap();
    json["version"] = serde_json::json!(1);
    json.as_object_mut().unwrap().remove("foreshadow_proposals");
    json["volumes"][0]
        .as_object_mut()
        .unwrap()
        .remove("deleted_at");
    json["chapters"][0]
        .as_object_mut()
        .unwrap()
        .remove("deleted_at");
    s.restore(&json.to_string()).unwrap();
    drop(s);
    let s = Service::open(&p).unwrap();
    assert_eq!(s.load().unwrap(), a);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&s.backup().unwrap()).unwrap()["version"],
        13
    );
    drop(s);
    std::fs::remove_file(p).unwrap();
}
#[test]
fn normal_order_skips_recycled_siblings() {
    let mut s = Service::open(":memory:").unwrap();
    let a = new_book(&mut s);
    let b = a.books[0].id.clone();
    let v = a.volumes[0].id.clone();
    let first = a.chapters[0].id.clone();
    for title in ["hidden", "last"] {
        s.apply(Command::CreateChapter {
            volume_id: v.clone(),
            title: title.into(),
        })
        .unwrap();
    }
    let a = s.load().unwrap();
    let hidden = a
        .chapters
        .iter()
        .find(|c| c.title == "hidden")
        .unwrap()
        .id
        .clone();
    let last = a
        .chapters
        .iter()
        .find(|c| c.title == "last")
        .unwrap()
        .id
        .clone();
    s.apply(Command::TrashChapter { id: hidden }).unwrap();
    s.apply(Command::MoveChapter {
        id: last.clone(),
        direction: -1,
    })
    .unwrap();
    let txt = s.export_book(&b).unwrap();
    assert!(txt.contains("第1章 last"));
    assert!(txt.contains("第2章 初章"));
    assert!(!txt.contains("hidden"));
    s.apply(Command::TrashVolume { id: v.clone() }).unwrap();
    s.apply(Command::CreateVolume {
        book_id: b.clone(),
        title: "visible".into(),
    })
    .unwrap();
    let txt = s.export_book(&b).unwrap();
    assert!(txt.contains("第1卷 visible"));
    assert!(!txt.contains("初章"));
    let _ = first;
}
#[test]
fn deletion_chain_blocks_sql_save_and_transfer_destination() {
    let p = std::env::temp_dir().join(format!("novel-chain-{}.sqlite", uuid::Uuid::new_v4()));
    let mut s = Service::open(&p).unwrap();
    let a = new_book(&mut s);
    let b = a.books[0].id.clone();
    let v = a.volumes[0].id.clone();
    let c = a.chapters[0].id.clone();
    s.apply(Command::CreateVolume {
        book_id: b.clone(),
        title: "目标卷".into(),
    })
    .unwrap();
    let target = s
        .load()
        .unwrap()
        .volumes
        .iter()
        .find(|v2| v2.id != v)
        .unwrap()
        .id
        .clone();
    s.apply(Command::TrashVolume { id: target.clone() })
        .unwrap();
    let before = s.backup().unwrap();
    assert!(s
        .apply(Command::TransferChapter {
            id: c.clone(),
            volume_id: target.clone()
        })
        .is_err());
    assert_eq!(before, s.backup().unwrap());
    s.apply(Command::RestoreVolume { id: target }).unwrap();
    s.apply(Command::TrashBook { id: b.clone() }).unwrap();
    let before = s.backup().unwrap();
    let a = s.load().unwrap();
    assert!(a.volumes.iter().all(|v| v.deleted_at.is_none()));
    assert!(a.chapters.iter().all(|c| c.deleted_at.is_none()));
    assert!(s
        .apply(Command::SaveChapter {
            id: c.clone(),
            body: "rejected".into(),
            expected_revision: 0
        })
        .is_err());
    assert_eq!(before, s.backup().unwrap());
    s.apply(Command::RestoreBook { id: b }).unwrap();
    let a = s
        .apply(Command::SaveChapter {
            id: c,
            body: "restored".into(),
            expected_revision: 0,
        })
        .unwrap();
    assert_eq!(a.chapters[0].body, "restored");
    assert_eq!(a.chapters[0].revision, 1);
    drop(s);
    std::fs::remove_file(p).unwrap();
}
#[test]
fn current_backup_preserves_recycle_and_rejects_unknown_versions() {
    let mut s = Service::open(":memory:").unwrap();
    let a = new_book(&mut s);
    s.apply(Command::TrashChapter {
        id: a.chapters[0].id.clone(),
    })
    .unwrap();
    s.apply(Command::TrashVolume {
        id: a.volumes[0].id.clone(),
    })
    .unwrap();
    let backup = s.backup().unwrap();
    let mut json: serde_json::Value = serde_json::from_str(&backup).unwrap();
    assert_eq!(json["version"], 13);
    s.restore(&backup).unwrap();
    assert_eq!(s.backup().unwrap(), backup);
    json["version"] = serde_json::json!(2);
    json.as_object_mut().unwrap().remove("foreshadow_proposals");
    s.restore(&json.to_string()).unwrap();
    assert_eq!(s.backup().unwrap(), backup);
    json["version"] = serde_json::json!(99);
    assert!(s.restore(&json.to_string()).is_err());
    assert_eq!(s.backup().unwrap(), backup);
    json["version"] = serde_json::json!(1);
    json["volumes"][0]
        .as_object_mut()
        .unwrap()
        .remove("deleted_at");
    json["chapters"][0]
        .as_object_mut()
        .unwrap()
        .remove("deleted_at");
    let restored = s.restore(&json.to_string()).unwrap();
    assert!(restored.volumes[0].deleted_at.is_none());
    assert!(restored.chapters[0].deleted_at.is_none());
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&s.backup().unwrap()).unwrap()["version"],
        13
    );
}
