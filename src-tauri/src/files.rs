//! File metadata, hashing and path helpers for direct file transfer.
//! Paths are derived from the local message ID, never from peer-supplied text.
use std::{fs::{self, File}, io::{self, Read}, path::{Path, PathBuf}};
use sha2::{Digest, Sha256};

pub use crate::protocol::MAX_FILE_BYTES;
pub const MAX_PREVIEW_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_TEXT_PREVIEW_BYTES: u64 = 256 * 1024;
/// Upper bound for workspace previews such as PDF. Larger than image previews but
/// still bounded so the IPC payload and WebView memory stay predictable.
pub const MAX_WORKSPACE_BYTES: u64 = 64 * 1024 * 1024;

/// Preview class decided from the local name only; never from peer-supplied MIME.
pub fn preview_kind(name: &str) -> &'static str {
    let extension = Path::new(name).extension().and_then(|ext| ext.to_str()).map(|ext| ext.to_ascii_lowercase());
    match extension.as_deref() {
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp") => "image",
        Some("html" | "htm") => "html",
        Some("md" | "markdown") => "markdown",
        Some("txt" | "log" | "csv" | "json" | "xml" | "ini" | "toml" | "yaml" | "yml") => "text",
        _ => "other",
    }
}

/// Read a bounded UTF-8 text preview; binary or oversized files are refused rather
/// than truncated silently, so the UI can explain why there is no preview.
pub fn read_text_preview(path: &Path) -> io::Result<Option<String>> {
    let size = file_size(path)?;
    if size == 0 || size > MAX_TEXT_PREVIEW_BYTES {
        return Ok(None);
    }
    let data = fs::read(path)?;
    if data.iter().take(8192).any(|byte| *byte == 0) {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&data);
    Ok(Some(text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned()))
}

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

/// PDF is the only workspace preview that needs raw bytes rather than text.
pub fn is_pdf(name: &str) -> bool {
    matches!(
        Path::new(name).extension().and_then(|ext| ext.to_str()).map(|ext| ext.to_ascii_lowercase()).as_deref(),
        Some("pdf")
    )
}

/// Read a completed PDF after checking the `%PDF-` header, so a renamed file is
/// never handed to the built-in viewer as a PDF. `None` means “not previewable”.
pub fn read_pdf_preview(path: &Path) -> io::Result<Option<Vec<u8>>> {
    let size = file_size(path)?;
    if size == 0 || size > MAX_WORKSPACE_BYTES {
        return Ok(None);
    }
    let data = fs::read(path)?;
    if !data.starts_with(b"%PDF-") {
        return Ok(None);
    }
    Ok(Some(data))
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

const MAX_RECV_PATH: usize = 240;

fn trimmed_lower(path: &str) -> String {
    path.trim().trim_end_matches(|c| c == '\\' || c == '/').to_lowercase()
}

fn is_or_inside(candidate: &str, base: &str) -> bool {
    candidate == base || candidate.starts_with(&format!("{base}\\")) || candidate.starts_with(&format!("{base}/"))
}

/// Validate and normalize a user-chosen receive directory.
/// The directory must be absolute, writable and outside Windows/Program Files and
/// the install directory, because uninstalling or overwriting those would delete
/// user attachments. Returns the path to persist on success.
pub fn validate_recv_dir(raw: &str, exe_dir: Option<&Path>, data_dir: Option<&Path>) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() { return Err("请选择接收目录".into()); }
    if trimmed.len() > MAX_RECV_PATH { return Err("路径过长（最多 240 个字符）".into()); }
    let path = Path::new(trimmed);
    if !path.is_absolute() { return Err("请选择完整的绝对路径".into()); }
    if path.components().any(|part| matches!(part, std::path::Component::ParentDir)) {
        return Err("路径不能包含 ..".into());
    }
    let lower = trimmed_lower(trimmed);
    if lower.len() <= 2 && lower.ends_with(':') { return Err("不能选择磁盘根目录".into()); }
    for variable in ["WINDIR", "ProgramFiles", "ProgramFiles(x86)"] {
        if let Ok(value) = std::env::var(variable) {
            let protected = trimmed_lower(&value);
            if !protected.is_empty() && is_or_inside(&lower, &protected) {
                return Err("不能选择 Windows 或 Program Files 等受保护目录".into());
            }
        }
    }
    if let Some(exe_dir) = exe_dir {
        let install = trimmed_lower(&exe_dir.to_string_lossy());
        // The install directory is protected except its data folder, which is the
        // documented data location of an installed build.
        let allowed = data_dir.map(|dir| trimmed_lower(&dir.to_string_lossy())).filter(|dir| !dir.is_empty());
        let inside_data = allowed.as_deref().map(|dir| is_or_inside(&lower, dir)).unwrap_or(false);
        if !install.is_empty() && is_or_inside(&lower, &install) && !inside_data {
            return Err("不能选择程序安装目录，卸载或覆盖安装可能删除附件".into());
        }
    }
    if !path.exists() {
        fs::create_dir_all(path).map_err(|_| "无法创建目录，请检查路径与权限".to_owned())?;
    }
    if !path.is_dir() { return Err("所选路径不是文件夹".into()); }
    // Probe writability without leaving a file behind.
    let probe = path.join(format!(".qbird-write-test-{}", std::process::id()));
    fs::write(&probe, b"probe").map_err(|_| "目录不可写，请换一个位置".to_owned())?;
    let _ = fs::remove_file(&probe);
    Ok(trimmed.trim_end_matches(|c| c == '\\' || c == '/').to_owned())
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
        assert!(is_pdf("report.PDF") && !is_pdf("report.pdf.txt"));
    }

    #[test]
    fn preview_kind_is_decided_by_local_extension() {
        assert_eq!(preview_kind("a.PNG"), "image");
        assert_eq!(preview_kind("a.html"), "html");
        assert_eq!(preview_kind("a.md"), "markdown");
        assert_eq!(preview_kind("报告.TXT"), "text");
        assert_eq!(preview_kind("a.pdf"), "other");
        assert_eq!(preview_kind("a.exe"), "other");
        assert_eq!(preview_kind("noext"), "other");
    }

    #[test]
    fn text_preview_is_bounded_and_rejects_binary() {
        let dir = std::env::temp_dir();
        let base = format!("qbird-preview-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
        let text = dir.join(format!("{base}.md"));
        fs::write(&text, "# 标题\n正文").unwrap();
        assert_eq!(read_text_preview(&text).unwrap().as_deref(), Some("# 标题\n正文"));
        let binary = dir.join(format!("{base}.bin"));
        fs::write(&binary, [0u8, 1, 2, 3]).unwrap();
        assert_eq!(read_text_preview(&binary).unwrap(), None);
        let large = dir.join(format!("{base}-large.txt"));
        fs::write(&large, vec![b'a'; (MAX_TEXT_PREVIEW_BYTES + 1) as usize]).unwrap();
        assert_eq!(read_text_preview(&large).unwrap(), None);
        remove_quietly(&text);
        remove_quietly(&binary);
        remove_quietly(&large);
    }

    #[test]
    fn receive_directory_validation_rejects_unsafe_paths() {
        assert!(validate_recv_dir("", None, None).is_err());
        assert!(validate_recv_dir("relative\\dir", None, None).is_err());
        assert!(validate_recv_dir("C:\\", None, None).is_err());
        if let Ok(windir) = std::env::var("WINDIR") {
            assert!(validate_recv_dir(&windir, None, None).is_err());
        }
        let base = std::env::temp_dir().join(format!("qbird-recv-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let inner = base.join("附件");
        let saved = validate_recv_dir(&inner.to_string_lossy(), None, None).unwrap();
        assert!(Path::new(&saved).is_dir());
        // Anything inside the install directory is refused, except its data folder.
        assert!(validate_recv_dir(&inner.to_string_lossy(), Some(&base), None).is_err());
        assert!(validate_recv_dir(&inner.to_string_lossy(), Some(&base), Some(&base)).is_ok());
        let _ = fs::remove_dir_all(&base);
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
