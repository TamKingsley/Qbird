//! Encrypted single-conversation chat archives.
//!
//! The archive carries one conversation's messages and selected complete attachments.
//! The same password-derived AES-256-GCM envelope as backups is used, with its own
//! magic so a backup can never be mistaken for a chat file (or the reverse).
use std::fs;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::storage::{ArchiveExport, ArchiveImportReport, ArchiveMessage, Store};

const MAGIC: &[u8; 12] = b"QBIRD-CHAT-1";
const FORMAT_VERSION: u8 = 2;
const MIN_PASSWORD: usize = 8;
const MAX_PASSWORD: usize = 256;
const MAX_ATTACH_TOTAL: u64 = 512 * 1024 * 1024;
const MAX_ARCHIVE: u64 = 768 * 1024 * 1024;
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
    #[serde(default)]
    attachments: Vec<ArchiveAttachment>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArchiveAttachment {
    message_id: String,
    data_b64: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ChatArchiveInfo {
    pub kind: String,
    pub name: String,
    pub message_count: u64,
    pub attachment_count: u64,
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

pub fn export(kind: &str, target: &str, password: &str, store: &Store, self_id: &str, destination: &Path, since: i64, until: i64)
    -> Result<ChatArchiveInfo, String> {
    export_with_options(kind, target, password, store, self_id, destination, since, until, None, true, true, true)
}

pub fn export_with_options(kind: &str, target: &str, password: &str, store: &Store, self_id: &str, destination: &Path,
    since: i64, until: i64, message_ids: Option<&[String]>, include_text: bool, include_images: bool, include_files: bool)
    -> Result<ChatArchiveInfo, String> {
    validate_password(password)?;
    let text = destination.to_string_lossy();
    if text.is_empty() || text.len() > 4096 || text.contains('\0') { return Err("导出路径无效".into()); }
    if destination.exists() { return Err("目标文件已存在，请换一个文件名".into()); }
    let ArchiveExport { kind, target, name, owner, messages, attachments: candidates } =
        store.archive_export_with_options(kind, target, self_id, since, until, message_ids, include_text, include_images, include_files)
        .map_err(|_| invalid("无法读取会话，或会话附件未完成/缺失；请确认权限并检查附件状态"))?;
    if messages.len() > MAX_MESSAGES { return Err("会话消息过多，无法一次导出".into()); }
    let mut attachments = Vec::with_capacity(candidates.len());
    let mut attachment_bytes = 0u64;
    for candidate in candidates {
        let name = candidate.file_name.as_deref().unwrap_or("未知附件");
        let path = candidate.path.as_deref().ok_or_else(|| format!("附件缺失，无法导出：{name}"))?;
        let size = candidate.file_size.filter(|value| *value > 0).ok_or_else(|| format!("附件大小无效，无法导出：{name}"))? as u64;
        let expected_hash = candidate.file_sha256.as_deref().filter(|hash| valid_sha256(hash))
            .ok_or_else(|| format!("附件校验信息缺失，无法导出：{name}"))?;
        if size > crate::protocol::MAX_FILE_BYTES { return Err(format!("附件超出大小限制，无法导出：{name}")); }
        let metadata = fs::metadata(path).map_err(|_| format!("附件缺失或不可读取，无法导出：{name}"))?;
        if !metadata.is_file() || metadata.len() != size { return Err(format!("附件未完整保存，无法导出：{name}")); }
        let data = fs::read(path).map_err(|_| format!("附件读取失败，无法导出：{name}"))?;
        if data.len() as u64 != size || sha256_hex(&data) != expected_hash {
            return Err(format!("附件大小或校验值不匹配，无法导出：{name}"));
        }
        attachment_bytes = attachment_bytes.saturating_add(size);
        if attachment_bytes > MAX_ATTACH_TOTAL { return Err("所选附件总量超过 512 MiB，无法导出".into()); }
        attachments.push(ArchiveAttachment { message_id: candidate.message_id, data_b64: crate::base64::encode(&data) });
    }
    let header = Header {
        version: FORMAT_VERSION, kind: kind.clone(), target, name: name.clone(),
        owner, exported_at: unix_now(), messages, attachments,
    };
    let payload = serde_json::to_vec(&header).map_err(|_| invalid("无法生成聊天记录文件"))?;
    if payload.len() as u64 > MAX_ARCHIVE { return Err("聊天记录档案超过 768 MiB 限制".into()); }
    let sealed = crate::backup::encrypt_with(MAGIC, password, &payload)?;
    let mut output = fs::OpenOptions::new().write(true).create_new(true).open(destination)
        .map_err(|error| if error.kind() == std::io::ErrorKind::AlreadyExists {
            invalid("目标文件已存在，请换一个文件名")
        } else { invalid("无法写入聊天记录文件") })?;
    if output.write_all(&sealed).and_then(|_| output.sync_all()).is_err() {
        drop(output);
        if fs::remove_file(destination).is_err() {
            return Err(invalid("写入失败，且无法移除不完整聊天记录文件"));
        }
        return Err(invalid("无法写入聊天记录文件"));
    }
    let count = header.messages.len() as u64;
    Ok(ChatArchiveInfo { kind, name, message_count: count, attachment_count: header.attachments.len() as u64 })
}

pub fn import(password: &str, path: &Path, store: &Store, self_id: &str) -> Result<ArchiveImportReport, String> {
    validate_password(password)?;
    let meta = fs::metadata(path).map_err(|_| invalid("无法读取聊天记录文件"))?;
    if !meta.is_file() || meta.len() > MAX_ARCHIVE { return Err("聊天记录文件无效或超出大小限制".into()); }
    let data = fs::read(path).map_err(|_| invalid("无法读取聊天记录文件"))?;
    let plaintext = crate::backup::decrypt_with(MAGIC, password, &data, MAX_ARCHIVE)?;
    let header: Header = serde_json::from_slice(&plaintext).map_err(|_| invalid("聊天记录内容损坏"))?;
    if !matches!(header.version, 1 | FORMAT_VERSION) { return Err("不支持的聊天记录格式版本".into()); }
    if header.messages.len() > MAX_MESSAGES { return Err("聊天记录数量超出限制".into()); }
    let attachments = validate_attachments(&header)?;
    store.archive_import_with_attachments(&header.kind, &header.target, self_id, &header.messages, &attachments, unix_now())
        .map_err(|_| invalid("无法导入：会话不存在、未确认联系人或内容无效"))
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(data).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn validate_attachments(header: &Header) -> Result<Vec<(String, Vec<u8>)>, String> {
    let messages: std::collections::HashMap<&str, &ArchiveMessage> = header.messages.iter()
        .map(|message| (message.id.as_str(), message)).collect();
    if messages.len() != header.messages.len() { return Err(invalid("聊天记录消息 ID 重复")); }
    let mut seen = std::collections::HashSet::with_capacity(header.attachments.len());
    let mut total = 0u64;
    let mut decoded = Vec::with_capacity(header.attachments.len());
    for attachment in &header.attachments {
        if attachment.message_id.is_empty() || attachment.message_id.len() > 128 || !seen.insert(attachment.message_id.as_str()) {
            return Err(invalid("聊天记录附件索引无效"));
        }
        let Some(message) = messages.get(attachment.message_id.as_str()).copied() else {
            return Err(invalid("聊天记录附件找不到对应消息"));
        };
        if message.kind != "file" || message.file_name.is_none() || message.file_size.is_none()
            || message.file_sha256.as_deref().is_none_or(|hash| !valid_sha256(hash)) {
            return Err(invalid("聊天记录附件元数据无效"));
        }
        let expected_size = message.file_size.filter(|size| *size > 0 && *size as u64 <= crate::protocol::MAX_FILE_BYTES)
            .ok_or_else(|| invalid("聊天记录附件大小无效"))? as u64;
        let encoded_size = expected_size.saturating_add(2) / 3 * 4;
        if attachment.data_b64.len() as u64 != encoded_size || expected_size > MAX_ATTACH_TOTAL.saturating_sub(total) {
            return Err(invalid("聊天记录附件大小超过限制或编码长度不匹配"));
        }
        let data = crate::base64::decode(&attachment.data_b64).ok_or_else(|| invalid("聊天记录附件数据损坏"))?;
        if data.len() as u64 != expected_size || sha256_hex(&data) != message.file_sha256.as_deref().unwrap_or_default() {
            return Err(invalid("聊天记录附件大小或校验值不匹配"));
        }
        total += expected_size;
        decoded.push((attachment.message_id.clone(), data));
    }
    if header.version >= 2 {
        for message in &header.messages {
            if message.kind == "file" && !seen.contains(message.id.as_str()) {
                return Err(invalid("聊天记录缺少文件附件内容"));
            }
        }
    }
    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Store;

    /// `Store::open` needs a real path, so each test uses its own temporary database.
    fn memory_store(label: &str) -> (Store, std::path::PathBuf) {
        let directory = std::env::temp_dir().join(format!(
            "qbird-chat-{label}-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|value| value.as_nanos()).unwrap_or(0)));
        std::fs::create_dir_all(&directory).unwrap();
        (Store::open(&directory.join("store.db")).unwrap(), directory)
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
        let info = export("direct", &peer, "password123", &store, &me, &path, 0, 0).unwrap();
        assert_eq!(info.message_count, 2);
        // A second export to the same path is refused, never overwritten.
        let original_archive = std::fs::read(&path).unwrap();
        assert!(export("direct", &peer, "password123", &store, &me, &path, 0, 0).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original_archive);
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

        let _ = std::fs::remove_file(&path);
        for directory in [&store_path, &target_path, &stranger_path] { let _ = std::fs::remove_dir_all(directory); }
    }

    #[test]
    fn chat_archive_rejects_oversized_or_foreign_files() {
        let (store, store_path) = memory_store("foreign");
        let me = "a".repeat(64);
        let path = temp_file("foreign");
        std::fs::write(&path, b"not an encrypted archive").unwrap();
        assert!(import("password123", &path, &store, &me).is_err());
        assert!(export("direct", &"b".repeat(64), "short", &store, &me, &temp_file("pw"), 0, 0).is_err());
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&store_path);
    }

    #[test]
    fn chat_archive_v2_roundtrips_file_attachments_and_deduplicates_restore() {
        let (source, source_dir) = memory_store("attachment-source");
        let me = "a".repeat(64);
        let peer = "b".repeat(64);
        source.add_candidate(&peer, "Bob").unwrap();
        source.confirm_candidate(&peer).unwrap();
        let bytes = b"attachment bytes for encrypted archive";
        let source_file = source_dir.join("report.bin");
        std::fs::write(&source_file, bytes).unwrap();
        let file_message = source.queue_direct_file(&me, &peer, "report.bin", bytes.len() as u64,
            &sha256_hex(bytes), &source_file, 10).unwrap();
        source.complete_file_transfer(&file_message.id, 11).unwrap();

        let archive_path = temp_file("attachment-roundtrip");
        let exported = export("direct", &peer, "password123", &source, &me, &archive_path, 0, 0).unwrap();
        assert_eq!((exported.message_count, exported.attachment_count), (1, 1));

        let (target, target_dir) = memory_store("attachment-target");
        target.add_candidate(&peer, "Bob").unwrap();
        target.confirm_candidate(&peer).unwrap();
        let imported = import("password123", &archive_path, &target, &me).unwrap();
        assert_eq!((imported.inserted, imported.restored_attachments), (1, 1));
        let messages = target.list_messages(&peer, 10, 0).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].body, "report.bin");
        let transfer = target.file_transfer(&file_message.id).unwrap().unwrap();
        assert_eq!(transfer.status, "completed");
        assert_eq!(transfer.transferred, bytes.len() as i64);
        let file_hits = target.search_all(&me, "report.bin", "file", Some("direct"), Some(&peer), 100, 0).unwrap();
        assert_eq!(file_hits.len(), 1);
        let restored_dir = target.attachments_dir().join("chat-archives");
        let restored_path = std::fs::read_dir(&restored_dir).unwrap().next().unwrap().unwrap().path();
        assert_eq!(std::fs::read(&restored_path).unwrap(), bytes);

        let repeated = import("password123", &archive_path, &target, &me).unwrap();
        assert_eq!((repeated.inserted, repeated.skipped, repeated.restored_attachments), (0, 1, 0));
        assert_eq!(std::fs::read_dir(restored_dir).unwrap().count(), 1);

        let _ = std::fs::remove_file(&archive_path);
        let _ = std::fs::remove_dir_all(&source_dir);
        let _ = std::fs::remove_dir_all(&target_dir);
    }

    #[test]
    fn chat_archive_import_accepts_legacy_v1_text_without_attachment_fields() {
        let (target, target_dir) = memory_store("legacy-v1-target");
        let me = "a".repeat(64);
        let peer = "b".repeat(64);
        target.add_candidate(&peer, "Bob").unwrap();
        target.confirm_candidate(&peer).unwrap();
        let legacy = format!(
            "{{\"version\":1,\"kind\":\"direct\",\"target\":\"{peer}\",\"name\":\"Bob\",\"owner\":null,\"exported_at\":1,\"messages\":[{{\"id\":\"legacy-1\",\"sender\":\"{peer}\",\"body\":\"old text\",\"created_at\":1,\"logical\":1}}]}}"
        );
        let archive_path = temp_file("legacy-v1");
        std::fs::write(&archive_path, crate::backup::encrypt_with(MAGIC, "password123", legacy.as_bytes()).unwrap()).unwrap();
        let report = import("password123", &archive_path, &target, &me).unwrap();
        assert_eq!((report.inserted, report.restored_attachments), (1, 0));
        let imported = target.list_messages(&peer, 10, 0).unwrap();
        assert_eq!(imported[0].body, "old text");
        assert!(target.search_all(&me, "old text", "text", Some("direct"), Some(&peer), 100, 0).unwrap().len() == 1);
        let _ = std::fs::remove_file(&archive_path);
        let _ = std::fs::remove_dir_all(&target_dir);
    }
}
