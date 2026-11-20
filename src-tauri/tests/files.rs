use novel_core::files::write_new_file;
#[test]
fn existing_target_is_never_truncated() {
    let p = std::env::temp_dir().join(format!("novel-file-{}", uuid::Uuid::new_v4()));
    std::fs::write(&p, b"original").unwrap();
    assert!(write_new_file(&p, b"replacement").is_err());
    assert_eq!(std::fs::read(&p).unwrap(), b"original");
    std::fs::remove_file(p).unwrap()
}
#[test]
fn new_target_has_exact_utf8_bytes() {
    let p = std::env::temp_dir().join(format!("novel-file-{}", uuid::Uuid::new_v4()));
    write_new_file(&p, "中文\r\n正文".as_bytes()).unwrap();
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "中文\r\n正文");
    std::fs::remove_file(p).unwrap()
}
