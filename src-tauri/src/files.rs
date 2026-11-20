use std::{fs::OpenOptions, io::Write, path::Path};
pub fn write_new_file(path: &Path, data: &[u8]) -> Result<(), String> {
    write_new_file_using(path, data, |file, data| {
        file.write_all(data).and_then(|_| file.sync_all())
    })
}
fn write_new_file_using(
    path: &Path,
    data: &[u8],
    write: impl FnOnce(&mut std::fs::File, &[u8]) -> std::io::Result<()>,
) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("无法新建文件（已有同名文件时请另选名称）：{e}"))?;
    let result = write(&mut file, data);
    drop(file);
    if let Err(e) = result {
        if std::fs::remove_file(path).is_err() {
            return Err(format!("写入失败且未能清理不完整文件，请检查目标位置：{e}"));
        }
        return Err(format!("写入失败：{e}"));
    }
    Ok(())
}

/// Read a user-selected local TXT file with a hard allocation/read ceiling.
/// Encoding is validated by the import parser; the bytes are never rewritten.
pub fn read_txt_file(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    const LIMIT: u64 = 16 * 1024 * 1024;
    let file = std::fs::File::open(path).map_err(|_| "无法读取所选TXT文件".to_string())?;
    let metadata = file
        .metadata()
        .map_err(|_| "无法读取文件信息".to_string())?;
    if !metadata.is_file() || metadata.len() > LIMIT {
        return Err("请选择不超过16MiB的TXT普通文件，原文件未修改".into());
    }
    let mut bytes = Vec::new();
    file.take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "读取TXT文件失败，未导入".to_string())?;
    if bytes.len() as u64 > LIMIT {
        return Err("TXT文件超过16MiB限制，未导入".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod export_write_tests {
    use super::*;
    #[test]
    fn existing_file_is_never_removed_or_replaced() {
        let path = std::env::temp_dir().join(format!("export-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"existing").unwrap();
        assert!(write_new_file(&path, b"replacement").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"existing");
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn partial_failure_removes_only_new_output() {
        let path = std::env::temp_dir().join(format!("export-{}", uuid::Uuid::new_v4()));
        let result = write_new_file_using(&path, b"whole", |f, _| {
            f.write_all(b"part")?;
            Err(std::io::Error::new(
                std::io::ErrorKind::StorageFull,
                "injected",
            ))
        });
        assert!(result.is_err());
        assert!(!path.exists());
    }
}
