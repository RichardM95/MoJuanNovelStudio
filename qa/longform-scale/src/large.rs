//! Opt-in larger service workload. No database schema or application hooks.
use super::*;

const CASES: [&str; 3] = ["300-shallow", "1000-shallow", "1000-deep"];
const DEEP_CHAPTERS: usize = 24;
const EXTRA_REVISIONS: i64 = 64;

fn expected_count(case: &str) -> usize {
    match case {
        "300-shallow" => 300,
        "1000-shallow" | "1000-deep" => 1000,
        _ => panic!("unknown large-scenes case"),
    }
}
fn large_chapter_title(i: usize) -> String {
    format!("潮记{:04}·{}", i + 1, PLACES[i % PER_VOLUME])
}
// Reuse original exact-size paragraph fixture, but distinguish all 1000 chapters.
fn large_body(i: usize, revision: i64, late: u8) -> String {
    let seed = i % CHAPTERS;
    // Two extra index digits replace two fixed scalars; all titles are unique.
    body(seed, revision, late)
        .replacen(&chapter_title(seed), &large_chapter_title(i), 1)
        .replacen("留底码", "底", 1)
}
fn db(case: &str) -> PathBuf {
    PathBuf::from(format!("{case}.db"))
}
fn ids(case: &str) -> PathBuf {
    PathBuf::from(format!("{case}.json"))
}
fn seed_shallow(case: &str) -> Corpus {
    let count = expected_count(case);
    let mut core = Service::open(db(case)).unwrap();
    let shelf = core
        .apply(Command::CreateBook {
            title: BOOK.into(),
            author: AUTHOR.into(),
        })
        .unwrap();
    let book = shelf.books[0].id.clone();
    let mut volumes = vec![shelf.volumes[0].id.clone()];
    core.apply(Command::RenameVolume {
        id: volumes[0].clone(),
        title: VOLUMES[0].into(),
    })
    .unwrap();
    core.apply(Command::RenameChapter {
        id: shelf.chapters[0].id.clone(),
        title: large_chapter_title(0),
    })
    .unwrap();
    for title in &VOLUMES[1..] {
        let next = core
            .apply(Command::CreateVolume {
                book_id: book.clone(),
                title: (*title).into(),
            })
            .unwrap();
        volumes.push(
            next.volumes
                .iter()
                .find(|v| v.title == *title)
                .unwrap()
                .id
                .clone(),
        );
    }
    for i in 1..count {
        core.apply(Command::CreateChapter {
            volume_id: volumes[i * volumes.len() / count].clone(),
            title: large_chapter_title(i),
        })
        .unwrap();
    }
    let outline = core.get_outline(&book).unwrap();
    assert_eq!(outline.chapters.len(), count);
    let chapters: Vec<_> = outline
        .chapters
        .into_iter()
        .enumerate()
        .map(|(i, c)| {
            assert_eq!(c.title, large_chapter_title(i));
            assert_eq!(c.volume_id, volumes[i * volumes.len() / count]);
            c.id
        })
        .collect();
    for (i, id) in chapters.iter().enumerate() {
        let text = large_body(i, 1, 0);
        assert_eq!(text.chars().count(), SCALARS);
        save(&mut core, id, text, 0).unwrap();
    }
    Corpus {
        book,
        volumes,
        chapters,
    }
}
fn seed_deep(c: &Corpus) {
    fs::copy(db("1000-shallow"), db("1000-deep")).unwrap();
    let mut deep = Service::open(db("1000-deep")).unwrap();
    for rev in 2..=1 + EXTRA_REVISIONS {
        for (i, id) in c.chapters.iter().enumerate().take(DEEP_CHAPTERS) {
            // Latest prose is deliberately the shallow fixture's text.
            let text = large_body(i, if rev == 1 + EXTRA_REVISIONS { 1 } else { rev }, 0);
            save(&mut deep, id, text, rev - 1).unwrap();
        }
    }
    let deep_state = deep.load().unwrap();
    let shallow = Service::open(db("1000-shallow")).unwrap();
    let shallow_state = shallow.load().unwrap();
    assert_eq!(
        (&deep_state.books, &deep_state.volumes),
        (&shallow_state.books, &shallow_state.volumes)
    );
    assert_eq!(deep_state.chapters.len(), c.chapters.len());
    assert_eq!(shallow_state.chapters.len(), c.chapters.len());
    for (a, b) in deep_state.chapters.iter().zip(&shallow_state.chapters) {
        // `load` need not use story order; match the same stable identity first.
        assert_eq!(a.id, b.id);
        assert_eq!(
            a.body.as_bytes(),
            b.body.as_bytes(),
            "different current prose: {}",
            a.id
        );
    }
}
fn inspect(case: &str, c: &Corpus) -> Value {
    let core = Service::open(db(case)).unwrap();
    let mut body_bytes = 0;
    let mut body_scalars = 0;
    let mut history_bytes = 0;
    let mut history_rows = 0;
    for (i, id) in c.chapters.iter().enumerate() {
        let chapter = core.get_chapter(id).unwrap();
        let versions = core.chapter_history(id).unwrap();
        let expected_revision = if case == "1000-deep" && i < DEEP_CHAPTERS {
            1 + EXTRA_REVISIONS
        } else {
            1
        };
        assert_eq!(chapter.revision, expected_revision);
        assert_eq!(chapter.body, large_body(i, 1, 0));
        assert_eq!(versions.len(), expected_revision as usize + 1);
        assert!(versions
            .iter()
            .any(|v| v.revision == expected_revision && v.body == chapter.body));
        body_bytes += chapter.body.len();
        body_scalars += chapter.body.chars().count();
        history_rows += versions.len();
        history_bytes += versions.iter().map(|v| v.body.len()).sum::<usize>();
    }
    drop(core);
    json!({"case":case, "chapter_count":c.chapters.len(), "volume_count":c.volumes.len(),
           "current_utf8_bytes":body_bytes,"current_scalars":body_scalars,
           "history_rows":history_rows,"history_utf8_bytes":history_bytes,
           "closed_db_file_bytes":fs::metadata(db(case)).unwrap().len()})
}
pub(super) fn seed_large() -> Value {
    for case in CASES {
        assert!(
            !db(case).exists() && !ids(case).exists(),
            "refusing to reuse {case}"
        );
    }
    let mut measurements = Vec::new();
    for case in &CASES[..2] {
        eprintln!("seeding {case}");
        let (c, ms) = timed(|| seed_shallow(case));
        write_new_file(&ids(case), &serde_json::to_vec(&c).unwrap()).unwrap();
        let mut stats = inspect(case, &c);
        stats["seed_ms"] = json!(ms);
        measurements.push(stats);
    }
    let shallow: Corpus = serde_json::from_slice(&fs::read(ids("1000-shallow")).unwrap()).unwrap();
    eprintln!("seeding 1000-deep");
    let ((), ms) = timed(|| seed_deep(&shallow));
    write_new_file(&ids("1000-deep"), &serde_json::to_vec(&shallow).unwrap()).unwrap();
    let mut stats = inspect("1000-deep", &shallow);
    stats["seed_ms"] = json!(ms);
    assert_eq!(
        stats["current_utf8_bytes"],
        measurements[1]["current_utf8_bytes"]
    );
    assert_eq!(stats["current_scalars"], measurements[1]["current_scalars"]);
    measurements.push(stats);
    json!({"fixtures":measurements,"deep_hot_chapters":DEEP_CHAPTERS,"deep_extra_versions_per_hot_chapter":EXTRA_REVISIONS})
}
pub(super) fn measure_large(case: &str, round: usize) -> Value {
    let expected = expected_count(case);
    assert!((1..=3).contains(&round));
    let c: Corpus = serde_json::from_slice(&fs::read(ids(case)).unwrap()).unwrap();
    assert_eq!(c.chapters.len(), expected);
    let dir = PathBuf::from(format!("sample-{case}-{round}"));
    fs::create_dir(&dir).unwrap();
    let live = dir.join("live.db");
    let saved = dir.join("saved.sqlite");
    let recovery = dir.join("recovery.sqlite");
    fs::copy(db(case), &live).unwrap();

    let (mut s, open_ms) = timed(|| Service::open(&live).unwrap());
    let (state, full_load_ms) = timed(|| s.load().unwrap());
    assert_eq!(state.chapters.len(), expected);
    drop(state);
    let (outline, outline_ms) = timed(|| s.get_outline(&c.book).unwrap());
    assert!(outline
        .chapters
        .iter()
        .map(|row| &row.id)
        .eq(c.chapters.iter()));
    let mut get_chapter_ms = Vec::new();
    for i in [0, expected / 2, expected - 1] {
        let (chapter, ms) = timed(|| s.get_chapter(&c.chapters[i]).unwrap());
        assert_eq!(chapter.body, large_body(i, 1, 0));
        get_chapter_ms.push(ms);
    }

    let target = &c.chapters[0];
    let before = s.get_chapter(target).unwrap();
    let (page, history_page_ms) = timed(|| {
        s.list_chapter_versions(&c.book, target, None, PAGE, false)
            .unwrap()
    });
    let rows = page.items.len();
    assert_eq!(rows, (before.revision as usize + 1).min(PAGE as usize));
    assert_eq!(
        page.next_cursor.is_some(),
        before.revision as usize + 1 > PAGE as usize
    );
    let (detail, history_detail_ms) = timed(|| {
        s.get_chapter_version(&c.book, target, &page.items[0].id)
            .unwrap()
    });
    assert_eq!(detail.version.body, before.body);

    // Save on a private clone; write one alternate body, then restore that
    // clone to the backed-up state after a further distinguishable edit.
    let saved_body = large_body(0, 1, 1);
    let (saved_state, save_ms) =
        timed(|| save(&mut s, target, saved_body.clone(), before.revision).unwrap());
    assert!(saved_state.chapters.iter().any(|row| row.id == *target
        && row.revision == before.revision + 1
        && row.body == saved_body));
    let saved_history = s.chapter_history(target).unwrap();
    let (_, backup_ms) = timed(|| s.export_sqlite_backup(&saved, |_| Ok(())).unwrap());
    let backup_bytes = fs::metadata(&saved).unwrap().len();
    let live_file_bytes_at_backup = ["live.db", "live.db-wal", "live.db-shm"]
        .map(|name| fs::metadata(dir.join(name)).ok().map(|x| x.len()));
    save(&mut s, target, large_body(0, 1, 2), before.revision + 1).unwrap();
    let later = s.get_chapter(target).unwrap();
    assert_ne!(later.body, saved_body);
    let guard = s.restore_guard().unwrap();
    let (prepared, restore_prepare_ms) = timed(|| prepare_restore(&saved, |_| Ok(())).unwrap());
    let (restored, restore_commit_ms) = timed(|| {
        s.commit_prepared_restore(prepared, &guard, &recovery, |_| Ok(()))
            .unwrap()
    });
    assert_eq!(restored.summary.chapters, expected as u64);
    let recovery_bytes = fs::metadata(&recovery).unwrap().len();
    drop(s);

    let reopened = Service::open(&live).unwrap();
    assert_eq!(reopened.load().unwrap(), saved_state);
    assert_eq!(reopened.chapter_history(target).unwrap(), saved_history);
    let protective = Service::open(&recovery).unwrap();
    assert_eq!(protective.get_chapter(target).unwrap(), later);
    drop((protective, reopened));
    fs::remove_dir_all(&dir).unwrap(); // only this run's synthetic, private clone

    json!({"case":case,"round":round,"open_ms":open_ms,"full_load_ms":full_load_ms,
           "outline_ms":outline_ms,"get_chapter_ms":get_chapter_ms,"history_page_ms":history_page_ms,
           "history_page_rows":rows,"history_detail_ms":history_detail_ms,"save_ms":save_ms,
           "backup_ms":backup_ms,"backup_bytes":backup_bytes,"live_db_wal_shm_at_backup":live_file_bytes_at_backup,"restore_prepare_ms":restore_prepare_ms,
           "restore_commit_ms":restore_commit_ms,"recovery_bytes":recovery_bytes})
}
