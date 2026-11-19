//! Encrypted single-conversation chat archives.
//!
//! The archive carries text history only; attachments stay on the original device.
//! The same password-derived AES-256-GCM envelope as backups is used, with its own
//! magic so a backup can never be mistaken for a chat file (or the reverse).
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::storage::{ArchiveExport, ArchiveImportReport, ArchiveMessage, Store};

const MAGIC: &[u8; 12] = b"QBIRD-CHAT-1";
const FORMAT_VERSION: u8 = 1;
const MIN_PASSWORD: usize = 8;
const MAX_PASSWORD: usize = 256;
const MAX_ARCHIVE: u64 = 64 * 1024 * 1024 + 4096;
const MAX_MESSAGES: usize = 50_000;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    version: u8,
    kind: String,
    target: String,
    name: String,
    owner: Option<String>,
    exported_at: i64,
    messages: Vec<ArchiveMessage>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ChatArchiveInfo {
    pub kind: String,
    pub name: String,
    pub message_count: u64,
}

fn invalid(message: &str) -> String { message.to_owned() }

fn validate_password(password: &str) -> Result<(), String> {
    let length = password.chars().count();
    if length < MIN_PASSWORD || length > MAX_PASSWORD {
        return Err(format!("聊天记录密码需为 {MIN_PASSWORD}–{MAX_PASSWORD} 个字符"));
    }
    Ok(())
}

fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs() as i64).unwrap_or(0)
}

pub fn export(kind: &str, target: &str, password: &str, store: &Store, self_id: &str, destination: &Path)
    -> Result<ChatArchiveInfo, String> {
    validate_password(password)?;
    let text = destination.to_string_lossy();
    if text.is_empty() || text.len() > 4096 || text.contains('\0') { return Err("导出路径无效".into()); }
    if destination.exists() { return Err("目标文件已存在，请换一个文件名".into()); }
    let ArchiveExport { kind, target, name, owner, messages } = store.archive_export(kind, target, self_id)
        .map_err(|_| invalid("无法读取该会话；请确认联系人已确认或你仍是群成员"))?;
    if messages.len() > MAX_MESSAGES { return Err("会话消息过多，无法一次导出".into()); }
    let header = Header {
        version: FORMAT_VERSION, kind: kind.clone(), target, name: name.clone(),
        owner, exported_at: unix_now(), messages,
    };
    let payload = serde_json::to_vec(&header).map_err(|_| invalid("无法生成聊天记录文件"))?;
    let sealed = crate::backup::encrypt_with(MAGIC, password, &payload)?;
    fs::write(destination, sealed).map_err(|_| invalid("无法写入聊天记录文件"))?;
    let count = header.messages.len() as u64;
    Ok(ChatArchiveInfo { kind, name, message_count: count })
}

pub fn import(password: &str, path: &Path, store: &Store, self_id: &str) -> Result<ArchiveImportReport, String> {
    validate_password(password)?;
    let meta = fs::metadata(path).map_err(|_| invalid("无法读取聊天记录文件"))?;
    if !meta.is_file() || meta.len() > MAX_ARCHIVE { return Err("聊天记录文件无效或超出大小限制".into()); }
    let data = fs::read(path).map_err(|_| invalid("无法读取聊天记录文件"))?;
    let plaintext = crate::backup::decrypt_with(MAGIC, password, &data, MAX_ARCHIVE)?;
    let header: Header = serde_json::from_slice(&plaintext).map_err(|_| invalid("聊天记录内容损坏"))?;
    if header.version != FORMAT_VERSION { return Err("不支持的聊天记录格式版本".into()); }
    if header.messages.len() > MAX_MESSAGES { return Err("聊天记录数量超出限制".into()); }
    store.archive_import(&header.kind, &header.target, self_id, &header.messages, unix_now())
        .map_err(|_| invalid("无法导入：会话不存在、未确认联系人或内容无效"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Store;

    /// `Store::open` needs a real path, so each test uses its own temporary database.
    fn memory_store(label: &str) -> (Store, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "qbird-chat-{label}-{}-{}.db", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|value| value.as_nanos()).unwrap_or(0)));
        (Store::open(&path).unwrap(), path)
    }

    fn temp_file(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("qbird-chat-{label}-{}-{}.qchat", std::process::id(), unix_now()))
    }

    #[test]
    fn chat_archive_roundtrips_and_merges_without_overwriting() {
        let (store, store_path) = memory_store("source");
        let me = "a".repeat(64);
        let peer = "b".repeat(64);
        store.add_candidate(&peer, "Bob").unwrap();
        store.confirm_candidate(&peer).unwrap();
        let message = store.queue_direct(&me, &peer, "你好", 1).unwrap();
        let reply = store.receive_direct(&peer, "reply-1", "在的", 2, message.logical + 1).unwrap();
        assert!(matches!(reply, crate::storage::IncomingResult::Inserted));

        let path = temp_file("roundtrip");
        let info = export("direct", &peer, "password123", &store, &me, &path).unwrap();
        assert_eq!(info.message_count, 2);
        // A second export to the same path is refused, never overwritten.
        assert!(export("direct", &peer, "password123", &store, &me, &path).is_err());
        // Wrong password and a missing file both fail cleanly.
        assert!(import("wrongpass", &path, &store, &me).is_err());
        assert!(import("password123", &temp_file("missing"), &store, &me).is_err());

        // Importing into a fresh store inserts, then a repeat only skips.
        let (target, target_path) = memory_store("target");
        target.add_candidate(&peer, "Bob").unwrap();
        target.confirm_candidate(&peer).unwrap();
        let report = import("password123", &path, &target, &me).unwrap();
        assert_eq!((report.inserted, report.skipped, report.conflicts), (2, 0, 0));
        let repeat = import("password123", &path, &target, &me).unwrap();
        assert_eq!((repeat.inserted, repeat.skipped), (0, 2));

        // Unverified peers cannot import.
        let (stranger, stranger_path) = memory_store("stranger");
        assert!(import("password123", &path, &stranger, &me).is_err());

        for file in [&path, &store_path, &target_path, &stranger_path] { let _ = std::fs::remove_file(file); }
    }

    #[test]
    fn chat_archive_rejects_oversized_or_foreign_files() {
        let (store, store_path) = memory_store("foreign");
        let me = "a".repeat(64);
        let path = temp_file("foreign");
        std::fs::write(&path, b"not an encrypted archive").unwrap();
        assert!(import("password123", &path, &store, &me).is_err());
        assert!(export("direct", &"b".repeat(64), "short", &store, &me, &temp_file("pw")).is_err());
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&store_path);
    }
}
