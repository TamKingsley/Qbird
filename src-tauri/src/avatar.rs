//! Locally stored profile avatar.
//!
//! The avatar is a single square PNG living next to the database. It is local
//! only in this stage and never sent to peers; the bytes are checked for a PNG
//! signature, square dimensions and a bounded size before replacing the file.
//! Only the header is inspected, so this is a sanity check rather than a full
//! decoder: the image is produced by the app's own canvas crop.
use std::fs;
use std::path::{Path, PathBuf};

pub const MAX_AVATAR_BYTES: usize = 512 * 1024;
pub const MAX_SOURCE_BYTES: usize = 8 * 1024 * 1024;
pub const MIN_EDGE: u32 = 64;
pub const MAX_EDGE: u32 = 1024;

const FILE_NAME: &str = "avatar.png";
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

pub fn avatar_path(dir: &Path) -> PathBuf {
    dir.join(FILE_NAME)
}

fn png_edge(bytes: &[u8]) -> Option<u32> {
    if bytes.len() < 24 || bytes[..8] != PNG_SIGNATURE || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    (width == height).then_some(width)
}

pub fn validate(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > MAX_AVATAR_BYTES {
        return Err("头像图片不能超过 512 KiB".into());
    }
    let Some(edge) = png_edge(bytes) else {
        return Err("头像必须是正方形 PNG 图片".into());
    };
    if !(MIN_EDGE..=MAX_EDGE).contains(&edge) {
        return Err(format!("头像边长需为 {MIN_EDGE}–{MAX_EDGE} 像素"));
    }
    Ok(())
}

/// Writes through a temporary file so a failed write never leaves a half image.
pub fn save(dir: &Path, bytes: &[u8]) -> Result<(), String> {
    validate(bytes)?;
    fs::create_dir_all(dir).map_err(|_| "无法写入头像".to_owned())?;
    let temp = dir.join("avatar.png.tmp");
    fs::write(&temp, bytes).map_err(|_| "无法写入头像".to_owned())?;
    fs::rename(&temp, avatar_path(dir)).map_err(|_| "无法写入头像".to_owned())
}

/// A corrupt or oversized file is treated as "no avatar" rather than an error.
pub fn load(dir: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::read(avatar_path(dir)) {
        Ok(bytes) if validate(&bytes).is_ok() => Ok(Some(bytes)),
        Ok(_) => Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("无法读取头像".to_owned()),
    }
}

pub fn remove(dir: &Path) -> Result<(), String> {
    match fs::remove_file(avatar_path(dir)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("无法移除头像".to_owned()),
    }
}

/// Reads a user-picked source image for cropping: bounded size, PNG or JPEG only.
pub fn read_source(path: &Path) -> Result<Vec<u8>, String> {
    let meta = fs::metadata(path).map_err(|_| "无法读取所选图片".to_owned())?;
    if !meta.is_file() {
        return Err("所选路径不是文件".into());
    }
    if meta.len() as usize > MAX_SOURCE_BYTES {
        return Err("所选图片不能超过 8 MiB".into());
    }
    let bytes = fs::read(path).map_err(|_| "无法读取所选图片".to_owned())?;
    let is_png = bytes.len() > 8 && bytes[..8] == PNG_SIGNATURE;
    let is_jpeg = bytes.len() > 3 && bytes[..3] == [0xff, 0xd8, 0xff];
    if !is_png && !is_jpeg {
        return Err("仅支持 PNG 或 JPEG 图片".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(edge: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&PNG_SIGNATURE);
        bytes.extend_from_slice(&[0, 0, 0, 13]);
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&edge.to_be_bytes());
        bytes.extend_from_slice(&edge.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
        bytes
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "qbird-avatar-{label}-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn avatar_roundtrip_rejects_bad_input_and_removes() {
        let dir = temp_dir("roundtrip");
        assert!(load(&dir).unwrap().is_none());
        assert!(validate(b"not a png").is_err());
        assert!(validate(&png(32)).is_err());
        assert!(validate(&png(2048)).is_err());
        assert!(save(&dir, &png(512)).is_ok());
        assert_eq!(load(&dir).unwrap().unwrap(), png(512));
        remove(&dir).unwrap();
        assert!(load(&dir).unwrap().is_none());
        // Removing twice stays idempotent.
        remove(&dir).unwrap();
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn oversized_avatar_is_rejected() {
        let dir = temp_dir("oversized");
        let mut bytes = png(512);
        bytes.resize(MAX_AVATAR_BYTES + 1, 0);
        assert!(save(&dir, &bytes).is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn source_image_must_be_a_bounded_png_or_jpeg() {
        let dir = temp_dir("source");
        let png_path = dir.join("a.png");
        fs::write(&png_path, png(512)).unwrap();
        assert_eq!(read_source(&png_path).unwrap().len(), png(512).len());
        let jpeg_path = dir.join("b.jpg");
        fs::write(&jpeg_path, [0xff, 0xd8, 0xff, 0xe0, 0, 0]).unwrap();
        assert!(read_source(&jpeg_path).is_ok());
        let text_path = dir.join("c.txt");
        fs::write(&text_path, b"plain text").unwrap();
        assert!(read_source(&text_path).is_err());
        assert!(read_source(&dir).is_err());
        let _ = fs::remove_dir_all(dir);
    }
}
