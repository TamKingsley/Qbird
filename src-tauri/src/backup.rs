//! Password-protected SQLite snapshots.
//!
//! Format: magic | version | PBKDF2 iterations | salt | nonce | AES-256-GCM(snap)
//! The plaintext starts with its own magic, creation time, flags, message count and
//! the SHA-256 of the snapshot, so a wrong password and silent corruption are told apart.
use std::{fs, io::Write, num::NonZeroU32, path::Path, time::{SystemTime, UNIX_EPOCH}};

use ring::{aead, pbkdf2, rand::{SecureRandom, SystemRandom}};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::storage::{RestoreReport, Store};

const MAGIC: &[u8; 14] = b"QBIRD-BACKUP-1";
const SNAP_MAGIC: &[u8; 10] = b"QBIRDSNAP1";
const FORMAT_VERSION: u8 = 1;
const ITERATIONS: u32 = 200_000;
const SALT_LEN: usize = 32;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
const MIN_PASSWORD: usize = 8;
const MAX_PASSWORD: usize = 256;
const MAX_SNAPSHOT: u64 = 256 * 1024 * 1024;
const MAX_BACKUP: u64 = MAX_SNAPSHOT + 4096;
const CHECKSUM_HEX: usize = 64;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BackupInfo {
    pub created_at: i64,
    pub include_chat: bool,
    pub message_count: u64,
    pub checksum: String,
}

fn invalid(message: &str) -> String { message.to_owned() }

fn validate_password(password: &str) -> Result<(), String> {
    let length = password.chars().count();
    if length < MIN_PASSWORD || length > MAX_PASSWORD {
        return Err(format!("备份密码需为 {MIN_PASSWORD}–{MAX_PASSWORD} 个字符"));
    }
    Ok(())
}

fn validate_path(path: &Path) -> Result<(), String> {
    let text = path.to_string_lossy();
    if text.is_empty() || text.len() > 4096 || text.contains('\0') {
        return Err("备份路径无效".into());
    }
    Ok(())
}

fn unix_now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|value| value.as_secs() as i64).unwrap_or(0)
}

fn derive_key(password: &str, salt: &[u8]) -> Result<[u8; KEY_LEN], String> {
    let rounds = NonZeroU32::new(ITERATIONS).ok_or_else(|| invalid("无效的迭代次数"))?;
    let mut key = [0u8; KEY_LEN];
    pbkdf2::derive(pbkdf2::PBKDF2_HMAC_SHA256, rounds, salt, password.as_bytes(), &mut key);
    Ok(key)
}

fn aad(iterations: u32, salt: &[u8]) -> Vec<u8> {
    let mut value = Vec::with_capacity(MAGIC.len() + 5 + salt.len());
    value.extend_from_slice(MAGIC);
    value.push(FORMAT_VERSION);
    value.extend_from_slice(&iterations.to_be_bytes());
    value.extend_from_slice(salt);
    value
}

fn encrypt(password: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
    let rng = SystemRandom::new();
    let mut salt = [0u8; SALT_LEN];
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rng.fill(&mut salt).map_err(|_| invalid("随机数生成失败"))?;
    rng.fill(&mut nonce_bytes).map_err(|_| invalid("随机数生成失败"))?;
    let mut key_bytes = derive_key(password, &salt)?;
    let unbound = aead::UnboundKey::new(&aead::AES_256_GCM, &key_bytes).map_err(|_| invalid("无法初始化加密"))?;
    key_bytes.fill(0);
    let key = aead::LessSafeKey::new(unbound);
    let mut buffer = payload.to_vec();
    let nonce = aead::Nonce::assume_unique_for_key(nonce_bytes);
    key.seal_in_place_append_tag(nonce, aead::Aad::from(aad(ITERATIONS, &salt)), &mut buffer)
        .map_err(|_| invalid("加密失败"))?;
    let mut output = Vec::with_capacity(MAGIC.len() + 1 + 4 + SALT_LEN + NONCE_LEN + buffer.len());
    output.extend_from_slice(MAGIC);
    output.push(FORMAT_VERSION);
    output.extend_from_slice(&ITERATIONS.to_be_bytes());
    output.extend_from_slice(&salt);
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&buffer);
    Ok(output)
}

fn decrypt(password: &str, data: &[u8]) -> Result<Vec<u8>, String> {
    let header = MAGIC.len() + 1 + 4 + SALT_LEN + NONCE_LEN;
    if data.len() <= header || data.len() as u64 > MAX_BACKUP {
        return Err("备份文件不完整或超出大小限制".into());
    }
    if &data[..MAGIC.len()] != MAGIC {
        return Err("不是 Qbird 备份文件".into());
    }
    let version = data[MAGIC.len()];
    if version != FORMAT_VERSION {
        return Err("不支持的备份格式版本".into());
    }
    let mut cursor = MAGIC.len() + 1;
    let iterations = u32::from_be_bytes(data[cursor..cursor + 4].try_into().map_err(|_| invalid("备份头损坏"))?);
    cursor += 4;
    if !(10_000..=1_000_000).contains(&iterations) {
        return Err("备份头损坏".into());
    }
    let salt = &data[cursor..cursor + SALT_LEN];
    cursor += SALT_LEN;
    let nonce_bytes: [u8; NONCE_LEN] = data[cursor..cursor + NONCE_LEN].try_into().map_err(|_| invalid("备份头损坏"))?;
    cursor += NONCE_LEN;
    let rounds = NonZeroU32::new(iterations).ok_or_else(|| invalid("备份头损坏"))?;
    let mut key_bytes = [0u8; KEY_LEN];
    pbkdf2::derive(pbkdf2::PBKDF2_HMAC_SHA256, rounds, salt, password.as_bytes(), &mut key_bytes);
    let unbound = aead::UnboundKey::new(&aead::AES_256_GCM, &key_bytes).map_err(|_| invalid("无法初始化解密"))?;
    key_bytes.fill(0);
    let key = aead::LessSafeKey::new(unbound);
    let mut buffer = data[cursor..].to_vec();
    let plaintext = key.open_in_place(aead::Nonce::assume_unique_for_key(nonce_bytes),
        aead::Aad::from(aad(iterations, salt)), &mut buffer)
        .map_err(|_| invalid("密码错误或备份已损坏"))?;
    Ok(plaintext.to_vec())
}

fn build_payload(snapshot: &[u8], include_chat: bool, message_count: u64, created_at: i64) -> (BackupInfo, Vec<u8>) {
    let checksum = Sha256::digest(snapshot).iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    let info = BackupInfo { created_at, include_chat, message_count, checksum: checksum.clone() };
    let mut payload = Vec::with_capacity(SNAP_MAGIC.len() + 1 + 8 + 8 + CHECKSUM_HEX + snapshot.len());
    payload.extend_from_slice(SNAP_MAGIC);
    payload.push(if include_chat { 1 } else { 0 });
    payload.extend_from_slice(&created_at.to_be_bytes());
    payload.extend_from_slice(&message_count.to_be_bytes());
    payload.extend_from_slice(checksum.as_bytes());
    payload.extend_from_slice(snapshot);
    (info, payload)
}

/// Returns the verified header and the raw snapshot bytes.
fn parse_payload(password: &str, data: &[u8]) -> Result<(BackupInfo, Vec<u8>), String> {
    let plaintext = decrypt(password, data)?;
    let header = SNAP_MAGIC.len() + 1 + 8 + 8 + CHECKSUM_HEX;
    if plaintext.len() < header || &plaintext[..SNAP_MAGIC.len()] != SNAP_MAGIC {
        return Err("备份内容损坏".into());
    }
    let mut cursor = SNAP_MAGIC.len();
    let include_chat = plaintext[cursor] & 1 == 1;
    cursor += 1;
    let created_at = i64::from_be_bytes(plaintext[cursor..cursor + 8].try_into().map_err(|_| invalid("备份内容损坏"))?);
    cursor += 8;
    let message_count = u64::from_be_bytes(plaintext[cursor..cursor + 8].try_into().map_err(|_| invalid("备份内容损坏"))?);
    cursor += 8;
    let checksum = String::from_utf8_lossy(&plaintext[cursor..cursor + CHECKSUM_HEX]).to_ascii_lowercase();
    cursor += CHECKSUM_HEX;
    if checksum.len() != CHECKSUM_HEX || !checksum.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("备份校验值损坏".into());
    }
    let snapshot = plaintext[cursor..].to_vec();
    if snapshot.is_empty() || snapshot.len() as u64 > MAX_SNAPSHOT {
        return Err("备份内容超出大小限制".into());
    }
    let actual: String = Sha256::digest(&snapshot).iter().map(|byte| format!("{byte:02x}")).collect();
    if actual != checksum {
        return Err("备份完整性校验失败".into());
    }
    if !snapshot.starts_with(b"SQLite format 3\0") {
        return Err("备份中的数据库格式无效".into());
    }
    Ok((BackupInfo { created_at, include_chat, message_count, checksum }, snapshot))
}

fn count_messages(snapshot_path: &Path) -> Result<u64, String> {
    let connection = rusqlite::Connection::open_with_flags(snapshot_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| invalid("无法读取备份数据库"))?;
    let count: i64 = connection.query_row(
        "SELECT (SELECT count(*) FROM direct_messages) + (SELECT count(*) FROM group_messages)
              + (SELECT count(*) FROM local_messages)", [], |row| row.get(0),
    ).map_err(|_| invalid("备份数据库缺少必要数据表"))?;
    Ok(count.max(0) as u64)
}

fn temp_path(store: &Store, prefix: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|value| value.as_nanos()).unwrap_or(0);
    store.data_dir().join(format!("{prefix}-{nanos}.db"))
}

pub fn create(password: &str, store: &Store, destination: &Path, include_chat: bool) -> Result<BackupInfo, String> {
    validate_password(password)?;
    validate_path(destination)?;
    if destination.exists() { return Err("目标文件已存在，请换一个文件名".into()); }
    let temp = temp_path(store, "backup-src");
    let result = (|| -> Result<BackupInfo, String> {
        store.snapshot_to(&temp, include_chat).map_err(|_| invalid("无法生成数据库快照"))?;
        let size = fs::metadata(&temp).map_err(|_| invalid("无法读取数据库快照"))?.len();
        if size == 0 || size > MAX_SNAPSHOT { return Err("数据库超过备份大小限制".into()); }
        let snapshot = fs::read(&temp).map_err(|_| invalid("无法读取数据库快照"))?;
        let message_count = if include_chat { count_messages(&temp)? } else { 0 };
        let (info, payload) = build_payload(&snapshot, include_chat, message_count, unix_now());
        let encrypted = encrypt(password, &payload)?;
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(destination)
            .map_err(|_| invalid("无法创建备份文件"))?;
        file.write_all(&encrypted).and_then(|_| file.sync_all()).map_err(|_| invalid("写入备份文件失败"))?;
        Ok(info)
    })();
    let _ = fs::remove_file(&temp);
    result
}

pub fn inspect(password: &str, path: &Path) -> Result<BackupInfo, String> {
    validate_password(password)?;
    validate_path(path)?;
    let metadata = fs::metadata(path).map_err(|_| invalid("无法读取备份文件"))?;
    if metadata.len() as u64 > MAX_BACKUP { return Err("备份文件超出大小限制".into()); }
    let data = fs::read(path).map_err(|_| invalid("无法读取备份文件"))?;
    parse_payload(password, &data).map(|(info, _)| info)
}

pub fn restore(password: &str, path: &Path, store: &mut Store, overwrite: bool) -> Result<RestoreReport, String> {
    validate_password(password)?;
    validate_path(path)?;
    let metadata = fs::metadata(path).map_err(|_| invalid("无法读取备份文件"))?;
    if metadata.len() as u64 > MAX_BACKUP { return Err("备份文件超出大小限制".into()); }
    let data = fs::read(path).map_err(|_| invalid("无法读取备份文件"))?;
    let (_, snapshot) = parse_payload(password, &data)?;
    let temp = temp_path(store, "restore-src");
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&temp)
        .map_err(|_| invalid("无法写入临时恢复文件"))?;
    file.write_all(&snapshot).map_err(|_| invalid("无法写入临时恢复文件"))?;
    drop(file);
    let result = store.restore_from(&temp, overwrite, unix_now());
    let _ = fs::remove_file(&temp);
    result.map_err(|error| match error {
        rusqlite::Error::InvalidQuery => invalid("备份内容与本地数据不兼容").to_owned(),
        other => format!("恢复失败：{other}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{StorageUsage, DEFAULT_STORAGE_QUOTA};

    fn make_store() -> Store {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!("qbird-backup-{}-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(), SEQ.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        Store::open(&dir.join("qbird.db")).unwrap()
    }

    fn backup_path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("qbird-backup-{label}-{}-{}.qbird", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()))
    }

    #[test]
    fn backup_roundtrip_rejects_wrong_password_and_detects_tampering() {
        let store = make_store();
        let me = "a".repeat(64);
        let peer = "b".repeat(64);
        store.add_candidate(&peer, "Bob").unwrap();
        store.confirm_candidate(&peer).unwrap();
        store.queue_direct(&me, &peer, "hello backup", 10).unwrap();
        store.send_self(&me, "self note", 11).unwrap();
        let path = backup_path("roundtrip");

        assert!(create("short", &store, &path, true).is_err());
        let info = create("correct horse", &store, &path, true).unwrap();
        assert!(info.include_chat);
        assert_eq!(info.message_count, 2);
        assert_eq!(info.checksum.len(), CHECKSUM_HEX);
        assert!(create("correct horse", &store, &path, true).is_err(), "existing file must not be overwritten");

        assert_eq!(inspect("correct horse", &path).unwrap(), info);
        assert!(inspect("wrong password", &path).is_err());

        let mut tampered = fs::read(&path).unwrap();
        let last = tampered.len() - 1;
        tampered[last] ^= 0x01;
        let tampered_path = backup_path("tampered");
        fs::write(&tampered_path, &tampered).unwrap();
        assert!(inspect("correct horse", &tampered_path).is_err());

        let mut target = make_store();
        let report = restore("correct horse", &path, &mut target, false).unwrap();
        assert_eq!(report.mode, "merge");
        assert_eq!(report.inserted, 2);
        assert_eq!(report.conflicts, 0);
        assert_eq!(target.list_contacts().unwrap().len(), 1);
        assert_eq!(target.storage_status().unwrap(), StorageUsage { used: 0, quota: DEFAULT_STORAGE_QUOTA });

        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&tampered_path);
    }

    #[test]
    fn merge_skip_overwrite_and_conflicts_are_reported() {
        let source = make_store();
        let me = "a".repeat(64);
        let peer = "b".repeat(64);
        source.add_candidate(&peer, "Bob").unwrap();
        source.confirm_candidate(&peer).unwrap();
        source.queue_direct(&me, &peer, "first", 10).unwrap();
        let path = backup_path("merge");
        create("password123", &source, &path, true).unwrap();

        // Restoring the same backup twice skips the already present message id.
        let mut target = make_store();
        let first = restore("password123", &path, &mut target, false).unwrap();
        assert_eq!((first.inserted, first.skipped, first.conflicts), (1, 0, 0));
        let second = restore("password123", &path, &mut target, false).unwrap();
        assert_eq!((second.inserted, second.skipped, second.conflicts), (0, 1, 0));
        assert_eq!(target.list_direct(&peer, 10, None).unwrap().len(), 1);

        // The same message id with different content aborts the merge without writing.
        let mut conflicting = make_store();
        restore("password123", &path, &mut conflicting, false).unwrap();
        let snapshot = temp_path(&source, "conflict-src");
        source.snapshot_to(&snapshot, true).unwrap();
        let connection = rusqlite::Connection::open(&snapshot).unwrap();
        let message_id: String = connection.query_row("SELECT id FROM direct_messages LIMIT 1", [], |row| row.get(0)).unwrap();
        connection.execute("UPDATE direct_messages SET body = 'changed' WHERE id = ?1", [&message_id]).unwrap();
        drop(connection);
        let snapshot_bytes = fs::read(&snapshot).unwrap();
        let (_, payload) = build_payload(&snapshot_bytes, true, 1, unix_now());
        let encrypted = encrypt("password123", &payload).unwrap();
        let conflict_path = backup_path("conflict");
        fs::write(&conflict_path, encrypted).unwrap();
        let report = restore("password123", &conflict_path, &mut conflicting, false).unwrap();
        assert_eq!(report.conflicts, 1);
        assert_eq!(report.inserted, 0);
        assert_eq!(conflicting.list_direct(&peer, 10, None).unwrap().len(), 1);

        // Overwrite replaces local data and leaves a pre-restore snapshot.
        let report = restore("password123", &path, &mut target, true).unwrap();
        assert_eq!(report.mode, "overwrite");
        assert!(report.snapshot_path.is_some());
        assert_eq!(target.list_direct(&peer, 10, None).unwrap().len(), 1);

        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&conflict_path);
        let _ = fs::remove_file(&snapshot);
    }
}
