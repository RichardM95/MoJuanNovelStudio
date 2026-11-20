//! Opt-in synthetic workload. All book data is created/read through public core APIs.
use novel_core::{
    files::write_new_file, prepare_restore, BackupJob, BackupJobManager, Command, ExportOptions,
    LibraryState, RestoreSummary, Service, VersionSummary,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

mod large;

const DB: &str = "library.db";
const MANIFEST: &str = "corpus.json";
const BOOK: &str = "潮城灯谱";
const AUTHOR: &str = "规模验收虚构样本";
const VOLUMES: [&str; 3] = ["潮城来信", "暗渠星图", "灯归远岸"];
const PLACES: [&str; 8] = [
    "旧渡口",
    "青瓦街",
    "观潮楼",
    "沉钟巷",
    "银沙井",
    "薄雾埠",
    "石潮门",
    "双灯塔",
];
const SCALARS: usize = 8192;
const PER_VOLUME: usize = 8;
const CHAPTERS: usize = VOLUMES.len() * PER_VOLUME;
const PAGE: u32 = 50;
const MIB: usize = 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct Corpus {
    book: String,
    volumes: Vec<String>,
    chapters: Vec<String>, // story order, not UUID order
}

struct Verified {
    current_bytes: usize,
    current_scalars: usize,
    history_bytes: usize,
    index: Vec<VersionSummary>,
}

#[derive(Clone, Copy)]
struct Draft {
    revisions: i64,
    late_round: u8,
}

impl Draft {
    fn standard(revisions: i64) -> Self {
        Self {
            revisions,
            late_round: 0,
        }
    }

    fn current_revision(self, chapter: usize) -> i64 {
        self.revisions + i64::from(chapter == 0 && self.late_round != 0)
    }

    fn body(self, chapter: usize, rev: i64) -> String {
        let late = if rev > self.revisions {
            self.late_round
        } else {
            0
        };
        body(chapter, rev, late)
    }
}

fn chapter_title(i: usize) -> String {
    format!("潮记{:02}·{}", i + 1, PLACES[i % PER_VOLUME])
}

fn marker(chapter: usize, rev: i64, late: u8) -> String {
    format!("尾声信标·章{:02}·稿{rev:03}·留底{late}", chapter + 1)
}

// Intentionally synthetic original template paragraphs, not a literary quality test.
fn body(chapter: usize, rev: i64, late: u8) -> String {
    if rev == 0 {
        return String::new(); // production creates an empty first baseline snapshot
    }
    let place = PLACES[chapter % PER_VOLUME];
    let volume = VOLUMES[chapter / PER_VOLUME];
    let tail = format!("\r\n{}。🌒\n", marker(chapter, rev, late));
    let mut out = format!(
        "【{volume}】{}；第{rev:03}稿，留底码{late}。\r\n",
        chapter_title(chapter)
    );
    let mut remaining = SCALARS - out.chars().count() - tail.chars().count();
    let mut paragraph = 0;
    while remaining > 0 {
        let next = PLACES[(chapter + paragraph + rev as usize) % PER_VOLUME];
        let passage = match (chapter + paragraph + rev as usize + late as usize) % 4 {
            0 => format!("雾从{place}的石阶爬过来。林渡把空灯放上窗台，等着{next}的钟声把一夜分成两半。她没有问那封信是谁写的，只把封口朝向了一点青光。\r\n"),
            1 => format!("阿澜在{place}捡到一截浸水的绳子。绳结系成旧船家的样子，却打在错误的一端。她沿着积水走向{next}，每经过一盏灯就松开一扣。\n"),
            2 => format!("潮水退到{place}的栏杆底下，露出一线细沙。守灯人将{next}画进手掌上的旧地图，又把手合上。那条看不见的归路在指缝间停了一会儿。\r\n"),
            _ => format!("天亮前，{place}还没有人开门。岑叔从怀里取出两片瓦，轻轻一敲，像有人在{next}回答。屋檐下只有风，桌上那盏灯却转了个方向。🌒\n"),
        };
        let count = remaining.min(passage.chars().count());
        out.extend(passage.chars().take(count));
        remaining -= count;
        paragraph += 1;
    }
    out.push_str(&tail);
    out
}

fn timed<T>(run: impl FnOnce() -> T) -> (T, f64) {
    let start = Instant::now();
    let value = run();
    (value, start.elapsed().as_secs_f64() * 1000.0)
}

fn files_now() -> Value {
    json!(["library.db", "library.db-wal", "library.db-shm"]
        .map(|p| fs::metadata(p).ok().map(|m| m.len())))
}

fn corpus() -> Corpus {
    serde_json::from_slice(&fs::read(MANIFEST).expect("read fixture IDs"))
        .expect("decode fixture IDs")
}

fn seed() -> Value {
    assert!(
        !Path::new(DB).exists(),
        "refusing to seed over an existing library"
    );
    let (mut core, open_ms) = timed(|| Service::open(DB).expect("open new fixture"));
    let ((state, c), create_ms) = timed(|| {
        let s = core
            .apply(Command::CreateBook {
                title: BOOK.into(),
                author: AUTHOR.into(),
            })
            .expect("create fixture book");
        let book = s.books[0].id.clone();
        let mut volumes = vec![s.volumes[0].id.clone()];
        core.apply(Command::RenameVolume {
            id: volumes[0].clone(),
            title: VOLUMES[0].into(),
        })
        .expect("rename first fixture volume");
        core.apply(Command::RenameChapter {
            id: s.chapters[0].id.clone(),
            title: chapter_title(0),
        })
        .expect("rename first fixture chapter");
        for title in &VOLUMES[1..] {
            let state = core
                .apply(Command::CreateVolume {
                    book_id: book.clone(),
                    title: (*title).into(),
                })
                .expect("create fixture volume");
            volumes.push(
                state
                    .volumes
                    .iter()
                    .find(|v| v.title == *title)
                    .expect("find new volume")
                    .id
                    .clone(),
            );
        }
        for i in 1..CHAPTERS {
            core.apply(Command::CreateChapter {
                volume_id: volumes[i / PER_VOLUME].clone(),
                title: chapter_title(i),
            })
            .expect("create fixture chapter");
        }
        let state = core.load().expect("read seeded library");
        let chapters = (0..CHAPTERS)
            .map(|i| {
                state
                    .chapters
                    .iter()
                    .find(|c| c.title == chapter_title(i))
                    .expect("find chapter")
                    .id
                    .clone()
            })
            .collect();
        (
            state,
            Corpus {
                book,
                volumes,
                chapters,
            },
        )
    });
    verify_state(&state, &c, Draft::standard(0));
    write_new_file(Path::new(MANIFEST), &serde_json::to_vec_pretty(&c).unwrap())
        .expect("persist fixture IDs");
    json!({"open_ms": open_ms, "create_tree_ms": create_ms, "volumes": c.volumes.len(),
           "chapters": c.chapters.len(), "live_db_wal_shm_open_bytes": files_now()})
}

fn open_live() -> Service {
    assert!(Path::new(DB).is_file(), "fixture DB must be seeded first");
    Service::open(DB).expect("open live fixture")
}

fn verify_state(state: &LibraryState, c: &Corpus, draft: Draft) -> (usize, usize) {
    assert_eq!(state.books.len(), 1);
    let book = &state.books[0];
    assert!(
        book.id == c.book
            && book.title == BOOK
            && book.author == AUTHOR
            && book.deleted_at.is_none(),
        "fixture book identity, title, author or recycle state differs"
    );
    assert_eq!(state.volumes.len(), VOLUMES.len());
    for (i, v) in state.volumes.iter().enumerate() {
        assert!(
            v.id == c.volumes[i]
                && v.book_id == c.book
                && v.title == VOLUMES[i]
                && v.sort_order == i as i64
                && v.deleted_at.is_none(),
            "volume {} differs",
            i + 1
        );
    }
    assert_eq!(state.chapters.len(), CHAPTERS);
    let mut bytes = 0;
    let mut scalars = 0;
    for (i, id) in c.chapters.iter().enumerate() {
        let row = state
            .chapters
            .iter()
            .find(|row| row.id == *id)
            .expect("chapter lost");
        let rev = draft.current_revision(i);
        assert!(
            row.volume_id == c.volumes[i / PER_VOLUME]
                && row.sort_order == (i % PER_VOLUME) as i64
                && row.title == chapter_title(i)
                && row.deleted_at.is_none()
                && row.revision == rev
                && row.body == draft.body(i, rev),
            "chapter {} current text or metadata differs at revision {rev}",
            i + 1
        );
        bytes += row.body.len();
        scalars += row.body.chars().count();
    }
    (bytes, scalars)
}

fn verify(core: &Service, c: &Corpus, draft: Draft) -> Verified {
    let (current_bytes, current_scalars) =
        verify_state(&core.load().expect("load full fixture"), c, draft);
    let mut index = Vec::new();
    let mut history_bytes = 0;
    for (i, id) in c.chapters.iter().enumerate() {
        let mut next = None;
        let mut revisions_left = draft.current_revision(i) + 1;
        loop {
            let page = core
                .list_chapter_versions(&c.book, id, next.as_deref(), PAGE, false)
                .expect("list history page");
            assert!(
                !page.items.is_empty(),
                "lost history page of chapter {}",
                i + 1
            );
            for row in page.items {
                revisions_left -= 1;
                assert!(revisions_left >= 0 && row.revision == revisions_left && row.chapter_id == *id,
                    "history order, chapter identity or revision differs for chapter {} at {revisions_left}", i + 1);
                let detail = core
                    .get_chapter_version(&c.book, id, &row.id)
                    .expect("load history version");
                let v = detail.version;
                assert!(
                    v.body == draft.body(i, revisions_left)
                        && v.id == row.id
                        && v.chapter_id == row.chapter_id
                        && v.revision == row.revision
                        && v.source == row.source
                        && v.created_at == row.created_at
                        && row.source
                            == if revisions_left == 0 {
                                "baseline"
                            } else {
                                "gui"
                            }
                        && detail.label == row.label
                        && detail.label_revision == row.label_revision
                        && detail.epoch == page.epoch,
                    "historical body or metadata differs in chapter {} revision {revisions_left}",
                    i + 1
                );
                history_bytes += v.body.len();
                index.push(row);
            }
            next = page.next_cursor;
            if next.is_none() {
                break;
            }
        }
        assert_eq!(
            revisions_left,
            0,
            "missing history rows for chapter {}",
            i + 1
        );
    }
    Verified {
        current_bytes,
        current_scalars,
        history_bytes,
        index,
    }
}

fn save(
    core: &mut Service,
    id: &str,
    text: String,
    previous: i64,
) -> novel_core::Result<LibraryState> {
    core.apply(Command::SaveChapter {
        id: id.into(),
        body: text,
        expected_revision: previous,
    })
}

fn distribution(mut values: Vec<f64>) -> Value {
    values.sort_by(f64::total_cmp);
    let n = values.len();
    json!({"n": n, "sum_ms": values.iter().sum::<f64>(), "min_ms": values[0],
           "p50_ms": values[(n-1)/2..=n/2].iter().sum::<f64>() / (2-n%2) as f64,
           "p95_ms": values[(n*95).div_ceil(100)-1], "max_ms": values[n-1]})
}

fn grow(from: i64, to: i64) -> Value {
    let c = corpus();
    let (mut core, open_ms) = timed(open_live);
    let (before, initial_load_ms) = timed(|| core.load().expect("load fixture before growth"));
    verify_state(&before, &c, Draft::standard(from));
    drop(before);
    let mut last_save = Vec::with_capacity(CHAPTERS);
    let mut last_generation = Vec::with_capacity(CHAPTERS);
    let mut all_save = Vec::with_capacity((to - from) as usize * CHAPTERS);
    let mut generation_ms = 0.0;
    let mut generated_bytes = 0;
    for rev in from + 1..=to {
        for (i, id) in c.chapters.iter().enumerate() {
            let (text, gen_ms) = timed(|| body(i, rev, 0));
            generated_bytes += text.len();
            generation_ms += gen_ms;
            let (result, save_ms) = timed(|| save(&mut core, id, text, rev - 1));
            all_save.push(save_ms);
            let state =
                result.unwrap_or_else(|e| panic!("save chapter {} revision {rev}: {e}", i + 1));
            let row = state
                .chapters
                .iter()
                .find(|ch| ch.id == *id)
                .expect("save returned no target chapter");
            assert!(
                state.chapters.len() == CHAPTERS && row.revision == rev,
                "save returned wrong chapter set or revision"
            );
            if rev == to {
                last_save.push(save_ms);
                last_generation.push(gen_ms);
            }
        }
    }
    json!({"from_revisions": from, "to_revisions": to, "open_ms": open_ms,
           "initial_full_load_ms": initial_load_ms, "all_saves": distribution(all_save),
           "checkpoint_24_saves": distribution(last_save),
           "checkpoint_24_generations": distribution(last_generation),
           "body_generation_ms": generation_ms, "generated_utf8_bytes": generated_bytes,
           "live_db_wal_shm_open_bytes": files_now()})
}

fn expected_txt(draft: Draft) -> Vec<u8> {
    let mut text = format!("\u{feff}{BOOK}\r\n作者：{AUTHOR}\r\n\r\n");
    for (vi, volume) in VOLUMES.iter().enumerate() {
        text.push_str(&format!("第{}卷 {volume}\r\n\r\n", vi + 1));
        for i in vi * PER_VOLUME..(vi + 1) * PER_VOLUME {
            text.push_str(&format!("第{}章 {}\r\n\r\n", i + 1, chapter_title(i)));
            text.push_str(
                &draft
                    .body(i, draft.current_revision(i))
                    .replace("\r\n", "\n")
                    .replace('\r', "\n")
                    .replace('\n', "\r\n"),
            );
            text.push_str("\r\n\r\n");
        }
    }
    text.into_bytes()
}

fn probe(revisions: i64) -> Value {
    let c = corpus();
    let draft = Draft::standard(revisions);
    let (mut core, open_ms) = timed(open_live);
    let (state, load_ms) = timed(|| core.load().expect("load full library"));
    verify_state(&state, &c, draft);
    drop(state);
    let (outline, outline_ms) = timed(|| {
        core.get_outline(&c.book)
            .expect("load metadata-only outline")
    });
    assert!(
        outline
            .chapters
            .iter()
            .map(|row| &row.id)
            .eq(c.chapters.iter()),
        "outline chapter order differs"
    );
    let (chapter, get_chapter_ms) = timed(|| {
        core.get_chapter(&c.chapters[0])
            .expect("read current chapter")
    });
    assert!(
        chapter.body == draft.body(0, revisions),
        "direct current chapter read differs"
    );
    let (page, history_first_page_ms) = timed(|| {
        core.list_chapter_versions(&c.book, &c.chapters[0], None, PAGE, false)
            .expect("read history first page")
    });
    let (detail, history_detail_ms) = timed(|| {
        core.get_chapter_version(&c.book, &c.chapters[0], &page.items[0].id)
            .expect("read one history body")
    });
    assert!(
        detail.version.body == draft.body(0, revisions),
        "first history detail differs"
    );
    let word = marker(CHAPTERS - 1, revisions, 0);
    let (hits, search_ms) = timed(|| {
        core.search_book(&c.book, &word, 30)
            .expect("search whole current book")
    });
    assert!(
        hits.items.len() == 1
            && !hits.truncated
            && hits.items[0].id == c.chapters[CHAPTERS - 1]
            && hits.items[0].snippet.contains(&word),
        "unique last-chapter marker not found"
    );
    let options = ExportOptions {
        scope: "book".into(),
        target_id: None,
        include_metadata: true,
        include_headings: true,
        newline: "crlf".into(),
        bom: true,
    };
    let (preview, txt_preview_ms) = timed(|| {
        core.prepare_txt_export(&c.book, options)
            .expect("preview full TXT")
    });
    let path = PathBuf::from(format!("book-{revisions:03}.txt"));
    let ((), txt_take_and_write_ms) = timed(|| {
        let bytes = core
            .take_txt_export(&preview.id)
            .expect("take frozen TXT bytes");
        write_new_file(&path, &bytes).expect("exclusive create and sync TXT");
    });
    let file = fs::read(&path).expect("read exported TXT");
    let file_text = std::str::from_utf8(&file)
        .expect("UTF-8 TXT")
        .trim_start_matches('\u{feff}');
    assert!(
        preview.chapters == CHAPTERS
            && preview.volumes == VOLUMES.len()
            && preview.bytes == file.len()
            && preview.truncated
            && preview.text.chars().count() == 4000
            && file_text.starts_with(&preview.text)
            && file == expected_txt(draft),
        "preview/output bytes, headings or newline normalization differ"
    );
    let (verified, exact_verify_ms) = timed(|| verify(&core, &c, draft));
    json!({"revisions_per_chapter": revisions, "chapters": CHAPTERS,
           "open_ms": open_ms, "full_load_ms": load_ms, "outline_ms": outline_ms,
           "get_chapter_ms": get_chapter_ms, "history_first_page_ms": history_first_page_ms,
           "history_first_page_rows": page.items.len(), "history_detail_ms": history_detail_ms,
           "search_ms": search_ms, "search_hits": hits.items.len(), "txt_preview_ms": txt_preview_ms,
           "txt_take_and_write_ms": txt_take_and_write_ms, "txt_bytes": file.len(),
           "exact_verify_ms": exact_verify_ms, "current_utf8_bytes": verified.current_bytes,
           "current_scalars": verified.current_scalars, "history_utf8_bytes": verified.history_bytes,
           "history_rows": verified.index.len(), "live_db_wal_shm_open_bytes": files_now()})
}

fn json_boundary(revisions: i64) -> Value {
    let c = corpus();
    let mut core = open_live();
    let draft = Draft::standard(revisions);
    let before = verify(&core, &c, draft);
    let guard = core
        .restore_guard()
        .expect("guard before negative controls");
    let (cas, wrong_revision_ms) = timed(|| {
        save(
            &mut core,
            &c.chapters[0],
            "应拒绝此旧基线稿".into(),
            revisions - 1,
        )
    });
    let conflict = cas.expect_err("invalid revision unexpectedly saved");
    assert!(
        conflict.starts_with("revision_conflict:"),
        "wrong CAS failure: {conflict}"
    );
    assert_eq!(
        guard,
        core.restore_guard().expect("guard after wrong revision")
    );
    let after_cas = verify(&core, &c, draft);
    assert!(
        before.index == after_cas.index,
        "CAS failure changed history metadata"
    );
    let (backup, json_backup_ms) = timed(|| core.backup());
    let (result, json_encoded_bytes, json_error) = if revisions == 128 {
        assert!(
            before.history_bytes > 64 * MIB,
            "did not reach >64 MiB raw history via production saves"
        );
        let message = backup.expect_err("over-limit JSON unexpectedly succeeded");
        assert!(
            message.contains("超64MiB") && message.contains("原库未更改"),
            "wrong JSON failure: {message}"
        );
        ("over_limit_rejected", None, Some(message))
    } else {
        let string = backup.expect("smoke JSON is within limit");
        let encoded: Value = serde_json::from_str(&string).expect("decode smoke JSON");
        assert_eq!(encoded["version"], 13);
        assert_eq!(
            encoded["history"]
                .as_array()
                .expect("smoke JSON includes history")
                .len(),
            before.index.len()
        );
        ("within_limit", Some(string.len()), None)
    };
    assert_eq!(guard, core.restore_guard().expect("guard after JSON"));
    let after_json = verify(&core, &c, draft);
    assert!(
        before.index == after_json.index,
        "JSON backup changed history metadata"
    );
    json!({"result": result, "json_error": json_error, "json_backup_ms": json_backup_ms,
           "json_encoded_bytes": json_encoded_bytes, "wrong_revision_ms": wrong_revision_ms,
           "wrong_revision_error": conflict, "history_rows": before.index.len(),
           "history_utf8_bytes": before.history_bytes,
           "unchanged_current_history_and_guard": true,
           "live_db_wal_shm_open_bytes": files_now()})
}

fn wait(manager: &BackupJobManager, job: BackupJob) -> BackupJob {
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        let status = manager.status(&job.id).expect("read backup job status");
        if matches!(
            status.status.as_str(),
            "prepared" | "done" | "failed" | "cancelled" | "expired"
        ) {
            return status;
        }
        assert!(
            Instant::now() < deadline,
            "local job timeout: {} {} phase {}",
            status.kind,
            status.status,
            status.progress.phase
        );
        thread::sleep(Duration::from_millis(2));
    }
}

fn check_job(job: &BackupJob, expected: &str) {
    assert!(
        job.status == expected,
        "backup job {} status {} ({:?} / {:?}), expected {expected}",
        job.kind,
        job.status,
        job.error_code,
        job.error_message
    );
}

fn check_summary(summary: &RestoreSummary, verified: &Verified) {
    assert_eq!(summary.format, "sqlite");
    assert_eq!(
        (
            summary.books,
            summary.volumes,
            summary.chapters,
            summary.history
        ),
        (
            1,
            VOLUMES.len() as u64,
            CHAPTERS as u64,
            verified.index.len() as u64
        )
    );
}

fn backup_round(revisions: i64, round: u8) -> Value {
    let c = corpus();
    let baseline = Draft::standard(revisions);
    let latest = Draft {
        revisions,
        late_round: round,
    };
    let root = std::env::current_dir().expect("get isolated test directory");
    let backup = root.join(format!("backup-{round}.sqlite"));
    let recovery_dir = root.join(format!("before-{round}"));
    let expected = verify(&open_live(), &c, baseline);
    let store = Arc::new(Mutex::new(open_live()));
    let manager = BackupJobManager::new(store.clone(), recovery_dir.clone());

    eprintln!("round {round}: export baseline");
    let (exported, export_ms) = timed(|| {
        wait(
            &manager,
            manager
                .start_export(backup.clone())
                .expect("start SQLite backup export"),
        )
    });
    check_job(&exported, "done");
    assert_eq!(
        exported
            .result
            .expect("backup export receipt")
            .output_path
            .as_ref(),
        Some(&backup)
    );
    let backup_info = fs::metadata(&backup).expect("backup file exists");

    eprintln!("round {round}: prepare and invalidate with a real subsequent save");
    let (prepared, stale_prepare_ms) = timed(|| {
        wait(
            &manager,
            manager
                .start_prepare_restore(backup.clone())
                .expect("start SQLite restore preparation"),
        )
    });
    check_job(&prepared, "prepared");
    check_summary(
        prepared
            .preview
            .as_ref()
            .expect("preview required before confirmation"),
        &expected,
    );
    let original_guard = store.lock().unwrap().restore_guard().unwrap();
    assert!(
        manager
            .confirm_restore(&prepared.id, "not-the-token")
            .is_err(),
        "wrong confirmation token accepted"
    );
    assert_eq!(
        original_guard,
        store.lock().unwrap().restore_guard().unwrap()
    );
    save(
        &mut store.lock().unwrap(),
        &c.chapters[0],
        latest.body(0, revisions + 1),
        revisions,
    )
    .expect("save newer unique draft");
    let after_save_guard = store.lock().unwrap().restore_guard().unwrap();
    let (stale, stale_confirm_ms) = timed(|| {
        wait(
            &manager,
            manager
                .confirm_restore(
                    &prepared.id,
                    prepared
                        .confirmation_token
                        .as_deref()
                        .expect("explicit restore token"),
                )
                .expect("submit stale token for backend guard check"),
        )
    });
    check_job(&stale, "failed");
    assert_eq!(stale.error_code.as_deref(), Some("restore_conflict"));
    assert!(
        stale.result.is_none() && !recovery_dir.exists(),
        "stale restore reported or created a recovery snapshot"
    );
    assert_eq!(
        after_save_guard,
        store.lock().unwrap().restore_guard().unwrap()
    );
    let before_restore = verify(&store.lock().unwrap(), &c, latest);
    assert!(
        before_restore.index[1..] == expected.index,
        "stale confirm changed historical metadata"
    );

    eprintln!("round {round}: fresh prepare and explicit confirmation");
    let (ready, prepare_ms) = timed(|| {
        wait(
            &manager,
            manager
                .start_prepare_restore(backup.clone())
                .expect("fresh SQLite restore prepare"),
        )
    });
    check_job(&ready, "prepared");
    let summary = ready.preview.as_ref().expect("new preview summary");
    check_summary(summary, &expected);
    assert_eq!(
        after_save_guard,
        store.lock().unwrap().restore_guard().unwrap()
    );
    let (restored, confirm_ms) = timed(|| {
        wait(
            &manager,
            manager
                .confirm_restore(
                    &ready.id,
                    ready
                        .confirmation_token
                        .as_deref()
                        .expect("fresh explicit token"),
                )
                .expect("confirm SQLite replacement"),
        )
    });
    check_job(&restored, "done");
    let receipt = restored.result.expect("restore result");
    check_summary(
        receipt.summary.as_ref().expect("restore summary"),
        &expected,
    );
    let recovery = receipt
        .recovery_path
        .expect("published pre-restore recovery SQLite file");
    assert_ne!(
        after_save_guard.epoch,
        store.lock().unwrap().restore_guard().unwrap().epoch
    );
    assert_eq!(
        backup_info.len(),
        fs::metadata(&backup).expect("backup still exists").len()
    );
    assert_eq!(
        backup_info.modified().unwrap(),
        fs::metadata(&backup).unwrap().modified().unwrap()
    );

    eprintln!("round {round}: close and reopen original library");
    let ((), live_verify_ms) = timed(|| {
        let actual = verify(&store.lock().unwrap(), &c, baseline);
        assert!(
            actual.index == expected.index,
            "live restored history metadata differs"
        );
    });
    manager.shutdown();
    drop(manager);
    drop(store);
    let (reopened, reopen_ms) = timed(open_live);
    let (reopened, reopen_verify_ms) = timed(|| verify(&reopened, &c, baseline));
    assert!(
        reopened.index == expected.index,
        "reopened restored history metadata differs"
    );

    eprintln!("round {round}: formally prepare untouched published recovery file, then actually restore into a separate library");
    let (prepped_snapshot, snapshot_prepare_ms) = timed(|| {
        prepare_restore(&recovery, |_| Ok(())).expect("published snapshot cannot be prepared as-is")
    });
    check_summary(prepped_snapshot.summary(), &before_restore);
    let published_snapshot_bytes = fs::metadata(&recovery)
        .expect("published snapshot metadata")
        .len();
    let imported = root.join(format!("recovered-{round}.db"));
    assert!(
        !imported.exists(),
        "refusing to overwrite synthetic recovery destination"
    );
    let mut recovery_core =
        Service::open(&imported).expect("open separate blank restore destination");
    let guard = recovery_core
        .restore_guard()
        .expect("blank destination guard");
    let blank_snapshot = root.join(format!("before-recovering-{round}.sqlite"));
    let (snapshot_outcome, snapshot_confirm_ms) = timed(|| {
        recovery_core
            .commit_prepared_restore(prepped_snapshot, &guard, &blank_snapshot, |_| Ok(()))
            .expect("restore actual pre-replacement snapshot")
    });
    check_summary(&snapshot_outcome.summary, &before_restore);
    drop(recovery_core);
    let (snapshot_state, snapshot_reopen_verify_ms) = timed(|| {
        verify(
            &Service::open(&imported).expect("reopen separate recovered library"),
            &c,
            latest,
        )
    });
    assert!(
        snapshot_state.index == before_restore.index,
        "published recovery snapshot restored different historical metadata"
    );
    json!({"round": round, "export_ms": export_ms, "stale_prepare_ms": stale_prepare_ms,
           "stale_confirm_ms": stale_confirm_ms, "stale_error": stale.error_code,
           "prepare_ms": prepare_ms, "confirm_ms": confirm_ms, "live_verify_ms": live_verify_ms,
           "reopen_ms": reopen_ms, "reopen_verify_ms": reopen_verify_ms,
           "snapshot_prepare_ms": snapshot_prepare_ms, "snapshot_confirm_ms": snapshot_confirm_ms,
           "snapshot_reopen_verify_ms": snapshot_reopen_verify_ms,
           "backup_sqlite_bytes": backup_info.len(), "canonical_prepared_bytes": summary.canonical_bytes,
           "published_snapshot_bytes": published_snapshot_bytes, "published_snapshot_path": recovery,
           "reopened_history_rows": reopened.index.len(), "snapshot_history_rows": snapshot_state.index.len(),
           "snapshot_history_utf8_bytes": snapshot_state.history_bytes,
           "latest_recovered_chapter_revision": latest.current_revision(0),
           "live_db_wal_shm_open_bytes": files_now()})
}

// A separate file-gate control: the sparse padding is not 512 MiB of canonical
// manuscript or pages, and this deliberately does not test the at-limit file.
fn sqlite_file_limit() -> Value {
    const OVER_FILE_LIMIT: u64 = 512 * MIB as u64 + 1;
    let c = corpus();
    let draft = Draft::standard(2);
    let root = std::env::current_dir().expect("isolated file-limit directory");
    let backup = root.join("boundary-source.sqlite");
    let rejected_input = root.join("oversized-candidate.sqlite");
    let recovery_dir = root.join("boundary-recovery");
    let store = Arc::new(Mutex::new(open_live()));
    let manager = BackupJobManager::new(store.clone(), recovery_dir.clone());
    let source = verify(&store.lock().unwrap(), &c, draft);
    let guard = store.lock().unwrap().restore_guard().unwrap();
    let exported = wait(
        &manager,
        manager
            .start_export(backup.clone())
            .expect("export valid boundary fixture"),
    );
    check_job(&exported, "done");
    check_summary(
        prepare_restore(&backup, |_| Ok(()))
            .expect("small original backup is restorable")
            .summary(),
        &source,
    );
    let before_backup = fs::read(&backup).expect("read small existing backup bytes");
    let ((), create_fixture_ms) = timed(|| {
        write_new_file(&rejected_input, &before_backup).expect("exclusive synthetic fixture copy");
        let file = fs::OpenOptions::new()
            .write(true)
            .open(&rejected_input)
            .expect("open copy for sparse extension");
        file.set_len(OVER_FILE_LIMIT)
            .expect("set sparse candidate's logical file size");
        file.sync_all().expect("sync sparse candidate metadata");
    });
    let size = fs::metadata(&rejected_input).expect("stat candidate").len();
    assert_eq!(size, OVER_FILE_LIMIT);
    let (raw, raw_reject_ms) = timed(|| prepare_restore(&rejected_input, |_| Ok(())));
    let raw_error = raw.err().expect("sparse over-limit file was accepted");
    assert!(
        raw_error.contains("SQLite备份限512MiB"),
        "wrong raw rejection: {raw_error}"
    );
    let (failed, backend_reject_ms) = timed(|| {
        wait(
            &manager,
            manager
                .start_prepare_restore(rejected_input)
                .expect("run backend prepare of over-limit input"),
        )
    });
    check_job(&failed, "failed");
    assert_eq!(failed.error_code.as_deref(), Some("limit_exceeded"));
    assert!(
        failed.preview.is_none()
            && failed.result.is_none()
            && failed.confirmation_token.is_none()
            && !recovery_dir.exists(),
        "rejected file produced a preview or recovery artifact"
    );
    assert_eq!(guard, store.lock().unwrap().restore_guard().unwrap());
    assert!(
        source.index == verify(&store.lock().unwrap(), &c, draft).index,
        "file limit rejection changed source history metadata"
    );
    assert!(
        before_backup == fs::read(&backup).expect("read existing backup after reject"),
        "file limit rejection changed existing backup bytes"
    );
    manager.shutdown();
    json!({"oversized_candidate_bytes": size, "original_backup_bytes": before_backup.len(),
           "source_history_rows": source.index.len(), "raw_error": raw_error, "raw_reject_ms": raw_reject_ms,
           "backend_reject_ms": backend_reject_ms, "backend_error_code": failed.error_code,
           "backend_error_message": failed.error_message, "create_sparse_fixture_ms": create_fixture_ms,
           "source_data_unchanged": true, "existing_backup_byte_identical": true})
}

fn n(value: &str) -> i64 {
    let parsed: i64 = value.parse().expect("expected integral revision");
    assert!(
        (0..=128).contains(&parsed),
        "revision outside bounded workload"
    );
    parsed
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let relative = args.iter().map(String::as_str).collect::<Vec<_>>();
    let metrics = match relative.as_slice() {
        ["seed"] => seed(),
        ["large-seed"] => large::seed_large(),
        ["large-measure", case, round] => large::measure_large(case, n(round) as usize),
        ["sqlite-file-limit"] => sqlite_file_limit(),
        ["grow", from, to] if n(from) < n(to) => grow(n(from), n(to)),
        ["probe", rev] if n(rev) > 0 => probe(n(rev)),
        ["json", rev] if matches!(n(rev), 2 | 128) => json_boundary(n(rev)),
        ["restore", rev, round] if matches!(n(rev), 2 | 128) && matches!(n(round), 1..=3) => {
            backup_round(n(rev), n(round) as u8)
        }
        _ => panic!("invalid stage; use scripts/longform_scale.py"),
    };
    println!("{metrics}");
}
