//! File metadata, hashing and path helpers for direct file transfer.
//! Paths are derived from the local message ID, never from peer-supplied text.
use std::{fs::{self, File}, io::{self, Read}, path::{Path, PathBuf}};
use sha2::{Digest, Sha256};

pub use crate::protocol::MAX_FILE_BYTES;
pub const MAX_PREVIEW_BYTES: u64 = 8 * 1024 * 1024;

const RESERVED: [&str; 1] = ["."];

/// Reduce a peer-supplied name to a safe single path segment. Returns `None` when
/// nothing usable remains, so the caller can reject instead of guessing.
pub fn sanitize_name(name: &str) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > crate::protocol::MAX_FILE_NAME {
        return None;
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return None;
    }
    let base = Path::new(trimmed).file_name()?.to_string_lossy().to_string();
    let cleaned: String = base
        .chars()
        .map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
        .collect();
    let cleaned = cleaned.trim_matches(|c: char| c == '.' || c == ' ').to_string();
    if cleaned.is_empty() || cleaned.len() > crate::protocol::MAX_FILE_NAME || RESERVED.contains(&cleaned.as_str()) {
        return None;
    }
    Some(cleaned)
}

pub fn is_image(name: &str) -> bool {
    matches!(
        Path::new(name).extension().and_then(|ext| ext.to_str()).map(|ext| ext.to_ascii_lowercase()).as_deref(),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp")
    )
}

/// Incoming: `<message id>-<safe name>` keeps the extension for external opening.
pub fn incoming_target(dir: &Path, id: &str, name: &str) -> PathBuf {
    dir.join(format!("{id}-{name}"))
}

pub fn incoming_partial(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.part"))
}

pub fn outgoing_source(store: &Path, id: &str, name: &str) -> PathBuf {
    store.join(format!("{id}-{name}"))
}

pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

pub fn hex(digest: &[u8]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn file_size(path: &Path) -> io::Result<u64> {
    let metadata = fs::metadata(path)?;
    if metadata.is_file() {
        Ok(metadata.len())
    } else {
        Err(io::Error::new(io::ErrorKind::InvalidInput, "not a regular file"))
    }
}

/// Standard base64 with padding, used only for bounded image previews.
pub fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        output.push(TABLE[(first >> 2) as usize] as char);
        output.push(TABLE[(((first & 0x03) << 4) | (second >> 4)) as usize] as char);
        output.push(if chunk.len() > 1 { TABLE[(((second & 0x0f) << 2) | (third >> 6)) as usize] as char } else { '=' });
        output.push(if chunk.len() > 2 { TABLE[(third & 0x3f) as usize] as char } else { '=' });
    }
    output
}

/// Remove a temporary or unverified file without touching anything outside the
/// attachments directory; missing files are not an error.
pub fn remove_quietly(path: &Path) {
    let _ = fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_reduced_to_safe_segments() {
        assert_eq!(sanitize_name("  报告.pdf ").as_deref(), Some("报告.pdf"));
        assert_eq!(sanitize_name("../secret.txt").as_deref(), Some("secret.txt"));
        assert_eq!(sanitize_name("a/b\\c:d*.png").as_deref(), Some("c_d_.png"));
        assert_eq!(sanitize_name(".."), None);
        assert_eq!(sanitize_name("  "), None);
        assert_eq!(sanitize_name("a\u{0000}b"), None);
        assert_eq!(sanitize_name(&"x".repeat(201)), None);
        assert!(is_image("photo.JPG") && is_image("a.png") && !is_image("a.pdf"));
    }

    #[test]
    fn hashing_and_base64_match_known_values() {
        assert_eq!(base64_encode(b"any carnal pleasure"), "YW55IGNhcm5hbCBwbGVhc3VyZQ==");
        assert_eq!(base64_encode(b"a"), "YQ==");
        assert_eq!(base64_encode(b"ab"), "YWI=");
        let path = std::env::temp_dir().join(format!("qbird-hash-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::write(&path, b"abc").unwrap();
        assert_eq!(sha256_file(&path).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        remove_quietly(&path);
    }
}
