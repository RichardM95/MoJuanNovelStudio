//! Exact UTF-8 TXT size boundaries with real files and file-backed SQLite.
use novel_core::{
    files::{read_txt_file, write_new_file},
    Command, ExportOptions, Service,
};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
};

const MIB: usize = 1024 * 1024;
const IMPORT: usize = 16 * MIB;
const EXPORT: usize = 32 * MIB;
const BOM: &str = "\u{feff}";

struct TestDir(PathBuf);
impl TestDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("mojuan-txt-size-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        Self(dir)
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// Produce real UTF-8 data of exactly the requested byte size, using repeatable
// Chinese prose and valid ASCII for the final 0–2 bytes. No sparse files.
fn body(size: usize, opening: &str, closing: &str) -> String {
    let run = "潮水渐退，归舟贴着石岸寻路。";
    let remaining = size - opening.len() - closing.len();
    let mut text = String::with_capacity(size);
    text.push_str(opening);
    for _ in 0..remaining / run.len() {
        text.push_str(run);
    }
    let tail = remaining % run.len();
    text.push_str(&run[..tail / 3 * 3]);
    text.push_str(&"xy"[..tail % 3]);
    text.push_str(closing);
    assert_eq!(text.len(), size);
    text
}

#[test]
fn allocated_chinese_import_at_16_mib_succeeds_and_plus_one_is_readonly() {
    let dir = TestDir::new();
    let path = dir.0.join("双卷原稿.txt");
    let live = dir.0.join("import.sqlite");
    let pre1 = "\u{feff}第1卷 初潮\r\n第1章 首章\r\n";
    let pre2 = "第2卷 归潮\r\n第2章 尾章\r\n";
    let first = body(8 * MIB, "首章起\r\n", "首章终\r\n");
    let last = body(
        IMPORT - pre1.len() - pre2.len() - first.len(),
        "尾章起\r\n",
        "尾章终",
    );
    let mut fixture = String::with_capacity(IMPORT + 1);
    fixture.push_str(pre1);
    fixture.push_str(&first);
    fixture.push_str(pre2);
    fixture.push_str(&last);
    assert_eq!(fixture.len(), IMPORT);

    write_new_file(&path, fixture.as_bytes()).unwrap();
    assert_eq!(fs::metadata(&path).unwrap().len(), IMPORT as u64);
    let bytes = read_txt_file(&path).unwrap();
    assert_eq!(bytes, fixture.as_bytes());

    let mut service = Service::open(&live).unwrap();
    let preview = service.prepare_txt_import(&bytes, "双卷原稿.txt").unwrap();
    assert!(service.load().unwrap().books.is_empty());
    assert_eq!((preview.volumes.len(), preview.total_chapters), (2, 2));
    assert_eq!(preview.total_bytes, first.len() + last.len());
    for (i, expected) in [&first, &last].into_iter().enumerate() {
        let actual = service
            .get_txt_import_chapter(&preview.id, i as u32)
            .unwrap();
        assert_eq!(actual.body.as_bytes(), expected.as_bytes());
    }
    let imported = service.commit_txt_import(&preview.id).unwrap();
    let state = service.load().unwrap();
    assert_eq!((imported.volumes, imported.chapters), (2, 2));
    assert_eq!(state.books.len(), 1);
    let outline = service.get_outline(&imported.book_id).unwrap();
    assert_eq!(
        outline
            .volumes
            .iter()
            .map(|v| v.title.as_str())
            .collect::<Vec<_>>(),
        ["初潮", "归潮"]
    );
    let chapters = [("首章", &first), ("尾章", &last)];
    for (i, (title, expected)) in chapters.into_iter().enumerate() {
        let c = service.get_chapter(&outline.chapters[i].id).unwrap();
        assert_eq!(
            (
                c.title.as_str(),
                &c.volume_id,
                c.revision,
                c.body.as_bytes()
            ),
            (title, &outline.volumes[i].id, 0, expected.as_bytes())
        );
    }

    // The +1 case stays valid UTF-8, so errors exercise the byte limit.
    OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"x")
        .unwrap();
    fixture.push('x');
    assert_eq!(fs::metadata(&path).unwrap().len(), IMPORT as u64 + 1);
    let err = read_txt_file(&path).unwrap_err();
    assert!(
        err.contains("不超过16MiB") && err.contains("原文件未修改"),
        "{err}"
    );
    assert_eq!(fs::read(&path).unwrap(), fixture.as_bytes());

    // Independently exercise the service boundary when bytes are already loaded;
    // the desktop itself stops at the file check above.
    let prior_candidate = service
        .prepare_txt_import(b"small valid draft", "未提交.txt")
        .unwrap();
    let err = service
        .prepare_txt_import(fixture.as_bytes(), "超限.txt")
        .unwrap_err();
    assert_eq!(
        (err.code.as_str(), err.message.as_str()),
        ("limit_exceeded", "TXT文件上限16MiB，文件名最多4096字节")
    );
    assert!(service.commit_txt_import(&prior_candidate.id).is_err());
    assert_eq!(service.load().unwrap(), state);
    drop(service);
    assert_eq!(Service::open(&live).unwrap().load().unwrap(), state);
}

#[test]
fn allocated_chinese_export_at_32_mib_is_exact_and_one_more_is_rejected() {
    let dir = TestDir::new();
    let live = dir.0.join("export.sqlite");
    let output = dir.0.join("全书.txt");
    let exists = dir.0.join("已有.txt");
    let absent = dir.0.join("超限.txt");
    let sentinel = "这是原有文件，不可覆盖。".as_bytes();
    write_new_file(&exists, sentinel).unwrap();

    let mut service = Service::open(&live).unwrap();
    let shelf = service
        .apply(Command::CreateBook {
            title: "全书容量验收".into(),
            author: "合成测试".into(),
        })
        .unwrap();
    let book = &shelf.books[0].id;
    let first_volume = &shelf.volumes[0].id;
    service
        .apply(Command::RenameVolume {
            id: first_volume.clone(),
            title: "初潮".into(),
        })
        .unwrap();
    service
        .apply(Command::RenameChapter {
            id: shelf.chapters[0].id.clone(),
            title: "首章".into(),
        })
        .unwrap();
    let with_second = service
        .apply(Command::CreateVolume {
            book_id: book.clone(),
            title: "归潮".into(),
        })
        .unwrap();
    let second_volume = &with_second
        .volumes
        .iter()
        .find(|v| &v.id != first_volume)
        .unwrap()
        .id;
    for (volume, title) in [
        (first_volume, "承章"),
        (second_volume, "转章"),
        (second_volume, "尾章"),
    ] {
        service
            .apply(Command::CreateChapter {
                volume_id: volume.clone(),
                title: title.into(),
            })
            .unwrap();
    }
    let outline = service.get_outline(book).unwrap();
    assert_eq!((outline.volumes.len(), outline.chapters.len()), (2, 4));
    let headers = [
        "第1卷 初潮\r\n\r\n第1章 首章\r\n\r\n",
        "第2章 承章\r\n\r\n",
        "第2卷 归潮\r\n\r\n第3章 转章\r\n\r\n",
        "第4章 尾章\r\n\r\n",
    ];
    let overhead = BOM.len() + headers.iter().map(|h| h.len()).sum::<usize>() + 4 * 4;
    let sizes = [8 * MIB, 8 * MIB, 8 * MIB, EXPORT - 24 * MIB - overhead];
    let mut expected = String::with_capacity(EXPORT);
    expected.push_str(BOM);
    for (i, c) in outline.chapters.iter().enumerate() {
        let text = body(
            sizes[i],
            &format!("潮记{}起\r\n", i + 1),
            &format!("潮记{}止", i + 1),
        );
        assert!(text.len() < 10 * MIB);
        expected.push_str(headers[i]);
        expected.push_str(&text);
        expected.push_str("\r\n\r\n");
        service
            .apply(Command::SaveChapter {
                id: c.id.clone(),
                body: text,
                expected_revision: 0,
            })
            .unwrap();
    }
    assert_eq!(expected.len(), EXPORT);

    let options = ExportOptions {
        scope: "book".into(),
        target_id: None,
        include_metadata: false,
        include_headings: true,
        newline: "crlf".into(),
        bom: true,
    };
    let preview = service.prepare_txt_export(book, options.clone()).unwrap();
    assert_eq!(
        (preview.volumes, preview.chapters, preview.bytes),
        (2, 4, EXPORT)
    );
    assert!(preview.truncated);
    let expected_prefix: String = expected[BOM.len()..].chars().take(4000).collect();
    assert_eq!(preview.text, expected_prefix);
    let bytes = service.take_txt_export(&preview.id).unwrap();
    let err = write_new_file(&exists, &bytes).unwrap_err();
    assert!(err.contains("已有同名文件"), "{err}");
    assert_eq!(fs::read(&exists).unwrap(), sentinel);
    assert!(service.take_txt_export(&preview.id).is_err());
    drop(bytes);
    // Match the desktop retry: a taken candidate is spent even if file creation failed.
    let fresh = service.prepare_txt_export(book, options.clone()).unwrap();
    let bytes = service.take_txt_export(&fresh.id).unwrap();
    write_new_file(&output, &bytes).unwrap();
    assert_eq!(fs::metadata(&output).unwrap().len(), EXPORT as u64);
    assert_eq!(fs::read(&output).unwrap(), expected.as_bytes());
    drop(bytes);
    let err = read_txt_file(&output).unwrap_err();
    assert!(err.contains("不超过16MiB"), "{err}");

    // Keep an unconsumed candidate, then increase the visible intended result
    // by one valid ASCII byte without changing title/format/other chapters.
    let old_candidate = service.prepare_txt_export(book, options.clone()).unwrap();
    let last = &outline.chapters[3].id;
    let mut chapter = service.get_chapter(last).unwrap();
    chapter.body.push('x');
    assert_eq!(chapter.body.len(), sizes[3] + 1);
    service
        .apply(Command::SaveChapter {
            id: last.clone(),
            body: chapter.body,
            expected_revision: chapter.revision,
        })
        .unwrap();
    let before = service.load().unwrap();
    let history = service.chapter_history(last).unwrap();
    let err = service
        .prepare_txt_export(book, options.clone())
        .unwrap_err();
    assert_eq!(err.message, "导出超过32MiB，请改为单卷或单章");
    assert!(service.take_txt_export(&old_candidate.id).is_err());
    assert!(!absent.exists());
    assert_eq!(fs::read(&exists).unwrap(), sentinel);
    assert_eq!(fs::read(&output).unwrap(), expected.as_bytes());
    assert_eq!(service.load().unwrap(), before);
    assert_eq!(service.chapter_history(last).unwrap(), history);
    drop(service);

    let mut reopened = Service::open(&live).unwrap();
    assert_eq!(reopened.load().unwrap(), before);
    assert_eq!(reopened.chapter_history(last).unwrap(), history);
    let one = reopened
        .prepare_txt_export(
            book,
            ExportOptions {
                scope: "chapter".into(),
                target_id: Some(last.clone()),
                ..options.clone()
            },
        )
        .unwrap();
    assert_eq!(one.chapters, 1);
    let bytes = reopened.take_txt_export(&one.id).unwrap();
    let one = std::str::from_utf8(&bytes).unwrap();
    assert!(one.starts_with("\u{feff}第1卷 归潮\r\n\r\n第1章 尾章\r\n\r\n潮记4起"));
    assert!(one.ends_with("潮记4止x\r\n\r\n"));
    drop(bytes);
    drop(before);
    drop(history);

    // Same four saved chapter bodies, only encoding/newline options change.
    // The test oracle starts from the independent text above, not the preview.
    let crlf_count = expected.matches("\r\n").count();
    assert_eq!(crlf_count, 24);
    let mut lf_expected = expected[BOM.len()..].replace("\r\n", "\n");
    drop(expected);
    lf_expected.insert(lf_expected.len() - 2, 'x');
    assert_eq!(lf_expected.len(), EXPORT + 1 - BOM.len() - crlf_count);
    let lf = reopened
        .prepare_txt_export(
            book,
            ExportOptions {
                newline: "lf".into(),
                bom: false,
                ..options
            },
        )
        .unwrap();
    assert_eq!(
        (lf.volumes, lf.chapters, lf.bytes),
        (2, 4, lf_expected.len())
    );
    assert!(lf.truncated);
    assert_eq!(lf.text, lf_expected.chars().take(4000).collect::<String>());
    let lf_bytes = reopened.take_txt_export(&lf.id).unwrap();
    let lf_path = dir.0.join("全书-LF.txt");
    write_new_file(&lf_path, &lf_bytes).unwrap();
    drop(lf_bytes);
    assert_eq!(
        fs::metadata(&lf_path).unwrap().len(),
        lf_expected.len() as u64
    );
    assert_eq!(fs::read(&lf_path).unwrap(), lf_expected.as_bytes());
    assert_eq!(fs::read(&exists).unwrap(), sentinel);
}
