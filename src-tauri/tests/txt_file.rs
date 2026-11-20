use novel_core::files::read_txt_file;
use std::fs;
#[test]
fn reads_chosen_file_bytes_without_normalizing_or_writing_source() {
    let p = std::env::temp_dir().join(format!("mojuan-txt-{}.txt", uuid::Uuid::new_v4()));
    let bytes = b"\xef\xbb\xbfchapter\r\nbody\nlast";
    fs::write(&p, bytes).unwrap();
    assert_eq!(read_txt_file(&p).unwrap(), bytes);
    assert_eq!(fs::read(&p).unwrap(), bytes);
    fs::remove_file(p).unwrap();
}
#[test]
fn rejects_oversized_and_missing_file() {
    let p = std::env::temp_dir().join(format!("mojuan-txt-{}.txt", uuid::Uuid::new_v4()));
    assert!(read_txt_file(&p).is_err());
    let f = fs::File::create(&p).unwrap();
    f.set_len(16 * 1024 * 1024 + 1).unwrap();
    drop(f);
    assert!(read_txt_file(&p).is_err());
    assert_eq!(fs::metadata(&p).unwrap().len(), 16 * 1024 * 1024 + 1);
    fs::remove_file(p).unwrap();
}
