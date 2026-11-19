//! Local sticker collection.
//!
//! Stickers are ordinary image files (PNG/JPEG/GIF/WebP, at most 512 KiB) kept
//! in the app data directory. They are local-only: sending a sticker reuses the
//! normal image file transfer. File names are content hashes, so adding the same
//! image twice keeps a single copy, and every name is validated before use to
//! keep reads inside the stickers directory.
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

pub const MAX_STICKER_BYTES: usize = 512 * 1024;
pub const MAX_STICKERS: usize = 64;
const DIR_NAME: &str = "stickers";

pub fn stickers_dir(root: &Path) -> PathBuf {
    root.join(DIR_NAME)
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 96
        && !name.contains("..")
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

fn extension(bytes: &[u8]) -> Option<&'static str> {
    let head = bytes;
    if head.len() > 8 && head[..8] == [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a] {
        return Some("png");
    }
    if head.len() > 6 && (head[..6] == *b"GIF87a" || head[..6] == *b"GIF89a") {
        return Some("gif");
    }
    if head.len() > 3 && head[..3] == [0xff, 0xd8, 0xff] {
        return Some("jpg");
    }
    if head.len() > 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        return Some("webp");
    }
    None
}

/// Lists stored stickers in stable name order; corrupt or unexpected files are skipped.
pub fn list(root: &Path) -> Result<Vec<String>, String> {
    let dir = stickers_dir(root);
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("无法读取表情收藏".to_owned()),
    };
    let mut names: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().map(|kind| kind.is_file()).unwrap_or(false))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| valid_name(name))
        .collect();
    names.sort();
    names.truncate(MAX_STICKERS);
    Ok(names)
}

/// Copies a picked image into the collection and returns its stored name.
pub fn add(root: &Path, source: &Path) -> Result<String, String> {
    let meta = fs::metadata(source).map_err(|_| "无法读取所选图片".to_owned())?;
    if !meta.is_file() {
        return Err("所选路径不是文件".into());
    }
    if meta.len() as usize > MAX_STICKER_BYTES {
        return Err("表情图片不能超过 512 KiB".into());
    }
    let bytes = fs::read(source).map_err(|_| "无法读取所选图片".to_owned())?;
    let Some(extension) = extension(&bytes) else {
        return Err("仅支持 PNG、JPEG、GIF 或 WebP 表情".into());
    };
    if bytes.len() > MAX_STICKER_BYTES {
        return Err("表情图片不能超过 512 KiB".into());
    }
    let dir = stickers_dir(root);
    fs::create_dir_all(&dir).map_err(|_| "无法保存表情".to_owned())?;
    let digest = Sha256::digest(&bytes);
    let name = format!("{}-{}.{}", crate::files::hex(&digest[..8]), bytes.len(), extension);
    if !valid_name(&name) {
        return Err("无法生成表情文件名".into());
    }
    let target = dir.join(&name);
    if target.exists() {
        return Ok(name);
    }
    if list(root)?.len() >= MAX_STICKERS {
        return Err(format!("表情收藏最多保存 {MAX_STICKERS} 个"));
    }
    fs::write(&target, &bytes).map_err(|_| "无法保存表情".to_owned())?;
    Ok(name)
}

pub fn path(root: &Path, name: &str) -> Result<PathBuf, String> {
    if !valid_name(name) {
        return Err("表情名称无效".into());
    }
    Ok(stickers_dir(root).join(name))
}

pub fn read(root: &Path, name: &str) -> Result<Vec<u8>, String> {
    let path = path(root, name)?;
    let meta = fs::metadata(&path).map_err(|_| "表情不存在".to_owned())?;
    if meta.len() as usize > MAX_STICKER_BYTES {
        return Err("表情文件超出大小上限".into());
    }
    fs::read(&path).map_err(|_| "无法读取表情".to_owned())
}

pub fn remove(root: &Path, name: &str) -> Result<(), String> {
    let path = path(root, name)?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("无法移除表情".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "qbird-stickers-{label}-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn png(edge: u32) -> Vec<u8> {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13];
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&edge.to_be_bytes());
        bytes.extend_from_slice(&edge.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
        bytes
    }

    #[test]
    fn add_read_list_remove_and_deduplicate() {
        let dir = temp_dir("crud");
        let source = dir.join("source.png");
        fs::write(&source, png(128)).unwrap();
        let name = add(&dir, &source).unwrap();
        assert_eq!(list(&dir).unwrap(), vec![name.clone()]);
        assert_eq!(read(&dir, &name).unwrap(), png(128));
        // The same bytes map to the same stored name.
        assert_eq!(add(&dir, &source).unwrap(), name);
        assert_eq!(list(&dir).unwrap().len(), 1);
        remove(&dir, &name).unwrap();
        assert!(list(&dir).unwrap().is_empty());
        // Removing twice stays idempotent.
        remove(&dir, &name).unwrap();
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_bad_names_and_unsupported_sources() {
        let dir = temp_dir("invalid");
        let text = dir.join("note.txt");
        fs::write(&text, b"plain").unwrap();
        assert!(add(&dir, &text).is_err());
        assert!(read(&dir, "../avatar.png").is_err());
        assert!(read(&dir, "missing.png").is_err());
        assert!(read(&dir, "bad/name.png").is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn oversized_source_is_rejected() {
        let dir = temp_dir("large");
        let source = dir.join("big.gif");
        let mut bytes = b"GIF89a".to_vec();
        bytes.resize(MAX_STICKER_BYTES + 1, 0);
        fs::write(&source, &bytes).unwrap();
        assert!(add(&dir, &source).is_err());
        let _ = fs::remove_dir_all(dir);
    }
}
