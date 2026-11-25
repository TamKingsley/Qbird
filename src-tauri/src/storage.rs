use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension, Result};
use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: i64 = 27;
/// Upper bound on remembered "do not ask again" senders.
const MAX_REMEMBERED_SOURCES: usize = 200;

/// Default cap for locally received attachments; the user may change it in settings.
pub const DEFAULT_STORAGE_QUOTA: u64 = 2 * 1024 * 1024 * 1024;
pub const MIN_STORAGE_QUOTA: u64 = 256 * 1024 * 1024;
pub const MAX_STORAGE_QUOTA: u64 = 64 * 1024 * 1024 * 1024;

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct StorageUsage {
    pub used: u64,
    pub quota: u64,
}

/// Result of a database restore. `conflicts > 0` means nothing was applied.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct RestoreReport {
    pub mode: String,
    pub inserted: u64,
    pub skipped: u64,
    pub conflicts: u64,
    pub snapshot_path: Option<String>,
    /// Attachment files restored from a format-2 backup; zero for v1 backups.
    pub restored_attachments: u64,
}

/// Upper bound for a quoted snippet, kept short so it fits one compose bar.
pub const REPLY_PREVIEW_MAX: usize = 160;

/// One quoted message as stored alongside a reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupReply {
    pub message_id: String,
    pub sender_name: String,
    pub preview: String,
}

/// An invitation that waits for the local user because auto-join is off (default).
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct GroupInvitation {
    pub id: String,
    pub group_id: String,
    pub name: String,
    pub owner: String,
    pub owner_name: Option<String>,
    pub member_count: u32,
    pub created_at: i64,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Contact {
    pub fingerprint: String,
    pub display_name: String,
    pub trust_state: String,
    /// Peer username captured from LAN discovery or entered by the user. Display only.
    pub username: Option<String>,
    /// Local-only note; never sent to peers and never changes identity.
    pub remark: Option<String>,
    /// Local-only category/group label used to organise the contact list.
    pub category: Option<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Profile {
    pub nickname: String,
    pub email: String,
    /// Searchable LAN name; not proof of identity and not globally unique.
    pub username: String,
}

/// One message matched by the cross-conversation search.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct SearchHit {
    /// "self", "direct" or "group".
    pub conversation_kind: String,
    /// "self", the peer fingerprint or the group id.
    pub conversation_id: String,
    pub conversation_name: String,
    pub message_id: String,
    /// Fingerprint of the author; self messages use the local fingerprint.
    pub sender: String,
    pub sender_name: String,
    pub body: String,
    pub created_at: i64,
    pub logical: i64,
    /// "text" or "file"; self-chat messages are currently text-only.
    pub kind: String,
}

/// One paginated record-window result after date, keyword, and content filters.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct HistorySearchHit {
    pub id: String,
    pub sender: String,
    pub sender_name: String,
    pub body: String,
    pub created_at: i64,
    pub logical: i64,
    pub kind: String,
    pub file_name: Option<String>,
    pub recalled_at: Option<i64>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct PendingDelivery {
    pub peer: String,
    pub address: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DirectMessage {
    pub id: String,
    pub peer: String,
    pub sender: String,
    pub body: String,
    pub created_at: i64,
    pub logical: i64,
    pub status: String,
    pub reply_to: Option<String>,
    pub reply_sender: Option<String>,
    pub reply_preview: Option<String>,
    pub kind: String,
    pub file_name: Option<String>,
    pub file_size: Option<i64>,
    pub file_sha256: Option<String>,
    pub file_status: Option<String>,
    pub file_transferred: Option<i64>,
    pub file_error: Option<String>,
    /// Unix time when the sender withdrew this message; body is blank in that case.
    pub recalled_at: Option<i64>,
}

/// Transfer state exposed to the UI; paths stay on the backend.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct FileProgress {
    pub message_id: String,
    pub direction: String,
    pub status: String,
    pub transferred: i64,
    pub total: i64,
    pub error: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct FileTransfer {
    pub message_id: String,
    pub peer: String,
    pub direction: String,
    pub source_path: Option<String>,
    pub target_path: Option<String>,
    pub total: i64,
    pub transferred: i64,
    pub status: String,
    pub error: Option<String>,
}

/// Result of validating an incoming offer against local state and partial files.
#[derive(Debug, PartialEq, Eq)]
pub enum FileOfferDecision {
    /// Continue from `offset` into the given local paths.
    Ready { offset: u64, target: PathBuf, partial: PathBuf },
    /// The complete file is already stored; only ask for confirmation.
    Complete,
    Conflict,
    Untrusted,
    /// Accepting the file would exceed the local storage quota.
    NoSpace,
}

/// Outcome of one inbound group file offer. Trust is already implied by the group
/// membership checks in `receive_group`, so there is no separate Untrusted case.
#[derive(Debug)]
pub enum GroupFileDecision {
    Ready { offset: u64, target: PathBuf, partial: PathBuf },
    Complete,
    Conflict,
    NoSpace,
    NotMember,
}

const MESSAGE_COLUMNS: &str = "m.id, m.peer, m.sender, m.body, m.created_at, m.logical, m.status, m.kind, m.file_name, m.file_size, m.file_sha256, f.status, f.transferred, f.error, m.recalled_at, m.reply_to, m.reply_sender, m.reply_preview";

#[derive(Debug, serde::Deserialize)]
pub struct DirectCursor {
    pub logical: i64,
    pub sender: String,
    pub id: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Conversation {
    pub id: String,
    pub hidden: bool,
    pub last_body: String,
    pub last_logical: i64,
    /// "self", "direct" or "group"; the UI uses it to route a conversation row.
    pub kind: String,
    /// Incoming messages not yet read in this conversation.
    pub unread: u32,
    /// Local "do not disturb" flag; the UI shows a dot instead of a count.
    pub muted: bool,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct LocalMessage {
    pub id: String,
    pub conversation_id: String,
    pub sender: String,
    pub body: String,
    pub created_at: i64,
    pub logical: i64,
    pub recalled_at: Option<i64>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum IncomingResult {
    Inserted,
    Duplicate,
    Unverified,
    IdConflict,
    /// The local device is not an active member of the group any more.
    NotMember,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct GroupSummary {
    pub id: String,
    pub name: String,
    pub owner: String,
    pub member_count: i64,
    pub hidden: bool,
    pub is_owner: bool,
    pub last_body: String,
    pub last_logical: i64,
}

/// How long after sending a message may still be withdrawn (both direct and group).
pub const RECALL_WINDOW_SECONDS: i64 = 120;

#[derive(Debug, PartialEq, Eq)]
pub enum RecallResult {
    Recalled { delivered: bool },
    TooLate,
    NotYours,
    Missing,
    Already,
    NotMember,
}

/// One message still owed a recall notice, retried until the peer is reachable.
#[derive(Debug, PartialEq, Eq)]
pub struct PendingRecall {
    pub id: String,
    pub peer: String,
    pub address: String,
    pub message_id: String,
    pub created_at: Option<i64>,
    pub logical: Option<i64>,
}

/// A friend request in either direction; `in` rows wait for a local answer.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ContactRequest {
    pub id: String,
    pub direction: String,
    pub peer: String,
    pub nickname: String,
    pub username: Option<String>,
    pub address: Option<String>,
    pub created_at: i64,
    pub status: String,
}

/// A group that no longer exists but keeps its local history.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct DissolvedGroup {
    pub id: String,
    pub name: String,
    pub owner: String,
    pub dissolved_at: Option<i64>,
    pub last_body: String,
    pub last_logical: i64,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct GroupMember {
    pub group_id: String,
    pub member: String,
    pub display_name: String,
    pub role: String,
    pub joined_at: i64,
    pub active: bool,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct GroupMessage {
    pub id: String,
    pub group_id: String,
    pub sender: String,
    pub sender_name: String,
    pub body: String,
    pub created_at: i64,
    pub logical: i64,
    /// Number of members this message is still waiting to reach (0 once delivered to all).
    pub pending: i64,
    /// The quoted message this one replies to, if any.
    pub reply_to: Option<String>,
    pub reply_sender: Option<String>,
    pub reply_preview: Option<String>,
    /// Members explicitly mentioned with @; only active members are ever stored.
    pub mentions: Vec<String>,
    /// "text" or "file"; file messages carry the metadata below.
    pub kind: String,
    pub file_name: Option<String>,
    pub file_size: Option<i64>,
    pub file_sha256: Option<String>,
    pub file_status: Option<String>,
    pub file_transferred: Option<i64>,
    pub file_error: Option<String>,
    /// Unix time when the sender withdrew this message; body is blank in that case.
    pub recalled_at: Option<i64>,
}

/// Which conversation a received file belongs to; direct and group files keep
/// separate default receive folders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecvKind { Direct, Group }

/// How incoming files are handled before bytes are written to disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FileAcceptMode { Always, Remember, Auto }

impl FileAcceptMode {
    pub fn as_str(self) -> &'static str {
        match self { Self::Always => "always", Self::Remember => "remember", Self::Auto => "auto" }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value { "always" => Some(Self::Always), "remember" => Some(Self::Remember), "auto" => Some(Self::Auto), _ => None }
    }
}

/// One received file offer waiting for a local yes/no.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct FileOfferRequest {
    pub message_id: String,
    pub peer: String,
    pub name: String,
    pub total: i64,
    pub created_at: i64,
    pub status: String,
    /// Set for group files so the group view can list its own pending offers.
    pub group_id: Option<String>,
}

fn default_archive_message_kind() -> String { "text".to_owned() }

/// One text or file message inside a chat archive. Missing fields in legacy v1
/// archives default to a text message with no attachment metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveMessage {
    pub id: String,
    pub sender: String,
    pub body: String,
    pub created_at: i64,
    pub logical: i64,
    #[serde(default = "default_archive_message_kind")]
    pub kind: String,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub file_size: Option<i64>,
    #[serde(default)]
    pub file_sha256: Option<String>,
}

/// Local-only source information for one attachment; paths must never enter an archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveAttachmentSource {
    pub message_id: String,
    pub direction: Option<String>,
    pub file_name: Option<String>,
    pub file_size: Option<i64>,
    pub file_sha256: Option<String>,
    pub path: Option<String>,
    pub status: Option<String>,
    pub transferred: Option<i64>,
    pub total: Option<i64>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ArchiveExport {
    pub kind: String,
    pub target: String,
    pub name: String,
    pub owner: Option<String>,
    pub messages: Vec<ArchiveMessage>,
    #[serde(skip)]
    pub attachments: Vec<ArchiveAttachmentSource>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ArchiveImportReport {
    pub kind: String,
    pub name: String,
    pub inserted: u64,
    pub skipped: u64,
    pub conflicts: u64,
    pub restored_attachments: u64,
}

/// One installed preview plugin as recorded in the database.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct PluginRow {
    pub id: String,
    pub name: String,
    pub version: String,
    pub enabled: bool,
}

/// Links an image message to the bubble it belongs to.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ImageBatch {
    pub message_id: String,
    pub batch_id: String,
    pub anchor_id: String,
    pub caption: String,
}

/// One join application, either received (incoming) or sent by this device.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct JoinRequest {
    pub id: String,
    pub group_id: String,
    pub group_name: String,
    pub owner: String,
    pub applicant: String,
    pub applicant_name: String,
    pub created_at: i64,
    pub status: String,
}

/// Local transfer state for one group file message.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct GroupFileProgress {
    pub message_id: String,
    pub direction: String,
    pub total: i64,
    pub transferred: i64,
    pub status: String,
    pub error: Option<String>,
    pub target_path: Option<String>,
}

/// One member a stored group message still has to be delivered to.
#[derive(Debug, PartialEq, Eq)]
pub struct GroupDelivery {
    pub member: String,
    pub address: String,
    pub message_id: String,
}

/// Everything one outbound group frame needs; the member list is a local snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupDispatch {
    pub message_id: String,
    pub group_id: String,
    pub group_name: String,
    pub owner: String,
    pub members: Vec<String>,
    pub body: String,
    pub created_at: i64,
    pub logical: i64,
    pub reply_to: Option<String>,
    pub reply_sender: Option<String>,
    pub reply_preview: Option<String>,
    pub mentions: Vec<String>,
    /// "text" or "file"; file dispatches also carry the local source path.
    pub kind: String,
    pub file_name: Option<String>,
    pub file_size: Option<i64>,
    pub file_sha256: Option<String>,
    pub source_path: Option<String>,
}

/// A pending invite, member-list update, removal or leave notice.
#[derive(Debug, PartialEq, Eq)]
pub struct GroupControlTask {
    pub id: String,
    pub group_id: String,
    pub target: String,
    pub address: String,
    pub kind: String,
    pub payload: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Message {
    pub id: String,
    pub peer_fingerprint: String,
    pub body: String,
    pub direction: String,
    pub created_at: i64,
    pub status: String,
}

/// Local SQLite storage. Database contents are not encrypted by this module.
pub struct Store {
    connection: Connection,
    data_dir: PathBuf,
    address_revision: std::cell::Cell<u64>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let data_dir = path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
        Self::initialize_in(Connection::open(path)?, data_dir)
    }

    #[cfg(test)]
    fn initialize(connection: Connection) -> Result<Self> {
        let data_dir = std::env::temp_dir().join(format!("qbird-store-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)));
        Self::initialize_in(connection, data_dir)
    }

    fn initialize_in(mut connection: Connection, data_dir: PathBuf) -> Result<Self> {
        connection.execute_batch("PRAGMA foreign_keys = ON;")?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(rusqlite::Error::InvalidQuery);
        }
        if version == 0 {
            let transaction = connection.transaction()?;
            transaction.execute_batch(
                "CREATE TABLE contacts (
                    fingerprint TEXT PRIMARY KEY NOT NULL CHECK(length(fingerprint) = 64),
                    display_name TEXT NOT NULL,
                    trust_state TEXT NOT NULL CHECK(trust_state IN ('pending', 'verified'))
                );
                CREATE TABLE messages (
                    id TEXT PRIMARY KEY NOT NULL,
                    peer_fingerprint TEXT NOT NULL REFERENCES contacts(fingerprint),
                    body TEXT NOT NULL CHECK(length(body) <= 32768),
                    direction TEXT NOT NULL CHECK(direction IN ('incoming', 'outgoing')),
                    created_at INTEGER NOT NULL,
                    status TEXT NOT NULL CHECK(status IN ('pending', 'delivered', 'failed', 'received'))
                );
                CREATE INDEX messages_by_peer_time ON messages(peer_fingerprint, created_at);
                PRAGMA user_version = 1;",
            )?;
            transaction.commit()?;
        }
        if version < 2 {
            let transaction = connection.transaction()?;
            transaction.execute_batch(
                "CREATE TABLE conversations (
                    id TEXT PRIMARY KEY NOT NULL,
                    kind TEXT NOT NULL CHECK(kind IN ('self')),
                    hidden INTEGER NOT NULL DEFAULT 0 CHECK(hidden IN (0, 1))
                );
                CREATE TABLE logical_clock (
                    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                    value INTEGER NOT NULL CHECK(value >= 0)
                );
                INSERT INTO logical_clock(singleton, value) VALUES (1, 0);
                CREATE TABLE local_messages (
                    id TEXT PRIMARY KEY NOT NULL,
                    conversation_id TEXT NOT NULL REFERENCES conversations(id),
                    sender TEXT NOT NULL CHECK(length(sender) = 64),
                    body TEXT NOT NULL CHECK(length(body) > 0 AND length(body) <= 32768),
                    created_at INTEGER NOT NULL,
                    logical INTEGER NOT NULL CHECK(logical > 0)
                );
                CREATE INDEX local_messages_order ON local_messages(conversation_id, logical, sender, id);
                PRAGMA user_version = 2;",
            )?;
            transaction.commit()?;
        }
        if version < 3 {
            let transaction = connection.transaction()?;
            transaction.execute_batch(
                "CREATE TABLE direct_conversations (
                    peer TEXT PRIMARY KEY NOT NULL REFERENCES contacts(fingerprint),
                    hidden INTEGER NOT NULL DEFAULT 0 CHECK(hidden IN (0, 1))
                );
                CREATE TABLE direct_messages (
                    id TEXT PRIMARY KEY NOT NULL,
                    peer TEXT NOT NULL REFERENCES direct_conversations(peer),
                    sender TEXT NOT NULL CHECK(length(sender) = 64),
                    body TEXT NOT NULL CHECK(length(body) > 0 AND length(body) <= 32768),
                    created_at INTEGER NOT NULL,
                    logical INTEGER NOT NULL CHECK(logical > 0),
                    status TEXT NOT NULL CHECK(status IN ('pending', 'delivered', 'received'))
                );
                CREATE INDEX direct_messages_order ON direct_messages(peer, logical, sender, id);
                PRAGMA user_version = 3;",
            )?;
            transaction.commit()?;
        }
        if version < 4 {
            let transaction = connection.transaction()?;
            transaction.execute_batch(
                "CREATE TABLE contact_addresses (
                    peer TEXT PRIMARY KEY NOT NULL REFERENCES contacts(fingerprint),
                    address TEXT NOT NULL CHECK(length(address) > 0 AND length(address) <= 128)
                );
                PRAGMA user_version = 4;",
            )?;
            transaction.commit()?;
        }
        if version < 5 {
            let transaction = connection.transaction()?;
            // Group ids are random 128-bit values rendered as 32 hex characters, never fingerprints.
            // A group's member list is local state; only the owner's snapshot may change it.
            transaction.execute_batch(
                "CREATE TABLE IF NOT EXISTS groups (
                    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) = 32),
                    name TEXT NOT NULL CHECK(length(name) > 0 AND length(name) <= 128),
                    owner TEXT NOT NULL CHECK(length(owner) = 64),
                    created_at INTEGER NOT NULL
                );
                CREATE TABLE IF NOT EXISTS group_members (
                    group_id TEXT NOT NULL REFERENCES groups(id),
                    member TEXT NOT NULL CHECK(length(member) = 64),
                    role TEXT NOT NULL CHECK(role IN ('owner', 'member')),
                    joined_at INTEGER NOT NULL,
                    active INTEGER NOT NULL DEFAULT 1 CHECK(active IN (0, 1)),
                    hidden INTEGER NOT NULL DEFAULT 0 CHECK(hidden IN (0, 1)),
                    PRIMARY KEY(group_id, member)
                );
                CREATE INDEX IF NOT EXISTS group_members_by_member ON group_members(member, active);
                CREATE TABLE IF NOT EXISTS group_messages (
                    id TEXT PRIMARY KEY NOT NULL,
                    group_id TEXT NOT NULL REFERENCES groups(id),
                    sender TEXT NOT NULL CHECK(length(sender) = 64),
                    body TEXT NOT NULL CHECK(length(body) > 0 AND length(body) <= 32768),
                    created_at INTEGER NOT NULL,
                    logical INTEGER NOT NULL CHECK(logical > 0)
                );
                CREATE INDEX IF NOT EXISTS group_messages_order ON group_messages(group_id, logical, sender, id);
                CREATE TABLE IF NOT EXISTS group_outbox (
                    message_id TEXT NOT NULL REFERENCES group_messages(id),
                    group_id TEXT NOT NULL,
                    member TEXT NOT NULL CHECK(length(member) = 64),
                    status TEXT NOT NULL CHECK(status IN ('pending', 'delivered')),
                    PRIMARY KEY(message_id, member)
                );
                CREATE INDEX IF NOT EXISTS group_outbox_pending ON group_outbox(member, status, message_id);
                CREATE TABLE IF NOT EXISTS group_control (
                    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) = 32),
                    group_id TEXT NOT NULL,
                    target TEXT NOT NULL CHECK(length(target) = 64),
                    kind TEXT NOT NULL CHECK(kind IN ('invite', 'remove', 'leave')),
                    payload TEXT NOT NULL CHECK(length(payload) <= 65536),
                    status TEXT NOT NULL CHECK(status IN ('pending', 'delivered')),
                    created_at INTEGER NOT NULL
                );
                CREATE INDEX IF NOT EXISTS group_control_pending ON group_control(target, status);
                PRAGMA user_version = 5;",
            )?;
            transaction.commit()?;
        }
        if version < 6 {
            let transaction = connection.transaction()?;
            // File messages reuse the direct message ordering; bytes live outside SQLite.
            // Existing databases may already carry some columns; only add the missing ones.
            for statement in [
                "ALTER TABLE direct_messages ADD COLUMN kind TEXT NOT NULL DEFAULT 'text' CHECK(kind IN ('text', 'file'))",
                "ALTER TABLE direct_messages ADD COLUMN file_name TEXT",
                "ALTER TABLE direct_messages ADD COLUMN file_size INTEGER",
                "ALTER TABLE direct_messages ADD COLUMN file_sha256 TEXT",
            ] {
                if let Err(error) = transaction.execute_batch(statement) {
                    if !error.to_string().contains("duplicate column name") { return Err(error); }
                }
            }
            transaction.execute_batch(
                "CREATE TABLE IF NOT EXISTS file_transfers (
                    message_id TEXT PRIMARY KEY NOT NULL REFERENCES direct_messages(id),
                    peer TEXT NOT NULL CHECK(length(peer) = 64),
                    direction TEXT NOT NULL CHECK(direction IN ('incoming', 'outgoing')),
                    source_path TEXT,
                    target_path TEXT,
                    total INTEGER NOT NULL CHECK(total > 0 AND total <= 536870912),
                    transferred INTEGER NOT NULL DEFAULT 0 CHECK(transferred >= 0 AND transferred <= total),
                    status TEXT NOT NULL CHECK(status IN ('pending', 'transferring', 'completed', 'failed')),
                    error TEXT,
                    updated_at INTEGER NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS file_transfers_by_peer ON file_transfers(peer, status);
                 PRAGMA user_version = 6;",
            )?;
            transaction.commit()?;
        }
        if version < 7 {
            let transaction = connection.transaction()?;
            // Replies quote a short local snippet so the UI can show it even before the
            // original is paged in; mentions are stored as a JSON array of fingerprints.
            for statement in [
                "ALTER TABLE group_messages ADD COLUMN reply_to TEXT",
                "ALTER TABLE group_messages ADD COLUMN reply_sender TEXT",
                "ALTER TABLE group_messages ADD COLUMN reply_preview TEXT",
                "ALTER TABLE group_messages ADD COLUMN mentions TEXT NOT NULL DEFAULT '[]'",
                "ALTER TABLE groups ADD COLUMN active INTEGER NOT NULL DEFAULT 1",
            ] {
                if let Err(error) = transaction.execute_batch(statement) {
                    if !error.to_string().contains("duplicate column name") { return Err(error); }
                }
            }
            // Transfer and dissolve are new control kinds; SQLite cannot alter a CHECK
            // constraint, so rebuild the table while keeping every pending row.
            transaction.execute_batch(
                "CREATE TABLE group_control_v7 (
                    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) = 32),
                    group_id TEXT NOT NULL,
                    target TEXT NOT NULL CHECK(length(target) = 64),
                    kind TEXT NOT NULL CHECK(kind IN ('invite', 'members', 'remove', 'leave', 'transfer', 'dissolve')),
                    payload TEXT NOT NULL CHECK(length(payload) <= 65536),
                    status TEXT NOT NULL CHECK(status IN ('pending', 'delivered')),
                    created_at INTEGER NOT NULL
                );
                INSERT INTO group_control_v7(id, group_id, target, kind, payload, status, created_at)
                    SELECT id, group_id, target, kind, payload, status, created_at FROM group_control;
                DROP TABLE group_control;
                ALTER TABLE group_control_v7 RENAME TO group_control;
                CREATE INDEX IF NOT EXISTS group_control_pending ON group_control(target, status);
                PRAGMA user_version = 7;",
            )?;
            transaction.commit()?;
        }
        if version < 8 {
            let transaction = connection.transaction()?;
            // Small key/value settings table; missing values fall back to code defaults.
            transaction.execute_batch(
                "CREATE TABLE IF NOT EXISTS settings (
                    key TEXT PRIMARY KEY NOT NULL CHECK(length(key) <= 64),
                    value TEXT NOT NULL CHECK(length(value) <= 4096)
                );
                PRAGMA user_version = 8;",
            )?;
            transaction.commit()?;
        }
        if version < 9 {
            let transaction = connection.transaction()?;
            for statement in [
                "ALTER TABLE contacts ADD COLUMN remark TEXT",
                "ALTER TABLE contacts ADD COLUMN category TEXT",
            ] {
                if let Err(error) = transaction.execute_batch(statement) {
                    if !error.to_string().contains("duplicate column name") { return Err(error); }
                }
            }
            transaction.execute_batch("PRAGMA user_version = 9;")?;
            transaction.commit()?;
        }
        if version < 11 {
            let transaction = connection.transaction()?;
            // Group file messages reuse the accept / chunk / complete exchange and keep
            // their own transfer rows so a direct and a group id can never collide.
            for statement in [
                "ALTER TABLE group_messages ADD COLUMN kind TEXT NOT NULL DEFAULT 'text' CHECK(kind IN ('text', 'file'))",
                "ALTER TABLE group_messages ADD COLUMN file_name TEXT",
                "ALTER TABLE group_messages ADD COLUMN file_size INTEGER",
                "ALTER TABLE group_messages ADD COLUMN file_sha256 TEXT",
            ] {
                if let Err(error) = transaction.execute_batch(statement) {
                    if !error.to_string().contains("duplicate column name") { return Err(error); }
                }
            }
            transaction.execute_batch(
                "CREATE TABLE IF NOT EXISTS group_file_transfers (
                    message_id TEXT PRIMARY KEY NOT NULL,
                    group_id TEXT NOT NULL CHECK(length(group_id) = 32),
                    peer TEXT NOT NULL CHECK(length(peer) = 64),
                    direction TEXT NOT NULL CHECK(direction IN ('incoming', 'outgoing')),
                    source_path TEXT,
                    target_path TEXT,
                    total INTEGER NOT NULL CHECK(total > 0 AND total <= 536870912),
                    transferred INTEGER NOT NULL DEFAULT 0 CHECK(transferred >= 0 AND transferred <= total),
                    status TEXT NOT NULL CHECK(status IN ('pending', 'transferring', 'completed', 'failed')),
                    error TEXT,
                    updated_at INTEGER NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS group_file_transfers_by_peer ON group_file_transfers(peer, status);
                 PRAGMA user_version = 11;",
            )?;
            transaction.commit()?;
        }
        if version < 16 {
            let transaction = connection.transaction()?;
            // Installed preview plugins; the files live under <data>/plugins/<id>.
            transaction.execute_batch(
                "CREATE TABLE IF NOT EXISTS plugins (
                    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 2 AND 64),
                    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 64),
                    version TEXT NOT NULL CHECK(length(version) BETWEEN 1 AND 32),
                    enabled INTEGER NOT NULL DEFAULT 1,
                    installed_at INTEGER NOT NULL
                );
                PRAGMA user_version = 16;",
            )?;
            transaction.commit()?;
        }
        if version < 15 {
            let transaction = connection.transaction()?;
            // Several images may share one bubble: the batch table links each image
            // message to a batch id plus the caption typed with them.
            transaction.execute_batch(
                "CREATE TABLE IF NOT EXISTS image_batches (
                    message_id TEXT PRIMARY KEY NOT NULL CHECK(length(message_id) <= 128),
                    batch_id TEXT NOT NULL CHECK(length(batch_id) = 32),
                    caption TEXT NOT NULL CHECK(length(caption) <= 8000),
                    created_at INTEGER NOT NULL
                );
                CREATE INDEX IF NOT EXISTS image_batches_by_batch ON image_batches(batch_id);
                PRAGMA user_version = 15;",
            )?;
            transaction.commit()?;
        }
        if version < 14 {
            let transaction = connection.transaction()?;
            // Avatars are small owner-chosen PNGs; members pull them on demand.
            transaction.execute_batch(
                "CREATE TABLE IF NOT EXISTS group_avatars (
                    group_id TEXT PRIMARY KEY NOT NULL CHECK(length(group_id) = 32),
                    png BLOB NOT NULL,
                    updated_at INTEGER NOT NULL
                );
                PRAGMA user_version = 14;",
            )?;
            transaction.commit()?;
        }
        if version < 13 {
            let transaction = connection.transaction()?;
            // Join/refuse notices are queued like any other owner control message, so the
            // CHECK constraint has to accept the two new kinds.
            transaction.execute_batch(
                "CREATE TABLE group_control_v13 (
                    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) = 32),
                    group_id TEXT NOT NULL,
                    target TEXT NOT NULL CHECK(length(target) = 64),
                    kind TEXT NOT NULL CHECK(kind IN ('invite', 'members', 'remove', 'leave', 'transfer', 'dissolve', 'join', 'join_reject')),
                    payload TEXT NOT NULL CHECK(length(payload) <= 65536),
                    status TEXT NOT NULL CHECK(status IN ('pending', 'delivered')),
                    created_at INTEGER NOT NULL
                );
                INSERT INTO group_control_v13(id, group_id, target, kind, payload, status, created_at)
                    SELECT id, group_id, target, kind, payload, status, created_at FROM group_control;
                DROP TABLE group_control;
                ALTER TABLE group_control_v13 RENAME TO group_control;
                CREATE INDEX IF NOT EXISTS group_control_pending ON group_control(target, status);
                CREATE TABLE IF NOT EXISTS group_join_requests (
                    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) = 32),
                    group_id TEXT NOT NULL CHECK(length(group_id) = 32),
                    group_name TEXT NOT NULL CHECK(length(group_name) > 0 AND length(group_name) <= 128),
                    owner TEXT NOT NULL CHECK(length(owner) = 64),
                    applicant TEXT NOT NULL CHECK(length(applicant) = 64),
                    created_at INTEGER NOT NULL,
                    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'accepted', 'rejected'))
                );
                CREATE UNIQUE INDEX IF NOT EXISTS group_join_requests_unique ON group_join_requests(group_id, applicant);
                CREATE INDEX IF NOT EXISTS group_join_requests_by_owner ON group_join_requests(owner, status);
                PRAGMA user_version = 13;",
            )?;
            transaction.commit()?;
        }
        if version < 12 {
            let transaction = connection.transaction()?;
            // Offers waiting for the local user are local state only; no bytes are stored yet.
            transaction.execute_batch(
                "CREATE TABLE IF NOT EXISTS file_offer_requests (
                    message_id TEXT PRIMARY KEY NOT NULL,
                    peer TEXT NOT NULL CHECK(length(peer) = 64),
                    name TEXT NOT NULL,
                    total INTEGER NOT NULL CHECK(total > 0 AND total <= 536870912),
                    created_at INTEGER NOT NULL,
                    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'accepted', 'rejected')),
                    target_dir TEXT,
                    group_id TEXT
                 );
                 CREATE INDEX IF NOT EXISTS file_offer_requests_by_peer ON file_offer_requests(peer, status);
                 PRAGMA user_version = 12;",
            )?;
            transaction.commit()?;
        }
        if version < 10 {
            let transaction = connection.transaction()?;
            // Unread counters are local UI state; they never affect message ordering or trust.
            for statement in [
                "ALTER TABLE direct_conversations ADD COLUMN unread INTEGER NOT NULL DEFAULT 0 CHECK(unread >= 0)",
                "ALTER TABLE group_members ADD COLUMN unread INTEGER NOT NULL DEFAULT 0 CHECK(unread >= 0)",
            ] {
                if let Err(error) = transaction.execute_batch(statement) {
                    if !error.to_string().contains("duplicate column name") { return Err(error); }
                }
            }
            transaction.execute_batch("PRAGMA user_version = 10;")?;
            transaction.commit()?;
        }
        if version < 17 {
            let transaction = connection.transaction()?;
            // Per-offer destination chosen in the confirmation dialog. Fresh databases get
            // both columns from the v12 table shape, so existing ones only need them added.
            for statement in [
                "ALTER TABLE file_offer_requests ADD COLUMN target_dir TEXT",
                "ALTER TABLE file_offer_requests ADD COLUMN group_id TEXT",
            ] {
                if let Err(error) = transaction.execute_batch(statement) {
                    let text = error.to_string();
                    if !text.contains("duplicate column name") && !text.contains("no such table") { return Err(error); }
                }
            }
            transaction.execute_batch("PRAGMA user_version = 17;")?;
            transaction.commit()?;
        }
        if version < 18 {
            let transaction = connection.transaction()?;
            // Remembered "stop asking for this sender" choices get their own table so the
            // list is not limited by the small size cap on settings values.
            transaction.execute_batch(
                "CREATE TABLE IF NOT EXISTS file_accept_sources (
                    source TEXT PRIMARY KEY NOT NULL CHECK(length(source) BETWEEN 2 AND 71),
                    added_at INTEGER NOT NULL
                );
                PRAGMA user_version = 18;",
            )?;
            // Carry the old JSON blob over once, then drop it.
            let legacy: Option<String> = transaction.query_row(
                "SELECT value FROM settings WHERE key = 'file_accept_remembered'", [], |row| row.get(0)).optional()?;
            if let Some(raw) = legacy {
                let list: Vec<String> = serde_json::from_str(&raw).unwrap_or_default();
                for source in list.into_iter().filter(|item| valid_offer_source(item)).take(MAX_REMEMBERED_SOURCES) {
                    transaction.execute(
                        "INSERT OR IGNORE INTO file_accept_sources(source, added_at) VALUES (?1, 0)", [&source])?;
                }
                transaction.execute("DELETE FROM settings WHERE key = 'file_accept_remembered'", [])?;
            }
            transaction.commit()?;
        }
        if version < 19 {
            let transaction = connection.transaction()?;
            // Usernames come from peers over unauthenticated discovery; they are shown and
            // searched but never used as identity or trust evidence. Qbird ID stays derived
            // from the fingerprint.
            if let Err(error) = transaction.execute_batch("ALTER TABLE contacts ADD COLUMN username TEXT") {
                if !error.to_string().contains("duplicate column name") { return Err(error); }
            }
            transaction.execute_batch("PRAGMA user_version = 19;")?;
            transaction.commit()?;
        }
        if version < 20 {
            let transaction = connection.transaction()?;
            // Invitations stay here until the user answers. Group rows and membership are
            // created only after acceptance, so a declined invite never joins the group.
            transaction.execute_batch(
                "CREATE TABLE IF NOT EXISTS group_invitations (
                    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
                    group_id TEXT NOT NULL CHECK(length(group_id) = 32),
                    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 128),
                    owner TEXT NOT NULL CHECK(length(owner) = 64),
                    members TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    status TEXT NOT NULL CHECK(status IN ('pending', 'accepted', 'declined'))
                );
                CREATE INDEX IF NOT EXISTS group_invitations_by_status ON group_invitations(status, created_at);
                PRAGMA user_version = 20;",
            )?;
            transaction.commit()?;
        }
        if version < 21 {
            let transaction = connection.transaction()?;
            // "Mute" is local UI state like `hidden` and `unread`: it only silences the
            // numeric badge, never the message flow or the conversation order.
            for statement in [
                "ALTER TABLE conversations ADD COLUMN muted INTEGER NOT NULL DEFAULT 0 CHECK(muted IN (0, 1))",
                "ALTER TABLE direct_conversations ADD COLUMN muted INTEGER NOT NULL DEFAULT 0 CHECK(muted IN (0, 1))",
                "ALTER TABLE group_members ADD COLUMN muted INTEGER NOT NULL DEFAULT 0 CHECK(muted IN (0, 1))",
            ] {
                if let Err(error) = transaction.execute_batch(statement) {
                    if !error.to_string().contains("duplicate column name") { return Err(error); }
                }
            }
            transaction.execute_batch("PRAGMA user_version = 21;")?;
            transaction.commit()?;
        }
        if version < 22 {
            let transaction = connection.transaction()?;
            // Recall marks stay on the message row; bodies are replaced with a single space
            // because both message tables require a non-empty body.
            for statement in [
                "ALTER TABLE direct_messages ADD COLUMN recalled_at INTEGER",
                "ALTER TABLE group_messages ADD COLUMN recalled_at INTEGER",
                "ALTER TABLE groups ADD COLUMN dissolved_at INTEGER",
                "ALTER TABLE groups ADD COLUMN dissolved_by TEXT",
            ] {
                if let Err(error) = transaction.execute_batch(statement) {
                    if !error.to_string().contains("duplicate column name") { return Err(error); }
                }
            }
            transaction.execute_batch(
                "CREATE TABLE IF NOT EXISTS contact_requests (
                    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) = 32),
                    direction TEXT NOT NULL CHECK(direction IN ('in', 'out')),
                    peer TEXT NOT NULL CHECK(length(peer) = 64),
                    nickname TEXT NOT NULL DEFAULT '',
                    username TEXT,
                    address TEXT,
                    created_at INTEGER NOT NULL,
                    status TEXT NOT NULL CHECK(status IN ('pending', 'accepted', 'rejected'))
                );
                CREATE UNIQUE INDEX IF NOT EXISTS contact_requests_pending_pair
                    ON contact_requests(peer, direction) WHERE status = 'pending';
                CREATE INDEX IF NOT EXISTS contact_requests_by_state ON contact_requests(direction, status, created_at);
                CREATE TABLE IF NOT EXISTS direct_recalls (
                    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) = 32),
                    peer TEXT NOT NULL CHECK(length(peer) = 64),
                    message_id TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    status TEXT NOT NULL CHECK(status IN ('pending', 'delivered'))
                );
                CREATE INDEX IF NOT EXISTS direct_recalls_pending ON direct_recalls(peer, status);
                CREATE TABLE group_control_v22 (
                    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) = 32),
                    group_id TEXT NOT NULL,
                    target TEXT NOT NULL CHECK(length(target) = 64),
                    kind TEXT NOT NULL CHECK(kind IN ('invite', 'members', 'remove', 'leave', 'transfer', 'dissolve', 'join', 'join_reject', 'recall')),
                    payload TEXT NOT NULL CHECK(length(payload) <= 65536),
                    status TEXT NOT NULL CHECK(status IN ('pending', 'delivered')),
                    created_at INTEGER NOT NULL
                );
                INSERT INTO group_control_v22(id, group_id, target, kind, payload, status, created_at)
                    SELECT id, group_id, target, kind, payload, status, created_at FROM group_control;
                DROP TABLE group_control;
                ALTER TABLE group_control_v22 RENAME TO group_control;
                CREATE INDEX IF NOT EXISTS group_control_pending ON group_control(target, status);
                PRAGMA user_version = 22;",
            )?;
            transaction.commit()?;
        }
        if version < 23 {
            let transaction = connection.transaction()?;
            transaction.execute_batch(
                "CREATE TABLE group_control_v23 (
                    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) = 32),
                    group_id TEXT NOT NULL,
                    target TEXT NOT NULL CHECK(length(target) = 64),
                    kind TEXT NOT NULL CHECK(kind IN ('invite', 'members', 'remove', 'leave', 'transfer', 'dissolve', 'join', 'join_reject', 'recall', 'notice')),
                    payload TEXT NOT NULL CHECK(length(payload) <= 65536),
                    status TEXT NOT NULL CHECK(status IN ('pending', 'delivered')),
                    created_at INTEGER NOT NULL
                );
                INSERT INTO group_control_v23(id, group_id, target, kind, payload, status, created_at)
                    SELECT id, group_id, target, kind, payload, status, created_at FROM group_control;
                DROP TABLE group_control;
                ALTER TABLE group_control_v23 RENAME TO group_control;
                CREATE INDEX IF NOT EXISTS group_control_pending ON group_control(target, status);
                CREATE TABLE IF NOT EXISTS group_notices (
                    group_id TEXT PRIMARY KEY NOT NULL REFERENCES groups(id),
                    notice TEXT NOT NULL CHECK(length(notice) <= 2048),
                    updated_at INTEGER NOT NULL,
                    updated_by TEXT NOT NULL CHECK(length(updated_by) = 64)
                );
                 PRAGMA user_version = 23;",
            )?;
            transaction.commit()?;
        }
        if version < 24 {
            let transaction = connection.transaction()?;
            for statement in [
                "ALTER TABLE direct_recalls ADD COLUMN message_created_at INTEGER",
                "ALTER TABLE direct_recalls ADD COLUMN message_logical INTEGER",
            ] {
                if let Err(error) = transaction.execute_batch(statement) {
                    if !error.to_string().contains("duplicate column name") { return Err(error); }
                }
            }
            transaction.execute_batch(
                "UPDATE direct_recalls SET
                     message_created_at = COALESCE(message_created_at, (SELECT m.created_at FROM direct_messages m WHERE m.id = direct_recalls.message_id AND m.peer = direct_recalls.peer)),
                     message_logical = COALESCE(message_logical, (SELECT m.logical FROM direct_messages m WHERE m.id = direct_recalls.message_id AND m.peer = direct_recalls.peer));
                 PRAGMA user_version = 24;",
            )?;
            transaction.commit()?;
        }
        if version < 25 {
            let transaction = connection.transaction()?;
            transaction.execute_batch(
                "CREATE TABLE IF NOT EXISTS peer_avatars (
                    peer TEXT PRIMARY KEY NOT NULL CHECK(length(peer) = 64),
                    png BLOB NOT NULL CHECK(length(png) BETWEEN 1 AND 524288)
                );
                CREATE TABLE IF NOT EXISTS avatar_addresses (
                    peer TEXT PRIMARY KEY NOT NULL CHECK(length(peer) = 64),
                    address TEXT NOT NULL CHECK(length(address) BETWEEN 1 AND 128)
                );
                PRAGMA user_version = 25;",
            )?;
            transaction.commit()?;
        }
        if version < 26 {
            let transaction = connection.transaction()?;
            if let Err(error) = transaction.execute_batch("ALTER TABLE local_messages ADD COLUMN recalled_at INTEGER") {
                if !error.to_string().contains("duplicate column name") { return Err(error); }
            }
            transaction.execute_batch("PRAGMA user_version = 26;")?;
            transaction.commit()?;
        }
        if version < 27 {
            let transaction = connection.transaction()?;
            for statement in [
                "ALTER TABLE direct_messages ADD COLUMN reply_to TEXT",
                "ALTER TABLE direct_messages ADD COLUMN reply_sender TEXT",
                "ALTER TABLE direct_messages ADD COLUMN reply_preview TEXT",
            ] {
                if let Err(error) = transaction.execute_batch(statement) {
                    if !error.to_string().contains("duplicate column name") { return Err(error); }
                }
            }
            transaction.execute_batch("PRAGMA user_version = 27;")?;
            transaction.commit()?;
        }
        Ok(Self { connection, data_dir, address_revision: std::cell::Cell::new(0) })
    }

    /// Records a candidate without replacing an existing trust decision.
    pub fn add_candidate(&self, fingerprint: &str, display_name: &str) -> Result<bool> {
        self.add_candidate_with_username(fingerprint, display_name, None)
    }

    /// Same as `add_candidate`, also storing the peer's advertised username when present.
    pub fn add_candidate_with_username(&self, fingerprint: &str, display_name: &str, username: Option<&str>) -> Result<bool> {
        if !valid_fingerprint(fingerprint) || display_name.len() > 256 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let username = username.map(str::trim).filter(|value| valid_username(value));
        Ok(self.connection.execute(
            "INSERT INTO contacts (fingerprint, display_name, trust_state, username)
             VALUES (?1, ?2, 'pending', ?3) ON CONFLICT(fingerprint) DO NOTHING",
            params![fingerprint, display_name, username],
        )? == 1)
    }

    /// Only acknowledge Inserted or Duplicate after an authenticated peer and full payload match.
    pub fn save_incoming(&self, id: &str, peer: &str, body: &str, created_at: i64) -> Result<IncomingResult> {
        if !valid_fingerprint(peer) || id.is_empty() || id.len() > 128 || body.is_empty() || body.len() > 32768 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let tx = self.connection.unchecked_transaction()?;
        let verified: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM contacts WHERE fingerprint = ?1 AND trust_state = 'verified')",
            [peer], |row| row.get(0),
        )?;
        if !verified { return Ok(IncomingResult::Unverified); }
        let old: Option<(String, String, String, i64)> = tx.query_row(
            "SELECT peer_fingerprint, body, direction, created_at FROM messages WHERE id = ?1",
            [id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).optional()?;
        if let Some((old_peer, old_body, direction, time)) = old {
            return Ok(if old_peer == peer && old_body == body && direction == "incoming" && time == created_at {
                IncomingResult::Duplicate
            } else { IncomingResult::IdConflict });
        }
        tx.execute(
            "INSERT INTO messages (id, peer_fingerprint, body, direction, created_at, status)
             VALUES (?1, ?2, ?3, 'incoming', ?4, 'received')",
            params![id, peer, body, created_at],
        )?;
        tx.commit()?;
        Ok(IncomingResult::Inserted)
    }

    /// Keyset pagination remains stable when late messages are inserted before the anchor.
    pub fn list_direct(&self, peer: &str, limit: u32, before: Option<&DirectCursor>) -> Result<Vec<DirectMessage>> {
        if !valid_fingerprint(peer) || limit == 0 || limit > 200 || before.is_some_and(|c| c.logical <= 0 || !valid_fingerprint(&c.sender) || c.id.is_empty() || c.id.len() > 128) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let mut statement = self.connection.prepare(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM direct_messages m LEFT JOIN file_transfers f ON f.message_id = m.id
             WHERE m.peer = ?1 AND (?2 IS NULL OR (m.logical, m.sender, m.id) < (?2, ?3, ?4))
             ORDER BY m.logical DESC, m.sender DESC, m.id DESC LIMIT ?5"
        ))?;
        let rows = statement.query_map(params![peer, before.map(|c| c.logical), before.map(|c| c.sender.as_str()), before.map(|c| c.id.as_str()), limit], map_message)?;
        rows.collect()
    }

    /// Substring search within one direct conversation, newest first. Matching is
    /// case-insensitive for ASCII and works as plain substring matching for Chinese.
    pub fn search_direct(&self, peer: &str, query: &str, category: &str, limit: u32, offset: u32) -> Result<Vec<DirectMessage>> {
        if !valid_fingerprint(peer) || !valid_search(&query, category, limit, offset) { return Err(rusqlite::Error::InvalidQuery); }
        if !matches!(category, "all" | "text" | "file" | "image") { return Err(rusqlite::Error::InvalidQuery); }
        let pattern = like_pattern(query);
        let category_filter = match category {
            "text" => "m.kind = 'text'",
            "file" => "m.kind = 'file'",
            "image" => "m.kind = 'file' AND (lower(m.file_name) LIKE '%.png' OR lower(m.file_name) LIKE '%.jpg' OR lower(m.file_name) LIKE '%.jpeg' OR lower(m.file_name) LIKE '%.gif' OR lower(m.file_name) LIKE '%.webp' OR lower(m.file_name) LIKE '%.bmp')",
            _ => "1 = 1",
        };
        let mut statement = self.connection.prepare(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM direct_messages m LEFT JOIN file_transfers f ON f.message_id = m.id
             WHERE m.peer = ?1 AND m.recalled_at IS NULL AND {category_filter}
               AND (?2 = '' OR m.body LIKE ?3 ESCAPE '\\' OR COALESCE(m.file_name, '') LIKE ?3 ESCAPE '\\')
             ORDER BY m.logical DESC, m.sender DESC, m.id DESC LIMIT ?4 OFFSET ?5"
        ))?;
        let rows = statement.query_map(params![peer, query, pattern, limit, offset], map_message)?;
        rows.collect()
    }

    /// Fetch one pending outgoing message without imposing a queue-page limit.
    pub fn pending_direct_by_id(&self, peer: &str, id: &str) -> Result<Option<DirectMessage>> {
        if !valid_fingerprint(peer) || id.is_empty() || id.len() > 128 { return Err(rusqlite::Error::InvalidQuery); }
        self.connection.query_row(
            &format!("SELECT {MESSAGE_COLUMNS} FROM direct_messages m LEFT JOIN file_transfers f ON f.message_id = m.id
             WHERE m.peer = ?1 AND m.id = ?2 AND m.sender != ?1 AND m.status = 'pending'
             AND EXISTS(SELECT 1 FROM contacts WHERE fingerprint = ?1 AND trust_state = 'verified')"),
            params![peer, id], map_message,
        ).optional()
    }

    /// The caller wraps to None after reaching the end of the peer-key order.
    pub fn next_pending_delivery(&self, after: Option<&str>) -> Result<Option<PendingDelivery>> {
        if after.is_some_and(|peer| !valid_fingerprint(peer)) { return Err(rusqlite::Error::InvalidQuery); }
        self.connection.query_row(
            "SELECT a.peer, a.address,
                (SELECT m.id FROM direct_messages m WHERE m.peer = a.peer AND m.sender != a.peer AND m.status = 'pending'
                 ORDER BY m.logical, m.sender, m.id LIMIT 1) AS message_id
             FROM contact_addresses a JOIN contacts c ON c.fingerprint = a.peer
             WHERE c.trust_state = 'verified' AND (?1 IS NULL OR a.peer > ?1)
             AND EXISTS(SELECT 1 FROM direct_messages m WHERE m.peer = a.peer AND m.sender != a.peer AND m.status = 'pending')
             ORDER BY a.peer LIMIT 1", [after],
            |row| Ok(PendingDelivery { peer: row.get(0)?, address: row.get(1)?, id: row.get(2)? }),
        ).optional()
    }

    pub fn open_direct(&self, peer: &str) -> Result<bool> {
        if !valid_fingerprint(peer) { return Err(rusqlite::Error::InvalidQuery); }
        Ok(self.connection.execute(
            "INSERT INTO direct_conversations(peer, hidden)
             SELECT fingerprint, 0 FROM contacts WHERE fingerprint = ?1 AND trust_state = 'verified'
             ON CONFLICT(peer) DO UPDATE SET hidden = 0, unread = 0", [peer],
        )? == 1)
    }

    pub fn hide_direct(&self, peer: &str) -> Result<bool> {
        if !valid_fingerprint(peer) { return Err(rusqlite::Error::InvalidQuery); }
        Ok(self.connection.execute("UPDATE direct_conversations SET hidden = 1 WHERE peer = ?1 AND hidden = 0", [peer])? == 1)
    }

    /// Toggles "do not disturb" for one direct conversation. Muting never stops the
    /// unread counter; it only asks the list to show a dot instead of a number.
    pub fn set_direct_muted(&self, peer: &str, muted: bool) -> Result<bool> {
        if !valid_fingerprint(peer) { return Err(rusqlite::Error::InvalidQuery); }
        Ok(self.connection.execute(
            "UPDATE direct_conversations SET muted = ?2 WHERE peer = ?1", params![peer, i64::from(muted)],
        )? == 1)
    }

    /// Marks a conversation unread by hand. The existing count is kept when it is larger,
    /// and a hidden row is brought back so the mark is visible.
    pub fn mark_direct_unread(&self, peer: &str) -> Result<bool> {
        if !valid_fingerprint(peer) { return Err(rusqlite::Error::InvalidQuery); }
        Ok(self.connection.execute(
            "UPDATE direct_conversations SET unread = (CASE WHEN unread < 1 THEN 1 ELSE unread END), hidden = 0 WHERE peer = ?1",
            [peer])? == 1)
    }

    /// A peer may acknowledge only its own outgoing pending messages.
    pub fn ack_direct(&self, peer: &str, id: &str) -> Result<bool> {
        if !valid_fingerprint(peer) || id.is_empty() || id.len() > 128 { return Err(rusqlite::Error::InvalidQuery); }
        Ok(self.connection.execute(
            "UPDATE direct_messages SET status = 'delivered' WHERE id = ?1 AND peer = ?2 AND sender != ?2 AND status = 'pending'
             AND EXISTS(SELECT 1 FROM contacts WHERE fingerprint = ?2 AND trust_state = 'verified')", params![id, peer],
        )? == 1)
    }

    pub fn pending_direct(&self, peer: &str, limit: u32) -> Result<Vec<DirectMessage>> {
        if !valid_fingerprint(peer) || limit == 0 || limit > 200 { return Err(rusqlite::Error::InvalidQuery); }
        let mut statement = self.connection.prepare(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM direct_messages m LEFT JOIN file_transfers f ON f.message_id = m.id
             WHERE m.peer = ?1 AND m.sender != ?1 AND m.status = 'pending'
             AND EXISTS(SELECT 1 FROM contacts WHERE fingerprint = ?1 AND trust_state = 'verified')
             ORDER BY m.logical, m.sender, m.id LIMIT ?2"
        ))?;
        let rows = statement.query_map(params![peer, limit], map_message)?;
        rows.collect()
    }

    /// An ACK can update only a message addressed to this verified peer.
    pub fn mark_delivered(&self, id: &str, peer: &str) -> Result<bool> {
        Ok(self.connection.execute(
            "UPDATE messages SET status = 'delivered' WHERE id = ?1 AND peer_fingerprint = ?2
             AND direction = 'outgoing' AND status IN ('pending', 'failed')
             AND EXISTS(SELECT 1 FROM contacts WHERE fingerprint = ?2 AND trust_state = 'verified')",
            params![id, peer],
        )? == 1)
    }

    /// Call only after the user verifies the full fingerprint out of band.
    pub fn confirm_candidate(&self, fingerprint: &str) -> Result<bool> {
        Ok(self.connection.execute(
            "UPDATE contacts SET trust_state = 'verified' WHERE fingerprint = ?1 AND trust_state = 'pending'",
            params![fingerprint],
        )? == 1)
    }

    /// Removes an inbound pending contact request; verified contacts are never touched.
    /// Removes a confirmed contact locally. Chat history is kept on this device,
    /// mirroring how left groups retain their records.
    pub fn remove_contact(&self, fingerprint: &str) -> Result<bool> {
        let removed = self.connection.execute("DELETE FROM contacts WHERE fingerprint = ?1", [fingerprint])?;
        Ok(removed > 0)
    }

    pub fn decline_candidate(&self, fingerprint: &str) -> Result<bool> {
        if !valid_fingerprint(fingerprint) { return Err(rusqlite::Error::InvalidQuery); }
        Ok(self.connection.execute(
            "DELETE FROM contacts WHERE fingerprint = ?1 AND trust_state = 'pending'",
            [fingerprint],
        )? == 1)
    }

    pub fn is_verified(&self, fingerprint: &str) -> Result<bool> {
        let trust: Option<String> = self.connection.query_row(
            "SELECT trust_state FROM contacts WHERE fingerprint = ?1",
            [fingerprint],
            |row| row.get(0),
        ).optional()?;
        Ok(trust.as_deref() == Some("verified"))
    }

    /// Authorization is independent of cached images and discovery hints.
    pub fn avatar_authorized(&self, self_id: &str, peer: &str) -> Result<bool> {
        if !valid_fingerprint(self_id) || !valid_fingerprint(peer) || self_id == peer {
            return Err(rusqlite::Error::InvalidQuery);
        }
        self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM contacts WHERE fingerprint = ?2 AND trust_state = 'verified')
                OR EXISTS(SELECT 1 FROM groups g
                  JOIN group_members me ON me.group_id = g.id AND me.member = ?1 AND me.active = 1
                  JOIN group_members other ON other.group_id = g.id AND other.member = ?2 AND other.active = 1
                  WHERE g.active = 1)", params![self_id, peer], |row| row.get(0))
    }

    /// Images are presentation data, never identity or authorization evidence.
    pub fn peer_avatar(&self, self_id: &str, peer: &str) -> Result<Option<Vec<u8>>> {
        if !self.avatar_authorized(self_id, peer)? { return Ok(None); }
        self.connection.query_row("SELECT png FROM peer_avatars WHERE peer = ?1", [peer], |row| row.get(0)).optional()
    }

    /// Call only for a response from the peer's authenticated TLS session.
    pub fn set_peer_avatar(&self, self_id: &str, peer: &str, png: Option<&[u8]>) -> Result<bool> {
        if !self.avatar_authorized(self_id, peer)? { return Ok(false); }
        match png {
            None => { self.connection.execute("DELETE FROM peer_avatars WHERE peer = ?1", [peer])?; }
            Some(bytes) => {
                if crate::avatar::validate(bytes).is_err() { return Err(rusqlite::Error::InvalidQuery); }
                self.connection.execute(
                    "INSERT INTO peer_avatars(peer, png) VALUES (?1, ?2)
                     ON CONFLICT(peer) DO UPDATE SET png = excluded.png", params![peer, bytes])?;
            }
        }
        Ok(true)
    }

    /// Additional certificate fingerprints eligible only for avatar requests.
    pub fn avatar_group_peers(&self, self_id: &str) -> Result<Vec<String>> {
        if !valid_fingerprint(self_id) { return Err(rusqlite::Error::InvalidQuery); }
        let mut statement = self.connection.prepare(
            "SELECT DISTINCT other.member FROM groups g
             JOIN group_members me ON me.group_id = g.id AND me.member = ?1 AND me.active = 1
             JOIN group_members other ON other.group_id = g.id AND other.active = 1
             WHERE g.active = 1 AND other.member != ?1")?;
        let rows = statement.query_map([self_id], |row| row.get(0))?;
        rows.collect()
    }

    /// Called only after a probe has proven this fingerprint, and only as a hint.
    pub fn set_avatar_address(&self, self_id: &str, peer: &str, address: &str) -> Result<bool> {
        if !self.avatar_authorized(self_id, peer)? { return Ok(false); }
        let address = crate::address::parse_manual_address(address).map_err(|_| rusqlite::Error::InvalidQuery)?.to_string();
        self.connection.execute(
            "INSERT INTO avatar_addresses(peer, address) VALUES (?1, ?2)
             ON CONFLICT(peer) DO UPDATE SET address = excluded.address", params![peer, address])?;
        Ok(true)
    }

    pub fn avatar_address(&self, self_id: &str, peer: &str) -> Result<Option<String>> {
        if !self.avatar_authorized(self_id, peer)? { return Ok(None); }
        if let Some(address) = self.contact_address(peer)? { return Ok(Some(address)); }
        self.connection.query_row("SELECT address FROM avatar_addresses WHERE peer = ?1", [peer], |row| row.get(0)).optional()
    }

    /// Addresses are connection hints, never proof of identity.
    pub fn set_contact_address(&self, peer: &str, address: &str) -> Result<bool> {
        if !valid_fingerprint(peer) { return Err(rusqlite::Error::InvalidQuery); }
        let canonical = crate::address::parse_manual_address(address).map_err(|_| rusqlite::Error::InvalidQuery)?.to_string();
        let revision = self.address_revision.get().checked_add(1).ok_or(rusqlite::Error::InvalidQuery)?;
        let saved = self.connection.execute(
            "INSERT INTO contact_addresses(peer, address)
             SELECT fingerprint, ?2 FROM contacts WHERE fingerprint = ?1 AND trust_state = 'verified'
             ON CONFLICT(peer) DO UPDATE SET address = excluded.address",
            params![peer, canonical],
        )? == 1;
        if saved { self.address_revision.set(revision); }
        Ok(saved)
    }

    /// Snapshot under the store lock before probing; any intervening address write cancels the result.
    pub fn address_revision(&self) -> u64 { self.address_revision.get() }

    /// The peer must be derived from a completed TLS probe, not discovery metadata.
    pub fn set_discovered_address(&self, peer: &str, address: &str, revision: u64) -> Result<bool> {
        if self.address_revision.get() != revision { return Ok(false); }
        let canonical = crate::address::parse_manual_address(address).map_err(|_| rusqlite::Error::InvalidQuery)?.to_string();
        if self.contact_address(peer)?.as_deref() == Some(canonical.as_str()) { return Ok(true); }
        self.set_contact_address(peer, address)
    }

    pub fn contact_address(&self, peer: &str) -> Result<Option<String>> {
        if !valid_fingerprint(peer) { return Err(rusqlite::Error::InvalidQuery); }
        self.connection.query_row(
            "SELECT a.address FROM contact_addresses a JOIN contacts c ON c.fingerprint = a.peer
             WHERE a.peer = ?1 AND c.trust_state = 'verified'", [peer], |row| row.get(0),
        ).optional()
    }

    pub fn list_contacts(&self) -> Result<Vec<Contact>> {
        let mut statement = self.connection.prepare(
            "SELECT fingerprint, display_name, trust_state, username, remark, category FROM contacts ORDER BY display_name, fingerprint LIMIT 1000"
        )?;
        let rows = statement.query_map([], |row| Ok(Contact {
            fingerprint: row.get(0)?, display_name: row.get(1)?, trust_state: row.get(2)?,
            username: row.get(3)?, remark: row.get(4)?, category: row.get(5)?,
        }))?;
        rows.collect()
    }

    /// Sets local remark/category on an existing contact. Neither field affects trust.
    pub fn set_contact_profile(&self, fingerprint: &str, remark: Option<&str>, category: Option<&str>) -> Result<bool> {
        if !valid_fingerprint(fingerprint) { return Err(rusqlite::Error::InvalidQuery); }
        for value in [remark, category].into_iter().flatten() {
            if value.chars().count() > 64 || value.chars().any(char::is_control) {
                return Err(rusqlite::Error::InvalidQuery);
            }
        }
        let remark = remark.map(str::trim).filter(|value| !value.is_empty());
        let category = category.map(str::trim).filter(|value| !value.is_empty());
        Ok(self.connection.execute(
            "UPDATE contacts SET remark = ?2, category = ?3 WHERE fingerprint = ?1",
            params![fingerprint, remark, category])? == 1)
    }

    pub fn get_profile(&self) -> Result<Profile> {
        let read = |key: &str| -> Result<Option<String>> {
            self.connection.query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| row.get(0)).optional()
        };
        Ok(Profile {
            nickname: read("profile_nickname")?.unwrap_or_default(),
            email: read("profile_email")?.unwrap_or_default(),
            username: read("profile_username")?.unwrap_or_default(),
        })
    }

    pub fn set_profile(&self, nickname: &str, email: &str) -> Result<bool> {
        let nickname = nickname.trim();
        let email = email.trim();
        if nickname.chars().count() > 64 || nickname.chars().any(char::is_control) { return Ok(false); }
        if email.chars().count() > 128 || email.chars().any(char::is_control) || (email.matches('@').count() != 1 && !email.is_empty()) {
            return Ok(false);
        }
        for (key, value) in [("profile_nickname", nickname), ("profile_email", email)] {
            self.connection.execute(
                "INSERT INTO settings(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value])?;
        }
        Ok(true)
    }

    /// Adding a discovered peer also adds us on the peer side unless it opted out.
    /// This is a local preference, never sent as a claim about trust.
    pub fn auto_accept_contacts(&self) -> Result<bool> {
        Ok(self.get_setting("auto_accept_contacts")?.as_deref() != Some("0"))
    }

    pub fn set_auto_accept_contacts(&self, enabled: bool) -> Result<()> {
        self.set_setting("auto_accept_contacts", Some(if enabled { "1" } else { "0" }))?;
        Ok(())
    }

    /// Usernames are display/search labels only; they never replace the fingerprint.
    pub fn set_username(&self, username: &str) -> Result<bool> {
        let username = username.trim();
        if !valid_username(username) { return Ok(false); }
        self.connection.execute(
            "INSERT INTO settings(key, value) VALUES ('profile_username', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [username])?;
        Ok(true)
    }

    pub fn set_onboarding_done(&self) -> Result<()> {
        self.set_setting("onboarding_done", Some("1"))?;
        Ok(())
    }

    pub fn onboarding_done(&self) -> Result<bool> {
        Ok(self.get_setting("onboarding_done")?.as_deref() == Some("1"))
    }

    /// Contact list search across name, local remark, category and fingerprint.
    pub fn search_contacts(&self, query: &str, limit: u32) -> Result<Vec<Contact>> {
        if query.chars().count() > 64 || limit == 0 || limit > 200 || query.chars().any(char::is_control) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let pattern = like_pattern(query);
        let mut statement = self.connection.prepare(
            "SELECT fingerprint, display_name, trust_state, username, remark, category FROM contacts
             WHERE ?1 = '' OR display_name LIKE ?2 ESCAPE '\\' OR COALESCE(username, '') LIKE ?2 ESCAPE '\\'
                OR COALESCE(remark, '') LIKE ?2 ESCAPE '\\'
                OR COALESCE(category, '') LIKE ?2 ESCAPE '\\' OR fingerprint LIKE ?2 ESCAPE '\\'
             ORDER BY display_name, fingerprint LIMIT ?3"
        )?;
        let rows = statement.query_map(params![query, pattern, limit], |row| Ok(Contact {
            fingerprint: row.get(0)?, display_name: row.get(1)?, trust_state: row.get(2)?,
            username: row.get(3)?, remark: row.get(4)?, category: row.get(5)?,
        }))?;
        rows.collect()
    }

    /// Persist before attempting transmission; wall-clock time is for display only.
    pub fn queue_direct(&self, sender: &str, peer: &str, body: &str, created_at: i64) -> Result<DirectMessage> {
        self.queue_direct_with_reply(sender, peer, body, created_at, None)
    }

    pub fn queue_direct_with_reply(&self, sender: &str, peer: &str, body: &str, created_at: i64,
        reply_to: Option<&str>) -> Result<DirectMessage> {
        if !valid_fingerprint(sender) || !valid_fingerprint(peer) || sender == peer || body.trim().is_empty() || body.len() > 32768 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let tx = self.connection.unchecked_transaction()?;
        let verified: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM contacts WHERE fingerprint = ?1 AND trust_state = 'verified')", [peer], |r| r.get(0))?;
        if !verified { return Err(rusqlite::Error::InvalidQuery); }
        let (reply_sender, reply_preview) = direct_quote(&tx, sender, peer, reply_to)?;
        tx.execute("INSERT INTO direct_conversations(peer, hidden) VALUES (?1, 0) ON CONFLICT(peer) DO UPDATE SET hidden = 0", [peer])?;
        if tx.execute("UPDATE logical_clock SET value = value + 1 WHERE singleton = 1 AND value < 9223372036854775807", [])? != 1 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let (id, logical): (String, i64) = tx.query_row("SELECT lower(hex(randomblob(16))), value FROM logical_clock WHERE singleton = 1", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let collision: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM local_messages WHERE id = ?1 UNION ALL SELECT 1 FROM messages WHERE id = ?1)", [&id], |r| r.get(0))?;
        if collision { return Err(rusqlite::Error::InvalidQuery); }
        tx.execute("INSERT INTO direct_messages(id, peer, sender, body, created_at, logical, status, reply_to, reply_sender, reply_preview)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7, ?8, ?9)",
            params![id, peer, sender, body, created_at, logical, reply_to, reply_sender, reply_preview])?;
        tx.commit()?;
        Ok(DirectMessage {
            id, peer: peer.into(), sender: sender.into(), body: body.into(), created_at, logical, status: "pending".into(),
            kind: "text".into(), file_name: None, file_size: None, file_sha256: None, file_status: None, file_transferred: None, file_error: None,
            recalled_at: None,
            reply_to: reply_to.map(str::to_owned), reply_sender, reply_preview,
        })
    }

    /// `peer` must be the fingerprint authenticated by the transport, never a payload claim.
    /// Only Inserted or Duplicate may be acknowledged after this transaction returns.
    pub fn receive_direct(&self, peer: &str, id: &str, body: &str, created_at: i64, logical: i64) -> Result<IncomingResult> {
        self.receive_direct_with_reply(peer, id, body, created_at, logical, None, None, None)
    }

    pub fn receive_direct_with_reply(&self, peer: &str, id: &str, body: &str, created_at: i64, logical: i64,
        reply_to: Option<&str>, reply_sender: Option<&str>, reply_preview: Option<&str>) -> Result<IncomingResult> {
        if !valid_fingerprint(peer) || id.is_empty() || id.len() > 128 || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') || body.is_empty() || body.len() > 32768 || logical <= 0 || logical == i64::MAX {
            return Err(rusqlite::Error::InvalidQuery);
        }
        if !valid_reply_metadata(reply_to, reply_sender, reply_preview) { return Err(rusqlite::Error::InvalidQuery); }
        let tx = self.connection.unchecked_transaction()?;
        let verified: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM contacts WHERE fingerprint = ?1 AND trust_state = 'verified')", [peer], |r| r.get(0))?;
        if !verified { return Ok(IncomingResult::Unverified); }
        let existing: Option<(String, String, String, i64, i64, String, Option<String>, Option<String>, Option<String>)> = tx.query_row(
            "SELECT peer, sender, body, created_at, logical, status, reply_to, reply_sender, reply_preview FROM direct_messages WHERE id = ?1", [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?)),
        ).optional()?;
        if let Some((old_peer, sender, old_body, time, old_logical, status, old_reply_to, old_reply_sender, old_reply_preview)) = existing {
            return Ok(if old_peer == peer && sender == peer && old_body == body && time == created_at && old_logical == logical
                && status == "received" && old_reply_to.as_deref() == reply_to && old_reply_sender.as_deref() == reply_sender
                && old_reply_preview.as_deref() == reply_preview {
                IncomingResult::Duplicate
            } else { IncomingResult::IdConflict });
        }
        let collision: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM local_messages WHERE id = ?1 UNION ALL SELECT 1 FROM messages WHERE id = ?1)", [id], |r| r.get(0))?;
        if collision { return Ok(IncomingResult::IdConflict); }
        if tx.execute("UPDATE logical_clock SET value = max(value, ?1) + 1 WHERE singleton = 1 AND value < 9223372036854775807", [logical])? != 1 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        tx.execute(
            "INSERT INTO direct_conversations(peer, hidden, unread) VALUES (?1, 0, 1)
             ON CONFLICT(peer) DO UPDATE SET hidden = 0, unread = unread + 1", [peer],
        )?;
        tx.execute("INSERT INTO direct_messages(id, peer, sender, body, created_at, logical, status, reply_to, reply_sender, reply_preview)
            VALUES (?1, ?2, ?2, ?3, ?4, ?5, 'received', ?6, ?7, ?8)",
            params![id, peer, body, created_at, logical, reply_to, reply_sender, reply_preview])?;
        tx.commit()?;
        Ok(IncomingResult::Inserted)
    }

    pub fn attachments_dir(&self) -> PathBuf { self.data_dir.join("attachments") }

    /// Incoming files are separated by conversation kind so a direct chat and a
    /// group never compete for the same file name.
    fn default_recv_dir(&self, kind: RecvKind) -> PathBuf {
        self.attachments_dir().join("in").join(match kind { RecvKind::Direct => "direct", RecvKind::Group => "group" })
    }

    /// Effective receive directory: the user's choice when set, otherwise the
    /// built-in default. A missing or unreadable setting falls back silently here
    /// because saving already validates the directory.
    pub fn recv_dir(&self, kind: RecvKind) -> PathBuf {
        let key = match kind { RecvKind::Direct => "file_recv_dir_direct", RecvKind::Group => "file_recv_dir_group" };
        match self.get_setting(key) {
            Ok(Some(value)) if !value.trim().is_empty() => PathBuf::from(value),
            _ => self.default_recv_dir(kind),
        }
    }

    /// Whether the user chose a custom directory for this conversation kind.
    pub fn recv_dir_custom(&self, kind: RecvKind) -> bool {
        let key = match kind { RecvKind::Direct => "file_recv_dir_direct", RecvKind::Group => "file_recv_dir_group" };
        matches!(self.get_setting(key), Ok(Some(value)) if !value.trim().is_empty())
    }

    /// `None` clears the custom directory and restores the built-in default.
    pub fn set_recv_dir(&self, kind: RecvKind, dir: Option<&str>) -> Result<()> {
        let key = match kind { RecvKind::Direct => "file_recv_dir_direct", RecvKind::Group => "file_recv_dir_group" };
        self.set_setting(key, dir.filter(|value| !value.trim().is_empty()))?;
        Ok(())
    }

    /// Folder for one inbound file: an in-flight transfer keeps the folder it started
    /// in, then a one-shot "save as" choice, then the configured default.
    fn incoming_dir_for(&self, id: &str, kind: RecvKind) -> Result<PathBuf> {
        if let Some(existing) = self.file_transfer_target(id)? {
            if let Some(parent) = existing.parent() { return Ok(parent.to_path_buf()); }
        }
        if let Some(chosen) = self.file_offer_target_dir(id)? { return Ok(chosen); }
        Ok(self.recv_dir(kind))
    }

    /// True when the directory is inside the app attachment store, which is the only
    /// area the storage quota covers. Custom folders are managed by the user.
    pub fn within_attachments(&self, dir: &Path) -> bool {
        let base = self.attachments_dir().to_string_lossy().to_lowercase();
        let candidate = dir.to_string_lossy().to_lowercase();
        candidate == base || candidate.starts_with(&format!("{base}\\")) || candidate.starts_with(&format!("{base}/"))
    }

    /// Bytes currently held by received attachments, including partial downloads.
    pub fn storage_used(&self) -> u64 { directory_usage(&self.attachments_dir()) }

    pub fn storage_quota(&self) -> Result<u64> {
        let stored: Option<String> = self.connection.query_row(
            "SELECT value FROM settings WHERE key = 'storage_quota_bytes'", [], |row| row.get(0),
        ).optional()?;
        Ok(stored.and_then(|value| value.parse::<u64>().ok())
            .filter(|bytes| (MIN_STORAGE_QUOTA..=MAX_STORAGE_QUOTA).contains(bytes))
            .unwrap_or(DEFAULT_STORAGE_QUOTA))
    }

    pub fn set_storage_quota(&self, bytes: u64) -> Result<bool> {
        if !(MIN_STORAGE_QUOTA..=MAX_STORAGE_QUOTA).contains(&bytes) { return Ok(false); }
        self.connection.execute(
            "INSERT INTO settings(key, value) VALUES ('storage_quota_bytes', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [bytes.to_string()],
        )?;
        Ok(true)
    }

    pub fn storage_status(&self) -> Result<StorageUsage> {
        Ok(StorageUsage { used: self.storage_used(), quota: self.storage_quota()? })
    }

    pub fn data_dir(&self) -> &Path { &self.data_dir }

    /// Direct access for maintenance tasks (backup path remapping); read-write.
    pub fn raw_connection(&self) -> &rusqlite::Connection { &self.connection }

    /// Writes a consistent SQLite snapshot with `VACUUM INTO`. When chat history is
    /// excluded only contacts/groups remain; the destination must not exist yet.
    pub fn snapshot_to(&self, destination: &Path, include_chat: bool) -> Result<()> {
        if destination.exists() { return Err(rusqlite::Error::InvalidQuery); }
        let escaped = destination.to_string_lossy().replace('\'', "''");
        self.connection.execute_batch(&format!("VACUUM INTO '{escaped}'"))?;
        if !include_chat {
            let snapshot = Connection::open(destination)?;
            snapshot.execute_batch(
                "PRAGMA foreign_keys = OFF;
                 DELETE FROM file_transfers;
                 DELETE FROM direct_messages;
                 DELETE FROM group_outbox;
                 DELETE FROM group_messages;
                 DELETE FROM local_messages;
                 DELETE FROM messages;",
            )?;
        }
        Ok(())
    }

    /// Merges or replaces local data from a snapshot written by `snapshot_to`.
    /// In merge mode any message id whose immutable fields differ aborts the whole
    /// restore; delivery queues are never imported because they are device-local.
    pub fn restore_from(&mut self, snapshot: &Path, overwrite: bool, now: i64) -> Result<RestoreReport> {
        // VACUUM and ATTACH are both rejected inside a transaction, so the pre-overwrite
        // snapshot and the attachment happen first and only the data copy is transactional.
        let mut snapshot_path = None;
        if overwrite {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|value| value.as_nanos()).unwrap_or(0);
            let target = self.data_dir.join(format!("pre-restore-{now}-{nanos}.db"));
            let escaped_target = target.to_string_lossy().replace('\'', "''");
            self.connection.execute_batch(&format!("VACUUM INTO '{escaped_target}'"))?;
            snapshot_path = Some(target.to_string_lossy().to_string());
        }
        let escaped = snapshot.to_string_lossy().replace('\'', "''");
        self.connection.execute_batch(&format!("ATTACH DATABASE '{escaped}' AS src"))?;
        let result = self.restore_attached(overwrite, snapshot_path);
        let _ = self.connection.execute_batch("DETACH DATABASE src");
        result
    }

    fn restore_attached(&self, overwrite: bool, snapshot_path: Option<String>) -> Result<RestoreReport> {
        let tx = self.connection.unchecked_transaction()?;
        let conflicts: i64 = tx.query_row(
            "SELECT
                (SELECT count(*) FROM src.direct_messages s JOIN main.direct_messages m ON m.id = s.id
                  WHERE s.peer <> m.peer OR s.sender <> m.sender OR s.body <> m.body OR s.created_at <> m.created_at
                     OR s.logical <> m.logical OR s.kind <> m.kind
                     OR COALESCE(s.file_sha256, '') <> COALESCE(m.file_sha256, ''))
              + (SELECT count(*) FROM src.group_messages s JOIN main.group_messages m ON m.id = s.id
                  WHERE s.group_id <> m.group_id OR s.sender <> m.sender OR s.body <> m.body
                     OR s.created_at <> m.created_at OR s.logical <> m.logical
                     OR COALESCE(s.reply_to, '') <> COALESCE(m.reply_to, ''))
              + (SELECT count(*) FROM src.local_messages s JOIN main.local_messages m ON m.id = s.id
                  WHERE s.body <> m.body OR s.created_at <> m.created_at OR s.logical <> m.logical)",
            [], |row| row.get(0),
        )?;
        if conflicts > 0 {
            tx.commit()?;
            return Ok(RestoreReport { mode: "merge".into(), inserted: 0, skipped: 0, conflicts: conflicts as u64, snapshot_path, restored_attachments: 0 });
        }
        let snapshot_messages: i64 = tx.query_row(
            "SELECT (SELECT count(*) FROM src.direct_messages) + (SELECT count(*) FROM src.group_messages)
                  + (SELECT count(*) FROM src.local_messages)", [], |row| row.get(0),
        )?;
        let before: i64 = tx.query_row(
            "SELECT (SELECT count(*) FROM main.direct_messages) + (SELECT count(*) FROM main.group_messages)
                  + (SELECT count(*) FROM main.local_messages)", [], |row| row.get(0),
        )?;
        if overwrite {
            tx.execute_batch(
                "DELETE FROM main.file_transfers;
                 DELETE FROM main.direct_messages;
                 DELETE FROM main.direct_conversations;
                 DELETE FROM main.group_outbox;
                 DELETE FROM main.group_control;
                 DELETE FROM main.group_messages;
                 DELETE FROM main.group_members;
                 DELETE FROM main.groups;
                 DELETE FROM main.local_messages;
                 DELETE FROM main.messages;
                 DELETE FROM main.conversations;
                 DELETE FROM main.contact_addresses;
                 DELETE FROM main.contacts;
                 INSERT INTO main.contacts SELECT * FROM src.contacts;
                 INSERT INTO main.contact_addresses SELECT * FROM src.contact_addresses;
                 INSERT INTO main.conversations SELECT * FROM src.conversations;
                 INSERT INTO main.direct_conversations SELECT * FROM src.direct_conversations;
                 INSERT INTO main.direct_messages SELECT * FROM src.direct_messages;
                 INSERT INTO main.file_transfers SELECT * FROM src.file_transfers;
                 INSERT INTO main.groups SELECT * FROM src.groups;
                 INSERT INTO main.group_members SELECT * FROM src.group_members;
                 INSERT INTO main.group_messages SELECT * FROM src.group_messages;
                 INSERT INTO main.messages SELECT * FROM src.messages;",
            )?;
        } else {
            tx.execute_batch(
                "INSERT OR IGNORE INTO main.contacts SELECT * FROM src.contacts;
                 UPDATE main.contacts SET trust_state = 'verified'
                   WHERE trust_state = 'pending' AND fingerprint IN (SELECT fingerprint FROM src.contacts WHERE trust_state = 'verified');
                 INSERT OR IGNORE INTO main.contact_addresses SELECT * FROM src.contact_addresses;
                 INSERT OR IGNORE INTO main.conversations SELECT * FROM src.conversations;
                 INSERT OR IGNORE INTO main.direct_conversations SELECT * FROM src.direct_conversations;
                 INSERT OR IGNORE INTO main.direct_messages SELECT * FROM src.direct_messages;
                 INSERT OR IGNORE INTO main.file_transfers SELECT * FROM src.file_transfers;
                 INSERT OR IGNORE INTO main.groups SELECT * FROM src.groups;
                 INSERT OR IGNORE INTO main.group_members SELECT * FROM src.group_members;
                 INSERT OR IGNORE INTO main.group_messages SELECT * FROM src.group_messages;
                 INSERT OR IGNORE INTO main.messages SELECT * FROM src.messages;",
            )?;
        }
        let has_recalled: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('local_messages', 'src') WHERE name = 'recalled_at')",
            [], |row| row.get(0))?;
        let recalled = if has_recalled { "recalled_at" } else { "NULL" };
        let insert = if overwrite { "INSERT" } else { "INSERT OR IGNORE" };
        tx.execute_batch(&format!(
            "{insert} INTO main.local_messages(id, conversation_id, sender, body, created_at, logical, recalled_at)
             SELECT id, conversation_id, sender, body, created_at, logical, {recalled} FROM src.local_messages;"))?;
        // Keep the logical clock at least as high as every restored message.
        tx.execute_batch(
            "UPDATE main.logical_clock SET value = max(value,
                COALESCE((SELECT max(logical) FROM main.direct_messages), 0),
                COALESCE((SELECT max(logical) FROM main.group_messages), 0),
                COALESCE((SELECT max(logical) FROM main.local_messages), 0))
             WHERE singleton = 1 AND value < 9223372036854775807;",
        )?;
        let after: i64 = tx.query_row(
            "SELECT (SELECT count(*) FROM main.direct_messages) + (SELECT count(*) FROM main.group_messages)
                  + (SELECT count(*) FROM main.local_messages)", [], |row| row.get(0),
        )?;
        tx.commit()?;
        let inserted = if overwrite { snapshot_messages as u64 } else { (after - before).max(0) as u64 };
        let skipped = if overwrite { 0 } else { (snapshot_messages - (after - before)).max(0) as u64 };
        Ok(RestoreReport { mode: if overwrite { "overwrite".into() } else { "merge".into() }, inserted, skipped, conflicts: 0, snapshot_path, restored_attachments: 0 })
    }

    /// Queue an outgoing file. The file stays at `source`; the receiver verifies the hash.
    pub fn queue_direct_file(&self, sender: &str, peer: &str, name: &str, size: u64, sha256: &str, source: &Path, created_at: i64) -> Result<DirectMessage> {
        self.queue_direct_file_with_reply(sender, peer, name, size, sha256, source, created_at, None)
    }

    pub fn queue_direct_file_with_reply(&self, sender: &str, peer: &str, name: &str, size: u64, sha256: &str,
        source: &Path, created_at: i64, reply_to: Option<&str>) -> Result<DirectMessage> {
        let clean = crate::files::sanitize_name(name).ok_or(rusqlite::Error::InvalidQuery)?;
        let source_text = source.to_str().ok_or(rusqlite::Error::InvalidQuery)?;
        if !valid_fingerprint(sender) || !valid_fingerprint(peer) || sender == peer
            || size == 0 || size > crate::protocol::MAX_FILE_BYTES
            || !valid_sha256(sha256) || source_text.is_empty() || source_text.len() > 4096 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let size = i64::try_from(size).map_err(|_| rusqlite::Error::InvalidQuery)?;
        let tx = self.connection.unchecked_transaction()?;
        let verified: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM contacts WHERE fingerprint = ?1 AND trust_state = 'verified')", [peer], |r| r.get(0))?;
        if !verified { return Err(rusqlite::Error::InvalidQuery); }
        let (reply_sender, reply_preview) = direct_quote(&tx, sender, peer, reply_to)?;
        tx.execute("INSERT INTO direct_conversations(peer, hidden) VALUES (?1, 0) ON CONFLICT(peer) DO UPDATE SET hidden = 0", [peer])?;
        if tx.execute("UPDATE logical_clock SET value = value + 1 WHERE singleton = 1 AND value < 9223372036854775807", [])? != 1 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let (id, logical): (String, i64) = tx.query_row("SELECT lower(hex(randomblob(16))), value FROM logical_clock WHERE singleton = 1", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let collision: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM messages WHERE id = ?1 UNION ALL SELECT 1 FROM local_messages WHERE id = ?1)", [&id], |r| r.get(0))?;
        if collision { return Err(rusqlite::Error::InvalidQuery); }
        tx.execute(
            "INSERT INTO direct_messages(id, peer, sender, body, created_at, logical, status, kind, file_name, file_size, file_sha256, reply_to, reply_sender, reply_preview)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', 'file', ?4, ?7, ?8, ?9, ?10, ?11)",
            params![id, peer, sender, clean, created_at, logical, size, sha256, reply_to, reply_sender, reply_preview],
        )?;
        tx.execute(
            "INSERT INTO file_transfers(message_id, peer, direction, source_path, target_path, total, transferred, status, updated_at)
             VALUES (?1, ?2, 'outgoing', ?3, NULL, ?4, 0, 'pending', ?5)",
            params![id, peer, source_text, size, created_at],
        )?;
        tx.commit()?;
        Ok(DirectMessage {
            id, peer: peer.into(), sender: sender.into(), body: clean.clone(), created_at, logical, status: "pending".into(),
            kind: "file".into(), file_name: Some(clean), file_size: Some(size), file_sha256: Some(sha256.into()),
            file_status: Some("pending".into()), file_transferred: Some(0), file_error: None,
            recalled_at: None,
            reply_to: reply_to.map(str::to_owned), reply_sender, reply_preview,
        })
    }

    /// Legacy boolean kept for reading settings written before the three-way mode.
    pub fn file_accept_confirm(&self) -> Result<bool> {
        Ok(self.get_setting("file_accept_confirm")?.map(|value| value == "1").unwrap_or(false))
    }

    /// Three-way receive policy. Settings written by older versions (a boolean)
    /// still map onto it, so an upgrade never silently changes the user's choice.
    pub fn file_accept_mode(&self) -> Result<FileAcceptMode> {
        if let Some(value) = self.get_setting("file_accept_mode")? {
            if let Some(mode) = FileAcceptMode::parse(&value) { return Ok(mode); }
        }
        // A new install asks every time; an upgrade keeps whatever the user had.
        Ok(match self.get_setting("file_accept_confirm")? {
            Some(value) => if value == "1" { FileAcceptMode::Always } else { FileAcceptMode::Auto },
            None => FileAcceptMode::Always,
        })
    }

    pub fn set_file_accept_mode(&self, mode: FileAcceptMode) -> Result<()> {
        self.set_setting("file_accept_mode", Some(mode.as_str()))?;
        self.set_setting("file_accept_confirm", Some(if mode == FileAcceptMode::Auto { "0" } else { "1" }))?;
        Ok(())
    }

    /// Senders and groups the user chose to accept without further prompts.
    pub fn remembered_file_sources(&self) -> Result<Vec<String>> {
        let mut statement = self.connection.prepare(
            "SELECT source FROM file_accept_sources ORDER BY added_at, rowid LIMIT ?1")?;
        let rows = statement.query_map([MAX_REMEMBERED_SOURCES as i64], |row| row.get(0))?;
        rows.collect()
    }

    pub fn is_file_source_remembered(&self, source: &str) -> Result<bool> {
        if !valid_offer_source(source) { return Ok(false); }
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM file_accept_sources WHERE source = ?1)", [source],
            |row| row.get(0),
        )?)
    }

    pub fn remember_file_source(&self, source: &str) -> Result<()> {
        if !valid_offer_source(source) { return Err(rusqlite::Error::InvalidQuery); }
        let added_at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos() as i64).unwrap_or(0);
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute(
            "INSERT OR IGNORE INTO file_accept_sources(source, added_at) VALUES (?1, ?2)",
            params![source, added_at],
        )?;
        // Bounded list: a peer that keeps changing its id cannot grow storage without limit.
        transaction.execute(
            "DELETE FROM file_accept_sources WHERE rowid IN (
                SELECT rowid FROM file_accept_sources ORDER BY added_at DESC, rowid DESC
                LIMIT -1 OFFSET ?1)",
            [MAX_REMEMBERED_SOURCES as i64],
        )?;
        transaction.commit()
    }

    pub fn forget_file_sources(&self) -> Result<()> {
        self.connection.execute("DELETE FROM file_accept_sources", [])?;
        Ok(())
    }

    pub fn file_offer_status(&self, id: &str) -> Result<Option<String>> {
        if !valid_message_id(id) { return Err(rusqlite::Error::InvalidQuery); }
        self.connection.query_row(
            "SELECT status FROM file_offer_requests WHERE message_id = ?1", [id],
            |row| row.get(0),
        ).optional()
    }

    /// Records a deferred offer; repeated attempts keep the original request row.
    /// `group_id` is set only for group files, so the two views list disjoint sets.
    pub fn record_file_offer_request(&self, peer: &str, id: &str, name: &str, size: u64, created_at: i64, group_id: Option<&str>) -> Result<()> {
        let clean = crate::files::sanitize_name(name).ok_or(rusqlite::Error::InvalidQuery)?;
        let size = i64::try_from(size).map_err(|_| rusqlite::Error::InvalidQuery)?;
        if !valid_fingerprint(peer) || !valid_message_id(id) { return Err(rusqlite::Error::InvalidQuery); }
        if let Some(group) = group_id { if !valid_group_id(group) { return Err(rusqlite::Error::InvalidQuery); } }
        self.connection.execute(
            "INSERT INTO file_offer_requests(message_id, peer, name, total, created_at, status, group_id)
             VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6)
             ON CONFLICT(message_id) DO NOTHING",
            params![id, peer, clean, size, created_at, group_id],
        )?;
        Ok(())
    }

    pub fn list_file_offer_requests(&self, peer: &str) -> Result<Vec<FileOfferRequest>> {
        if !valid_fingerprint(peer) { return Err(rusqlite::Error::InvalidQuery); }
        let mut statement = self.connection.prepare(
            "SELECT message_id, peer, name, total, created_at, status, group_id FROM file_offer_requests
             WHERE peer = ?1 AND status = 'pending' AND group_id IS NULL ORDER BY created_at DESC LIMIT 50")?;
        let rows = statement.query_map([peer], |row| Ok(FileOfferRequest {
            message_id: row.get(0)?, peer: row.get(1)?, name: row.get(2)?, total: row.get(3)?,
            created_at: row.get(4)?, status: row.get(5)?, group_id: row.get(6)?,
        }))?;
        rows.collect()
    }

    pub fn list_group_file_offer_requests(&self, group_id: &str) -> Result<Vec<FileOfferRequest>> {
        if !valid_group_id(group_id) { return Err(rusqlite::Error::InvalidQuery); }
        let mut statement = self.connection.prepare(
            "SELECT message_id, peer, name, total, created_at, status, group_id FROM file_offer_requests
             WHERE group_id = ?1 AND status = 'pending' ORDER BY created_at DESC LIMIT 50")?;
        let rows = statement.query_map([group_id], |row| Ok(FileOfferRequest {
            message_id: row.get(0)?, peer: row.get(1)?, name: row.get(2)?, total: row.get(3)?,
            created_at: row.get(4)?, status: row.get(5)?, group_id: row.get(6)?,
        }))?;
        rows.collect()
    }

    /// `target_dir` is an optional one-shot destination chosen in the dialog; it is
    /// stored on the pending row so the later retry writes where the user asked.
    pub fn resolve_file_offer(&self, id: &str, accept: bool, target_dir: Option<&str>) -> Result<bool> {
        if !valid_message_id(id) { return Err(rusqlite::Error::InvalidQuery); }
        Ok(self.connection.execute(
            "UPDATE file_offer_requests SET status = ?2, target_dir = COALESCE(?3, target_dir)
             WHERE message_id = ?1 AND status = 'pending'",
            params![id, if accept { "accepted" } else { "rejected" }, target_dir],
        )? == 1)
    }

    /// Destination chosen for a pending or accepted offer, if any.
    pub fn file_offer_target_dir(&self, id: &str) -> Result<Option<PathBuf>> {
        if !valid_message_id(id) { return Err(rusqlite::Error::InvalidQuery); }
        let value: Option<Option<String>> = self.connection.query_row(
            "SELECT target_dir FROM file_offer_requests WHERE message_id = ?1", [id], |row| row.get(0),
        ).optional()?;
        Ok(value.flatten().filter(|text| !text.trim().is_empty()).map(PathBuf::from))
    }

    /// Target path recorded for a completed or in-flight transfer.
    pub fn file_transfer_target(&self, id: &str) -> Result<Option<PathBuf>> {
        if !valid_message_id(id) { return Err(rusqlite::Error::InvalidQuery); }
        let value: Option<String> = self.connection.query_row(
            "SELECT target_path FROM file_transfers WHERE message_id = ?1", [id], |row| row.get(0),
        ).optional()?;
        Ok(value.map(PathBuf::from))
    }

    /// Validate an offer against trust, existing records and any partial file.
    /// `peer` must be authenticated by the transport, never taken from the payload.
    pub fn receive_file_offer(&self, peer: &str, id: &str, name: &str, size: u64, sha256: &str, created_at: i64, logical: i64, now: i64) -> Result<FileOfferDecision> {
        self.receive_file_offer_with_reply(peer, id, name, size, sha256, created_at, logical, None, None, None, now)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn receive_file_offer_with_reply(&self, peer: &str, id: &str, name: &str, size: u64, sha256: &str,
        created_at: i64, logical: i64, reply_to: Option<&str>, reply_sender: Option<&str>, reply_preview: Option<&str>, now: i64)
        -> Result<FileOfferDecision> {
        let clean = match crate::files::sanitize_name(name) { Some(clean) => clean, None => return Ok(FileOfferDecision::Conflict) };
        if !valid_reply_metadata(reply_to, reply_sender, reply_preview) || (reply_to.is_some() && !is_image_file(&clean)) {
            return Ok(FileOfferDecision::Conflict);
        }
        if !valid_fingerprint(peer) || !valid_message_id(id) || size == 0 || size > crate::protocol::MAX_FILE_BYTES
            || !valid_sha256(sha256) || logical <= 0 || logical == i64::MAX {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let size_i64 = i64::try_from(size).map_err(|_| rusqlite::Error::InvalidQuery)?;
        let tx = self.connection.unchecked_transaction()?;
        let verified: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM contacts WHERE fingerprint = ?1 AND trust_state = 'verified')", [peer], |r| r.get(0))?;
        if !verified { return Ok(FileOfferDecision::Untrusted); }
        let dir = self.incoming_dir_for(id, RecvKind::Direct)?;
        let target = crate::files::incoming_target(&dir, id, &clean);
        let partial = crate::files::incoming_partial(&dir, id);
        let existing: Option<(String, String, Option<String>, Option<i64>, Option<String>, i64, i64, Option<String>, Option<String>, Option<String>)> = tx.query_row(
            "SELECT peer, sender, kind, file_size, file_sha256, created_at, logical, reply_to, reply_sender, reply_preview FROM direct_messages WHERE id = ?1", [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?)),
        ).optional()?;
        if let Some((old_peer, sender, kind, old_size, old_hash, old_created, old_logical, old_reply_to, old_reply_sender, old_reply_preview)) = existing {
            if old_peer != peer || sender != peer || kind.as_deref() != Some("file") || old_size != Some(size_i64)
                || old_hash.as_deref() != Some(sha256) || old_created != created_at || old_logical != logical
                || old_reply_to.as_deref() != reply_to || old_reply_sender.as_deref() != reply_sender
                || old_reply_preview.as_deref() != reply_preview {
                return Ok(FileOfferDecision::Conflict);
            }
            let (status, _): (String, i64) = tx.query_row(
                "SELECT status, transferred FROM file_transfers WHERE message_id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)),
            ).optional()?.ok_or(rusqlite::Error::InvalidQuery)?;
            if status == "completed" { return Ok(FileOfferDecision::Complete); }
            let offset = match std::fs::metadata(&partial) {
                Ok(metadata) if metadata.is_file() && metadata.len() <= size => metadata.len(),
                Ok(_) => { crate::files::remove_quietly(&partial); 0 }
                Err(_) => 0,
            };
            tx.execute(
                "UPDATE file_transfers SET status = 'transferring', transferred = ?2, target_path = ?3, error = NULL, updated_at = ?4
                 WHERE message_id = ?1",
                params![id, offset as i64, target.to_string_lossy(), now],
            )?;
            tx.commit()?;
            return Ok(FileOfferDecision::Ready { offset, target, partial });
        }
        let collision: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM messages WHERE id = ?1 UNION ALL SELECT 1 FROM local_messages WHERE id = ?1)", [id], |r| r.get(0))?;
        if collision { return Ok(FileOfferDecision::Conflict); }
        if self.within_attachments(&dir) && self.storage_used().saturating_add(size) > self.storage_quota()? {
            return Ok(FileOfferDecision::NoSpace);
        }
        if tx.execute("UPDATE logical_clock SET value = max(value, ?1) + 1 WHERE singleton = 1 AND value < 9223372036854775807", [logical])? != 1 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        tx.execute("INSERT INTO direct_conversations(peer, hidden) VALUES (?1, 0) ON CONFLICT(peer) DO UPDATE SET hidden = 0", [peer])?;
        tx.execute(
            "INSERT INTO direct_messages(id, peer, sender, body, created_at, logical, status, kind, file_name, file_size, file_sha256, reply_to, reply_sender, reply_preview)
             VALUES (?1, ?2, ?2, ?3, ?4, ?5, 'received', 'file', ?3, ?6, ?7, ?8, ?9, ?10)",
            params![id, peer, clean, created_at, logical, size_i64, sha256, reply_to, reply_sender, reply_preview],
        )?;
        tx.execute(
            "INSERT INTO file_transfers(message_id, peer, direction, source_path, target_path, total, transferred, status, updated_at)
             VALUES (?1, ?2, 'incoming', NULL, ?3, ?4, 0, 'transferring', ?5)",
            params![id, peer, target.to_string_lossy(), size_i64, now],
        )?;
        tx.commit()?;
        Ok(FileOfferDecision::Ready { offset: 0, target, partial })
    }

    pub fn update_file_progress(&self, id: &str, transferred: u64, now: i64) -> Result<()> {
        if !valid_message_id(id) { return Err(rusqlite::Error::InvalidQuery); }
        let transferred = i64::try_from(transferred).map_err(|_| rusqlite::Error::InvalidQuery)?;
        self.connection.execute(
            "UPDATE file_transfers SET transferred = max(0, min(?2, total)), status = 'transferring', updated_at = ?3
             WHERE message_id = ?1 AND status IN ('pending', 'transferring')",
            params![id, transferred, now],
        )?;
        Ok(())
    }

    pub fn complete_file_transfer(&self, id: &str, now: i64) -> Result<bool> {
        if !valid_message_id(id) { return Err(rusqlite::Error::InvalidQuery); }
        Ok(self.connection.execute(
            "UPDATE file_transfers SET status = 'completed', transferred = total, error = NULL, updated_at = ?2
             WHERE message_id = ?1 AND NOT EXISTS(SELECT 1 FROM direct_messages WHERE id = ?1 AND recalled_at IS NOT NULL)",
            params![id, now],
        )? == 1)
    }

    /// A failure keeps partial bytes so the next offer can resume from them.
    pub fn fail_file_transfer(&self, id: &str, error: &str, now: i64) -> Result<()> {
        if !valid_message_id(id) || error.is_empty() || error.len() > 200 { return Err(rusqlite::Error::InvalidQuery); }
        let tx = self.connection.unchecked_transaction()?;
        tx.execute(
            "UPDATE file_transfers SET status = 'failed', error = ?2, updated_at = ?3 WHERE message_id = ?1 AND COALESCE(error, '') != 'recalled'",
            params![id, error, now],
        )?;
        tx.execute("UPDATE direct_messages SET status = 'failed' WHERE id = ?1 AND status = 'pending'", [id])?;
        tx.commit()?;
        Ok(())
    }

    /// Transient transport errors leave the message pending so the retry worker resumes it.
    pub fn pause_file_transfer(&self, id: &str, error: &str, now: i64) -> Result<()> {
        if !valid_message_id(id) || error.is_empty() || error.len() > 200 { return Err(rusqlite::Error::InvalidQuery); }
        self.connection.execute(
            "UPDATE file_transfers SET status = 'pending', error = ?2, updated_at = ?3 WHERE message_id = ?1 AND status != 'completed'",
            params![id, error, now],
        )?;
        Ok(())
    }

    pub fn file_transfer(&self, id: &str) -> Result<Option<FileTransfer>> {
        if !valid_message_id(id) { return Err(rusqlite::Error::InvalidQuery); }
        self.connection.query_row(
            "SELECT message_id, peer, direction, source_path, target_path, total, transferred, status, error FROM file_transfers WHERE message_id = ?1",
            [id],
            |r| Ok(FileTransfer {
                message_id: r.get(0)?, peer: r.get(1)?, direction: r.get(2)?, source_path: r.get(3)?,
                target_path: r.get(4)?, total: r.get(5)?, transferred: r.get(6)?, status: r.get(7)?, error: r.get(8)?,
            }),
        ).optional()
    }

    pub fn list_file_progress(&self, peer: &str) -> Result<Vec<FileProgress>> {
        if !valid_fingerprint(peer) { return Err(rusqlite::Error::InvalidQuery); }
        let mut statement = self.connection.prepare(
            "SELECT message_id, direction, status, transferred, total, error FROM file_transfers
             WHERE peer = ?1 ORDER BY updated_at DESC, message_id LIMIT 200"
        )?;
        let rows = statement.query_map([peer], |r| Ok(FileProgress {
            message_id: r.get(0)?, direction: r.get(1)?, status: r.get(2)?, transferred: r.get(3)?, total: r.get(4)?, error: r.get(5)?,
        }))?;
        rows.collect()
    }

    /// Local path of a stored attachment, only once it is complete on this device.
    /// Completed group attachment path, only for active members of that group.
    pub fn group_attachment_path(&self, group_id: &str, member: &str, id: &str) -> Result<Option<(String, PathBuf)>> {
        if !valid_group_id(group_id) || !valid_fingerprint(member) || !valid_message_id(id) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let active: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM group_members WHERE group_id = ?1 AND member = ?2 AND active = 1)",
            params![group_id, member], |row| row.get(0),
        )?;
        if !active { return Ok(None); }
        let row: Option<(String, String, String)> = self.connection.query_row(
            "SELECT m.file_name, COALESCE(f.target_path, f.source_path), f.status
             FROM group_messages m JOIN group_file_transfers f ON f.message_id = m.id
             WHERE m.id = ?1 AND m.group_id = ?2", params![id, group_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).optional()?;
        let Some((name, path, status)) = row else { return Ok(None); };
        if status != "completed" { return Ok(None); }
        let path = PathBuf::from(path);
        if !path.is_file() { return Ok(None); }
        Ok(Some((name, path)))
    }

    pub fn attachment_path(&self, id: &str) -> Result<Option<(String, PathBuf)>> {
        if !valid_message_id(id) { return Err(rusqlite::Error::InvalidQuery); }
        let row: Option<(String, String, String)> = self.connection.query_row(
            "SELECT m.file_name, COALESCE(f.target_path, f.source_path), f.status FROM direct_messages m
             JOIN file_transfers f ON f.message_id = m.id WHERE m.id = ?1", [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).optional()?;
        let Some((name, path, status)) = row else { return Ok(None); };
        if status != "completed" { return Ok(None); }
        let path = PathBuf::from(path);
        if !path.is_file() { return Ok(None); }
        Ok(Some((name, path)))
    }

    /// A local-only self conversation is never treated as a remote trusted contact.
    pub fn open_self_conversation(&self) -> Result<()> {
        self.connection.execute(
            "INSERT INTO conversations(id, kind, hidden) VALUES ('self', 'self', 0)
             ON CONFLICT(id) DO UPDATE SET hidden = 0", [],
        )?;
        Ok(())
    }

    pub fn hide_self_conversation(&self) -> Result<bool> {
        Ok(self.connection.execute("UPDATE conversations SET hidden = 1 WHERE id = 'self' AND hidden = 0", [])? == 1)
    }

    /// The self conversation has no unread counter, but it can still be muted so the row
    /// looks consistent with the other conversations.
    pub fn set_self_muted(&self, muted: bool) -> Result<bool> {
        Ok(self.connection.execute("UPDATE conversations SET muted = ?1 WHERE id = 'self'", [i64::from(muted)])? == 1)
    }

    pub fn list_conversations(&self, self_id: &str) -> Result<Vec<Conversation>> {
        if !valid_fingerprint(self_id) { return Err(rusqlite::Error::InvalidQuery); }
        let mut statement = self.connection.prepare(
            "SELECT c.id AS id, c.hidden AS hidden, CASE WHEN m.recalled_at IS NULL THEN m.body ELSE '你撤回了一条消息' END AS body, m.logical AS logical, 'self' AS kind, 0 AS unread, c.muted AS muted FROM conversations c
             JOIN local_messages m ON m.id = (
               SELECT id FROM local_messages WHERE conversation_id = c.id
               ORDER BY logical DESC, sender DESC, id DESC LIMIT 1
             ) WHERE c.hidden = 0
             UNION ALL
             SELECT c.peer AS id, c.hidden AS hidden, m.body AS body, m.logical AS logical, 'direct' AS kind, c.unread AS unread, c.muted AS muted FROM direct_conversations c
             JOIN direct_messages m ON m.id = (
               SELECT id FROM direct_messages WHERE peer = c.peer
               ORDER BY logical DESC, sender DESC, id DESC LIMIT 1
             ) WHERE c.hidden = 0
             UNION ALL
             SELECT g.id AS id, gm.hidden AS hidden, COALESCE(m.body, '') AS body,
                    COALESCE(m.logical, 0) AS logical, 'group' AS kind, gm.unread AS unread, gm.muted AS muted
             FROM groups g JOIN group_members gm ON gm.group_id = g.id
             LEFT JOIN group_messages m ON m.id = (
               SELECT id FROM group_messages WHERE group_id = g.id
               ORDER BY logical DESC, sender DESC, id DESC LIMIT 1
             )
             WHERE gm.member = ?1 AND gm.active = 1 AND gm.hidden = 0
             ORDER BY logical DESC, id"
        )?;
        let rows = statement.query_map([self_id], |row| Ok(Conversation {
            id: row.get(0)?, hidden: row.get(1)?, last_body: row.get(2)?, last_logical: row.get(3)?, kind: row.get(4)?,
            unread: row.get(5)?, muted: row.get(6)?,
        }))?;
        rows.collect()
    }

    /// Creates a group owned by `owner` and queues an invite notice for every member.
    /// All members must already be locally verified contacts; a group adds no new trust.
    pub fn create_group(&self, owner: &str, name: &str, members: &[String], now: i64) -> Result<GroupSummary> {
        let name = name.trim();
        if !valid_fingerprint(owner) || name.is_empty() || name.len() > 128 || members.len() > 255 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let mut unique = std::collections::BTreeSet::new();
        for member in members {
            if !valid_fingerprint(member) || member == owner || !unique.insert(member.as_str()) {
                return Err(rusqlite::Error::InvalidQuery);
            }
        }
        let tx = self.connection.unchecked_transaction()?;
        for member in &unique {
            let verified: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM contacts WHERE fingerprint = ?1 AND trust_state = 'verified')",
                [member], |row| row.get(0),
            )?;
            if !verified { return Err(rusqlite::Error::InvalidQuery); }
        }
        let group_id: String = tx.query_row("SELECT lower(hex(randomblob(16)))", [], |row| row.get(0))?;
        tx.execute("INSERT INTO groups(id, name, owner, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![group_id, name, owner, now])?;
        tx.execute("INSERT INTO group_members(group_id, member, role, joined_at) VALUES (?1, ?2, 'owner', ?3)",
            params![group_id, owner, now])?;
        let mut roster = vec![owner.to_owned()];
        for member in &unique {
            tx.execute("INSERT INTO group_members(group_id, member, role, joined_at) VALUES (?1, ?2, 'member', ?3)",
                params![group_id, member, now])?;
            roster.push((*member).to_owned());
        }
        let payload = serde_json::json!({ "name": name, "owner": owner, "members": roster }).to_string();
        for member in &unique {
            tx.execute(
                "INSERT INTO group_control(id, group_id, target, kind, payload, status, created_at)
                 VALUES (lower(hex(randomblob(16))), ?1, ?2, 'invite', ?3, 'pending', ?4)",
                params![group_id, member, payload, now])?;
        }
        tx.commit()?;
        Ok(GroupSummary {
            id: group_id, name: name.to_owned(), owner: owner.to_owned(),
            member_count: 1 + unique.len() as i64, hidden: false, is_owner: true,
            last_body: String::new(), last_logical: 0,
        })
    }

    /// Only the owner may add a member; the member must already be a verified contact.
    pub fn add_group_member(&self, group_id: &str, actor: &str, member: &str, now: i64) -> Result<bool> {
        if !valid_group_id(group_id) || !valid_fingerprint(actor) || !valid_fingerprint(member) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let tx = self.connection.unchecked_transaction()?;
        let owner: Option<String> = tx.query_row("SELECT owner FROM groups WHERE id = ?1", [group_id], |row| row.get(0)).optional()?;
        let Some(owner) = owner else { return Ok(false); };
        if owner != actor || member == owner { return Ok(false); }
        let verified: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM contacts WHERE fingerprint = ?1 AND trust_state = 'verified')",
            [member], |row| row.get(0),
        )?;
        if !verified { return Ok(false); }
        let existing: Option<bool> = tx.query_row(
            "SELECT active FROM group_members WHERE group_id = ?1 AND member = ?2",
            params![group_id, member], |row| row.get(0),
        ).optional()?;
        match existing {
            Some(true) => return Ok(false),
            Some(false) => { tx.execute(
                "UPDATE group_members SET active = 1, hidden = 0, joined_at = ?3 WHERE group_id = ?1 AND member = ?2",
                params![group_id, member, now])?; },
            None => { tx.execute(
                "INSERT INTO group_members(group_id, member, role, joined_at) VALUES (?1, ?2, 'member', ?3)",
                params![group_id, member, now])?; },
        }
        let roster: Vec<String> = {
            let mut statement = tx.prepare("SELECT member FROM group_members WHERE group_id = ?1 AND active = 1 ORDER BY member")?;
            let rows = statement.query_map([group_id], |row| row.get(0))?;
            rows.collect::<Result<Vec<String>>>()?
        };
        let name: String = tx.query_row("SELECT name FROM groups WHERE id = ?1", [group_id], |row| row.get(0))?;
        let payload = serde_json::json!({ "name": name, "owner": owner, "members": roster }).to_string();
        tx.execute(
            "INSERT INTO group_control(id, group_id, target, kind, payload, status, created_at)
             VALUES (lower(hex(randomblob(16))), ?1, ?2, 'invite', ?3, 'pending', ?4)",
            params![group_id, member, payload, now])?;
        for other in &roster {
            if other == member || other == actor { continue; }
            tx.execute(
                "INSERT INTO group_control(id, group_id, target, kind, payload, status, created_at)
                 VALUES (lower(hex(randomblob(16))), ?1, ?2, 'invite', ?3, 'pending', ?4)",
                params![group_id, other, payload, now])?;
        }
        tx.commit()?;
        Ok(true)
    }

    /// Removing a member also drops its undelivered outbox rows and notifies the rest.
    pub fn remove_group_member(&self, group_id: &str, actor: &str, member: &str, now: i64) -> Result<bool> {
        if !valid_group_id(group_id) || !valid_fingerprint(actor) || !valid_fingerprint(member) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let tx = self.connection.unchecked_transaction()?;
        let owner: Option<String> = tx.query_row("SELECT owner FROM groups WHERE id = ?1", [group_id], |row| row.get(0)).optional()?;
        let Some(owner) = owner else { return Ok(false); };
        if owner != actor || member == owner { return Ok(false); }
        if tx.execute("UPDATE group_members SET active = 0 WHERE group_id = ?1 AND member = ?2 AND active = 1",
            params![group_id, member])? != 1 {
            return Ok(false);
        }
        tx.execute("DELETE FROM group_outbox WHERE group_id = ?1 AND member = ?2 AND status = 'pending'",
            params![group_id, member])?;
        let roster: Vec<String> = {
            let mut statement = tx.prepare("SELECT member FROM group_members WHERE group_id = ?1 AND active = 1 ORDER BY member")?;
            let rows = statement.query_map([group_id], |row| row.get(0))?;
            rows.collect::<Result<Vec<String>>>()?
        };
        let name: String = tx.query_row("SELECT name FROM groups WHERE id = ?1", [group_id], |row| row.get(0))?;
        let notice = serde_json::json!({ "name": name, "owner": owner, "members": roster }).to_string();
        tx.execute(
            "INSERT INTO group_control(id, group_id, target, kind, payload, status, created_at)
             VALUES (lower(hex(randomblob(16))), ?1, ?2, 'remove', ?3, 'pending', ?4)",
            params![group_id, member, notice, now])?;
        for other in &roster {
            if other == actor { continue; }
            tx.execute(
                "INSERT INTO group_control(id, group_id, target, kind, payload, status, created_at)
                 VALUES (lower(hex(randomblob(16))), ?1, ?2, 'invite', ?3, 'pending', ?4)",
                params![group_id, other, notice, now])?;
        }
        tx.commit()?;
        Ok(true)
    }

    /// A member leaves locally at once and the owner is notified so it can drop the member.
    pub fn leave_group(&self, group_id: &str, member: &str, now: i64) -> Result<bool> {
        if !valid_group_id(group_id) || !valid_fingerprint(member) { return Err(rusqlite::Error::InvalidQuery); }
        let tx = self.connection.unchecked_transaction()?;
        let owner: Option<String> = tx.query_row("SELECT owner FROM groups WHERE id = ?1", [group_id], |row| row.get(0)).optional()?;
        let Some(owner) = owner else { return Ok(false); };
        if owner == member { return Ok(false); }
        if tx.execute("UPDATE group_members SET active = 0, hidden = 1 WHERE group_id = ?1 AND member = ?2 AND active = 1",
            params![group_id, member])? != 1 {
            return Ok(false);
        }
        tx.execute("DELETE FROM group_outbox WHERE group_id = ?1 AND member = ?2", params![group_id, member])?;
        let payload = serde_json::json!({ "group_id": group_id, "member": member }).to_string();
        tx.execute(
            "INSERT INTO group_control(id, group_id, target, kind, payload, status, created_at)
             VALUES (lower(hex(randomblob(16))), ?1, ?2, 'leave', ?3, 'pending', ?4)",
            params![group_id, owner, payload, now])?;
        tx.commit()?;
        Ok(true)
    }

    /// Hands the group over to another active member. Only the current owner may do this,
    /// and the new owner is announced through an owner-signed roster update.
    pub fn transfer_group_owner(&self, group_id: &str, actor: &str, new_owner: &str, now: i64) -> Result<bool> {
        if !valid_group_id(group_id) || !valid_fingerprint(actor) || !valid_fingerprint(new_owner) || actor == new_owner {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let tx = self.connection.unchecked_transaction()?;
        let owner: Option<String> = tx.query_row("SELECT owner FROM groups WHERE id = ?1 AND active = 1", [group_id], |row| row.get(0)).optional()?;
        let Some(owner) = owner else { return Ok(false); };
        if owner != actor { return Ok(false); }
        let active: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM group_members WHERE group_id = ?1 AND member = ?2 AND active = 1)",
            params![group_id, new_owner], |row| row.get(0),
        )?;
        let verified: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM contacts WHERE fingerprint = ?1 AND trust_state = 'verified')",
            [new_owner], |row| row.get(0),
        )?;
        if !active || !verified { return Ok(false); }
        tx.execute("UPDATE groups SET owner = ?2 WHERE id = ?1", params![group_id, new_owner])?;
        tx.execute(
            "UPDATE group_members SET role = CASE WHEN member = ?2 THEN 'owner' ELSE 'member' END WHERE group_id = ?1",
            params![group_id, new_owner])?;
        let mut statement = tx.prepare("SELECT member FROM group_members WHERE group_id = ?1 AND active = 1 ORDER BY member")?;
        let roster: Vec<String> = statement.query_map([group_id], |row| row.get(0))?.collect::<Result<Vec<String>>>()?;
        drop(statement);
        let name: String = tx.query_row("SELECT name FROM groups WHERE id = ?1", [group_id], |row| row.get(0))?;
        let payload = serde_json::json!({ "name": name, "owner": new_owner, "members": roster }).to_string();
        for member in &roster {
            if member == actor { continue; }
            tx.execute(
                "INSERT INTO group_control(id, group_id, target, kind, payload, status, created_at)
                 VALUES (lower(hex(randomblob(16))), ?1, ?2, 'transfer', ?3, 'pending', ?4)",
                params![group_id, member, payload, now])?;
        }
        tx.commit()?;
        Ok(true)
    }

    /// Reads the current announcement of one group, if any.
    pub fn group_notice(&self, group_id: &str) -> Result<Option<String>> {
        if !valid_group_id(group_id) { return Err(rusqlite::Error::InvalidQuery); }
        let notice: Option<String> = self.connection.query_row(
            "SELECT notice FROM group_notices WHERE group_id = ?1", [group_id], |row| row.get(0)).optional()?;
        Ok(notice)
    }

    /// Publishes (or clears with an empty string) the group announcement as the
    /// owner, and queues a notice control message for every other member.
    pub fn set_group_notice(&self, group_id: &str, actor: &str, notice: &str, now: i64) -> Result<bool> {
        if !valid_group_id(group_id) || !valid_fingerprint(actor)
            || notice.chars().count() > 2048
            || notice.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let tx = self.connection.unchecked_transaction()?;
        let owner: Option<String> = tx.query_row("SELECT owner FROM groups WHERE id = ?1 AND active = 1", [group_id], |row| row.get(0)).optional()?;
        let Some(owner) = owner else { return Ok(false); };
        if owner != actor { return Ok(false); }
        tx.execute(
            "INSERT INTO group_notices(group_id, notice, updated_at, updated_by) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(group_id) DO UPDATE SET notice = excluded.notice, updated_at = excluded.updated_at, updated_by = excluded.updated_by",
            params![group_id, notice, now, actor])?;
        let payload = serde_json::json!({ "notice": notice }).to_string();
        let mut statement = tx.prepare("SELECT member FROM group_members WHERE group_id = ?1 AND active = 1")?;
        let members: Vec<String> = statement.query_map([group_id], |row| row.get(0))?.collect::<Result<Vec<String>>>()?;
        drop(statement);
        for member in &members {
            if member == actor { continue; }
            tx.execute(
                "INSERT INTO group_control(id, group_id, target, kind, payload, status, created_at)
                 VALUES (lower(hex(randomblob(16))), ?1, ?2, 'notice', ?3, 'pending', ?4)",
                params![group_id, member, payload, now])?;
        }
        tx.commit()?;
        Ok(true)
    }

    /// Dissolves a group: every member is notified, local membership ends and undelivered
    /// messages/cancelled notices are dropped, while history stays readable on this device.
    pub fn dissolve_group(&self, group_id: &str, actor: &str, now: i64) -> Result<bool> {
        if !valid_group_id(group_id) || !valid_fingerprint(actor) { return Err(rusqlite::Error::InvalidQuery); }
        let tx = self.connection.unchecked_transaction()?;
        let owner: Option<String> = tx.query_row("SELECT owner FROM groups WHERE id = ?1 AND active = 1", [group_id], |row| row.get(0)).optional()?;
        let Some(owner) = owner else { return Ok(false); };
        if owner != actor { return Ok(false); }
        let mut statement = tx.prepare("SELECT member FROM group_members WHERE group_id = ?1 AND active = 1 ORDER BY member")?;
        let roster: Vec<String> = statement.query_map([group_id], |row| row.get(0))?.collect::<Result<Vec<String>>>()?;
        drop(statement);
        for member in &roster {
            if member == actor { continue; }
            tx.execute(
                "INSERT INTO group_control(id, group_id, target, kind, payload, status, created_at)
                 VALUES (lower(hex(randomblob(16))), ?1, ?2, 'dissolve', '{}', 'pending', ?3)",
                params![group_id, member, now])?;
        }
        tx.execute("UPDATE groups SET active = 0 WHERE id = ?1", [group_id])?;
        tx.execute("UPDATE group_members SET active = 0, hidden = 1 WHERE group_id = ?1", [group_id])?;
        tx.execute("DELETE FROM group_outbox WHERE group_id = ?1 AND status = 'pending'", [group_id])?;
        tx.execute("DELETE FROM group_control WHERE group_id = ?1 AND kind != 'dissolve' AND status = 'pending'", [group_id])?;
        tx.commit()?;
        Ok(true)
    }

    /// `None` means the local user never joined; `Some(false)` means left but history remains.
    pub fn group_membership(&self, group_id: &str, self_id: &str) -> Result<Option<bool>> {
        if !valid_group_id(group_id) || !valid_fingerprint(self_id) { return Err(rusqlite::Error::InvalidQuery); }
        let active: Option<i64> = self.connection.query_row(
            "SELECT active FROM group_members WHERE group_id = ?1 AND member = ?2",
            params![group_id, self_id], |row| row.get(0)).optional()?;
        Ok(active.map(|value| value == 1))
    }

    /// Groups the user left, kept so history stays reachable. Messages are never deleted here.
    pub fn list_left_groups(&self, self_id: &str) -> Result<Vec<GroupSummary>> {
        if !valid_fingerprint(self_id) { return Err(rusqlite::Error::InvalidQuery); }
        let mut statement = self.connection.prepare(
            "SELECT g.id, g.name, g.owner, 0,
                (SELECT count(*) FROM group_members x WHERE x.group_id = g.id AND x.active = 1),
                COALESCE((SELECT m.body FROM group_messages m WHERE m.group_id = g.id
                          ORDER BY m.logical DESC, m.sender DESC, m.id DESC LIMIT 1), ''),
                COALESCE((SELECT m.logical FROM group_messages m WHERE m.group_id = g.id
                          ORDER BY m.logical DESC, m.sender DESC, m.id DESC LIMIT 1), 0)
             FROM groups g JOIN group_members gm ON gm.group_id = g.id
             WHERE gm.member = ?1 AND gm.active = 0 AND g.active = 1
               AND EXISTS(SELECT 1 FROM group_messages m WHERE m.group_id = g.id)
             ORDER BY 7 DESC LIMIT 100"
        )?;
        let rows = statement.query_map([self_id], |row| {
            let owner: String = row.get(2)?;
            Ok(GroupSummary {
                id: row.get(0)?, name: row.get(1)?, owner: owner.clone(), member_count: row.get(4)?,
                hidden: true, is_owner: owner == self_id, last_body: row.get(5)?, last_logical: row.get(6)?,
            })
        })?;
        rows.collect()
    }

    /// Keep the original identity and ordering while queuing a durable recall notice.
    pub fn recall_direct(&self, self_id: &str, message_id: &str, now: i64) -> Result<RecallResult> {
        if !valid_fingerprint(self_id) || message_id.is_empty() || message_id.len() > 128 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let tx = self.connection.unchecked_transaction()?;
        let row: Option<(String, String, i64, Option<i64>, String, Option<String>)> = tx.query_row(
            "SELECT m.sender, m.status, m.created_at, m.recalled_at, m.kind, f.status
             FROM direct_messages m LEFT JOIN file_transfers f ON f.message_id = m.id WHERE m.id = ?1",
            [message_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        ).optional()?;
        let Some((sender, status, created_at, recalled_at, kind, file_status)) = row else {
            return Ok(RecallResult::Missing);
        };
        if sender != self_id { return Ok(RecallResult::NotYours); }
        if recalled_at.is_some() { return Ok(RecallResult::Already); }
        if now - created_at > RECALL_WINDOW_SECONDS { return Ok(RecallResult::TooLate); }
        let delivered = status != "pending";
        tx.execute("UPDATE direct_messages SET body = ' ', recalled_at = ?2, status = 'delivered' WHERE id = ?1", params![message_id, now])?;
        if kind == "file" && file_status.as_deref() != Some("completed") {
            tx.execute("UPDATE file_transfers SET status = 'failed', error = 'recalled', updated_at = ?2 WHERE message_id = ?1",
                params![message_id, now])?;
            tx.execute("DELETE FROM file_offer_requests WHERE message_id = ?1", [message_id])?;
        }
        tx.execute(
            "INSERT OR IGNORE INTO direct_recalls(id, peer, message_id, created_at, status, message_created_at, message_logical)
             SELECT lower(hex(randomblob(16))), peer, id, ?2, 'pending', created_at, logical FROM direct_messages WHERE id = ?1",
            params![message_id, now],
        )?;
        tx.commit()?;
        Ok(RecallResult::Recalled { delivered })
    }

    /// Receiver-side recall. Files that already completed stay untouched by design.
    pub fn receive_recall_direct(&self, self_id: &str, peer: &str, message_id: &str, now: i64) -> Result<IncomingResult> {
        if !valid_fingerprint(self_id) || !valid_fingerprint(peer) || message_id.is_empty() || message_id.len() > 128 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let tx = self.connection.unchecked_transaction()?;
        let verified: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM contacts WHERE fingerprint = ?1 AND trust_state = 'verified')",
            [peer], |r| r.get(0))?;
        if !verified { return Ok(IncomingResult::Unverified); }
        let row: Option<(String, Option<i64>, String, Option<String>)> = tx.query_row(
            "SELECT m.sender, m.recalled_at, m.kind, f.status FROM direct_messages m
             LEFT JOIN file_transfers f ON f.message_id = m.id WHERE m.id = ?1 AND m.peer = ?2",
            params![message_id, peer],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        ).optional()?;
        let Some((sender, recalled_at, kind, file_status)) = row else { return Ok(IncomingResult::Duplicate); };
        if sender != peer { return Ok(IncomingResult::Unverified); }
        if recalled_at.is_some() { return Ok(IncomingResult::Duplicate); }
        tx.execute("UPDATE direct_messages SET body = ' ', recalled_at = ?2 WHERE id = ?1", params![message_id, now])?;
        if kind == "file" && file_status.as_deref() != Some("completed") {
            tx.execute("UPDATE file_transfers SET status = 'failed', error = 'recalled', updated_at = ?2 WHERE message_id = ?1",
                params![message_id, now])?;
            tx.execute("DELETE FROM file_offer_requests WHERE message_id = ?1", [message_id])?;
        }
        tx.commit()?;
        Ok(IncomingResult::Inserted)
    }

    /// Apply an authenticated sender's recall even when the original was not received.
    pub fn receive_direct_placeholder(&self, self_id: &str, peer: &str, message_id: &str, created_at: i64, logical: i64, now: i64) -> Result<IncomingResult> {
        if !valid_fingerprint(self_id) || !valid_fingerprint(peer) || self_id == peer
            || !valid_message_id(message_id) || logical <= 0 || logical == i64::MAX {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let verified: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM contacts WHERE fingerprint = ?1 AND trust_state = 'verified')", [peer], |r| r.get(0))?;
        if !verified { return Ok(IncomingResult::Unverified); }
        let existing: Option<(String, String, i64, i64)> = self.connection.query_row(
            "SELECT peer, sender, created_at, logical FROM direct_messages WHERE id = ?1", [message_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).optional()?;
        if let Some((conversation, sender, time, clock)) = existing {
            if conversation != peer || sender != peer || time != created_at || clock != logical { return Ok(IncomingResult::IdConflict); }
            return self.receive_recall_direct(self_id, peer, message_id, now);
        }
        let tx = self.connection.unchecked_transaction()?;
        let collision: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM local_messages WHERE id = ?1 UNION ALL SELECT 1 FROM messages WHERE id = ?1 UNION ALL SELECT 1 FROM group_messages WHERE id = ?1)",
            [message_id], |r| r.get(0))?;
        if collision { return Ok(IncomingResult::IdConflict); }
        if tx.execute("UPDATE logical_clock SET value = max(value, ?1) + 1 WHERE singleton = 1 AND value < 9223372036854775807", [logical])? != 1 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        tx.execute("INSERT INTO direct_conversations(peer, hidden, unread) VALUES (?1, 0, 1) ON CONFLICT(peer) DO UPDATE SET hidden = 0, unread = unread + 1", [peer])?;
        tx.execute("INSERT INTO direct_messages(id, peer, sender, body, created_at, logical, status, recalled_at) VALUES (?1, ?2, ?2, ' ', ?3, ?4, 'received', ?5)",
            params![message_id, peer, created_at, logical, now])?;
        tx.execute("DELETE FROM file_offer_requests WHERE message_id = ?1 AND peer = ?2 AND group_id IS NULL", params![message_id, peer])?;
        tx.commit()?;
        Ok(IncomingResult::Inserted)
    }

    /// Recall notices waiting for a reachable address; keyset order keeps retries fair.
    pub fn next_pending_recall(&self, after: Option<&str>) -> Result<Option<PendingRecall>> {
        let mut statement = self.connection.prepare(
            "SELECT r.id, r.peer, a.address, r.message_id, r.message_created_at, r.message_logical FROM direct_recalls r
             JOIN contact_addresses a ON a.peer = r.peer
             WHERE r.status = 'pending' AND (?1 IS NULL OR r.peer > ?1)
             ORDER BY r.peer, r.created_at, r.id LIMIT 1",
        )?;
        let rows = statement.query_map([after], |row| Ok(PendingRecall {
            id: row.get(0)?, peer: row.get(1)?, address: row.get(2)?, message_id: row.get(3)?,
            created_at: row.get(4)?, logical: row.get(5)?,
        }))?;
        let mut rows = rows;
        rows.next().transpose()
    }

    pub fn ack_recall(&self, id: &str) -> Result<bool> {
        Ok(self.connection.execute("UPDATE direct_recalls SET status = 'delivered' WHERE id = ?1", [id])? == 1)
    }

    /// Fan out durable recall notices to every original recipient.
    pub fn recall_group(&self, self_id: &str, group_id: &str, message_id: &str, now: i64) -> Result<RecallResult> {
        if !valid_fingerprint(self_id) || !valid_group_id(group_id) || message_id.is_empty() || message_id.len() > 128 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let tx = self.connection.unchecked_transaction()?;
        let member: Option<i64> = tx.query_row(
            "SELECT active FROM group_members WHERE group_id = ?1 AND member = ?2",
            params![group_id, self_id], |r| r.get(0)).optional()?;
        if member != Some(1) { return Ok(RecallResult::NotMember); }
        let row: Option<(String, i64, Option<i64>, String, Option<String>)> = tx.query_row(
            "SELECT m.sender, m.created_at, m.recalled_at, m.kind, f.status FROM group_messages m
             LEFT JOIN group_file_transfers f ON f.message_id = m.id WHERE m.id = ?1 AND m.group_id = ?2",
            params![message_id, group_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        ).optional()?;
        let Some((sender, created_at, recalled_at, kind, file_status)) = row else { return Ok(RecallResult::Missing); };
        if sender != self_id { return Ok(RecallResult::NotYours); }
        if recalled_at.is_some() { return Ok(RecallResult::Already); }
        if now - created_at > RECALL_WINDOW_SECONDS { return Ok(RecallResult::TooLate); }
        let recipients: Vec<(String, String)> = {
            let mut statement = tx.prepare(
                "SELECT member, status FROM group_outbox WHERE message_id = ?1 ORDER BY member")?;
            let rows = statement.query_map([message_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.collect::<Result<Vec<(String, String)>>>()?
        };
        let delivered = recipients.iter().any(|(_, status)| status == "delivered");
        let logical: i64 = tx.query_row("SELECT logical FROM group_messages WHERE id = ?1", [message_id], |row| row.get(0))?;
        tx.execute("UPDATE group_messages SET body = ' ', recalled_at = ?2 WHERE id = ?1", params![message_id, now])?;
        tx.execute("DELETE FROM group_outbox WHERE message_id = ?1 AND status = 'pending'", [message_id])?;
        if kind == "file" && file_status.as_deref() != Some("completed") {
            tx.execute("UPDATE group_file_transfers SET status = 'failed', error = 'recalled', updated_at = ?2 WHERE message_id = ?1",
                params![message_id, now])?;
            tx.execute("DELETE FROM file_offer_requests WHERE message_id = ?1", [message_id])?;
        }
        let payload = serde_json::json!({ "message_id": message_id, "created_at": created_at, "logical": logical }).to_string();
        for (target, _) in &recipients {
            tx.execute(
                "INSERT INTO group_control(id, group_id, target, kind, payload, status, created_at)
                 VALUES (lower(hex(randomblob(16))), ?1, ?2, 'recall', ?3, 'pending', ?4)",
                params![group_id, target, payload, now],
            )?;
        }
        tx.commit()?;
        Ok(RecallResult::Recalled { delivered })
    }

    /// Create a group recall placeholder only for authenticated active members.
    pub fn receive_group_placeholder(&self, self_id: &str, peer: &str, group_id: &str, message_id: &str, created_at: i64, logical: i64, now: i64) -> Result<IncomingResult> {
        if !valid_fingerprint(self_id) || !valid_fingerprint(peer) || self_id == peer || !valid_group_id(group_id)
            || !valid_message_id(message_id) || logical <= 0 || logical == i64::MAX {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let verified: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM contacts WHERE fingerprint = ?1 AND trust_state = 'verified')", [peer], |r| r.get(0))?;
        if !verified { return Ok(IncomingResult::Unverified); }
        let active: i64 = self.connection.query_row(
            "SELECT count(*) FROM group_members gm JOIN groups g ON g.id = gm.group_id AND g.active = 1 WHERE gm.group_id = ?1 AND gm.active = 1 AND gm.member IN (?2, ?3)",
            params![group_id, self_id, peer], |r| r.get(0))?;
        if active != 2 { return Ok(IncomingResult::NotMember); }
        let existing: Option<(String, String, i64, i64)> = self.connection.query_row(
            "SELECT group_id, sender, created_at, logical FROM group_messages WHERE id = ?1", [message_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).optional()?;
        if let Some((group, sender, time, clock)) = existing {
            if group != group_id || sender != peer || time != created_at || clock != logical { return Ok(IncomingResult::IdConflict); }
            let payload = serde_json::json!({ "message_id": message_id }).to_string();
            return self.receive_group_control(self_id, peer, "recall", group_id, &payload, now);
        }
        let tx = self.connection.unchecked_transaction()?;
        let collision: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM local_messages WHERE id = ?1 UNION ALL SELECT 1 FROM messages WHERE id = ?1 UNION ALL SELECT 1 FROM direct_messages WHERE id = ?1)",
            [message_id], |r| r.get(0))?;
        if collision { return Ok(IncomingResult::IdConflict); }
        if tx.execute("UPDATE logical_clock SET value = max(value, ?1) + 1 WHERE singleton = 1 AND value < 9223372036854775807", [logical])? != 1 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        tx.execute("INSERT INTO group_messages(id, group_id, sender, body, created_at, logical, recalled_at) VALUES (?1, ?2, ?3, ' ', ?4, ?5, ?6)",
            params![message_id, group_id, peer, created_at, logical, now])?;
        tx.execute("DELETE FROM file_offer_requests WHERE message_id = ?1 AND peer = ?2 AND group_id = ?3", params![message_id, peer, group_id])?;
        tx.execute("UPDATE group_members SET hidden = 0, unread = unread + 1 WHERE group_id = ?1 AND member = ?2", params![group_id, self_id])?;
        tx.commit()?;
        Ok(IncomingResult::Inserted)
    }

    /// Cancels an unsent or in-flight file and forgets the pending offer.
    pub fn cancel_file(&self, message_id: &str, now: i64) -> Result<bool> {
        let tx = self.connection.unchecked_transaction()?;
        tx.execute("UPDATE file_transfers SET status = 'failed', error = 'recalled', updated_at = ?2 WHERE message_id = ?1",
            params![message_id, now])?;
        tx.execute("UPDATE group_file_transfers SET status = 'failed', error = 'recalled', updated_at = ?2 WHERE message_id = ?1",
            params![message_id, now])?;
        tx.execute("DELETE FROM file_offer_requests WHERE message_id = ?1", [message_id])?;
        let changed = tx.execute(
            "UPDATE direct_messages SET file_status = 'cancelled', file_error = 'recalled' WHERE id = ?1 AND kind = 'file'",
            [message_id])?;
        let changed_group = if changed == 0 {
            tx.execute("UPDATE group_messages SET file_status = 'cancelled', file_error = 'recalled' WHERE id = ?1 AND kind = 'file'", [message_id])?
        } else { changed };
        tx.commit()?;
        Ok(changed_group > 0)
    }

    /// Groups the user once belonged to that are no longer active (dissolved or left).
    pub fn list_dissolved_groups(&self, self_id: &str) -> Result<Vec<DissolvedGroup>> {
        if !valid_fingerprint(self_id) { return Err(rusqlite::Error::InvalidQuery); }
        let mut statement = self.connection.prepare(
            "SELECT g.id, g.name, g.owner, g.dissolved_at,
                COALESCE((SELECT m.body FROM group_messages m WHERE m.group_id = g.id
                          ORDER BY m.logical DESC, m.sender DESC, m.id DESC LIMIT 1), ''),
                COALESCE((SELECT m.logical FROM group_messages m WHERE m.group_id = g.id
                          ORDER BY m.logical DESC, m.sender DESC, m.id DESC LIMIT 1), 0)
             FROM groups g JOIN group_members gm ON gm.group_id = g.id
             WHERE gm.member = ?1 AND g.active = 0
               AND EXISTS(SELECT 1 FROM group_messages m WHERE m.group_id = g.id)
             ORDER BY 6 DESC, g.id LIMIT 100",
        )?;
        let rows = statement.query_map([self_id], |row| Ok(DissolvedGroup {
            id: row.get(0)?, name: row.get(1)?, owner: row.get(2)?, dissolved_at: row.get(3)?,
            last_body: row.get(4)?, last_logical: row.get(5)?,
        }))?;
        rows.collect()
    }

    /// Removes every local trace of a group, including downloaded attachments.
    pub fn delete_group_history(&self, group_id: &str) -> Result<bool> {
        if !valid_group_id(group_id) { return Err(rusqlite::Error::InvalidQuery); }
        let paths: Vec<String> = {
            let mut statement = self.connection.prepare(
                "SELECT f.target_path FROM group_file_transfers f
                 JOIN group_messages m ON m.id = f.message_id
                 WHERE m.group_id = ?1 AND f.target_path IS NOT NULL")?;
            let rows = statement.query_map([group_id], |row| row.get::<_, String>(0))?;
            rows.collect::<Result<Vec<String>>>()?
        };
        for path in paths {
            let _ = std::fs::remove_file(path);
        }
        let tx = self.connection.unchecked_transaction()?;
        tx.execute("DELETE FROM file_transfers WHERE message_id IN (SELECT id FROM group_messages WHERE group_id = ?1)", [group_id])?;
        tx.execute("DELETE FROM file_offer_requests WHERE group_id = ?1", [group_id])?;
        tx.execute("DELETE FROM group_outbox WHERE group_id = ?1", [group_id])?;
        tx.execute("DELETE FROM group_control WHERE group_id = ?1", [group_id])?;
        tx.execute("DELETE FROM group_messages WHERE group_id = ?1", [group_id])?;
        tx.execute("DELETE FROM group_members WHERE group_id = ?1", [group_id])?;
        let removed = tx.execute("DELETE FROM groups WHERE id = ?1 AND active = 0", [group_id])?;
        tx.commit()?;
        Ok(removed == 1)
    }

    /// Deletes the selected messages from one local conversation. Only local
    /// rows are removed; peers keep their own copies of the same messages.
    pub fn delete_messages(&self, kind: &str, id: &str, ids: &[String]) -> Result<usize> {
        if ids.is_empty() || ids.len() > 1000 || ids.iter().any(|mid| mid.is_empty() || mid.len() > 128) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let tx = self.connection.unchecked_transaction()?;
        let mut removed = 0usize;
        match kind {
            "self" => {
                let mut statement = tx.prepare("DELETE FROM local_messages WHERE conversation_id = 'self' AND id = ?1")?;
                for mid in ids { removed += statement.execute([mid])?; }
            }
            "direct" => {
                let mut statement = tx.prepare("DELETE FROM direct_messages WHERE peer = ?1 AND id = ?2")?;
                for mid in ids { removed += statement.execute([id, mid])?; }
            }
            "group" => {
                let mut statement = tx.prepare("SELECT id FROM group_messages WHERE group_id = ?1 AND id = ?2")?;
                let mut doomed: Vec<String> = Vec::new();
                for mid in ids {
                    if statement.query_row([id, mid], |row| row.get::<_, String>(0)).is_ok() {
                        doomed.push(mid.clone());
                    }
                }
                drop(statement);
                let mut outbox = tx.prepare("DELETE FROM group_outbox WHERE message_id = ?1")?;
                let mut transfers = tx.prepare("DELETE FROM file_transfers WHERE message_id = ?1")?;
                let mut group_files = tx.prepare("DELETE FROM group_file_transfers WHERE message_id = ?1")?;
                for mid in &doomed {
                    outbox.execute([mid])?;
                    transfers.execute([mid])?;
                    group_files.execute([mid])?;
                }
                drop(outbox); drop(transfers); drop(group_files);
                let mut statement = tx.prepare("DELETE FROM group_messages WHERE group_id = ?1 AND id = ?2")?;
                for mid in &doomed { removed += statement.execute([id, mid])?; }
            }
            _ => return Err(rusqlite::Error::InvalidQuery),
        }
        tx.commit()?;
        Ok(removed)
    }

    /// Friend requests are separate rows so a declined or withdrawn request never
    /// becomes a contact. `direction` is `in` for requests received by this device.
    pub fn create_contact_request(&self, direction: &str, peer: &str, nickname: &str,
        username: Option<&str>, address: Option<&str>, now: i64) -> Result<Option<ContactRequest>> {
        if !matches!(direction, "in" | "out") || !valid_fingerprint(peer) || nickname.len() > 128
            || address.is_some_and(|value| value.is_empty() || value.len() > 128) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        if let Some(existing) = self.pending_contact_request(peer, direction)? { return Ok(Some(existing)); }
        let tx = self.connection.unchecked_transaction()?;
        let id: String = tx.query_row("SELECT lower(hex(randomblob(16)))", [], |row| row.get(0))?;
        tx.execute(
            "INSERT INTO contact_requests(id, direction, peer, nickname, username, address, created_at, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending')",
            params![id, direction, peer, nickname, username, address, now],
        )?;
        tx.commit()?;
        self.contact_request(&id)
    }

    pub fn contact_request(&self, id: &str) -> Result<Option<ContactRequest>> {
        self.connection.query_row(
            "SELECT id, direction, peer, nickname, username, address, created_at, status
             FROM contact_requests WHERE id = ?1", [id],
            |row| Ok(ContactRequest {
                id: row.get(0)?, direction: row.get(1)?, peer: row.get(2)?, nickname: row.get(3)?,
                username: row.get(4)?, address: row.get(5)?, created_at: row.get(6)?, status: row.get(7)?,
            }),
        ).optional()
    }

    pub fn pending_contact_request(&self, peer: &str, direction: &str) -> Result<Option<ContactRequest>> {
        self.connection.query_row(
            "SELECT id, direction, peer, nickname, username, address, created_at, status
             FROM contact_requests WHERE peer = ?1 AND direction = ?2 AND status = 'pending'",
            params![peer, direction],
            |row| Ok(ContactRequest {
                id: row.get(0)?, direction: row.get(1)?, peer: row.get(2)?, nickname: row.get(3)?,
                username: row.get(4)?, address: row.get(5)?, created_at: row.get(6)?, status: row.get(7)?,
            }),
        ).optional()
    }

    pub fn list_contact_requests(&self, direction: &str) -> Result<Vec<ContactRequest>> {
        if !matches!(direction, "in" | "out") { return Err(rusqlite::Error::InvalidQuery); }
        let mut statement = self.connection.prepare(
            "SELECT id, direction, peer, nickname, username, address, created_at, status
             FROM contact_requests WHERE direction = ?1 ORDER BY created_at DESC LIMIT 200")?;
        let rows = statement.query_map([direction], |row| Ok(ContactRequest {
            id: row.get(0)?, direction: row.get(1)?, peer: row.get(2)?, nickname: row.get(3)?,
            username: row.get(4)?, address: row.get(5)?, created_at: row.get(6)?, status: row.get(7)?,
        }))?;
        rows.collect()
    }

    /// Answers a request. Accepting an inbound or outbound request makes the peer a
    /// verified contact; rejecting only closes the request row.
    pub fn resolve_contact_request(&self, id: &str, accept: bool, _now: i64) -> Result<Option<ContactRequest>> {
        let Some(request) = self.contact_request(id)? else { return Ok(None); };
        if request.status != "pending" { return Ok(None); }
        let tx = self.connection.unchecked_transaction()?;
        tx.execute("UPDATE contact_requests SET status = ?2 WHERE id = ?1",
            params![id, if accept { "accepted" } else { "rejected" }])?;
        if accept {
            tx.execute(
                "INSERT INTO contacts(fingerprint, display_name, trust_state, username)
                 VALUES (?1, ?2, 'verified', ?3)
                 ON CONFLICT(fingerprint) DO UPDATE SET trust_state = 'verified', display_name = excluded.display_name",
                params![request.peer, request.nickname, request.username],
            )?;
            if let Some(address) = request.address.as_deref() {
                tx.execute(
                    "INSERT INTO contact_addresses(peer, address) VALUES (?1, ?2)
                     ON CONFLICT(peer) DO UPDATE SET address = excluded.address",
                    params![request.peer, address])?;
            }
            tx.execute("INSERT INTO direct_conversations(peer, hidden) VALUES (?1, 0) ON CONFLICT(peer) DO UPDATE SET hidden = 0",
                [request.peer])?;
        }
        tx.commit()?;
        self.contact_request(id)
    }

    /// Answers the pending request with this peer, if any; used by the contact protocol.
    pub fn resolve_contact_request_by_peer(&self, peer: &str, direction: &str, accept: bool, now: i64) -> Result<bool> {
        if !valid_fingerprint(peer) || !matches!(direction, "in" | "out") {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let Some(request) = self.pending_contact_request(peer, direction)? else { return Ok(false); };
        Ok(self.resolve_contact_request(&request.id, accept, now)?.is_some())
    }

    pub fn withdraw_contact_request(&self, peer: &str) -> Result<bool> {
        if !valid_fingerprint(peer) { return Err(rusqlite::Error::InvalidQuery); }
        Ok(self.connection.execute(
            "UPDATE contact_requests SET status = 'rejected' WHERE peer = ?1 AND direction = 'out' AND status = 'pending'",
            [peer])? == 1)
    }

    pub fn lan_visible(&self) -> Result<bool> {
        Ok(self.get_setting("lan_visible")?.as_deref() != Some("0"))
    }

    pub fn set_lan_visible(&self, visible: bool) -> Result<()> {
        self.set_setting("lan_visible", Some(if visible { "1" } else { "0" }))?;
        Ok(())
    }

    /// `ask` (default) requires a local answer; `auto` accepts every request.
    pub fn contact_request_mode(&self) -> Result<String> {
        Ok(self.get_setting("contact_request_mode")?.unwrap_or_else(|| "ask".into()))
    }

    pub fn set_contact_request_mode(&self, mode: &str) -> Result<()> {
        if !matches!(mode, "ask" | "auto") { return Err(rusqlite::Error::InvalidQuery); }
        self.set_setting("contact_request_mode", Some(mode))?;
        Ok(())
    }

    pub fn open_group(&self, group_id: &str, self_id: &str) -> Result<bool> {
        if !valid_group_id(group_id) || !valid_fingerprint(self_id) { return Err(rusqlite::Error::InvalidQuery); }
        Ok(self.connection.execute(
            "UPDATE group_members SET hidden = 0, unread = 0 WHERE group_id = ?1 AND member = ?2 AND active = 1",
            params![group_id, self_id])? == 1)
    }

    pub fn hide_group(&self, group_id: &str, self_id: &str) -> Result<bool> {
        if !valid_group_id(group_id) || !valid_fingerprint(self_id) { return Err(rusqlite::Error::InvalidQuery); }
        Ok(self.connection.execute(
            "UPDATE group_members SET hidden = 1 WHERE group_id = ?1 AND member = ?2 AND active = 1",
            params![group_id, self_id])? == 1)
    }

    /// Per-member mute flag; other members are unaffected and history stays intact.
    pub fn set_group_muted(&self, group_id: &str, self_id: &str, muted: bool) -> Result<bool> {
        if !valid_group_id(group_id) || !valid_fingerprint(self_id) { return Err(rusqlite::Error::InvalidQuery); }
        Ok(self.connection.execute(
            "UPDATE group_members SET muted = ?3 WHERE group_id = ?1 AND member = ?2 AND active = 1",
            params![group_id, self_id, i64::from(muted)])? == 1)
    }

    pub fn mark_group_unread(&self, group_id: &str, self_id: &str) -> Result<bool> {
        if !valid_group_id(group_id) || !valid_fingerprint(self_id) { return Err(rusqlite::Error::InvalidQuery); }
        Ok(self.connection.execute(
            "UPDATE group_members SET unread = (CASE WHEN unread < 1 THEN 1 ELSE unread END), hidden = 0
             WHERE group_id = ?1 AND member = ?2 AND active = 1",
            params![group_id, self_id])? == 1)
    }

    pub fn list_groups(&self, self_id: &str) -> Result<Vec<GroupSummary>> {
        if !valid_fingerprint(self_id) { return Err(rusqlite::Error::InvalidQuery); }
        let mut statement = self.connection.prepare(
            "SELECT g.id, g.name, g.owner, gm.hidden,
                (SELECT count(*) FROM group_members x WHERE x.group_id = g.id AND x.active = 1),
                COALESCE((SELECT m.body FROM group_messages m WHERE m.group_id = g.id
                          ORDER BY m.logical DESC, m.sender DESC, m.id DESC LIMIT 1), ''),
                COALESCE((SELECT m.logical FROM group_messages m WHERE m.group_id = g.id
                          ORDER BY m.logical DESC, m.sender DESC, m.id DESC LIMIT 1), 0)
             FROM groups g JOIN group_members gm ON gm.group_id = g.id
             WHERE gm.member = ?1 AND gm.active = 1 AND g.active = 1
             ORDER BY 7 DESC, g.id"
        )?;
        let rows = statement.query_map([self_id], |row| {
            let owner: String = row.get(2)?;
            Ok(GroupSummary {
                id: row.get(0)?, name: row.get(1)?, owner: owner.clone(), member_count: row.get(4)?,
                hidden: row.get(3)?, is_owner: owner == self_id, last_body: row.get(5)?, last_logical: row.get(6)?,
            })
        })?;
        rows.collect()
    }

    /// Inactive (removed / left) members are kept so their join time and history stay explainable.
    pub fn list_group_members(&self, group_id: &str) -> Result<Vec<GroupMember>> {
        if !valid_group_id(group_id) { return Err(rusqlite::Error::InvalidQuery); }
        let mut statement = self.connection.prepare(
            "SELECT gm.group_id, gm.member, COALESCE(c.display_name, substr(gm.member, 1, 8)), gm.role, gm.joined_at, gm.active
             FROM group_members gm LEFT JOIN contacts c ON c.fingerprint = gm.member
             WHERE gm.group_id = ?1 ORDER BY gm.active DESC, (gm.role = 'owner') DESC, gm.joined_at, gm.member"
        )?;
        let rows = statement.query_map([group_id], |row| Ok(GroupMember {
            group_id: row.get(0)?, member: row.get(1)?, display_name: row.get(2)?, role: row.get(3)?,
            joined_at: row.get(4)?, active: row.get(5)?,
        }))?;
        rows.collect()
    }

    /// Stores a group message and queues one delivery row per other active member.
    /// Stores a group message with an optional quote and @ mentions, then queues one
    /// delivery row per other active member. The quote is a short local snippet so the
    /// UI never has to fetch the original before showing the reply.
    pub fn queue_group(&self, sender: &str, group_id: &str, body: &str, reply_to: Option<&str>,
        mentions: &[String], created_at: i64) -> Result<GroupMessage> {
        if !valid_fingerprint(sender) || !valid_group_id(group_id) || body.trim().is_empty() || body.len() > 32768 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        if let Some(id) = reply_to {
            if !valid_message_id(id) || id.len() > 128 { return Err(rusqlite::Error::InvalidQuery); }
        }
        if mentions.len() > 256 { return Err(rusqlite::Error::InvalidQuery); }
        let tx = self.connection.unchecked_transaction()?;
        let active: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM group_members WHERE group_id = ?1 AND member = ?2 AND active = 1)",
            params![group_id, sender], |row| row.get(0),
        )?;
        if !active { return Err(rusqlite::Error::InvalidQuery); }
        let mut unique_mentions = Vec::new();
        for member in mentions {
            if !valid_fingerprint(member) || unique_mentions.contains(member) { return Err(rusqlite::Error::InvalidQuery); }
            let member_active: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM group_members WHERE group_id = ?1 AND member = ?2 AND active = 1)",
                params![group_id, member], |row| row.get(0),
            )?;
            if !member_active { return Err(rusqlite::Error::InvalidQuery); }
            unique_mentions.push(member.clone());
        }
        let (reply_sender, reply_preview) = match reply_to {
            Some(id) => {
                let quoted: Option<(String, String)> = tx.query_row(
                    "SELECT sender, body FROM group_messages WHERE id = ?1 AND group_id = ?2",
                    params![id, group_id], |row| Ok((row.get(0)?, row.get(1)?)),
                ).optional()?;
                let Some((quoted_sender, quoted_body)) = quoted else { return Err(rusqlite::Error::InvalidQuery); };
                // A raw fingerprint prefix is not a name: prefer the group member name,
                // then the local remark/nickname, and only then a neutral label.
                let quote_name: String = tx.query_row(
                    "SELECT COALESCE(NULLIF(display_name, ''), '群成员') FROM group_members WHERE group_id = ?1 AND member = ?2",
                    params![group_id, &quoted_sender], |row| row.get(0),
                ).or_else(|_| tx.query_row(
                    "SELECT COALESCE(NULLIF(remark, ''), display_name) FROM contacts WHERE fingerprint = ?1",
                    [&quoted_sender], |row| row.get(0),
                )).unwrap_or_else(|_| "群成员".to_owned());
                (Some(quote_name), Some(quote_preview(&quoted_body)))
            }
            None => (None, None),
        };
        if tx.execute("UPDATE logical_clock SET value = value + 1 WHERE singleton = 1 AND value < 9223372036854775807", [])? != 1 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let (id, logical): (String, i64) = tx.query_row(
            "SELECT lower(hex(randomblob(16))), value FROM logical_clock WHERE singleton = 1", [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let collision: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM local_messages WHERE id = ?1
                UNION ALL SELECT 1 FROM direct_messages WHERE id = ?1
                UNION ALL SELECT 1 FROM group_messages WHERE id = ?1)", [&id], |row| row.get(0),
        )?;
        if collision { return Err(rusqlite::Error::InvalidQuery); }
        tx.execute(
            "INSERT INTO group_messages(id, group_id, sender, body, created_at, logical, reply_to, reply_sender, reply_preview, mentions)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![id, group_id, sender, body, created_at, logical, reply_to, reply_sender, reply_preview,
                serde_json::to_string(&unique_mentions).unwrap_or_else(|_| "[]".to_owned())])?;
        tx.execute("UPDATE group_members SET hidden = 0 WHERE group_id = ?1 AND member = ?2", params![group_id, sender])?;
        let pending = tx.execute(
            "INSERT INTO group_outbox(message_id, group_id, member, status)
             SELECT ?1, ?2, member, 'pending' FROM group_members WHERE group_id = ?2 AND active = 1 AND member != ?3",
            params![id, group_id, sender],
        )? as i64;
        tx.commit()?;
        let sender_name: String = self.connection.query_row(
            "SELECT COALESCE(display_name, substr(?1, 1, 8)) FROM contacts WHERE fingerprint = ?1",
            [sender], |row| row.get(0),
        ).unwrap_or_else(|_| sender[..8.min(sender.len())].to_owned());
        Ok(GroupMessage {
            id, group_id: group_id.into(), sender: sender.into(), sender_name,
            body: body.into(), created_at, logical, pending,
            reply_to: reply_to.map(str::to_owned), reply_sender, reply_preview,
            mentions: unique_mentions,
            kind: "text".into(), file_name: None, file_size: None, file_sha256: None,
            file_status: None, file_transferred: None, file_error: None,
            recalled_at: None,
        })
    }

    /// Newest page first, keyset paginated by the same deterministic order as direct chat.
    pub fn list_group_messages(&self, group_id: &str, limit: u32, before: Option<&DirectCursor>) -> Result<Vec<GroupMessage>> {
        if !valid_group_id(group_id) || limit == 0 || limit > 200
            || before.is_some_and(|c| c.logical <= 0 || !valid_fingerprint(&c.sender) || c.id.is_empty() || c.id.len() > 128) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let mut statement = self.connection.prepare(
            "SELECT m.id, m.group_id, m.sender, COALESCE(c.display_name, substr(m.sender, 1, 8)), m.body, m.created_at, m.logical,
                (SELECT count(*) FROM group_outbox o WHERE o.message_id = m.id AND o.status = 'pending'),
                m.reply_to, m.reply_sender, m.reply_preview, m.mentions,
                m.kind, m.file_name, m.file_size, m.file_sha256, f.status, f.transferred, f.error, m.recalled_at
             FROM group_messages m LEFT JOIN contacts c ON c.fingerprint = m.sender
             LEFT JOIN group_file_transfers f ON f.message_id = m.id
             WHERE m.group_id = ?1 AND (?2 IS NULL OR (m.logical, m.sender, m.id) < (?2, ?3, ?4))
             ORDER BY m.logical DESC, m.sender DESC, m.id DESC LIMIT ?5"
        )?;
        let rows = statement.query_map(params![group_id, before.map(|c| c.logical), before.map(|c| c.sender.as_str()), before.map(|c| c.id.as_str()), limit],
            group_message_row)?;
        rows.collect()
    }

    /// Cross-conversation substring search. Only conversations the user can see are
    /// included: the local self chat, direct chats with verified contacts, and groups
    /// where the local identity is an active member.
    pub fn search_all(&self, self_id: &str, query: &str, category: &str, conversation_kind: Option<&str>,
        conversation_id: Option<&str>, limit: u32, offset: u32) -> Result<Vec<SearchHit>> {
        if !valid_fingerprint(self_id) || !valid_search(query, category, limit, offset) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        match (conversation_kind, conversation_id) {
            (None, None) | (Some("self"), Some("self")) => {}
            (Some("direct"), Some(peer)) if valid_fingerprint(peer) && self.is_verified(peer)? => {}
            (Some("group"), Some(group_id)) if valid_group_id(group_id) => {
                let active: bool = self.connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM group_members WHERE group_id = ?1 AND member = ?2 AND active = 1)",
                    params![group_id, self_id], |row| row.get(0))?;
                if !active { return Err(rusqlite::Error::InvalidQuery); }
            }
            _ => return Err(rusqlite::Error::InvalidQuery),
        }
        let pattern = like_pattern(query);
        let mut statement = self.connection.prepare(
            "SELECT 'self' AS ck, 'self' AS cid, '自聊' AS cname, m.id, m.sender, '我', m.body, m.created_at, m.logical, 'text'
             FROM local_messages m
             WHERE m.conversation_id = 'self' AND m.recalled_at IS NULL AND ?3 IN ('all', 'text') AND (?1 = '' OR m.body LIKE ?2 ESCAPE '\\')
               AND (?5 IS NULL OR (?5 = 'self' AND ?6 = 'self'))
             UNION ALL
             SELECT 'direct', m.peer, COALESCE(c.display_name, substr(m.peer, 1, 8)), m.id, m.sender,
                    CASE WHEN m.sender = ?4 THEN '我' ELSE COALESCE(c.display_name, substr(m.peer, 1, 8)) END,
                    m.body, m.created_at, m.logical, m.kind
             FROM direct_messages m JOIN contacts c ON c.fingerprint = m.peer AND c.trust_state = 'verified'
             WHERE m.recalled_at IS NULL AND (?1 = '' OR m.body LIKE ?2 ESCAPE '\\' OR COALESCE(m.file_name, '') LIKE ?2 ESCAPE '\\')
               AND ((?3 IN ('all', 'text') AND m.kind = 'text')
                 OR (?3 IN ('all', 'file') AND m.kind = 'file')
                 OR (?3 = 'image' AND m.kind = 'file' AND (lower(m.file_name) LIKE '%.png' OR lower(m.file_name) LIKE '%.jpg'
                    OR lower(m.file_name) LIKE '%.jpeg' OR lower(m.file_name) LIKE '%.gif' OR lower(m.file_name) LIKE '%.webp'
                    OR lower(m.file_name) LIKE '%.bmp')))
               AND (?5 IS NULL OR (?5 = 'direct' AND m.peer = ?6))
             UNION ALL
             SELECT 'group', g.id, g.name, m.id, m.sender,
                    CASE WHEN m.sender = ?4 THEN '我' ELSE COALESCE(c.display_name, substr(m.sender, 1, 8)) END,
                    m.body, m.created_at, m.logical, m.kind
             FROM group_messages m
             JOIN groups g ON g.id = m.group_id
             JOIN group_members gm ON gm.group_id = g.id AND gm.member = ?4 AND gm.active = 1
             LEFT JOIN contacts c ON c.fingerprint = m.sender
             WHERE m.recalled_at IS NULL
               AND ((?3 IN ('all', 'text') AND m.kind = 'text' AND (?1 = '' OR m.body LIKE ?2 ESCAPE '\\'))
                 OR (?3 IN ('all', 'file') AND m.kind = 'file'
                     AND (?1 = '' OR m.body LIKE ?2 ESCAPE '\\' OR COALESCE(m.file_name, '') LIKE ?2 ESCAPE '\\'))
                 OR (?3 = 'image' AND m.kind = 'file'
                     AND (lower(m.file_name) LIKE '%.png' OR lower(m.file_name) LIKE '%.jpg' OR lower(m.file_name) LIKE '%.jpeg'
                       OR lower(m.file_name) LIKE '%.gif' OR lower(m.file_name) LIKE '%.webp' OR lower(m.file_name) LIKE '%.bmp')
                     AND (?1 = '' OR m.body LIKE ?2 ESCAPE '\\' OR COALESCE(m.file_name, '') LIKE ?2 ESCAPE '\\')))
               AND (?5 IS NULL OR (?5 = 'group' AND g.id = ?6))
             ORDER BY created_at DESC, logical DESC, id DESC, ck DESC, cid DESC LIMIT ?7 OFFSET ?8"
        )?;
        let rows = statement.query_map(params![query, pattern, category, self_id, conversation_kind, conversation_id, limit, offset], |row| Ok(SearchHit {
            conversation_kind: row.get(0)?, conversation_id: row.get(1)?, conversation_name: row.get(2)?,
            message_id: row.get(3)?, sender: row.get(4)?, sender_name: row.get(5)?, body: row.get(6)?,
            created_at: row.get(7)?, logical: row.get(8)?, kind: row.get(9)?,
        }))?;
        rows.collect()
    }

    /// Searches one chat-history window by date, keyword, and content type.
    /// Direct peers must be verified; groups must still have an active local member.
    pub fn search_chat_history(&self, kind: &str, target: &str, self_id: &str, query: &str, category: &str,
        since: Option<i64>, until: Option<i64>, recalled_label: &str, limit: u32, offset: u32)
        -> Result<Vec<HistorySearchHit>> {
        if !valid_fingerprint(self_id) || !valid_search(query, category, limit, offset)
            || since.is_some_and(|value| value < 0) || until.is_some_and(|value| value < 0)
            || since.zip(until).is_some_and(|(start, end)| start > end) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let pattern = like_pattern(query);
        let rows = match kind {
            "self" if target == "self" => {
                let mut statement = self.connection.prepare(
                    "SELECT id, sender, '我', body, created_at, logical, 'text', NULL, NULL
                     FROM local_messages
                     WHERE conversation_id = 'self' AND ?1 IN ('all', 'text')
                       AND (?2 = '' OR body LIKE ?3 ESCAPE '\\')
                       AND (?4 IS NULL OR created_at >= ?4) AND (?5 IS NULL OR created_at <= ?5)
                     ORDER BY created_at DESC, logical DESC, id DESC LIMIT ?6 OFFSET ?7"
                )?;
                let rows = statement.query_map(params![category, query, pattern, since, until, limit, offset], |row| Ok(HistorySearchHit {
                    id: row.get(0)?, sender: row.get(1)?, sender_name: row.get(2)?, body: row.get(3)?,
                    created_at: row.get(4)?, logical: row.get(5)?, kind: row.get(6)?, file_name: row.get(7)?, recalled_at: row.get(8)?,
                }))?;
                rows.collect::<Result<Vec<_>>>()?
            }
            "direct" if valid_fingerprint(target) && self.is_verified(target)? => {
                let category_filter = match category {
                    "text" => "COALESCE(m.kind, 'text') = 'text'",
                    "file" => "m.kind = 'file'",
                    "image" => "m.kind = 'file' AND (lower(m.file_name) LIKE '%.avif' OR lower(m.file_name) LIKE '%.bmp' OR lower(m.file_name) LIKE '%.gif' OR lower(m.file_name) LIKE '%.heic' OR lower(m.file_name) LIKE '%.heif' OR lower(m.file_name) LIKE '%.jpeg' OR lower(m.file_name) LIKE '%.jpg' OR lower(m.file_name) LIKE '%.png' OR lower(m.file_name) LIKE '%.tif' OR lower(m.file_name) LIKE '%.tiff' OR lower(m.file_name) LIKE '%.webp')",
                    _ => "1 = 1",
                };
                let sql = format!(
                    "SELECT m.id, m.sender,
                            CASE WHEN m.sender = ?2 THEN '我' ELSE COALESCE(c.display_name, substr(m.sender, 1, 8)) END,
                            m.body, m.created_at, m.logical, COALESCE(m.kind, 'text'), m.file_name, m.recalled_at
                     FROM direct_messages m JOIN contacts c ON c.fingerprint = m.peer AND c.trust_state = 'verified'
                     WHERE m.peer = ?1 AND {category_filter}
                       AND (?4 = '' OR (m.recalled_at IS NULL AND (m.body LIKE ?5 ESCAPE '\\' OR COALESCE(m.file_name, '') LIKE ?5 ESCAPE '\\'))
                         OR (m.recalled_at IS NOT NULL AND ?6 LIKE ?5 ESCAPE '\\'))
                       AND (?7 IS NULL OR m.created_at >= ?7) AND (?8 IS NULL OR m.created_at <= ?8)
                     ORDER BY m.created_at DESC, m.logical DESC, m.id DESC LIMIT ?9 OFFSET ?10"
                );
                let mut statement = self.connection.prepare(&sql)?;
                let rows = statement.query_map(params![target, self_id, category, query, pattern, recalled_label, since, until, limit, offset], |row| Ok(HistorySearchHit {
                    id: row.get(0)?, sender: row.get(1)?, sender_name: row.get(2)?, body: row.get(3)?,
                    created_at: row.get(4)?, logical: row.get(5)?, kind: row.get(6)?, file_name: row.get(7)?, recalled_at: row.get(8)?,
                }))?;
                rows.collect::<Result<Vec<_>>>()?
            }
            "group" if valid_group_id(target) => {
                let active: bool = self.connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM group_members WHERE group_id = ?1 AND member = ?2 AND active = 1)",
                    params![target, self_id], |row| row.get(0))?;
                if !active { return Err(rusqlite::Error::InvalidQuery); }
                let category_filter = match category {
                    "text" => "COALESCE(m.kind, 'text') = 'text'",
                    "file" => "m.kind = 'file'",
                    "image" => "m.kind = 'file' AND (lower(m.file_name) LIKE '%.avif' OR lower(m.file_name) LIKE '%.bmp' OR lower(m.file_name) LIKE '%.gif' OR lower(m.file_name) LIKE '%.heic' OR lower(m.file_name) LIKE '%.heif' OR lower(m.file_name) LIKE '%.jpeg' OR lower(m.file_name) LIKE '%.jpg' OR lower(m.file_name) LIKE '%.png' OR lower(m.file_name) LIKE '%.tif' OR lower(m.file_name) LIKE '%.tiff' OR lower(m.file_name) LIKE '%.webp')",
                    _ => "1 = 1",
                };
                let sql = format!(
                    "SELECT m.id, m.sender,
                            CASE WHEN m.sender = ?2 THEN '我' ELSE COALESCE(c.display_name, substr(m.sender, 1, 8)) END,
                            m.body, m.created_at, m.logical, COALESCE(m.kind, 'text'), m.file_name, m.recalled_at
                     FROM group_messages m JOIN groups g ON g.id = m.group_id
                     JOIN group_members gm ON gm.group_id = g.id AND gm.member = ?2 AND gm.active = 1
                     LEFT JOIN contacts c ON c.fingerprint = m.sender
                     WHERE m.group_id = ?1 AND {category_filter}
                       AND (?4 = '' OR (m.recalled_at IS NULL AND (m.body LIKE ?5 ESCAPE '\\' OR COALESCE(m.file_name, '') LIKE ?5 ESCAPE '\\'))
                         OR (m.recalled_at IS NOT NULL AND ?6 LIKE ?5 ESCAPE '\\'))
                       AND (?7 IS NULL OR m.created_at >= ?7) AND (?8 IS NULL OR m.created_at <= ?8)
                     ORDER BY m.created_at DESC, m.logical DESC, m.id DESC LIMIT ?9 OFFSET ?10"
                );
                let mut statement = self.connection.prepare(&sql)?;
                let rows = statement.query_map(params![target, self_id, category, query, pattern, recalled_label, since, until, limit, offset], |row| Ok(HistorySearchHit {
                    id: row.get(0)?, sender: row.get(1)?, sender_name: row.get(2)?, body: row.get(3)?,
                    created_at: row.get(4)?, logical: row.get(5)?, kind: row.get(6)?, file_name: row.get(7)?, recalled_at: row.get(8)?,
                }))?;
                rows.collect::<Result<Vec<_>>>()?
            }
            _ => return Err(rusqlite::Error::InvalidQuery),
        };
        Ok(rows)
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        if key.is_empty() || key.len() > 64 { return Err(rusqlite::Error::InvalidQuery); }
        self.connection.query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| row.get(0)).optional()
    }

    /// Writes or deletes one setting; `None` removes the row.
    pub fn set_setting(&self, key: &str, value: Option<&str>) -> Result<bool> {
        if key.is_empty() || key.len() > 64 || value.is_some_and(|text| text.len() > 4096) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        match value {
            Some(text) => Ok(self.connection.execute(
                "INSERT INTO settings(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, text])? == 1),
            None => { self.connection.execute("DELETE FROM settings WHERE key = ?1", [key])?; Ok(true) }
        }
    }

    /// Substring search within one group, newest first. Category `file`/`image` is
    /// reserved for group files and returns nothing until those exist.
    pub fn search_group_messages(&self, group_id: &str, query: &str, category: &str,
        member: Option<&str>, limit: u32, offset: u32) -> Result<Vec<GroupMessage>> {
        if !valid_group_id(group_id) || !valid_search(&query, category, limit, offset)
            || member.is_some_and(|value| !valid_fingerprint(value)) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let pattern = like_pattern(query);
        let mut statement = self.connection.prepare(
            "SELECT m.id, m.group_id, m.sender, COALESCE(c.display_name, substr(m.sender, 1, 8)), m.body, m.created_at, m.logical,
                0, m.reply_to, m.reply_sender, m.reply_preview, m.mentions,
                m.kind, m.file_name, m.file_size, m.file_sha256, f.status, f.transferred, f.error, m.recalled_at
             FROM group_messages m
             LEFT JOIN contacts c ON c.fingerprint = m.sender
             LEFT JOIN group_file_transfers f ON f.message_id = m.id
             WHERE m.group_id = ?1 AND m.recalled_at IS NULL
               AND ((?7 IN ('all', 'text') AND m.kind = 'text' AND (?2 = '' OR m.body LIKE ?3 ESCAPE '\\'))
                 OR (?7 IN ('all', 'file') AND m.kind = 'file'
                     AND (?2 = '' OR m.body LIKE ?3 ESCAPE '\\' OR COALESCE(m.file_name, '') LIKE ?3 ESCAPE '\\'))
                 OR (?7 = 'image' AND m.kind = 'file'
                     AND (lower(m.file_name) LIKE '%.png' OR lower(m.file_name) LIKE '%.jpg' OR lower(m.file_name) LIKE '%.jpeg'
                       OR lower(m.file_name) LIKE '%.gif' OR lower(m.file_name) LIKE '%.webp' OR lower(m.file_name) LIKE '%.bmp')))
               AND (?4 IS NULL OR m.sender = ?4)
             ORDER BY m.logical DESC, m.sender DESC, m.id DESC LIMIT ?5 OFFSET ?6"
        )?;
        let rows = statement.query_map(
            params![group_id, query, pattern, member, limit, offset, category], group_message_row)?;
        rows.collect()
    }

    /// A recipient confirms one group message; only that member's outbox row changes.
    pub fn ack_group(&self, member: &str, message_id: &str) -> Result<bool> {
        if !valid_fingerprint(member) || message_id.is_empty() || message_id.len() > 128 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        Ok(self.connection.execute(
            "UPDATE group_outbox SET status = 'delivered' WHERE message_id = ?1 AND member = ?2 AND status = 'pending'",
            params![message_id, member])? == 1)
    }

    /// Picks the next member that still needs a message and already has a connection address.
    pub fn next_pending_group_delivery(&self, after: Option<&str>) -> Result<Option<GroupDelivery>> {
        if after.is_some_and(|member| !valid_fingerprint(member)) { return Err(rusqlite::Error::InvalidQuery); }
        self.connection.query_row(
            "SELECT o.member, a.address, o.message_id
             FROM group_outbox o
             JOIN contact_addresses a ON a.peer = o.member
             JOIN contacts c ON c.fingerprint = o.member AND c.trust_state = 'verified'
             WHERE o.status = 'pending' AND (?1 IS NULL OR o.member > ?1)
             ORDER BY o.member, o.message_id LIMIT 1", [after],
            |row| Ok(GroupDelivery { member: row.get(0)?, address: row.get(1)?, message_id: row.get(2)? }),
        ).optional()
    }

    /// True only while this recipient still has an unrecalled message in the outbox.
    pub fn group_delivery_pending(&self, member: &str, message_id: &str) -> Result<bool> {
        if !valid_fingerprint(member) || !valid_message_id(message_id) { return Err(rusqlite::Error::InvalidQuery); }
        self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM group_outbox o JOIN group_messages m ON m.id = o.message_id
             WHERE o.member = ?1 AND o.message_id = ?2 AND o.status = 'pending' AND m.recalled_at IS NULL)",
            params![member, message_id], |row| row.get(0))
    }

    /// Snapshot of a group message plus the member list used when sending it.
    pub fn group_dispatch(&self, message_id: &str) -> Result<Option<GroupDispatch>> {
        if message_id.is_empty() || message_id.len() > 128 { return Err(rusqlite::Error::InvalidQuery); }
        let found: Option<(String, String, String, String, String, i64, i64, Option<String>, Option<String>, Option<String>, String, String, Option<String>, Option<i64>, Option<String>, Option<String>)> = self.connection.query_row(
            "SELECT m.id, m.group_id, g.name, g.owner, m.body, m.created_at, m.logical,
                m.reply_to, m.reply_sender, m.reply_preview, m.mentions,
                m.kind, m.file_name, m.file_size, m.file_sha256,
                (SELECT source_path FROM group_file_transfers WHERE message_id = m.id)
             FROM group_messages m JOIN groups g ON g.id = m.group_id WHERE m.id = ?1", [message_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?,
                row.get(7)?, row.get(8)?, row.get(9)?, row.get(10)?, row.get(11)?, row.get(12)?, row.get(13)?, row.get(14)?, row.get(15)?)),
        ).optional()?;
        let Some((message_id, group_id, group_name, owner, body, created_at, logical, reply_to, reply_sender, reply_preview, mentions, kind, file_name, file_size, file_sha256, source_path)) = found else { return Ok(None); };
        let mut statement = self.connection.prepare(
            "SELECT member FROM group_members WHERE group_id = ?1 AND active = 1 ORDER BY member")?;
        let rows = statement.query_map([&group_id], |row| row.get(0))?;
        let members: Vec<String> = rows.collect::<Result<Vec<String>>>()?;
        Ok(Some(GroupDispatch { message_id, group_id, group_name, owner, members, body, created_at, logical,
            reply_to, reply_sender, reply_preview, mentions: parse_mentions(&mentions),
            kind, file_name, file_size, file_sha256, source_path }))
    }

    /// Applies one inbound group text. The member snapshot is only trusted from the owner;
    /// other members may create the group for us but cannot rewrite its member list.
    #[allow(clippy::too_many_arguments)]
    /// Queues one group file message: the local source is referenced, one outbox row
    /// is created per active member, and the message body is the cleaned file name.
    pub fn queue_group_file(&self, sender: &str, group_id: &str, name: &str, size: u64, sha256: &str, source: &Path, created_at: i64) -> Result<GroupMessage> {
        self.queue_group_file_with_reply(sender, group_id, name, size, sha256, source, created_at, None)
    }

    pub fn queue_group_file_with_reply(&self, sender: &str, group_id: &str, name: &str, size: u64, sha256: &str,
        source: &Path, created_at: i64, reply_to: Option<&str>) -> Result<GroupMessage> {
        let clean = crate::files::sanitize_name(name).ok_or(rusqlite::Error::InvalidQuery)?;
        let source_text = source.to_str().ok_or(rusqlite::Error::InvalidQuery)?;
        if !valid_fingerprint(sender) || !valid_group_id(group_id)
            || size == 0 || size > crate::protocol::MAX_FILE_BYTES || !valid_sha256(sha256)
            || source_text.is_empty() || source_text.len() > 4096 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let size_i64 = i64::try_from(size).map_err(|_| rusqlite::Error::InvalidQuery)?;
        let tx = self.connection.unchecked_transaction()?;
        let active: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM group_members WHERE group_id = ?1 AND member = ?2 AND active = 1)",
            params![group_id, sender], |row| row.get(0),
        )?;
        if !active { return Err(rusqlite::Error::InvalidQuery); }
        let (reply_sender, reply_preview) = group_quote(&tx, group_id, reply_to)?;
        if tx.execute("UPDATE logical_clock SET value = value + 1 WHERE singleton = 1 AND value < 9223372036854775807", [])? != 1 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let (id, logical): (String, i64) = tx.query_row(
            "SELECT lower(hex(randomblob(16))), value FROM logical_clock WHERE singleton = 1", [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let collision: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM local_messages WHERE id = ?1
                UNION ALL SELECT 1 FROM direct_messages WHERE id = ?1
                UNION ALL SELECT 1 FROM group_messages WHERE id = ?1)", [&id], |row| row.get(0),
        )?;
        if collision { return Err(rusqlite::Error::InvalidQuery); }
        tx.execute(
            "INSERT INTO group_messages(id, group_id, sender, body, created_at, logical, reply_to, reply_sender, reply_preview, kind, file_name, file_size, file_sha256)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'file', ?4, ?10, ?11)",
            params![id, group_id, sender, clean, created_at, logical, reply_to, reply_sender, reply_preview, size_i64, sha256],
        )?;
        tx.execute("UPDATE group_members SET hidden = 0 WHERE group_id = ?1 AND member = ?2", params![group_id, sender])?;
        let pending = tx.execute(
            "INSERT INTO group_outbox(message_id, group_id, member, status)
             SELECT ?1, ?2, member, 'pending' FROM group_members WHERE group_id = ?2 AND active = 1 AND member != ?3",
            params![id, group_id, sender],
        )?;
        tx.execute(
            "INSERT INTO group_file_transfers(message_id, group_id, peer, direction, source_path, target_path, total, transferred, status, updated_at)
             VALUES (?1, ?2, ?3, 'outgoing', ?4, NULL, ?5, 0, 'pending', ?6)",
            params![id, group_id, sender, source_text, size_i64, created_at],
        )?;
        tx.commit()?;
        Ok(GroupMessage {
            id, group_id: group_id.into(), sender: sender.into(), sender_name: String::new(),
            body: clean.clone(), created_at, logical, pending: i64::try_from(pending).unwrap_or(0),
            reply_to: reply_to.map(str::to_owned), reply_sender, reply_preview, mentions: Vec::new(),
            kind: "file".into(), file_name: Some(clean), file_size: Some(size_i64), file_sha256: Some(sha256.into()),
            file_status: Some("pending".into()), file_transferred: Some(0), file_error: None,
            recalled_at: None,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn receive_group(&self, self_id: &str, peer: &str, group_id: &str, name: &str, owner: &str,
        members: &[String], id: &str, body: &str, created_at: i64, logical: i64,
        reply_to: Option<&str>, reply_sender: Option<&str>, reply_preview: Option<&str>,
        mentions: &[String], now: i64) -> Result<IncomingResult> {
        if !valid_fingerprint(self_id) || !valid_fingerprint(peer) || !valid_fingerprint(owner) || !valid_group_id(group_id)
            || name.is_empty() || name.len() > 128 || id.is_empty() || id.len() > 128
            || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || body.is_empty() || body.len() > 32768 || logical <= 0 || logical == i64::MAX {
            return Err(rusqlite::Error::InvalidQuery);
        }
        if reply_to.is_some_and(|value| !valid_message_id(value) || value.len() > 128)
            || reply_sender.is_some_and(|value| value.is_empty() || value.chars().count() > 128 || value.chars().any(char::is_control))
            || reply_preview.is_some_and(|value| value.chars().count() > REPLY_PREVIEW_MAX * 2)
            || (reply_to.is_some() != reply_preview.is_some())
            || mentions.len() > 256 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let mut roster = std::collections::BTreeSet::new();
        for member in members {
            if !valid_fingerprint(member) || !roster.insert(member.as_str()) { return Err(rusqlite::Error::InvalidQuery); }
        }
        if roster.len() > 256 || !roster.contains(owner) { return Err(rusqlite::Error::InvalidQuery); }
        let mut unique_mentions: Vec<String> = Vec::new();
        for member in mentions {
            if !valid_fingerprint(member) || !roster.contains(member.as_str()) || unique_mentions.contains(member) {
                return Err(rusqlite::Error::InvalidQuery);
            }
            unique_mentions.push(member.clone());
        }
        let tx = self.connection.unchecked_transaction()?;
        let verified: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM contacts WHERE fingerprint = ?1 AND trust_state = 'verified')",
            [peer], |row| row.get(0),
        )?;
        if !verified { return Ok(IncomingResult::Unverified); }
        let existing_owner: Option<String> = tx.query_row("SELECT owner FROM groups WHERE id = ?1", [group_id], |row| row.get(0)).optional()?;
        match existing_owner {
            None => {
                if !roster.contains(self_id) { return Ok(IncomingResult::NotMember); }
                tx.execute("INSERT INTO groups(id, name, owner, created_at) VALUES (?1, ?2, ?3, ?4)",
                    params![group_id, name, owner, now])?;
                for member in &roster {
                    tx.execute("INSERT INTO group_members(group_id, member, role, joined_at) VALUES (?1, ?2, ?3, ?4)",
                        params![group_id, member, if *member == owner { "owner" } else { "member" }, now])?;
                }
            }
            Some(local_owner) => {
                if local_owner != owner { return Ok(IncomingResult::IdConflict); }
                if peer == owner {
                    for member in &roster {
                        let role = if *member == owner { "owner" } else { "member" };
                        tx.execute(
                            "INSERT INTO group_members(group_id, member, role, joined_at) VALUES (?1, ?2, ?3, ?4)
                             ON CONFLICT(group_id, member) DO UPDATE SET active = 1, role = excluded.role",
                            params![group_id, member, role, now])?;
                    }
                    let stale: Vec<String> = {
                        let mut statement = tx.prepare("SELECT member FROM group_members WHERE group_id = ?1 AND active = 1")?;
                        let rows = statement.query_map([group_id], |row| row.get(0))?;
                        rows.collect::<Result<Vec<String>>>()?
                    };
                    for member in stale {
                        if !roster.contains(member.as_str()) {
                            tx.execute("UPDATE group_members SET active = 0 WHERE group_id = ?1 AND member = ?2",
                                params![group_id, member])?;
                        }
                    }
                }
            }
        }
        let active: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM group_members WHERE group_id = ?1 AND member = ?2 AND active = 1)",
            params![group_id, self_id], |row| row.get(0),
        )?;
        if !active {
            // The owner may have just removed us; keep that roster change.
            tx.commit()?;
            return Ok(IncomingResult::NotMember);
        }
        // A sender that is not an active member cannot post, even if we are a member.
        let sender_active: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM group_members WHERE group_id = ?1 AND member = ?2 AND active = 1)",
            params![group_id, peer], |row| row.get(0),
        )?;
        if !sender_active { return Ok(IncomingResult::NotMember); }
        let existing: Option<(String, String, String, i64, i64, Option<String>, Option<String>, Option<String>, String)> = tx.query_row(
            "SELECT group_id, sender, body, created_at, logical, reply_to, reply_sender, reply_preview, mentions FROM group_messages WHERE id = ?1", [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?)),
        ).optional()?;
        if let Some((old_group, sender, old_body, time, old_logical, old_reply, old_reply_sender, old_reply_preview, old_mentions)) = existing {
            let same = old_group == group_id && sender == peer && old_body == body && time == created_at
                && old_logical == logical && old_reply.as_deref() == reply_to
                && old_reply_sender.as_deref() == reply_sender && old_reply_preview.as_deref() == reply_preview
                && parse_mentions(&old_mentions) == unique_mentions;
            return Ok(if same { IncomingResult::Duplicate } else { IncomingResult::IdConflict });
        }
        let collision: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM local_messages WHERE id = ?1
                UNION ALL SELECT 1 FROM direct_messages WHERE id = ?1)", [id], |row| row.get(0),
        )?;
        if collision { return Ok(IncomingResult::IdConflict); }
        if tx.execute("UPDATE logical_clock SET value = max(value, ?1) + 1 WHERE singleton = 1 AND value < 9223372036854775807", [logical])? != 1 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        tx.execute(
            "INSERT INTO group_messages(id, group_id, sender, body, created_at, logical, reply_to, reply_sender, reply_preview, mentions)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![id, group_id, peer, body, created_at, logical, reply_to, reply_sender, reply_preview,
                serde_json::to_string(&unique_mentions).unwrap_or_else(|_| "[]".to_owned())])?;
        tx.execute(
            "UPDATE group_members SET hidden = 0, unread = unread + 1 WHERE group_id = ?1 AND member = ?2",
            params![group_id, self_id],
        )?;
        tx.commit()?;
        Ok(IncomingResult::Inserted)
    }

    /// Validates an inbound group file offer through the normal group membership path,
    /// then either creates the file record or resumes an existing partial transfer.
    #[allow(clippy::too_many_arguments)]
    pub fn receive_group_file(&self, self_id: &str, peer: &str, group_id: &str, name: &str, owner: &str,
        members: &[String], id: &str, file_name: &str, size: u64, sha256: &str, created_at: i64, logical: i64, now: i64)
        -> Result<GroupFileDecision> {
        self.receive_group_file_with_reply(self_id, peer, group_id, name, owner, members, id, file_name,
            size, sha256, created_at, logical, None, None, None, now)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn receive_group_file_with_reply(&self, self_id: &str, peer: &str, group_id: &str, name: &str, owner: &str,
        members: &[String], id: &str, file_name: &str, size: u64, sha256: &str, created_at: i64, logical: i64,
        reply_to: Option<&str>, reply_sender: Option<&str>, reply_preview: Option<&str>, now: i64)
        -> Result<GroupFileDecision> {
        let clean = match crate::files::sanitize_name(file_name) { Some(clean) => clean, None => return Ok(GroupFileDecision::Conflict) };
        if !valid_reply_metadata(reply_to, reply_sender, reply_preview) || (reply_to.is_some() && !is_image_file(&clean)) {
            return Ok(GroupFileDecision::Conflict);
        }
        if !valid_fingerprint(self_id) || !valid_fingerprint(peer) || !valid_group_id(group_id)
            || size == 0 || size > crate::protocol::MAX_FILE_BYTES || !valid_sha256(sha256)
            || logical <= 0 || logical == i64::MAX {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let size_i64 = i64::try_from(size).map_err(|_| rusqlite::Error::InvalidQuery)?;
        let known_group: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM groups WHERE id = ?1)", [group_id], |row| row.get(0),
        )?;
        if !known_group && !members.iter().any(|member| member == self_id) { return Ok(GroupFileDecision::NotMember); }
        let dir = self.incoming_dir_for(id, RecvKind::Group)?;
        let target = crate::files::incoming_target(&dir, id, &clean);
        let partial = crate::files::incoming_partial(&dir, id);
        // A partial download may already occupy space; count the full offer conservatively.
        if self.within_attachments(&dir) && self.storage_used().saturating_add(size) > self.storage_quota()? {
            return Ok(GroupFileDecision::NoSpace);
        }
        let result = self.receive_group(self_id, peer, group_id, name, owner, members, id, &clean, created_at, logical,
            reply_to, reply_sender, reply_preview, &[], now)?;
        if matches!(result, IncomingResult::NotMember) { return Ok(GroupFileDecision::NotMember); }
        if matches!(result, IncomingResult::Unverified) { return Ok(GroupFileDecision::Conflict); }
        if matches!(result, IncomingResult::Inserted) {
            let tx = self.connection.unchecked_transaction()?;
            tx.execute(
                "UPDATE group_messages SET kind = 'file', file_name = ?2, file_size = ?3, file_sha256 = ?4 WHERE id = ?1",
                params![id, clean, size_i64, sha256])?;
            tx.execute(
                "INSERT INTO group_file_transfers(message_id, group_id, peer, direction, source_path, target_path, total, transferred, status, updated_at)
                 VALUES (?1, ?2, ?3, 'incoming', NULL, ?4, ?5, 0, 'transferring', ?6)",
                params![id, group_id, peer, target.to_string_lossy(), size_i64, now])?;
            tx.commit()?;
            return Ok(GroupFileDecision::Ready { offset: 0, target, partial });
        }
        // Duplicate or a matching conflict: only resume when the stored metadata is identical.
        let stored: Option<(Option<i64>, Option<String>, i64, i64, Option<String>, Option<String>, Option<String>)> = self.connection.query_row(
            "SELECT file_size, file_sha256, created_at, logical, reply_to, reply_sender, reply_preview FROM group_messages WHERE id = ?1 AND group_id = ?2",
            params![id, group_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)),
        ).optional()?;
        let same = matches!(stored, Some((Some(old_size), Some(old_hash), old_created, old_logical, old_reply_to, old_reply_sender, old_reply_preview))
            if old_size == size_i64 && old_hash == sha256 && old_created == created_at && old_logical == logical
                && old_reply_to.as_deref() == reply_to && old_reply_sender.as_deref() == reply_sender
                && old_reply_preview.as_deref() == reply_preview);
        if !same { return Ok(GroupFileDecision::Conflict); }
        let state: Option<(String, i64)> = self.connection.query_row(
            "SELECT status, transferred FROM group_file_transfers WHERE message_id = ?1", [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional()?;
        let Some((status, _)) = state else { return Ok(GroupFileDecision::Conflict); };
        if status == "completed" { return Ok(GroupFileDecision::Complete); }
        let offset = match std::fs::metadata(&partial) {
            Ok(metadata) if metadata.is_file() && metadata.len() <= size => metadata.len(),
            Ok(_) => { crate::files::remove_quietly(&partial); 0 }
            Err(_) => 0,
        };
        self.connection.execute(
            "UPDATE group_file_transfers SET status = 'transferring', transferred = ?2, target_path = ?3, error = NULL, updated_at = ?4
             WHERE message_id = ?1",
            params![id, offset as i64, target.to_string_lossy(), now],
        )?;
        Ok(GroupFileDecision::Ready { offset, target, partial })
    }

    /// A shared failure keeps partial bytes so the next offer can resume.
    pub fn fail_group_file_transfer(&self, id: &str, error: &str, now: i64) -> Result<()> {
        if !valid_message_id(id) || error.is_empty() || error.len() > 200 { return Err(rusqlite::Error::InvalidQuery); }
        self.connection.execute(
            "UPDATE group_file_transfers SET status = 'failed', error = ?2, updated_at = ?3 WHERE message_id = ?1 AND COALESCE(error, '') != 'recalled'",
            params![id, error, now],
        )?;
        Ok(())
    }

    /// Transport hiccups pause the transfer without marking the message failed.
    pub fn pause_group_file_transfer(&self, id: &str, error: &str, now: i64) -> Result<()> {
        if !valid_message_id(id) || error.is_empty() || error.len() > 200 { return Err(rusqlite::Error::InvalidQuery); }
        self.connection.execute(
            "UPDATE group_file_transfers SET status = 'pending', error = ?2, updated_at = ?3 WHERE message_id = ?1 AND status IN ('pending', 'transferring')",
            params![id, error, now],
        )?;
        Ok(())
    }

    pub fn update_group_file_progress(&self, id: &str, transferred: u64, now: i64) -> Result<()> {
        if !valid_message_id(id) { return Err(rusqlite::Error::InvalidQuery); }
        let transferred = i64::try_from(transferred).map_err(|_| rusqlite::Error::InvalidQuery)?;
        self.connection.execute(
            "UPDATE group_file_transfers SET transferred = max(0, min(?2, total)), status = 'transferring', updated_at = ?3
             WHERE message_id = ?1 AND status IN ('pending', 'transferring')",
            params![id, transferred, now],
        )?;
        Ok(())
    }

    pub fn complete_group_file_transfer(&self, id: &str, now: i64) -> Result<bool> {
        if !valid_message_id(id) { return Err(rusqlite::Error::InvalidQuery); }
        Ok(self.connection.execute(
            "UPDATE group_file_transfers SET status = 'completed', transferred = total, error = NULL, updated_at = ?2
             WHERE message_id = ?1 AND NOT EXISTS(SELECT 1 FROM group_messages WHERE id = ?1 AND recalled_at IS NOT NULL)",
            params![id, now],
        )? == 1)
    }

    /// Local source path for an outgoing group file; never exposed to the UI.
    pub fn group_file_source(&self, id: &str) -> Result<Option<String>> {
        if !valid_message_id(id) { return Err(rusqlite::Error::InvalidQuery); }
        self.connection.query_row(
            "SELECT source_path FROM group_file_transfers WHERE message_id = ?1", [id],
            |row| row.get(0),
        ).optional()
    }

    pub fn group_file_transfer(&self, id: &str) -> Result<Option<GroupFileProgress>> {
        if !valid_message_id(id) { return Err(rusqlite::Error::InvalidQuery); }
        self.connection.query_row(
            "SELECT message_id, direction, total, transferred, status, error, target_path
             FROM group_file_transfers WHERE message_id = ?1", [id],
            |row| Ok(GroupFileProgress {
                message_id: row.get(0)?, direction: row.get(1)?, total: row.get(2)?, transferred: row.get(3)?,
                status: row.get(4)?, error: row.get(5)?, target_path: row.get(6)?,
            }),
        ).optional()
    }

    /// Local progress for one outgoing/incoming group file, used by the UI.
    pub fn group_file_progress(&self, group_id: &str, self_id: &str) -> Result<Vec<GroupFileProgress>> {
        if !valid_group_id(group_id) || !valid_fingerprint(self_id) { return Err(rusqlite::Error::InvalidQuery); }
        let mut statement = self.connection.prepare(
            "SELECT f.message_id, f.direction, f.total, f.transferred, f.status, f.error, f.target_path
             FROM group_file_transfers f
             JOIN group_messages m ON m.id = f.message_id AND m.group_id = ?1
             JOIN group_members gm ON gm.group_id = ?1 AND gm.member = ?2 AND gm.active = 1
             ORDER BY f.updated_at DESC LIMIT 200")?;
        let rows = statement.query_map(params![group_id, self_id], |row| Ok(GroupFileProgress {
            message_id: row.get(0)?, direction: row.get(1)?, total: row.get(2)?, transferred: row.get(3)?,
            status: row.get(4)?, error: row.get(5)?, target_path: row.get(6)?,
        }))?;
        rows.collect()
    }

    pub fn next_pending_control(&self, after: Option<&str>) -> Result<Option<GroupControlTask>> {
        if after.is_some_and(|target| !valid_fingerprint(target)) { return Err(rusqlite::Error::InvalidQuery); }
        self.connection.query_row(
            "SELECT c.id, c.group_id, c.target, a.address, c.kind, c.payload
             FROM group_control c
             JOIN contact_addresses a ON a.peer = c.target
             JOIN contacts t ON t.fingerprint = c.target AND t.trust_state = 'verified'
             WHERE c.status = 'pending' AND (?1 IS NULL OR c.target > ?1)
             ORDER BY c.target, c.created_at, c.id LIMIT 1", [after],
            |row| Ok(GroupControlTask { id: row.get(0)?, group_id: row.get(1)?, target: row.get(2)?, address: row.get(3)?, kind: row.get(4)?, payload: row.get(5)? }),
        ).optional()
    }

    pub fn ack_control(&self, id: &str) -> Result<bool> {
        if !valid_group_id(id) { return Err(rusqlite::Error::InvalidQuery); }
        Ok(self.connection.execute("UPDATE group_control SET status = 'delivered' WHERE id = ?1 AND status = 'pending'", [id])? == 1)
    }

    /// Reads the history of one conversation for an encrypted archive export.
    /// `since`/`until` bound `created_at` (0 = unbounded). Permissions mirror the
    /// UI: only verified direct peers and active groups.
    pub fn archive_export(&self, kind: &str, target: &str, self_id: &str, since: i64, until: i64) -> Result<ArchiveExport> {
        self.archive_export_with_options(kind, target, self_id, since, until, None, true, true, true)
    }

    /// Exports a full conversation or a selected message set with independent
    /// text, image, and other-file inclusion options.
    pub fn archive_export_with_options(&self, kind: &str, target: &str, self_id: &str, since: i64, until: i64,
        message_ids: Option<&[String]>, include_text: bool, include_images: bool, include_files: bool) -> Result<ArchiveExport> {
        if !valid_fingerprint(self_id) || since < 0 || until < 0 || (since > 0 && until > 0 && since > until)
            || message_ids.is_some_and(|ids| ids.len() > 50_000 || ids.iter().any(|id| !valid_message_id(id))) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let selected: Option<std::collections::HashSet<&str>> = message_ids.map(|ids| ids.iter().map(String::as_str).collect());
        if message_ids.is_some_and(|ids| selected.as_ref().is_some_and(|set| set.len() != ids.len())) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let mut found_selected = std::collections::HashSet::new();
        let include_kind = |message_kind: &str, file_name: Option<&str>| match message_kind {
            "text" => include_text,
            "file" if is_image_file(file_name.unwrap_or_default()) => include_images,
            "file" => include_files,
            _ => false,
        };
        let id_selected = |id: &str| selected.as_ref().is_none_or(|ids| ids.contains(id));
        fn time_clause(since: i64, until: i64) -> String {
            // Placeholders are numbered after ?1 (the peer/group id). Values bind as
            // strings; INTEGER affinity converts them back to numbers for comparison.
            let mut sql = String::new();
            let mut index = 2;
            if since > 0 { sql.push_str(&format!(" AND created_at >= ?{index}")); index += 1; }
            if until > 0 { sql.push_str(&format!(" AND created_at <= ?{index}")); }
            sql
        }
        fn time_params(since: i64, until: i64) -> Vec<String> {
            let mut params = Vec::new();
            if since > 0 { params.push(since.to_string()); }
            if until > 0 { params.push(until.to_string()); }
            params
        }
        fn named(kind: &str, target: &str, name: String, owner: Option<String>, messages: Vec<ArchiveMessage>,
            attachments: Vec<ArchiveAttachmentSource>) -> ArchiveExport {
            ArchiveExport { kind: kind.to_owned(), target: target.to_owned(), name, owner, messages, attachments }
        }
        fn collect_rows<I>(rows: I, id_selected: &dyn Fn(&str) -> bool, include_kind: &dyn Fn(&str, Option<&str>) -> bool,
            selected: Option<&std::collections::HashSet<&str>>, found_selected: &mut std::collections::HashSet<String>)
            -> Result<(Vec<ArchiveMessage>, Vec<ArchiveAttachmentSource>)>
        where I: IntoIterator<Item = rusqlite::Result<(ArchiveMessage, ArchiveAttachmentSource)>> {
            let mut messages = Vec::new();
            let mut attachments = Vec::new();
            for row in rows {
                let (message, attachment) = row?;
                if !id_selected(&message.id) { continue; }
                found_selected.insert(message.id.clone());
                if !include_kind(&message.kind, message.file_name.as_deref()) { continue; }
                if message.kind == "file" {
                    if attachment.status.as_deref() != Some("completed")
                        || attachment.transferred != attachment.total
                        || attachment.path.as_deref().is_none_or(str::is_empty)
                        || attachment.file_size != message.file_size
                        || attachment.file_sha256 != message.file_sha256 {
                        return Err(rusqlite::Error::InvalidQuery);
                    }
                    attachments.push(attachment);
                }
                if messages.len() >= 50_000 { return Err(rusqlite::Error::InvalidQuery); }
                messages.push(message);
            }
            if selected.is_some_and(|ids| ids.len() != found_selected.len()) {
                return Err(rusqlite::Error::InvalidQuery);
            }
            Ok((messages, attachments))
        }
        fn row_archive_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<(ArchiveMessage, ArchiveAttachmentSource)> {
            let id: String = row.get(0)?;
            let direction: Option<String> = row.get(9)?;
            let source_path: Option<String> = row.get(10)?;
            let target_path: Option<String> = row.get(11)?;
            let attachment = ArchiveAttachmentSource {
                message_id: id.clone(),
                path: match direction.as_deref() {
                    Some("outgoing") => source_path,
                    Some("incoming") => target_path,
                    _ => None,
                },
                direction,
                file_name: row.get(6)?,
                file_size: row.get(7)?,
                file_sha256: row.get(8)?,
                status: row.get(12)?,
                transferred: row.get(13)?,
                total: row.get(14)?,
            };
            Ok((ArchiveMessage {
                id, sender: row.get(1)?, body: row.get(2)?, created_at: row.get(3)?, logical: row.get(4)?,
                kind: row.get(5)?, file_name: row.get(6)?, file_size: row.get(7)?, file_sha256: row.get(8)?,
            }, attachment))
        }
        match kind {
            "self" => {
                let sql = format!(
                    "SELECT id, sender, body, created_at, logical, 'text', NULL, NULL, NULL,
                            NULL, NULL, NULL, NULL, NULL, NULL FROM local_messages
                     WHERE conversation_id = 'self' AND recalled_at IS NULL{} ORDER BY logical, sender, id LIMIT 50000", time_clause(since, until));
                let mut statement = self.connection.prepare(&sql)?;
                let rows = statement.query_map(rusqlite::params_from_iter(time_params(since, until)), row_archive_message)?;
                let (messages, attachments) = collect_rows(rows, &id_selected, &include_kind, selected.as_ref(), &mut found_selected)?;
                Ok(named("self", "self", "自己".into(), None, messages, attachments))
            }
            "direct" => {
                if !valid_fingerprint(target) { return Err(rusqlite::Error::InvalidQuery); }
                let verified: bool = self.connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM contacts WHERE fingerprint = ?1 AND trust_state = 'verified')",
                    [target], |row| row.get(0))?;
                if !verified { return Err(rusqlite::Error::InvalidQuery); }
                let name: String = self.connection.query_row(
                    "SELECT COALESCE(NULLIF(trim(COALESCE(remark, '')), ''), display_name) FROM contacts WHERE fingerprint = ?1",
                    [target], |row| row.get(0))?;
                let sql = format!(
                    "SELECT m.id, m.sender, m.body, m.created_at, m.logical, COALESCE(m.kind, 'text'),
                            m.file_name, m.file_size, m.file_sha256, t.direction, t.source_path, t.target_path,
                            t.status, t.transferred, t.total
                     FROM direct_messages m LEFT JOIN file_transfers t ON t.message_id = m.id
                     WHERE m.peer = ?1{} ORDER BY m.logical, m.sender, m.id", time_clause(since, until));
                let mut statement = self.connection.prepare(&sql)?;
                let bound = [target.to_string()].into_iter().chain(time_params(since, until)).collect::<Vec<_>>();
                let rows = statement.query_map(rusqlite::params_from_iter(bound.iter()), row_archive_message)?;
                let (messages, attachments) = collect_rows(rows, &id_selected, &include_kind, selected.as_ref(), &mut found_selected)?;
                Ok(named("direct", target, name, None, messages, attachments))
            }
            "group" => {
                if !valid_group_id(target) { return Err(rusqlite::Error::InvalidQuery); }
                let info: Option<(String, String)> = self.connection.query_row(
                    "SELECT g.name, g.owner FROM groups g JOIN group_members m ON m.group_id = g.id
                     WHERE g.id = ?1 AND m.member = ?2 AND m.active = 1", params![target, self_id],
                    |row| Ok((row.get(0)?, row.get(1)?))).optional()?;
                let Some((name, owner)) = info else { return Err(rusqlite::Error::InvalidQuery); };
                let sql = format!(
                    "SELECT m.id, m.sender, m.body, m.created_at, m.logical, COALESCE(m.kind, 'text'),
                            m.file_name, m.file_size, m.file_sha256, t.direction, t.source_path, t.target_path,
                            t.status, t.transferred, t.total
                     FROM group_messages m LEFT JOIN group_file_transfers t ON t.message_id = m.id
                     WHERE m.group_id = ?1{} ORDER BY m.logical, m.sender, m.id", time_clause(since, until));
                let mut statement = self.connection.prepare(&sql)?;
                let bound = [target.to_string()].into_iter().chain(time_params(since, until)).collect::<Vec<_>>();
                let rows = statement.query_map(rusqlite::params_from_iter(bound.iter()), row_archive_message)?;
                let (messages, attachments) = collect_rows(rows, &id_selected, &include_kind, selected.as_ref(), &mut found_selected)?;
                Ok(named("group", target, name, Some(owner), messages, attachments))
            }
            _ => Err(rusqlite::Error::InvalidQuery),
        }
    }

    /// Conversations available for archive export: self chat, verified direct
    /// contacts and active groups; the wizard uses this to render the picker.
    pub fn list_export_targets(&self, self_id: &str) -> Result<Vec<(String, String, String)>> {
        if !valid_fingerprint(self_id) { return Err(rusqlite::Error::InvalidQuery); }
        let mut targets = Vec::new();
        targets.push(("self".to_owned(), "self".to_owned(), "自己".to_owned()));
        let mut statement = self.connection.prepare(
            "SELECT fingerprint, COALESCE(NULLIF(trim(COALESCE(remark, '')), ''), display_name) FROM contacts
             WHERE trust_state = 'verified' ORDER BY display_name COLLATE NOCASE")?;
        let rows = statement.query_map([], |row| {
            Ok(("direct".to_owned(), row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows { targets.push(row?); }
        let mut statement = self.connection.prepare(
            "SELECT g.id, g.name FROM groups g JOIN group_members m ON m.group_id = g.id
             WHERE m.member = ?1 AND m.active = 1 ORDER BY g.name COLLATE NOCASE")?;
        let rows = statement.query_map([self_id], |row| {
            Ok(("group".to_owned(), row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows { targets.push(row?); }
        Ok(targets)
    }


    /// Imports a legacy text-only archive. Format-2 archives use the attachment-aware entry point.
    pub fn archive_import(&self, kind: &str, target: &str, self_id: &str, messages: &[ArchiveMessage], now: i64) -> Result<ArchiveImportReport> {
        self.archive_import_with_attachments(kind, target, self_id, messages, &[], now)
    }

    /// Merges a chat archive without overwriting conflicting message ids. File bytes are
    /// verified before any message is applied and stored under the managed attachment directory.
    pub fn archive_import_with_attachments(&self, kind: &str, target: &str, self_id: &str, messages: &[ArchiveMessage],
        attachment_data: &[(String, Vec<u8>)], now: i64) -> Result<ArchiveImportReport> {
        if !valid_fingerprint(self_id) || messages.len() > 50_000 || attachment_data.len() > 50_000 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let attachments: std::collections::HashMap<&str, &[u8]> = attachment_data.iter()
            .map(|(id, bytes)| (id.as_str(), bytes.as_slice())).collect();
        if attachments.len() != attachment_data.len() { return Err(rusqlite::Error::InvalidQuery); }
        let mut seen_messages = std::collections::HashSet::new();
        let mut expected_attachments = std::collections::HashSet::new();
        let mut total_attachment_bytes = 0u64;
        for message in messages {
            if !valid_message_id(&message.id) || !valid_fingerprint(&message.sender)
                || message.body.is_empty() || message.body.len() > 32768 || message.logical <= 0
                || !seen_messages.insert(message.id.as_str()) {
                return Err(rusqlite::Error::InvalidQuery);
            }
            match message.kind.as_str() {
                "text" if message.file_name.is_none() && message.file_size.is_none() && message.file_sha256.is_none() => {
                    if attachments.contains_key(message.id.as_str()) { return Err(rusqlite::Error::InvalidQuery); }
                }
                "file" if kind != "self" => {
                    let name = message.file_name.as_deref().ok_or(rusqlite::Error::InvalidQuery)?;
                    if crate::files::sanitize_name(name).as_deref() != Some(name) { return Err(rusqlite::Error::InvalidQuery); }
                    let size = message.file_size.filter(|size| *size > 0 && *size as u64 <= crate::protocol::MAX_FILE_BYTES)
                        .ok_or(rusqlite::Error::InvalidQuery)? as u64;
                    let hash = message.file_sha256.as_deref().filter(|hash| valid_sha256(hash))
                        .ok_or(rusqlite::Error::InvalidQuery)?;
                    let bytes = attachments.get(message.id.as_str()).ok_or(rusqlite::Error::InvalidQuery)?;
                    if bytes.len() as u64 != size || !archive_sha256_matches(bytes, hash) { return Err(rusqlite::Error::InvalidQuery); }
                    total_attachment_bytes = total_attachment_bytes.saturating_add(size);
                    if total_attachment_bytes > 512 * 1024 * 1024 { return Err(rusqlite::Error::InvalidQuery); }
                    expected_attachments.insert(message.id.as_str());
                }
                _ => return Err(rusqlite::Error::InvalidQuery),
            }
        }
        if expected_attachments.len() != attachments.len() || attachments.keys().any(|id| !expected_attachments.contains(id)) {
            return Err(rusqlite::Error::InvalidQuery);
        }

        let mut created_files = Vec::new();
        let outcome = (|| -> Result<ArchiveImportReport> {
            let tx = self.connection.unchecked_transaction()?;
            let mut report = ArchiveImportReport {
                kind: kind.to_owned(), name: String::new(), inserted: 0, skipped: 0, conflicts: 0, restored_attachments: 0,
            };
            let mut max_logical: i64 = 0;
            match kind {
                "self" => {
                    let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM conversations WHERE id = 'self')", [], |row| row.get(0))?;
                    if !exists { return Err(rusqlite::Error::InvalidQuery); }
                    report.name = "自己".into();
                    for message in messages {
                        match existing_text(&tx, "local_messages", &message.id)? {
                            Some(existing) if existing == (message.body.clone(), message.sender.clone(), message.logical) => {
                                report.skipped += 1; continue;
                            }
                            Some(_) => { report.conflicts += 1; continue; }
                            None => {}
                        }
                        tx.execute(
                            "INSERT INTO local_messages(id, conversation_id, sender, body, created_at, logical)
                             VALUES (?1, 'self', ?2, ?3, ?4, ?5)",
                            params![message.id, message.sender, message.body, message.created_at, message.logical])?;
                        report.inserted += 1;
                        max_logical = max_logical.max(message.logical);
                    }
                }
                "direct" => {
                    if !valid_fingerprint(target) { return Err(rusqlite::Error::InvalidQuery); }
                    let verified: bool = tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM contacts WHERE fingerprint = ?1 AND trust_state = 'verified')",
                        [target], |row| row.get(0))?;
                    if !verified { return Err(rusqlite::Error::InvalidQuery); }
                    report.name = tx.query_row(
                        "SELECT COALESCE(NULLIF(trim(COALESCE(remark, '')), ''), display_name) FROM contacts WHERE fingerprint = ?1",
                        [target], |row| row.get(0))?;
                    tx.execute("INSERT INTO direct_conversations(peer, hidden) VALUES (?1, 0) ON CONFLICT(peer) DO NOTHING", [target])?;
                    for message in messages {
                        let existing = existing_archive_message(&tx, "direct_messages", &message.id)?;
                        if let Some(old) = existing {
                            let same = old.0 == message.body && old.1 == message.sender && old.2 == message.logical
                                && old.3 == message.kind && old.4 == message.file_name && old.5 == message.file_size && old.6 == message.file_sha256;
                            if !same { report.conflicts += 1; continue; }
                            if message.kind == "file" && !self.archive_attachment_is_valid(&tx, "direct", &message.id,
                                message.file_size.unwrap_or_default(), message.file_sha256.as_deref().unwrap_or_default())? {
                                let path = self.persist_archive_attachment(&message.id, message.file_name.as_deref().unwrap_or("file"),
                                    attachments[message.id.as_str()], &mut created_files)?;
                                self.upsert_archive_transfer(&tx, "direct", target, message, &path, self_id, now)?;
                                report.restored_attachments += 1;
                            }
                            report.skipped += 1;
                            continue;
                        }
                        let status = if message.sender == self_id { "delivered" } else { "received" };
                        if message.kind == "file" {
                            let path = self.persist_archive_attachment(&message.id, message.file_name.as_deref().unwrap_or("file"),
                                attachments[message.id.as_str()], &mut created_files)?;
                            tx.execute(
                                "INSERT INTO direct_messages(id, peer, sender, body, created_at, logical, status, kind, file_name, file_size, file_sha256)
                                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'file', ?8, ?9, ?10)",
                                params![message.id, target, message.sender, message.body, message.created_at, message.logical, status,
                                    message.file_name, message.file_size, message.file_sha256])?;
                            self.upsert_archive_transfer(&tx, "direct", target, message, &path, self_id, now)?;
                            report.restored_attachments += 1;
                        } else {
                            tx.execute(
                                "INSERT INTO direct_messages(id, peer, sender, body, created_at, logical, status, kind)
                                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'text')",
                                params![message.id, target, message.sender, message.body, message.created_at, message.logical, status])?;
                        }
                        report.inserted += 1;
                        max_logical = max_logical.max(message.logical);
                    }
                }
                "group" => {
                    if !valid_group_id(target) { return Err(rusqlite::Error::InvalidQuery); }
                    let info: Option<String> = tx.query_row(
                        "SELECT g.name FROM groups g JOIN group_members m ON m.group_id = g.id
                         WHERE g.id = ?1 AND m.member = ?2 AND m.active = 1", params![target, self_id], |row| row.get(0)).optional()?;
                    let Some(name) = info else { return Err(rusqlite::Error::InvalidQuery); };
                    report.name = name;
                    for message in messages {
                        let existing = existing_archive_message(&tx, "group_messages", &message.id)?;
                        if let Some(old) = existing {
                            let same = old.0 == message.body && old.1 == message.sender && old.2 == message.logical
                                && old.3 == message.kind && old.4 == message.file_name && old.5 == message.file_size && old.6 == message.file_sha256;
                            if !same { report.conflicts += 1; continue; }
                            if message.kind == "file" && !self.archive_attachment_is_valid(&tx, "group", &message.id,
                                message.file_size.unwrap_or_default(), message.file_sha256.as_deref().unwrap_or_default())? {
                                let path = self.persist_archive_attachment(&message.id, message.file_name.as_deref().unwrap_or("file"),
                                    attachments[message.id.as_str()], &mut created_files)?;
                                self.upsert_archive_transfer(&tx, "group", target, message, &path, self_id, now)?;
                                report.restored_attachments += 1;
                            }
                            report.skipped += 1;
                            continue;
                        }
                        if message.kind == "file" {
                            let path = self.persist_archive_attachment(&message.id, message.file_name.as_deref().unwrap_or("file"),
                                attachments[message.id.as_str()], &mut created_files)?;
                            tx.execute(
                                "INSERT INTO group_messages(id, group_id, sender, body, created_at, logical, mentions, kind, file_name, file_size, file_sha256)
                                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, '[]', 'file', ?7, ?8, ?9)",
                                params![message.id, target, message.sender, message.body, message.created_at, message.logical,
                                    message.file_name, message.file_size, message.file_sha256])?;
                            self.upsert_archive_transfer(&tx, "group", target, message, &path, self_id, now)?;
                            report.restored_attachments += 1;
                        } else {
                            tx.execute(
                                "INSERT INTO group_messages(id, group_id, sender, body, created_at, logical, mentions, kind)
                                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, '[]', 'text')",
                                params![message.id, target, message.sender, message.body, message.created_at, message.logical])?;
                        }
                        report.inserted += 1;
                        max_logical = max_logical.max(message.logical);
                    }
                    tx.execute("UPDATE group_members SET hidden = 0 WHERE group_id = ?1 AND member = ?2", params![target, self_id])?;
                }
                _ => return Err(rusqlite::Error::InvalidQuery),
            }
            if max_logical > 0 {
                tx.execute("UPDATE logical_clock SET value = ?2 WHERE singleton = 1 AND value < ?2", params![now.min(0), max_logical])?;
            }
            tx.commit()?;
            Ok(report)
        })();
        if outcome.is_err() {
            for path in created_files { let _ = std::fs::remove_file(path); }
        }
        outcome
    }

    fn archive_attachment_is_valid(&self, tx: &rusqlite::Transaction<'_>, kind: &str, id: &str, size: i64, hash: &str) -> Result<bool> {
        let transfer: Option<(String, Option<String>, Option<String>, String, i64, i64)> = match kind {
            "direct" => tx.query_row("SELECT direction, source_path, target_path, status, transferred, total FROM file_transfers WHERE message_id = ?1",
                [id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))).optional()?,
            "group" => tx.query_row("SELECT direction, source_path, target_path, status, transferred, total FROM group_file_transfers WHERE message_id = ?1",
                [id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))).optional()?,
            _ => return Ok(false),
        };
        let Some((direction, source, target, status, transferred, total)) = transfer else { return Ok(false); };
        if status != "completed" || transferred != total || total != size { return Ok(false); }
        let path = match direction.as_str() { "outgoing" => source, "incoming" => target, _ => None };
        let Some(path) = path else { return Ok(false); };
        let Ok(data) = std::fs::read(path) else { return Ok(false); };
        Ok(data.len() as i64 == size && archive_sha256_matches(&data, hash))
    }

    fn persist_archive_attachment(&self, id: &str, name: &str, data: &[u8], created: &mut Vec<PathBuf>) -> Result<String> {
        use std::io::Write;
        let clean = crate::files::sanitize_name(name).ok_or(rusqlite::Error::InvalidQuery)?;
        let directory = self.attachments_dir().join("chat-archives");
        std::fs::create_dir_all(&directory).map_err(|_| rusqlite::Error::InvalidQuery)?;
        for suffix in 0..1000u32 {
            let path = directory.join(format!("{id}-{suffix}-{clean}"));
            let file = std::fs::OpenOptions::new().write(true).create_new(true).open(&path);
            match file {
                Ok(mut file) => {
                    created.push(path.clone());
                    file.write_all(data).map_err(|_| rusqlite::Error::InvalidQuery)?;
                    return Ok(path.to_string_lossy().into_owned());
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(rusqlite::Error::InvalidQuery),
            }
        }
        Err(rusqlite::Error::InvalidQuery)
    }

    fn upsert_archive_transfer(&self, tx: &rusqlite::Transaction<'_>, kind: &str, conversation_id: &str,
        message: &ArchiveMessage, path: &str, self_id: &str, now: i64) -> Result<()> {
        let size = message.file_size.ok_or(rusqlite::Error::InvalidQuery)?;
        if kind == "direct" {
            let direction = if message.sender == self_id { "outgoing" } else { "incoming" };
            tx.execute(
                "INSERT INTO file_transfers(message_id, peer, direction, source_path, target_path, total, transferred, status, error, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, 'completed', NULL, ?7)
                 ON CONFLICT(message_id) DO UPDATE SET peer=excluded.peer, direction=excluded.direction,
                    source_path=excluded.source_path, target_path=excluded.target_path, total=excluded.total,
                    transferred=excluded.transferred, status='completed', error=NULL, updated_at=excluded.updated_at",
                params![message.id, conversation_id, direction, if direction == "outgoing" { Some(path) } else { None },
                    if direction == "incoming" { Some(path) } else { None }, size, now])?;
        } else if kind == "group" {
            let direction = if message.sender == self_id { "outgoing" } else { "incoming" };
            tx.execute(
                "INSERT INTO group_file_transfers(message_id, group_id, peer, direction, source_path, target_path, total, transferred, status, error, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, 'completed', NULL, ?8)
                 ON CONFLICT(message_id) DO UPDATE SET group_id=excluded.group_id, peer=excluded.peer, direction=excluded.direction,
                    source_path=excluded.source_path, target_path=excluded.target_path, total=excluded.total,
                    transferred=excluded.transferred, status='completed', error=NULL, updated_at=excluded.updated_at",
                params![message.id, conversation_id, message.sender, direction, if direction == "outgoing" { Some(path) } else { None },
                    if direction == "incoming" { Some(path) } else { None }, size, now])?;
        } else { return Err(rusqlite::Error::InvalidQuery); }
        Ok(())
    }

    pub fn register_plugin(&self, id: &str, name: &str, version: &str, now: i64) -> Result<()> {
        self.connection.execute(
            "INSERT INTO plugins(id, name, version, enabled, installed_at) VALUES (?1, ?2, ?3, 1, ?4)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, version = excluded.version,
                enabled = 1, installed_at = excluded.installed_at",
            params![id, name, version, now])?;
        Ok(())
    }

    pub fn list_plugins(&self) -> Result<Vec<PluginRow>> {
        let mut statement = self.connection.prepare(
            "SELECT id, name, version, enabled FROM plugins ORDER BY name, id")?;
        let rows = statement.query_map([], |row| Ok(PluginRow {
            id: row.get(0)?, name: row.get(1)?, version: row.get(2)?, enabled: row.get::<_, i64>(3)? == 1,
        }))?;
        rows.collect()
    }

    pub fn set_plugin_enabled(&self, id: &str, enabled: bool) -> Result<bool> {
        Ok(self.connection.execute(
            "UPDATE plugins SET enabled = ?2 WHERE id = ?1",
            params![id, if enabled { 1 } else { 0 }])? > 0)
    }

    pub fn remove_plugin_row(&self, id: &str) -> Result<bool> {
        Ok(self.connection.execute("DELETE FROM plugins WHERE id = ?1", [id])? > 0)
    }

    pub fn is_plugin_enabled(&self, id: &str) -> Result<bool> {
        Ok(self.connection.query_row(
            "SELECT enabled FROM plugins WHERE id = ?1", [id], |row| row.get::<_, i64>(0)).optional()?.unwrap_or(0) == 1)
    }

    /// Creates a fresh 32-character batch id for one bubble of images.
    pub fn new_batch_id(&self) -> Result<String> {
        self.connection.query_row("SELECT lower(hex(randomblob(16)))", [], |row| row.get(0))
    }

    /// Marks one image message as part of a batch. Called for both sent and received
    /// offers, so a retransmit simply rewrites the same row.
    pub fn attach_image_batch(&self, message_id: &str, batch_id: &str, caption: &str, now: i64) -> Result<bool> {
        if !valid_message_id(message_id) || batch_id.len() != 32
            || !batch_id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || caption.chars().count() > crate::protocol::MAX_BATCH_CAPTION
            || caption.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let exists: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM direct_messages WHERE id = ?1 UNION ALL SELECT 1 FROM group_messages WHERE id = ?1)",
            [message_id], |row| row.get(0))?;
        if !exists { return Ok(false); }
        self.connection.execute(
            "INSERT INTO image_batches(message_id, batch_id, caption, created_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(message_id) DO UPDATE SET batch_id = excluded.batch_id, caption = excluded.caption",
            params![message_id, batch_id, caption, now])?;
        Ok(true)
    }

    /// Batch id and caption for one image message, if it belongs to a shared bubble.
    pub fn image_batch_of(&self, message_id: &str) -> Result<Option<(String, String)>> {
        self.connection.query_row(
            "SELECT batch_id, caption FROM image_batches WHERE message_id = ?1", [message_id],
            |row| Ok((row.get(0)?, row.get(1)?))).optional()
    }

    /// Batches for one conversation, bounded by the newest 2,000 images.
    pub fn list_image_batches(&self, kind: &str, target: &str) -> Result<Vec<ImageBatch>> {
        let sql = match kind {
            "direct" => "SELECT b.message_id, b.batch_id, b.caption,
                         COALESCE((SELECT b0.message_id FROM image_batches b0
                             JOIN direct_messages m0 ON m0.id = b0.message_id
                             WHERE b0.batch_id = b.batch_id AND m0.peer = m.peer
                             ORDER BY m0.logical, m0.sender, m0.id LIMIT 1), b.message_id)
                         FROM image_batches b JOIN direct_messages m ON m.id = b.message_id WHERE m.peer = ?1
                         ORDER BY b.created_at DESC LIMIT 2000",
            "group" => "SELECT b.message_id, b.batch_id, b.caption,
                        COALESCE((SELECT b0.message_id FROM image_batches b0
                            JOIN group_messages m0 ON m0.id = b0.message_id
                            WHERE b0.batch_id = b.batch_id AND m0.group_id = m.group_id
                            ORDER BY m0.logical, m0.sender, m0.id LIMIT 1), b.message_id)
                        FROM image_batches b JOIN group_messages m ON m.id = b.message_id WHERE m.group_id = ?1
                        ORDER BY b.created_at DESC LIMIT 2000",
            _ => return Err(rusqlite::Error::InvalidQuery),
        };
        if kind == "group" && !valid_group_id(target) { return Err(rusqlite::Error::InvalidQuery); }
        if kind == "direct" && !valid_fingerprint(target) { return Err(rusqlite::Error::InvalidQuery); }
        let mut statement = self.connection.prepare(sql)?;
        let rows = statement.query_map([target], |row| Ok(ImageBatch {
            message_id: row.get(0)?, batch_id: row.get(1)?, caption: row.get(2)?, anchor_id: row.get(3)?,
        }))?;
        rows.collect()
    }

    /// Owner-only; `None` removes the avatar. The bytes must be a bounded square PNG.
    pub fn set_group_avatar(&self, actor: &str, group_id: &str, bytes: Option<&[u8]>, now: i64) -> Result<bool> {
        if !valid_fingerprint(actor) || !valid_group_id(group_id) { return Err(rusqlite::Error::InvalidQuery); }
        let owner: Option<String> = self.connection.query_row(
            "SELECT owner FROM groups WHERE id = ?1", [group_id], |row| row.get(0)).optional()?;
        if owner.as_deref() != Some(actor) { return Ok(false); }
        match bytes {
            None => { self.connection.execute("DELETE FROM group_avatars WHERE group_id = ?1", [group_id])?; }
            Some(bytes) => {
                if bytes.len() > crate::protocol::MAX_GROUP_AVATAR_BYTES || crate::avatar::validate(bytes).is_err() {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                self.connection.execute(
                    "INSERT INTO group_avatars(group_id, png, updated_at) VALUES (?1, ?2, ?3)
                     ON CONFLICT(group_id) DO UPDATE SET png = excluded.png, updated_at = excluded.updated_at",
                    params![group_id, bytes, now])?;
            }
        }
        Ok(true)
    }

    pub fn group_avatar(&self, group_id: &str) -> Result<Option<Vec<u8>>> {
        if !valid_group_id(group_id) { return Err(rusqlite::Error::InvalidQuery); }
        self.connection.query_row("SELECT png FROM group_avatars WHERE group_id = ?1", [group_id], |row| row.get(0)).optional()
    }

    /// Only the owner may define the avatar, and only for members still in the group.
    pub fn receive_group_avatar(&self, self_id: &str, peer: &str, group_id: &str, png: Option<&[u8]>, now: i64) -> Result<IncomingResult> {
        if !valid_fingerprint(self_id) || !valid_fingerprint(peer) || !valid_group_id(group_id) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let tx = self.connection.unchecked_transaction()?;
        let verified: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM contacts WHERE fingerprint = ?1 AND trust_state = 'verified')",
            [peer], |row| row.get(0))?;
        if !verified { return Ok(IncomingResult::Unverified); }
        let owner: Option<String> = tx.query_row("SELECT owner FROM groups WHERE id = ?1", [group_id], |row| row.get(0)).optional()?;
        if owner.as_deref() != Some(peer) { return Ok(IncomingResult::Unverified); }
        let member: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM group_members WHERE group_id = ?1 AND member = ?2 AND active = 1)",
            params![group_id, self_id], |row| row.get(0))?;
        if !member { return Ok(IncomingResult::NotMember); }
        match png {
            None => { tx.execute("DELETE FROM group_avatars WHERE group_id = ?1", [group_id])?; }
            Some(bytes) => {
                if bytes.len() > crate::protocol::MAX_GROUP_AVATAR_BYTES || crate::avatar::validate(bytes).is_err() {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                tx.execute(
                    "INSERT INTO group_avatars(group_id, png, updated_at) VALUES (?1, ?2, ?3)
                     ON CONFLICT(group_id) DO UPDATE SET png = excluded.png, updated_at = excluded.updated_at",
                    params![group_id, bytes, now])?;
            }
        }
        tx.commit()?;
        Ok(IncomingResult::Inserted)
    }

    /// Groups this device belongs to whose name matches `query`; reported on request.
    pub fn search_known_groups(&self, self_id: &str, query: &str) -> Result<Vec<crate::protocol::GroupDigest>> {
        if !valid_fingerprint(self_id) || query.trim().is_empty() || query.len() > crate::protocol::MAX_SEARCH_QUERY {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let mut statement = self.connection.prepare(
            "SELECT g.id, g.name, g.owner,
                    (SELECT COUNT(*) FROM group_members m2 WHERE m2.group_id = g.id AND m2.active = 1)
             FROM groups g JOIN group_members m ON m.group_id = g.id AND m.member = ?1 AND m.active = 1
             WHERE g.name LIKE ?2 ESCAPE '\\'
             ORDER BY g.created_at DESC LIMIT ?3")?;
        let rows = statement.query_map(params![self_id, like_pattern(query), crate::protocol::MAX_SEARCH_RESULTS as i64], |row| {
            Ok(crate::protocol::GroupDigest {
                id: row.get(0)?, name: row.get(1)?, owner: row.get(2)?, member_count: row.get::<_, i64>(3)? as u32,
            })
        })?;
        rows.collect()
    }

    /// Records an inbound join application; only the group owner may hold one.
    pub fn record_join_request(&self, self_id: &str, applicant: &str, group_id: &str, name: &str, now: i64) -> Result<bool> {
        if !valid_fingerprint(self_id) || !valid_fingerprint(applicant) || !valid_group_id(group_id)
            || name.is_empty() || name.len() > 128 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let tx = self.connection.unchecked_transaction()?;
        let owner: Option<String> = tx.query_row("SELECT owner FROM groups WHERE id = ?1", [group_id], |row| row.get(0)).optional()?;
        if owner.as_deref() != Some(self_id) || applicant == self_id { return Ok(false); }
        let verified: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM contacts WHERE fingerprint = ?1 AND trust_state = 'verified')",
            [applicant], |row| row.get(0))?;
        if !verified { return Ok(false); }
        let member: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM group_members WHERE group_id = ?1 AND member = ?2 AND active = 1)",
            params![group_id, applicant], |row| row.get(0))?;
        if member { return Ok(false); }
        let id: String = tx.query_row("SELECT lower(hex(randomblob(16)))", [], |row| row.get(0))?;
        tx.execute(
            "INSERT INTO group_join_requests(id, group_id, group_name, owner, applicant, created_at, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending')
             ON CONFLICT(group_id, applicant) DO UPDATE SET created_at = excluded.created_at, status = 'pending'",
            params![id, group_id, name, self_id, applicant, now])?;
        tx.commit()?;
        Ok(true)
    }

    /// Queues an outbound application and remembers it locally; the request is not a member yet.
    pub fn queue_join_request(&self, self_id: &str, group_id: &str, name: &str, owner: &str, now: i64) -> Result<JoinRequest> {
        if !valid_fingerprint(self_id) || !valid_group_id(group_id) || !valid_fingerprint(owner) || owner == self_id
            || name.trim().is_empty() || name.len() > 128 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let tx = self.connection.unchecked_transaction()?;
        let verified: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM contacts WHERE fingerprint = ?1 AND trust_state = 'verified')",
            [owner], |row| row.get(0))?;
        if !verified { return Err(rusqlite::Error::InvalidQuery); }
        let member: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM group_members WHERE group_id = ?1 AND member = ?2 AND active = 1)",
            params![group_id, self_id], |row| row.get(0))?;
        if member { return Err(rusqlite::Error::InvalidQuery); }
        let id: String = tx.query_row("SELECT lower(hex(randomblob(16)))", [], |row| row.get(0))?;
        tx.execute(
            "INSERT INTO group_join_requests(id, group_id, group_name, owner, applicant, created_at, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending')
             ON CONFLICT(group_id, applicant) DO UPDATE SET created_at = excluded.created_at, status = 'pending'",
            params![id, group_id, name.trim(), owner, self_id, now])?;
        tx.execute(
            "INSERT INTO group_control(id, group_id, target, kind, payload, status, created_at)
             VALUES (?1, ?2, ?3, 'join', ?4, 'pending', ?5)",
            params![id, group_id, owner, serde_json::json!({ "name": name.trim() }).to_string(), now])?;
        tx.commit()?;
        Ok(JoinRequest {
            id, group_id: group_id.into(), group_name: name.trim().into(), owner: owner.into(),
            applicant: self_id.into(), applicant_name: String::new(), created_at: now, status: "pending".into(),
        })
    }

    pub fn list_join_requests(&self, self_id: &str, group_id: Option<&str>) -> Result<Vec<JoinRequest>> {
        if !valid_fingerprint(self_id) { return Err(rusqlite::Error::InvalidQuery); }
        let mut statement = self.connection.prepare(
            "SELECT r.id, r.group_id, r.group_name, r.owner, r.applicant, COALESCE(c.remark, c.display_name, ''), r.created_at, r.status
             FROM group_join_requests r
             LEFT JOIN contacts c ON c.fingerprint = r.applicant
             WHERE r.owner = ?1 AND r.applicant != ?1 AND r.status = 'pending'
               AND (?2 IS NULL OR r.group_id = ?2)
             ORDER BY r.created_at DESC LIMIT 200")?;
        let rows = statement.query_map(params![self_id, group_id], |row| Ok(JoinRequest {
            id: row.get(0)?, group_id: row.get(1)?, group_name: row.get(2)?, owner: row.get(3)?,
            applicant: row.get(4)?, applicant_name: row.get(5)?, created_at: row.get(6)?, status: row.get(7)?,
        }))?;
        rows.collect()
    }

    pub fn list_out_join_requests(&self, self_id: &str) -> Result<Vec<JoinRequest>> {
        if !valid_fingerprint(self_id) { return Err(rusqlite::Error::InvalidQuery); }
        let mut statement = self.connection.prepare(
            "SELECT id, group_id, group_name, owner, applicant, '', created_at, status
             FROM group_join_requests WHERE applicant = ?1 AND status = 'pending'
             ORDER BY created_at DESC LIMIT 200")?;
        let rows = statement.query_map([self_id], |row| Ok(JoinRequest {
            id: row.get(0)?, group_id: row.get(1)?, group_name: row.get(2)?, owner: row.get(3)?,
            applicant: row.get(4)?, applicant_name: row.get(5)?, created_at: row.get(6)?, status: row.get(7)?,
        }))?;
        rows.collect()
    }

    /// Owner decision; returns the group and applicant so the caller can queue the roster update.
    pub fn resolve_join_request(&self, self_id: &str, id: &str, accept: bool) -> Result<Option<(String, String)>> {
        if !valid_fingerprint(self_id) || !valid_message_id(id) { return Err(rusqlite::Error::InvalidQuery); }
        let tx = self.connection.unchecked_transaction()?;
        let row: Option<(String, String, String)> = tx.query_row(
            "SELECT group_id, applicant, owner FROM group_join_requests WHERE id = ?1 AND status = 'pending'",
            [id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).optional()?;
        let Some((group_id, applicant, owner)) = row else { return Ok(None); };
        if owner != self_id { return Ok(None); }
        tx.execute("UPDATE group_join_requests SET status = ?2 WHERE id = ?1",
            params![id, if accept { "accepted" } else { "rejected" }])?;
        tx.commit()?;
        Ok(Some((group_id, applicant)))
    }

    /// Queues the refusal notice so the applicant's UI stops waiting.
    pub fn queue_join_reject(&self, self_id: &str, group_id: &str, applicant: &str, now: i64) -> Result<bool> {
        if !valid_fingerprint(self_id) || !valid_fingerprint(applicant) || !valid_group_id(group_id) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let owner: Option<String> = self.connection.query_row("SELECT owner FROM groups WHERE id = ?1", [group_id], |row| row.get(0)).optional()?;
        if owner.as_deref() != Some(self_id) { return Ok(false); }
        let verified: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM contacts WHERE fingerprint = ?1 AND trust_state = 'verified')",
            [applicant], |row| row.get(0))?;
        if !verified { return Ok(false); }
        let id: String = self.connection.query_row("SELECT lower(hex(randomblob(16)))", [], |row| row.get(0))?;
        self.connection.execute(
            "INSERT INTO group_control(id, group_id, target, kind, payload, status, created_at)
             VALUES (?1, ?2, ?3, 'join_reject', '{}', 'pending', ?4)",
            params![id, group_id, applicant, now])?;
        Ok(true)
    }

    /// The owner refused; the applicant stops waiting.
    pub fn reject_out_join_request(&self, self_id: &str, group_id: &str) -> Result<bool> {
        if !valid_fingerprint(self_id) || !valid_group_id(group_id) { return Err(rusqlite::Error::InvalidQuery); }
        Ok(self.connection.execute(
            "UPDATE group_join_requests SET status = 'rejected' WHERE applicant = ?1 AND group_id = ?2 AND status = 'pending'",
            params![self_id, group_id])? > 0)
    }

    /// Applies an inbound invite / member update / removal / leave notice.
    /// A new invitation asks the user first unless auto-join is enabled.
    pub fn auto_join_groups(&self) -> Result<bool> {
        Ok(self.get_setting("auto_join_groups")?.as_deref() == Some("1"))
    }

    pub fn set_auto_join_groups(&self, enabled: bool) -> Result<()> {
        self.set_setting("auto_join_groups", Some(if enabled { "1" } else { "0" }))?;
        Ok(())
    }

    pub fn pending_group_invitations(&self, self_id: &str) -> Result<Vec<GroupInvitation>> {
        if !valid_fingerprint(self_id) { return Err(rusqlite::Error::InvalidQuery); }
        let mut statement = self.connection.prepare(
            "SELECT i.id, i.group_id, i.name, i.owner, c.display_name, i.members, i.created_at
             FROM group_invitations i LEFT JOIN contacts c ON c.fingerprint = i.owner
             WHERE i.status = 'pending' AND i.owner != ?1
             ORDER BY i.created_at DESC LIMIT 64",
        )?;
        let rows = statement.query_map([self_id], |row| {
            let members: String = row.get(5)?;
            let count = serde_json::from_str::<Vec<String>>(&members).map(|list| list.len()).unwrap_or(0) as u32;
            Ok(GroupInvitation {
                id: row.get(0)?, group_id: row.get(1)?, name: row.get(2)?, owner: row.get(3)?,
                owner_name: row.get(4)?, member_count: count, created_at: row.get(6)?,
            })
        })?;
        rows.collect()
    }

    /// Joins the announced roster. Returns the group id so the UI can open it.
    pub fn accept_group_invitation(&self, id: &str, self_id: &str, now: i64) -> Result<Option<String>> {
        if !valid_group_id_is(id) || !valid_fingerprint(self_id) { return Err(rusqlite::Error::InvalidQuery); }
        let tx = self.connection.unchecked_transaction()?;
        let row: Option<(String, String, String, String)> = tx.query_row(
            "SELECT group_id, name, owner, members FROM group_invitations WHERE id = ?1 AND status = 'pending'",
            [id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).optional()?;
        let Some((group_id, name, owner, members_json)) = row else { return Ok(None); };
        let members: Vec<String> = serde_json::from_str(&members_json).map_err(|_| rusqlite::Error::InvalidQuery)?;
        if !valid_fingerprint(&owner) || members.is_empty() || members.len() > MAX_GROUP_MEMBERS_LOCAL
            || !members.iter().all(|m| valid_fingerprint(m)) || !members.iter().any(|m| m == &owner)
        { return Err(rusqlite::Error::InvalidQuery); }
        let existing_owner: Option<String> = tx.query_row("SELECT owner FROM groups WHERE id = ?1", [&group_id], |row| row.get(0)).optional()?;
        match existing_owner {
            Some(local) if local != owner => return Ok(None),
            Some(_) => { tx.execute("UPDATE groups SET name = ?2, active = 1 WHERE id = ?1", params![group_id, name])?; },
            None => { tx.execute("INSERT INTO groups(id, name, owner, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![group_id, name, owner, now])?; },
        }
        for member in &members {
            let role = if member == &owner { "owner" } else { "member" };
            tx.execute(
                "INSERT INTO group_members(group_id, member, role, joined_at, active, hidden) VALUES (?1, ?2, ?3, ?4, 1, 0)
                 ON CONFLICT(group_id, member) DO UPDATE SET active = 1, hidden = 0, role = excluded.role",
                params![group_id, member, role, now])?;
        }
        tx.execute("UPDATE group_invitations SET status = 'accepted' WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(Some(group_id))
    }

    /// Declines an invitation and tells the owner to drop us from its delivery list.
    pub fn decline_group_invitation(&self, id: &str, self_id: &str, now: i64) -> Result<bool> {
        if !valid_group_id_is(id) || !valid_fingerprint(self_id) { return Err(rusqlite::Error::InvalidQuery); }
        let tx = self.connection.unchecked_transaction()?;
        let row: Option<(String, String)> = tx.query_row(
            "SELECT group_id, owner FROM group_invitations WHERE id = ?1 AND status = 'pending'",
            [id], |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional()?;
        let Some((group_id, owner)) = row else { return Ok(false); };
        tx.execute("UPDATE group_invitations SET status = 'declined' WHERE id = ?1", [id])?;
        let payload = serde_json::json!({ "group_id": group_id, "member": self_id }).to_string();
        tx.execute(
            "INSERT INTO group_control(id, group_id, target, kind, payload, status, created_at)
             VALUES (lower(hex(randomblob(16))), ?1, ?2, 'leave', ?3, 'pending', ?4)",
            params![group_id, owner, payload, now])?;
        tx.commit()?;
        Ok(true)
    }

    pub fn receive_group_control(&self, self_id: &str, peer: &str, kind: &str, group_id: &str, payload: &str, now: i64) -> Result<IncomingResult> {
        if !valid_fingerprint(self_id) || !valid_fingerprint(peer) || !valid_group_id(group_id)
            || payload.is_empty() || payload.len() > 65536 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let tx = self.connection.unchecked_transaction()?;
        let verified: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM contacts WHERE fingerprint = ?1 AND trust_state = 'verified')",
            [peer], |row| row.get(0),
        )?;
        if !verified { return Ok(IncomingResult::Unverified); }
        match kind {
            "invite" | "members" => {
                let value: serde_json::Value = serde_json::from_str(payload).map_err(|_| rusqlite::Error::InvalidQuery)?;
                let name = value.get("name").and_then(|v| v.as_str()).ok_or(rusqlite::Error::InvalidQuery)?;
                let owner = value.get("owner").and_then(|v| v.as_str()).ok_or(rusqlite::Error::InvalidQuery)?;
                let members: Vec<String> = value.get("members").and_then(|v| v.as_array())
                    .ok_or(rusqlite::Error::InvalidQuery)?
                    .iter().filter_map(|v| v.as_str().map(str::to_owned)).collect();
                if !valid_fingerprint(owner) || name.is_empty() || name.len() > 128 || members.len() > 256
                    || !members.iter().all(|m| valid_fingerprint(m)) || !members.iter().any(|m| m == owner) {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                if peer != owner { return Ok(IncomingResult::Unverified); }
                if !members.iter().any(|m| m == self_id) { return Ok(IncomingResult::NotMember); }
                let existing_owner: Option<String> = tx.query_row("SELECT owner FROM groups WHERE id = ?1", [group_id], |row| row.get(0)).optional()?;
                // A first invitation waits for the user unless auto-join is enabled.
                let active_member: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM group_members WHERE group_id = ?1 AND member = ?2 AND active = 1)",
                    params![group_id, self_id], |row| row.get(0),
                )?;
                let auto_join = tx.query_row("SELECT value FROM settings WHERE key = 'auto_join_groups'",
                    [], |row| row.get::<_, String>(0)).optional()?.as_deref() == Some("1");
                if existing_owner.is_none() && !active_member && !auto_join {
                    let members_json = serde_json::to_string(&members).map_err(|_| rusqlite::Error::InvalidQuery)?;
                    tx.execute(
                        "INSERT INTO group_invitations(id, group_id, name, owner, members, created_at, status)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending')
                         ON CONFLICT(id) DO UPDATE SET name = excluded.name, owner = excluded.owner, members = excluded.members",
                        params![group_id, group_id, name, owner, members_json, now])?;
                    tx.commit()?;
                    return Ok(IncomingResult::Inserted);
                }
                match existing_owner {
                    Some(local) if local != owner => return Ok(IncomingResult::IdConflict),
                    Some(_) => { tx.execute("UPDATE groups SET name = ?2 WHERE id = ?1", params![group_id, name])?; },
                    None => { tx.execute("INSERT INTO groups(id, name, owner, created_at) VALUES (?1, ?2, ?3, ?4)",
                        params![group_id, name, owner, now])?; },
                }
                let previous: Vec<String> = {
                    let mut statement = tx.prepare("SELECT member FROM group_members WHERE group_id = ?1")?;
                    let rows = statement.query_map([group_id], |row| row.get(0))?;
                    rows.collect::<Result<Vec<String>>>()?
                };
                for member in &members {
                    let role = if member == owner { "owner" } else { "member" };
                    tx.execute(
                        "INSERT INTO group_members(group_id, member, role, joined_at, active, hidden) VALUES (?1, ?2, ?3, ?4, 1, 0)
                         ON CONFLICT(group_id, member) DO UPDATE SET active = 1, role = excluded.role",
                        params![group_id, member, role, now])?;
                }
                for member in previous {
                    if !members.contains(&member) {
                        tx.execute("UPDATE group_members SET active = 0 WHERE group_id = ?1 AND member = ?2",
                            params![group_id, member])?;
                        tx.execute("DELETE FROM group_outbox WHERE group_id = ?1 AND member = ?2 AND status = 'pending'",
                            params![group_id, member])?;
                    }
                }
                // An invite that includes us settles any application we had pending.
                tx.execute(
                    "UPDATE group_join_requests SET status = 'accepted' WHERE applicant = ?1 AND group_id = ?2 AND status = 'pending'",
                    params![self_id, group_id])?;
                tx.commit()?;
                Ok(IncomingResult::Inserted)
            }
            "remove" => {
                let owner: Option<String> = tx.query_row("SELECT owner FROM groups WHERE id = ?1", [group_id], |row| row.get(0)).optional()?;
                let Some(owner) = owner else { return Ok(IncomingResult::NotMember); };
                if owner != peer { return Ok(IncomingResult::Unverified); }
                tx.execute("UPDATE group_members SET active = 0, hidden = 1 WHERE group_id = ?1 AND member = ?2",
                    params![group_id, self_id])?;
                tx.execute("DELETE FROM group_outbox WHERE group_id = ?1 AND member = ?2", params![group_id, self_id])?;
                tx.commit()?;
                Ok(IncomingResult::Inserted)
            }
            "leave" => {
                let owner: Option<String> = tx.query_row("SELECT owner FROM groups WHERE id = ?1", [group_id], |row| row.get(0)).optional()?;
                let Some(owner) = owner else { return Ok(IncomingResult::NotMember); };
                if owner != self_id { return Ok(IncomingResult::Unverified); }
                let value: serde_json::Value = serde_json::from_str(payload).map_err(|_| rusqlite::Error::InvalidQuery)?;
                let member = value.get("member").and_then(|v| v.as_str()).ok_or(rusqlite::Error::InvalidQuery)?;
                if !valid_fingerprint(member) || member != peer { return Ok(IncomingResult::Unverified); }
                tx.execute("UPDATE group_members SET active = 0 WHERE group_id = ?1 AND member = ?2",
                    params![group_id, member])?;
                tx.execute("DELETE FROM group_outbox WHERE group_id = ?1 AND member = ?2", params![group_id, member])?;
                tx.commit()?;
                Ok(IncomingResult::Inserted)
            }
            "transfer" => {
                // Only the current owner may hand the group over, and the new owner must
                // be part of the announced roster, including us if we are to stay.
                let current: Option<String> = tx.query_row("SELECT owner FROM groups WHERE id = ?1 AND active = 1", [group_id], |row| row.get(0)).optional()?;
                let Some(current) = current else { return Ok(IncomingResult::NotMember); };
                if current != peer { return Ok(IncomingResult::Unverified); }
                let value: serde_json::Value = serde_json::from_str(payload).map_err(|_| rusqlite::Error::InvalidQuery)?;
                let owner = value.get("owner").and_then(|v| v.as_str()).ok_or(rusqlite::Error::InvalidQuery)?;
                let name = value.get("name").and_then(|v| v.as_str()).ok_or(rusqlite::Error::InvalidQuery)?;
                let members: Vec<String> = value.get("members").and_then(|v| v.as_array())
                    .ok_or(rusqlite::Error::InvalidQuery)?
                    .iter().filter_map(|v| v.as_str().map(str::to_owned)).collect();
                if !valid_fingerprint(owner) || owner == peer || name.is_empty() || name.len() > 128
                    || members.len() > 256 || !members.iter().all(|m| valid_fingerprint(m))
                    || !members.iter().any(|m| m == owner) || !members.iter().any(|m| m == self_id) {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                tx.execute("UPDATE groups SET name = ?2, owner = ?3 WHERE id = ?1",
                    params![group_id, name, owner])?;
                let previous: Vec<String> = {
                    let mut statement = tx.prepare("SELECT member FROM group_members WHERE group_id = ?1")?;
                    let rows = statement.query_map([group_id], |row| row.get(0))?;
                    rows.collect::<Result<Vec<String>>>()?
                };
                for member in &members {
                    let role = if member == owner { "owner" } else { "member" };
                    tx.execute(
                        "INSERT INTO group_members(group_id, member, role, joined_at, active, hidden) VALUES (?1, ?2, ?3, ?4, 1, 0)
                         ON CONFLICT(group_id, member) DO UPDATE SET active = 1, role = excluded.role",
                        params![group_id, member, role, now])?;
                }
                for member in previous {
                    if !members.contains(&member) {
                        tx.execute("UPDATE group_members SET active = 0 WHERE group_id = ?1 AND member = ?2",
                            params![group_id, member])?;
                        tx.execute("DELETE FROM group_outbox WHERE group_id = ?1 AND member = ?2 AND status = 'pending'",
                            params![group_id, member])?;
                    }
                }
                tx.commit()?;
                Ok(IncomingResult::Inserted)
            }
            "dissolve" => {
                let current: Option<String> = tx.query_row("SELECT owner FROM groups WHERE id = ?1 AND active = 1", [group_id], |row| row.get(0)).optional()?;
                let Some(current) = current else { return Ok(IncomingResult::NotMember); };
                if current != peer { return Ok(IncomingResult::Unverified); }
                tx.execute("UPDATE groups SET active = 0, dissolved_at = ?2, dissolved_by = ?3 WHERE id = ?1",
                    params![group_id, now, peer])?;
                tx.execute("UPDATE group_members SET active = 0, hidden = 1 WHERE group_id = ?1", [group_id])?;
                tx.execute("DELETE FROM group_outbox WHERE group_id = ?1 AND status = 'pending'", [group_id])?;
                tx.commit()?;
                Ok(IncomingResult::Inserted)
            }
            "recall" => {
                let message_id = serde_json::from_str::<serde_json::Value>(payload).ok()
                    .and_then(|value| value.get("message_id").and_then(|item| item.as_str()).map(str::to_string));
                let Some(message_id) = message_id else { return Err(rusqlite::Error::InvalidQuery); };
                let member: Option<i64> = tx.query_row(
                    "SELECT active FROM group_members WHERE group_id = ?1 AND member = ?2",
                    params![group_id, self_id], |row| row.get(0)).optional()?;
                if member != Some(1) { return Ok(IncomingResult::NotMember); }
                let row: Option<(String, Option<i64>, String, Option<String>)> = tx.query_row(
                    "SELECT m.sender, m.recalled_at, m.kind, f.status FROM group_messages m
                     LEFT JOIN group_file_transfers f ON f.message_id = m.id WHERE m.id = ?1 AND m.group_id = ?2",
                    params![message_id, group_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).optional()?;
                let Some((sender, recalled_at, kind, file_status)) = row else { return Ok(IncomingResult::Duplicate); };
                if sender != peer { return Ok(IncomingResult::Unverified); }
                if recalled_at.is_some() { return Ok(IncomingResult::Duplicate); }
                tx.execute("UPDATE group_messages SET body = ' ', recalled_at = ?3 WHERE id = ?1 AND group_id = ?2",
                    params![message_id, group_id, now])?;
                if kind == "file" && file_status.as_deref() != Some("completed") {
                    tx.execute("UPDATE group_file_transfers SET status = 'failed', error = 'recalled', updated_at = ?2 WHERE message_id = ?1",
                        params![message_id, now])?;
                    tx.execute("DELETE FROM file_offer_requests WHERE message_id = ?1", [&message_id])?;
                }
                tx.commit()?;
                Ok(IncomingResult::Inserted)
            }
            "notice" => {
                // Only the current owner may publish an announcement, and only
                // active members keep it.
                let owner: Option<String> = tx.query_row("SELECT owner FROM groups WHERE id = ?1", [group_id], |row| row.get(0)).optional()?;
                let Some(owner) = owner else { return Ok(IncomingResult::NotMember); };
                if owner != peer { return Ok(IncomingResult::Unverified); }
                let active: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM group_members WHERE group_id = ?1 AND member = ?2 AND active = 1)",
                    params![group_id, self_id], |row| row.get(0))?;
                if !active { return Ok(IncomingResult::NotMember); }
                let value: serde_json::Value = serde_json::from_str(payload).map_err(|_| rusqlite::Error::InvalidQuery)?;
                let notice = value.get("notice").and_then(|v| v.as_str()).ok_or(rusqlite::Error::InvalidQuery)?;
                if notice.chars().count() > 2048
                    || notice.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                tx.execute(
                    "INSERT INTO group_notices(group_id, notice, updated_at, updated_by) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(group_id) DO UPDATE SET notice = excluded.notice, updated_at = excluded.updated_at, updated_by = excluded.updated_by",
                    params![group_id, notice, now, peer])?;
                tx.commit()?;
                Ok(IncomingResult::Inserted)
            }
            _ => Err(rusqlite::Error::InvalidQuery),
        }
    }

    /// Clock update, message insertion and unhide commit together. created_at is display only.
    pub fn send_self(&self, sender: &str, body: &str, created_at: i64) -> Result<LocalMessage> {
        if !valid_fingerprint(sender) || body.trim().is_empty() || body.len() > 32768 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let tx = self.connection.unchecked_transaction()?;
        tx.execute("INSERT INTO conversations(id, kind, hidden) VALUES ('self', 'self', 0) ON CONFLICT(id) DO UPDATE SET hidden = 0", [])?;
        if tx.execute("UPDATE logical_clock SET value = value + 1 WHERE singleton = 1 AND value < 9223372036854775807", [])? != 1 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let (id, logical): (String, i64) = tx.query_row("SELECT lower(hex(randomblob(16))), value FROM logical_clock WHERE singleton = 1", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
        tx.execute("INSERT INTO local_messages(id, conversation_id, sender, body, created_at, logical) VALUES (?1, 'self', ?2, ?3, ?4, ?5)", params![id, sender, body, created_at, logical])?;
        tx.commit()?;
        Ok(LocalMessage { id, conversation_id: "self".into(), sender: sender.into(), body: body.into(), created_at, logical, recalled_at: None })
    }

    pub fn recall_self(&self, self_id: &str, message_id: &str, now: i64) -> Result<RecallResult> {
        if !valid_fingerprint(self_id) || !valid_message_id(message_id) { return Err(rusqlite::Error::InvalidQuery); }
        let tx = self.connection.unchecked_transaction()?;
        let message: Option<(String, i64, Option<i64>)> = tx.query_row(
            "SELECT sender, created_at, recalled_at FROM local_messages WHERE conversation_id = 'self' AND id = ?1",
            [message_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).optional()?;
        let Some((sender, created_at, recalled_at)) = message else { return Ok(RecallResult::Missing); };
        if sender != self_id { return Ok(RecallResult::NotYours); }
        if recalled_at.is_some() { return Ok(RecallResult::Already); }
        if now.saturating_sub(created_at) > RECALL_WINDOW_SECONDS { return Ok(RecallResult::TooLate); }
        tx.execute("UPDATE local_messages SET body = ' ', recalled_at = ?2 WHERE id = ?1", params![message_id, now])?;
        tx.commit()?;
        Ok(RecallResult::Recalled { delivered: false })
    }

    /// Newest page first, ordered independently from device wall clock.
    pub fn list_self_messages(&self, limit: u32, offset: u32) -> Result<Vec<LocalMessage>> {
        if limit == 0 || limit > 200 || offset > 100_000 { return Err(rusqlite::Error::InvalidQuery); }
        let mut statement = self.connection.prepare(
            "SELECT id, conversation_id, sender, body, created_at, logical, recalled_at FROM local_messages
             WHERE conversation_id = 'self' ORDER BY logical DESC, sender DESC, id DESC LIMIT ?1 OFFSET ?2"
        )?;
        let rows = statement.query_map(params![limit, offset], |row| Ok(LocalMessage {
            id: row.get(0)?, conversation_id: row.get(1)?, sender: row.get(2)?, body: row.get(3)?, created_at: row.get(4)?, logical: row.get(5)?, recalled_at: row.get(6)?,
        }))?;
        rows.collect()
    }

    /// Newest page first, with deterministic ordering when timestamps coincide.
    pub fn list_messages(&self, peer: &str, limit: u32, offset: u32) -> Result<Vec<Message>> {
        if !valid_fingerprint(peer) || limit == 0 || limit > 200 || offset > 100_000 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let mut statement = self.connection.prepare(
            "SELECT id, peer_fingerprint, body, direction, created_at, status FROM messages
             WHERE peer_fingerprint = ?1 ORDER BY created_at DESC, id DESC LIMIT ?2 OFFSET ?3"
        )?;
        let rows = statement.query_map(params![peer, limit, offset], |row| Ok(Message {
            id: row.get(0)?, peer_fingerprint: row.get(1)?, body: row.get(2)?,
            direction: row.get(3)?, created_at: row.get(4)?, status: row.get(5)?,
        }))?;
        rows.collect()
    }

    /// Refuses to store a message for an unknown or unverified peer.
    pub fn save_outgoing(&self, id: &str, peer: &str, body: &str, created_at: i64) -> Result<bool> {
        if id.is_empty() || id.len() > 128 || body.is_empty() || body.len() > 32768 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        Ok(self.connection.execute(
            "INSERT INTO messages (id, peer_fingerprint, body, direction, created_at, status)
             SELECT ?1, fingerprint, ?3, 'outgoing', ?4, 'pending' FROM contacts
             WHERE fingerprint = ?2 AND trust_state = 'verified'",
            params![id, peer, body, created_at],
        )? == 1)
    }
}

/// Sum of regular file sizes below `dir`; unreadable entries are ignored and the
/// walk is depth-bounded so a hostile directory cannot stall the UI thread.
fn directory_usage(dir: &Path) -> u64 {
    fn walk(dir: &Path, depth: u8) -> u64 {
        if depth > 8 { return 0; }
        let Ok(entries) = std::fs::read_dir(dir) else { return 0; };
        let mut total = 0u64;
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else { continue; };
            if kind.is_file() {
                total = total.saturating_add(entry.metadata().map(|meta| meta.len()).unwrap_or(0));
            } else if kind.is_dir() {
                total = total.saturating_add(walk(&entry.path(), depth + 1));
            }
        }
        total
    }
    walk(dir, 0)
}

fn group_message_row(row: &rusqlite::Row<'_>) -> Result<GroupMessage> {
    Ok(GroupMessage {
        id: row.get(0)?, group_id: row.get(1)?, sender: row.get(2)?, sender_name: row.get(3)?,
        body: row.get(4)?, created_at: row.get(5)?, logical: row.get(6)?, pending: row.get(7)?,
        reply_to: row.get(8)?, reply_sender: row.get(9)?, reply_preview: row.get(10)?,
        mentions: parse_mentions(&row.get::<_, String>(11)?),
        kind: row.get(12)?, file_name: row.get(13)?, file_size: row.get(14)?, file_sha256: row.get(15)?,
        file_status: row.get(16)?, file_transferred: row.get(17)?, file_error: row.get(18)?,
        recalled_at: row.get(19)?,
    })
}

/// One-line quote snippet: whitespace collapsed, trimmed and cut on a character boundary.
fn quote_preview(body: &str) -> String {
    let collapsed = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut preview: String = collapsed.chars().take(REPLY_PREVIEW_MAX).collect();
    if collapsed.chars().count() > REPLY_PREVIEW_MAX { preview.push('…'); }
    preview
}

fn valid_reply_metadata(reply_to: Option<&str>, reply_sender: Option<&str>, reply_preview: Option<&str>) -> bool {
    reply_to.is_none_or(valid_message_id)
        && reply_sender.is_none_or(|value| !value.is_empty() && value.chars().count() <= 128 && !value.chars().any(char::is_control))
        && reply_preview.is_none_or(|value| value.chars().count() <= REPLY_PREVIEW_MAX)
        && reply_to.is_some() == reply_preview.is_some()
        && (reply_to.is_some() || reply_sender.is_none())
}

fn direct_quote(connection: &Connection, self_id: &str, peer: &str, reply_to: Option<&str>)
    -> Result<(Option<String>, Option<String>)> {
    let Some(message_id) = reply_to else { return Ok((None, None)); };
    if !valid_message_id(message_id) { return Err(rusqlite::Error::InvalidQuery); }
    let quoted: Option<(String, String, String, Option<String>, Option<i64>)> = connection.query_row(
        "SELECT sender, body, kind, file_name, recalled_at FROM direct_messages WHERE peer = ?1 AND id = ?2",
        params![peer, message_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
    ).optional()?;
    let Some((quoted_sender, body, kind, file_name, recalled_at)) = quoted else { return Err(rusqlite::Error::InvalidQuery); };
    if recalled_at.is_some() || ![self_id, peer].contains(&quoted_sender.as_str()) {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let preview = if kind == "file" {
        if !file_name.as_deref().is_some_and(is_image_file) { return Err(rusqlite::Error::InvalidQuery); }
        connection.query_row("SELECT caption FROM image_batches WHERE message_id = ?1", [message_id], |row| row.get::<_, String>(0))
            .optional()?.filter(|caption| !caption.trim().is_empty())
            .map(|caption| quote_preview(&caption)).unwrap_or_else(|| "图片".into())
    } else {
        quote_preview(&body)
    };
    let display_name = if quoted_sender == self_id {
        connection.query_row("SELECT value FROM settings WHERE key = 'profile_nickname'", [], |row| row.get::<_, String>(0))
            .optional()?.filter(|name| !name.trim().is_empty()).unwrap_or_else(|| "我".into())
    } else {
        connection.query_row("SELECT COALESCE(NULLIF(remark, ''), display_name) FROM contacts WHERE fingerprint = ?1", [peer], |row| row.get::<_, String>(0))
            .optional()?.unwrap_or_else(|| "联系人".into())
    };
    Ok((Some(display_name), Some(preview)))
}

fn group_quote(connection: &Connection, group_id: &str, reply_to: Option<&str>)
    -> Result<(Option<String>, Option<String>)> {
    let Some(message_id) = reply_to else { return Ok((None, None)); };
    if !valid_message_id(message_id) { return Err(rusqlite::Error::InvalidQuery); }
    let quoted: Option<(String, String, String, Option<String>, Option<i64>)> = connection.query_row(
        "SELECT sender, body, kind, file_name, recalled_at FROM group_messages WHERE group_id = ?1 AND id = ?2",
        params![group_id, message_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
    ).optional()?;
    let Some((sender, body, kind, file_name, recalled_at)) = quoted else { return Err(rusqlite::Error::InvalidQuery); };
    if recalled_at.is_some() { return Err(rusqlite::Error::InvalidQuery); }
    let display_name: String = connection.query_row(
        "SELECT COALESCE(NULLIF(c.display_name, ''), '群成员') FROM group_members gm
         LEFT JOIN contacts c ON c.fingerprint = gm.member
         WHERE gm.group_id = ?1 AND gm.member = ?2",
        params![group_id, sender], |row| row.get(0))?;
    let preview = if kind == "file" {
        if !file_name.as_deref().is_some_and(is_image_file) { return Err(rusqlite::Error::InvalidQuery); }
        connection.query_row("SELECT caption FROM image_batches WHERE message_id = ?1", [message_id], |row| row.get::<_, String>(0))
            .optional()?.filter(|caption| !caption.trim().is_empty())
            .map(|caption| quote_preview(&caption)).unwrap_or_else(|| "图片".into())
    } else {
        quote_preview(&body)
    };
    Ok((Some(display_name), Some(preview)))
}

fn parse_mentions(value: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(value).unwrap_or_default()
}

/// Body/sender/logical of an existing message id, used to detect identical archived rows.
fn existing_text(tx: &rusqlite::Transaction<'_>, table: &str, id: &str) -> Result<Option<(String, String, i64)>> {
    let sql = format!("SELECT body, sender, logical FROM {table} WHERE id = ?1");
    tx.query_row(&sql, [id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).optional()
}

fn existing_archive_message(tx: &rusqlite::Transaction<'_>, table: &str, id: &str)
    -> Result<Option<(String, String, i64, String, Option<String>, Option<i64>, Option<String>)>> {
    let sql = format!("SELECT body, sender, logical, COALESCE(kind, 'text'), file_name, file_size, file_sha256 FROM {table} WHERE id = ?1");
    tx.query_row(&sql, [id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?))).optional()
}

fn archive_sha256_matches(data: &[u8], expected: &str) -> bool {
    use sha2::Digest;
    let actual = sha2::Sha256::digest(data).iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    actual == expected
}

/// LIKE pattern with the escape character protected, so `%` and `_` stay literal.
fn like_pattern(query: &str) -> String {
    let mut escaped = String::with_capacity(query.len() + 2);
    escaped.push('%');
    for character in query.chars() {
        if matches!(character, '\\' | '%' | '_') { escaped.push('\\'); }
        escaped.push(character);
    }
    escaped.push('%');
    escaped
}

fn valid_search(query: &str, category: &str, limit: u32, offset: u32) -> bool {
    matches!(category, "all" | "text" | "file" | "image")
        && limit > 0 && limit <= 200 && offset <= 100_000
        && query.chars().count() <= 200 && !query.chars().any(char::is_control)
}

fn map_message(row: &rusqlite::Row<'_>) -> Result<DirectMessage> {
    Ok(DirectMessage {
        id: row.get(0)?, peer: row.get(1)?, sender: row.get(2)?, body: row.get(3)?,
        created_at: row.get(4)?, logical: row.get(5)?, status: row.get(6)?, kind: row.get(7)?,
        file_name: row.get(8)?, file_size: row.get(9)?, file_sha256: row.get(10)?,
        file_status: row.get(11)?, file_transferred: row.get(12)?, file_error: row.get(13)?,
        recalled_at: row.get(14)?,
        reply_to: row.get(15)?, reply_sender: row.get(16)?, reply_preview: row.get(17)?,
    })
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn is_image_file(name: &str) -> bool {
    Path::new(name).extension().and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(),
            "avif" | "bmp" | "gif" | "heic" | "heif" | "jpeg" | "jpg" | "png" | "tif" | "tiff" | "webp"))
}

fn valid_message_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Remembered sources are a contact fingerprint or a group id prefixed with "group:".
fn valid_offer_source(value: &str) -> bool {
    if let Some(group) = value.strip_prefix("group:") {
        valid_group_id(group)
    } else {
        valid_fingerprint(value)
    }
}

fn valid_fingerprint(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn valid_fingerprint_public(value: &str) -> bool { valid_fingerprint(value) }

pub fn valid_username_public(value: &str) -> bool { valid_username(value) }

/// Invitation ids are the same bounded opaque tokens as group control ids.
fn valid_group_id_is(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}

const MAX_GROUP_MEMBERS_LOCAL: usize = 256;

/// Usernames are 2-20 ASCII letters, digits or `_`, and never start with a digit.
fn valid_username(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else { return false };
    (2..=20).contains(&value.len())
        && (first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Group ids are random 128-bit values, never derived from a fingerprint.
fn valid_group_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory_store() -> Store {
        Store::initialize(Connection::open_in_memory().unwrap()).unwrap()
    }

    #[test]
    fn archive_export_filters_text_images_and_other_files_and_validates_selection() {
        let store = memory_store();
        let self_id = "a".repeat(64);
        let peer = "b".repeat(64);
        store.add_candidate(&peer, "Bob").unwrap();
        store.confirm_candidate(&peer).unwrap();
        let text = store.queue_direct(&self_id, &peer, "hello", 1).unwrap();
        let image = store.queue_direct_file(&self_id, &peer, "photo.PNG", 3, &"c".repeat(64), Path::new("photo.png"), 2).unwrap();
        let document = store.queue_direct_file(&self_id, &peer, "notes.txt", 4, &"d".repeat(64), Path::new("notes.txt"), 3).unwrap();
        store.complete_file_transfer(&image.id, 4).unwrap();
        store.complete_file_transfer(&document.id, 5).unwrap();

        let text_and_documents = store.archive_export_with_options("direct", &peer, &self_id, 0, 0, None, true, false, true).unwrap();
        assert_eq!(text_and_documents.messages.iter().map(|message| message.id.as_str()).collect::<Vec<_>>(), vec![text.id.as_str(), document.id.as_str()]);
        assert_eq!(text_and_documents.attachments.len(), 1);
        assert_eq!(text_and_documents.attachments[0].message_id, document.id);

        let images_only = store.archive_export_with_options("direct", &peer, &self_id, 0, 0, None, false, true, false).unwrap();
        assert_eq!(images_only.messages.len(), 1);
        assert_eq!(images_only.messages[0].id, image.id);
        assert_eq!(images_only.attachments[0].file_name.as_deref(), Some("photo.PNG"));

        let selected = vec![text.id.clone()];
        let selected_export = store.archive_export_with_options("direct", &peer, &self_id, 0, 0, Some(&selected), true, true, true).unwrap();
        assert_eq!(selected_export.messages.len(), 1);
        assert_eq!(selected_export.messages[0].id, text.id);
        assert!(selected_export.attachments.is_empty());

        let unknown = vec!["missing-message".to_owned()];
        assert!(store.archive_export_with_options("direct", &peer, &self_id, 0, 0, Some(&unknown), true, true, true).is_err());
        let duplicate = vec!["text-1".to_owned(), "text-1".to_owned()];
        assert!(store.archive_export_with_options("direct", &peer, &self_id, 0, 0, Some(&duplicate), true, true, true).is_err());
    }

    #[test]
    fn archive_export_refuses_selected_incomplete_attachments() {
        let store = memory_store();
        let self_id = "a".repeat(64);
        let peer = "b".repeat(64);
        store.add_candidate(&peer, "Bob").unwrap();
        store.confirm_candidate(&peer).unwrap();
        let file = store.queue_direct_file(&self_id, &peer, "pending.bin", 4, &"e".repeat(64), Path::new("pending.bin"), 1).unwrap();
        let selected = vec![file.id];
        assert!(store.archive_export_with_options("direct", &peer, &self_id, 0, 0, Some(&selected), true, true, true).is_err());
    }

    #[test]
    fn receive_policy_defaults_to_asking_and_remembers_sources() {
        let store = memory_store();
        // A fresh install asks before receiving any file.
        assert_eq!(store.file_accept_mode().unwrap(), FileAcceptMode::Always);
        let peer = "c".repeat(64);
        assert!(!store.is_file_source_remembered(&peer).unwrap());
        store.set_file_accept_mode(FileAcceptMode::Remember).unwrap();
        store.remember_file_source(&peer).unwrap();
        assert!(store.is_file_source_remembered(&peer).unwrap());
        assert_eq!(store.file_accept_mode().unwrap(), FileAcceptMode::Remember);
        store.forget_file_sources().unwrap();
        assert!(!store.is_file_source_remembered(&peer).unwrap());
        assert!(store.remember_file_source("not-a-source").is_err());
    }

    #[test]
    fn receive_directories_are_separate_and_fall_back_to_defaults() {
        let store = memory_store();
        assert!(!store.recv_dir_custom(RecvKind::Direct));
        assert_ne!(store.recv_dir(RecvKind::Direct), store.recv_dir(RecvKind::Group));
        let chosen = PathBuf::from("D:\\收件\\单聊");
        store.set_recv_dir(RecvKind::Direct, Some("D:\\收件\\单聊")).unwrap();
        assert!(store.recv_dir_custom(RecvKind::Direct));
        assert!(!store.recv_dir_custom(RecvKind::Group));
        assert_eq!(store.recv_dir(RecvKind::Direct), chosen);
        store.set_recv_dir(RecvKind::Direct, None).unwrap();
        assert!(!store.recv_dir_custom(RecvKind::Direct));
        assert_eq!(store.recv_dir(RecvKind::Direct), store.default_recv_dir(RecvKind::Direct));
    }

    #[test]
    fn save_as_directory_is_kept_for_the_next_attempt() {
        let store = memory_store();
        let peer = "e".repeat(64);
        let id = "f".repeat(32);
        store.record_file_offer_request(&peer, &id, "a.txt", 10, 1, None).unwrap();
        assert_eq!(store.file_offer_target_dir(&id).unwrap(), None);
        store.resolve_file_offer(&id, true, Some("D:\\另存为")).unwrap();
        assert_eq!(store.file_offer_target_dir(&id).unwrap(), Some(PathBuf::from("D:\\另存为")));
        // Group offers stay out of the direct view.
        assert!(store.list_file_offer_requests(&peer).unwrap().is_empty());
        assert_eq!(store.list_group_file_offer_requests(&"a".repeat(32)).unwrap().len(), 0);
    }

    #[test]
    fn pending_delivery_selection_rotates_peers_and_selects_oldest() {
        let store = memory_store();
        let me = "a".repeat(64);
        let b = "b".repeat(64);
        let c = "c".repeat(64);
        let d = "d".repeat(64);
        for peer in [&b, &c, &d] { store.add_candidate(peer, "contact").unwrap(); store.confirm_candidate(peer).unwrap(); }
        let first = store.queue_direct(&me, &b, "first", 1).unwrap();
        let second = store.queue_direct(&me, &b, "second", 2).unwrap();
        let other = store.queue_direct(&me, &c, "other", 3).unwrap();
        store.queue_direct(&me, &d, "no address", 4).unwrap();
        store.set_contact_address(&b, "127.0.0.1:1").unwrap();
        store.set_contact_address(&c, "127.0.0.1:2").unwrap();
        assert_eq!(store.next_pending_delivery(None).unwrap().unwrap(), PendingDelivery { peer: b.clone(), address: "127.0.0.1:1".into(), id: first.id.clone() });
        assert_eq!(store.next_pending_delivery(Some(&b)).unwrap().unwrap().id, other.id);
        assert_eq!(store.next_pending_delivery(Some(&c)).unwrap(), None);
        assert!(store.ack_direct(&b, &first.id).unwrap());
        assert_eq!(store.next_pending_delivery(None).unwrap().unwrap().id, second.id);
        assert!(store.next_pending_delivery(Some("invalid")).is_err());
    }

    #[test]
    fn contact_address_requires_trust_and_valid_literal_address() {
        let store = memory_store();
        let peer = "b".repeat(64);
        store.add_candidate(&peer, "Bob").unwrap();
        assert!(!store.set_contact_address(&peer, "127.0.0.1:42853").unwrap());
        assert_eq!(store.contact_address(&peer).unwrap(), None);
        assert!(!store.is_verified(&peer).unwrap());
        store.confirm_candidate(&peer).unwrap();
        assert!(store.set_contact_address(&peer, "127.0.0.1:42853").unwrap());
        for address in ["host.local:3", "0.0.0.0:1", "127.0.0.1:0", " 127.0.0.1:1"] {
            assert!(store.set_contact_address(&peer, address).is_err());
        }
        assert_eq!(store.contact_address(&peer).unwrap().as_deref(), Some("127.0.0.1:42853"));
        assert!(store.set_contact_address(&peer, "[::1]:9").unwrap());
        assert_eq!(store.contact_address(&peer).unwrap().as_deref(), Some("[::1]:9"));
        assert!(store.contact_address("bad").is_err());
    }

    #[test]
    fn discovered_address_respects_trust_and_intervening_address_changes() {
        let store = memory_store();
        let peer = "b".repeat(64);
        let unknown = "c".repeat(64);
        store.add_candidate(&peer, "Bob").unwrap();
        let revision = store.address_revision();
        assert!(!store.set_discovered_address(&unknown, "127.0.0.1:2", revision).unwrap());
        assert!(!store.set_discovered_address(&peer, "127.0.0.1:2", revision).unwrap());
        assert!(!store.is_verified(&peer).unwrap());
        store.confirm_candidate(&peer).unwrap();
        assert!(store.set_discovered_address(&peer, "127.0.0.1:2", revision).unwrap());
        let same_revision = store.address_revision();
        let writes = store.connection.total_changes();
        assert!(store.set_discovered_address(&peer, "127.0.0.1:2", same_revision).unwrap());
        assert_eq!(store.address_revision(), same_revision);
        assert_eq!(store.connection.total_changes(), writes);
        let before_probe = store.address_revision();
        assert!(store.set_contact_address(&peer, "127.0.0.1:3").unwrap());
        assert!(!store.set_discovered_address(&peer, "127.0.0.1:4", before_probe).unwrap());
        assert_eq!(store.contact_address(&peer).unwrap().as_deref(), Some("127.0.0.1:3"));
        assert!(store.set_discovered_address(&peer, "0.0.0.0:4", store.address_revision()).is_err());
        assert!(store.is_verified(&peer).unwrap());
    }

    #[test]
    fn v3_migration_preserves_messages_and_address_survives_reopen() {
        use std::{fs, time::{SystemTime, UNIX_EPOCH}};
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("storage-tests");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("addresses-{}-{}.sqlite", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        let peer = "b".repeat(64);
        let id;
        {
            let store = Store::open(&path).unwrap();
            store.add_candidate(&peer, "Bob").unwrap();
            store.confirm_candidate(&peer).unwrap();
            id = store.queue_direct(&"a".repeat(64), &peer, "still pending", 7).unwrap().id;
            store.connection.execute_batch("DROP TABLE contact_addresses; PRAGMA user_version = 3;").unwrap();
        }
        {
            let store = Store::open(&path).unwrap();
            assert!(store.is_verified(&peer).unwrap());
            assert_eq!(store.pending_direct_by_id(&peer, &id).unwrap().unwrap().body, "still pending");
            assert!(store.set_contact_address(&peer, "127.0.0.1:42853").unwrap());
        }
        let store = Store::open(&path).unwrap();
        assert_eq!(store.contact_address(&peer).unwrap().as_deref(), Some("127.0.0.1:42853"));
        assert_eq!(store.pending_direct_by_id(&peer, &id).unwrap().unwrap().status, "pending");
    }

    #[test]
    fn pending_by_id_is_not_limited_to_first_page_or_other_peers() {
        let store = memory_store();
        let me = "a".repeat(64);
        let peer = "b".repeat(64);
        let other = "c".repeat(64);
        for key in [&peer, &other] { store.add_candidate(key, "contact").unwrap(); store.confirm_candidate(key).unwrap(); }
        for _ in 0..200 { store.queue_direct(&me, &peer, "older", 1).unwrap(); }
        let last = store.queue_direct(&me, &peer, "last", 1).unwrap();
        assert!(!store.pending_direct(&peer, 200).unwrap().iter().any(|message| message.id == last.id));
        assert_eq!(store.pending_direct_by_id(&peer, &last.id).unwrap(), Some(last));
        let id = store.list_direct(&peer, 1, None).unwrap()[0].id.clone();
        assert!(store.pending_direct_by_id(&other, &id).unwrap().is_none());
        assert!(store.ack_direct(&peer, &id).unwrap());
        assert!(store.pending_direct_by_id(&peer, &id).unwrap().is_none());
        assert!(store.pending_direct_by_id(&peer, "").is_err());
    }

    #[test]
    fn conversation_list_combines_self_and_direct_and_respects_hidden() {
        let store = memory_store();
        let me = "a".repeat(64);
        let peer = "b".repeat(64);
        store.add_candidate(&peer, "Bob").unwrap();
        store.confirm_candidate(&peer).unwrap();
        store.send_self(&me, "self", 100).unwrap();
        store.queue_direct(&me, &peer, "peer", 50).unwrap();
        let list = store.list_conversations(&me).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, peer);
        assert_eq!(list[1].id, "self");
        assert!(store.hide_direct(&peer).unwrap());
        assert_eq!(store.list_conversations(&me).unwrap().len(), 1);
        assert_eq!(store.receive_direct(&peer, "new", "hello", 1, 5).unwrap(), IncomingResult::Inserted);
        assert_eq!(store.list_conversations(&me).unwrap()[0].id, peer);
    }

    #[test]
    fn direct_open_requires_trust_and_restores_hidden_conversation() {
        let store = memory_store();
        let peer = "b".repeat(64);
        store.add_candidate(&peer, "Bob").unwrap();
        assert!(!store.open_direct(&peer).unwrap());
        store.confirm_candidate(&peer).unwrap();
        assert!(store.open_direct(&peer).unwrap());
        assert!(store.hide_direct(&peer).unwrap());
        assert!(store.open_direct(&peer).unwrap());
        let hidden: bool = store.connection.query_row("SELECT hidden FROM direct_conversations WHERE peer = ?1", [&peer], |r| r.get(0)).unwrap();
        assert!(!hidden);
    }

    #[test]
    fn direct_order_uses_stable_cursor_and_ack_is_peer_scoped() {
        let store = memory_store();
        let me = "a".repeat(64);
        let peer = "b".repeat(64);
        let other = "c".repeat(64);
        for fingerprint in [&peer, &other] { store.add_candidate(fingerprint, "contact").unwrap(); store.confirm_candidate(fingerprint).unwrap(); }
        assert_eq!(store.receive_direct(&peer, "old", "old", 900, 1).unwrap(), IncomingResult::Inserted);
        assert_eq!(store.receive_direct(&peer, "b", "middle", 20, 4).unwrap(), IncomingResult::Inserted);
        assert_eq!(store.receive_direct(&peer, "c", "anchor", 10, 4).unwrap(), IncomingResult::Inserted);
        let page = store.list_direct(&peer, 1, None).unwrap();
        assert_eq!(page[0].id, "c");
        let anchor = DirectCursor { logical: page[0].logical, sender: page[0].sender.clone(), id: page[0].id.clone() };
        assert_eq!(store.receive_direct(&peer, "newer", "late arrival", 1, 9).unwrap(), IncomingResult::Inserted);
        let older = store.list_direct(&peer, 10, Some(&anchor)).unwrap();
        assert_eq!(older.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), vec!["b", "old"]);
        assert!(store.list_direct(&peer, 0, None).is_err());
        assert!(store.hide_direct(&peer).unwrap());
        let outgoing = store.queue_direct(&me, &peer, "queued", 0).unwrap();
        assert_eq!(store.pending_direct(&peer, 10).unwrap().len(), 1);
        assert!(!store.ack_direct(&other, &outgoing.id).unwrap());
        assert!(!store.ack_direct(&peer, "old").unwrap());
        assert!(store.ack_direct(&peer, &outgoing.id).unwrap());
        assert!(!store.ack_direct(&peer, &outgoing.id).unwrap());
        assert!(store.pending_direct(&peer, 10).unwrap().is_empty());
        let hidden: bool = store.connection.query_row("SELECT hidden FROM direct_conversations WHERE peer = ?1", [&peer], |r| r.get(0)).unwrap();
        assert!(!hidden);
    }

    #[test]
    fn direct_receive_validates_trust_deduplication_and_clock() {
        let store = memory_store();
        let me = "a".repeat(64);
        let peer = "b".repeat(64);
        let other = "c".repeat(64);
        store.add_candidate(&peer, "Bob").unwrap();
        store.add_candidate(&other, "Other").unwrap();
        assert_eq!(store.receive_direct(&peer, "id1", "hello", 100, 8).unwrap(), IncomingResult::Unverified);
        store.confirm_candidate(&peer).unwrap();
        store.confirm_candidate(&other).unwrap();
        assert_eq!(store.receive_direct(&peer, "id1", "hello", 100, 8).unwrap(), IncomingResult::Inserted);
        store.connection.execute("UPDATE direct_conversations SET hidden = 1 WHERE peer = ?1", [&peer]).unwrap();
        assert_eq!(store.receive_direct(&peer, "id1", "hello", 100, 8).unwrap(), IncomingResult::Duplicate);
        assert_eq!(store.receive_direct(&peer, "id1", "different", 100, 8).unwrap(), IncomingResult::IdConflict);
        assert_eq!(store.receive_direct(&peer, "id1", "hello", 101, 8).unwrap(), IncomingResult::IdConflict);
        assert_eq!(store.receive_direct(&peer, "id1", "hello", 100, 9).unwrap(), IncomingResult::IdConflict);
        assert_eq!(store.receive_direct(&other, "id1", "hello", 100, 8).unwrap(), IncomingResult::IdConflict);
        let hidden: bool = store.connection.query_row("SELECT hidden FROM direct_conversations WHERE peer = ?1", [&peer], |r| r.get(0)).unwrap();
        assert!(hidden, "a replay must not unhide a conversation");
        assert_eq!(store.receive_direct(&peer, "id2", "new", 2, 2).unwrap(), IncomingResult::Inserted);
        let hidden: bool = store.connection.query_row("SELECT hidden FROM direct_conversations WHERE peer = ?1", [&peer], |r| r.get(0)).unwrap();
        assert!(!hidden);
        assert_eq!(store.queue_direct(&me, &peer, "reply", 1).unwrap().logical, 11);
        assert!(store.receive_direct(&peer, "bad id", "hello", 0, 1).is_err());
        assert!(store.receive_direct(&peer, "id3", "hello", 0, 0).is_err());
        assert!(store.receive_direct(&peer, "id3", "hello", 0, i64::MAX).is_err());
    }

    #[test]
    fn direct_queue_requires_trust_and_allocates_logical_order_atomically() {
        let store = memory_store();
        let me = "a".repeat(64);
        let peer = "b".repeat(64);
        store.add_candidate(&peer, "Bob").unwrap();
        assert!(store.queue_direct(&me, &peer, "no trust", 200).is_err());
        let clock: i64 = store.connection.query_row("SELECT value FROM logical_clock", [], |r| r.get(0)).unwrap();
        assert_eq!(clock, 0);
        store.confirm_candidate(&peer).unwrap();
        assert!(store.queue_direct(&me, &peer, " ", 200).is_err());
        assert!(store.queue_direct(&me, &me, "self", 200).is_err());
        let first = store.queue_direct(&me, &peer, "first", 200).unwrap();
        let second = store.queue_direct(&me, &peer, "second", 100).unwrap();
        assert_ne!(first.id, second.id);
        assert_eq!((first.logical, second.logical), (1, 2));
        assert_eq!(second.status, "pending");
        let count: i64 = store.connection.query_row("SELECT count(*) FROM direct_messages WHERE peer = ?1 AND status = 'pending'", [&peer], |r| r.get(0)).unwrap();
        assert_eq!(count, 2);
        store.connection.execute("UPDATE logical_clock SET value = 9223372036854775807", []).unwrap();
        assert!(store.queue_direct(&me, &peer, "overflow", 100).is_err());
        let count_after: i64 = store.connection.query_row("SELECT count(*) FROM direct_messages", [], |r| r.get(0)).unwrap();
        assert_eq!(count_after, 2);
    }

    #[test]
    fn v2_migration_preserves_self_messages_and_clock() {
        let store = memory_store();
        let me = "a".repeat(64);
        let peer = "b".repeat(64);
        store.add_candidate(&peer, "Bob").unwrap();
        store.confirm_candidate(&peer).unwrap();
        store.send_self(&me, "existing", 5).unwrap();
        let Store { connection, .. } = store;
        connection.execute_batch("DROP TABLE contact_addresses; DROP TABLE direct_messages; DROP TABLE direct_conversations; PRAGMA user_version = 2;").unwrap();
        let upgraded = Store::initialize(connection).unwrap();
        assert_eq!(upgraded.list_self_messages(10, 0).unwrap()[0].body, "existing");
        assert_eq!(upgraded.queue_direct(&me, &peer, "new", 4).unwrap().logical, 2);
    }

    #[test]
    fn direct_queue_survives_reopening() {
        use std::{fs, time::{SystemTime, UNIX_EPOCH}};
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("storage-tests");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("direct-{}-{}.sqlite", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        let me = "a".repeat(64);
        let peer = "b".repeat(64);
        let id;
        {
            let store = Store::open(&path).unwrap();
            store.add_candidate(&peer, "Bob").unwrap();
            store.confirm_candidate(&peer).unwrap();
            id = store.queue_direct(&me, &peer, "queued", 7).unwrap().id;
        }
        let store = Store::open(&path).unwrap();
        let row: (String, String, i64) = store.connection.query_row("SELECT body, status, logical FROM direct_messages WHERE id = ?1", [&id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        assert_eq!(row, ("queued".into(), "pending".into(), 1));
        assert_eq!(store.queue_direct(&me, &peer, "after restart", 6).unwrap().logical, 2);
    }

    #[test]
    fn self_recall_preserves_order_and_enforces_author_and_time() {
        let store = memory_store();
        let me = "a".repeat(64);
        let other = "b".repeat(64);
        let first = store.send_self(&me, "secret", 10).unwrap();
        let second = store.send_self(&me, "too old", 10).unwrap();
        assert_eq!(store.recall_self(&other, &first.id, 100).unwrap(), RecallResult::NotYours);
        assert_eq!(store.recall_self(&me, &second.id, 131).unwrap(), RecallResult::TooLate);
        assert_eq!(store.recall_self(&me, &first.id, 130).unwrap(), RecallResult::Recalled { delivered: false });
        assert_eq!(store.recall_self(&me, &first.id, 131).unwrap(), RecallResult::Already);
        let rows = store.list_self_messages(10, 0).unwrap();
        assert_eq!(rows[1].id, first.id);
        assert_eq!(rows[1].body, " ");
        assert_eq!(rows[1].recalled_at, Some(130));
        assert_eq!(store.recall_self(&me, "missing", 1).unwrap(), RecallResult::Missing);
    }

    #[test]
    fn self_recall_snapshots_restore_old_and_new_schema() {
        let source = memory_store();
        let me = "a".repeat(64);
        let message = source.send_self(&me, "secret", 10).unwrap();
        source.recall_self(&me, &message.id, 11).unwrap();
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("self-recall-snapshots")
            .join(format!("{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("snapshot.sqlite");
        source.snapshot_to(&path, true).unwrap();
        let mut restored = memory_store();
        restored.restore_from(&path, false, 20).unwrap();
        assert_eq!(restored.list_self_messages(10, 0).unwrap()[0].recalled_at, Some(11));
        let old = Connection::open(&path).unwrap();
        old.execute_batch("ALTER TABLE local_messages DROP COLUMN recalled_at; PRAGMA user_version = 25;").unwrap();
        drop(old);
        let mut legacy_restored = memory_store();
        legacy_restored.restore_from(&path, false, 20).unwrap();
        assert_eq!(legacy_restored.list_self_messages(10, 0).unwrap()[0].recalled_at, None);
    }

    #[test]
    fn self_messages_remain_ordered_when_wall_clock_goes_back_and_restore_visibility() {
        let store = memory_store();
        let sender = "a".repeat(64);
        store.open_self_conversation().unwrap();
        assert!(store.list_conversations(&sender).unwrap().is_empty());
        assert!(store.send_self(&sender, " ", 20).is_err());
        assert!(store.send_self(&sender, &"x".repeat(32769), 20).is_err());
        let first = store.send_self(&sender, "first", 200).unwrap();
        assert!(store.hide_self_conversation().unwrap());
        assert!(store.list_conversations(&sender).unwrap().is_empty());
        assert_eq!(store.list_self_messages(20, 0).unwrap().len(), 1);
        let second = store.send_self(&sender, "second", 100).unwrap();
        assert_ne!(first.id, second.id);
        assert_eq!((first.logical, second.logical), (1, 2));
        assert_eq!(store.list_self_messages(20, 0).unwrap()[0].body, "second");
        assert_eq!(store.list_conversations(&sender).unwrap()[0].last_body, "second");
        assert!(store.hide_self_conversation().unwrap());
        store.open_self_conversation().unwrap();
        assert_eq!(store.list_conversations(&sender).unwrap().len(), 1);
        assert!(store.list_self_messages(0, 0).is_err());
        assert!(store.list_self_messages(201, 0).is_err());
    }

    #[test]
    fn v1_database_is_migrated_without_losing_contacts_or_messages() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE contacts(fingerprint TEXT PRIMARY KEY NOT NULL, display_name TEXT NOT NULL, trust_state TEXT NOT NULL);
            CREATE TABLE messages(id TEXT PRIMARY KEY NOT NULL, peer_fingerprint TEXT NOT NULL, body TEXT NOT NULL, direction TEXT NOT NULL, created_at INTEGER NOT NULL, status TEXT NOT NULL);
            PRAGMA user_version = 1;").unwrap();
        let peer = "b".repeat(64);
        connection.execute("INSERT INTO contacts VALUES (?1, 'Bob', 'verified')", [&peer]).unwrap();
        connection.execute("INSERT INTO messages VALUES ('old', ?1, 'preserved', 'outgoing', 1, 'pending')", [&peer]).unwrap();
        let store = Store::initialize(connection).unwrap();
        assert!(store.is_verified(&peer).unwrap());
        assert_eq!(store.list_messages(&peer, 20, 0).unwrap()[0].body, "preserved");
        assert_eq!(store.send_self(&"a".repeat(64), "new", 2).unwrap().logical, 1);
    }

    #[test]
    fn pending_contact_cannot_receive_saved_messages_until_verified() {
        let store = memory_store();
        let fingerprint = "a".repeat(64);
        assert!(store.add_candidate(&fingerprint, "Alice").unwrap());
        assert!(!store.is_verified(&fingerprint).unwrap());
        assert!(!store.save_outgoing("m1", &fingerprint, "hello", 1).unwrap());
        assert!(store.confirm_candidate(&fingerprint).unwrap());
        assert!(store.is_verified(&fingerprint).unwrap());
        assert!(store.save_outgoing("m1", &fingerprint, "hello", 1).unwrap());
        assert!(store.save_outgoing("m1", &fingerprint, "duplicate", 2).is_err());
    }

    #[test]
    fn rejects_bad_fingerprint_and_untrusted_peers() {
        let store = memory_store();
        assert!(store.add_candidate("not-a-fingerprint", "Bob").is_err());
        assert!(!store.confirm_candidate(&"f".repeat(64)).unwrap());
        assert!(!store.save_outgoing("m", &"f".repeat(64), "hello", 1).unwrap());
    }

    #[test]
    fn does_not_replace_or_downgrade_existing_trust() {
        let store = memory_store();
        let fingerprint = "1".repeat(64);
        assert!(store.add_candidate(&fingerprint, "Alice").unwrap());
        assert!(store.confirm_candidate(&fingerprint).unwrap());
        assert!(!store.add_candidate(&fingerprint, "Impostor").unwrap());
        assert!(store.is_verified(&fingerprint).unwrap());
    }

    #[test]
    fn refuses_unknown_future_schema_without_modification() {
        let connection = Connection::open_in_memory().unwrap();
        connection.pragma_update(None, "user_version", 99).unwrap();
        assert!(Store::initialize(connection).is_err());
    }

    #[test]
    fn queries_are_bounded_parameterized_and_ordered() {
        let store = memory_store();
        let peer = "a".repeat(64);
        let other = "b".repeat(64);
        store.add_candidate(&peer, "Robert'); DROP TABLE contacts;--").unwrap();
        store.add_candidate(&other, "Other").unwrap();
        store.confirm_candidate(&peer).unwrap();
        store.save_outgoing("1", &peer, "first", 10).unwrap();
        store.save_outgoing("2", &peer, "second", 10).unwrap();
        assert_eq!(store.list_contacts().unwrap().len(), 2);
        assert_eq!(store.list_messages(&peer, 1, 0).unwrap()[0].body, "second");
        assert_eq!(store.list_messages(&peer, 1, 1).unwrap()[0].body, "first");
        assert!(store.list_messages(&other, 20, 0).unwrap().is_empty());
        assert!(store.list_messages("' OR 1=1 --", 20, 0).is_err());
        assert!(store.list_messages(&peer, 0, 0).is_err());
        assert!(store.list_messages(&peer, 201, 0).is_err());
        assert!(store.list_messages(&peer, 1, 100_001).is_err());
    }

    #[test]
    fn contacts_and_messages_survive_reopen() {
        use std::{fs, time::{SystemTime, UNIX_EPOCH}};
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("storage-tests");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{}-{}.sqlite", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        let peer = "a".repeat(64);
        {
            let store = Store::open(&path).unwrap();
            store.add_candidate(&peer, "Alice").unwrap();
            store.confirm_candidate(&peer).unwrap();
            store.save_outgoing("id", &peer, "persistent", 5).unwrap();
        }
        let store = Store::open(&path).unwrap();
        assert!(store.is_verified(&peer).unwrap());
        assert_eq!(store.list_messages(&peer, 20, 0).unwrap()[0].body, "persistent");
    }

    #[test]
    fn group_file_offer_receives_resumes_and_completes() {
        let sender = memory_store();
        let receiver = memory_store();
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        sender.add_candidate(&b, "B").unwrap();
        sender.confirm_candidate(&b).unwrap();
        let group = sender.create_group(&a, "team", &[b.clone()], 1).unwrap();
        let dir = std::env::temp_dir().join(format!(
            "qbird-group-file-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("note.txt");
        std::fs::write(&source, b"hello").unwrap();
        let sha = crate::files::sha256_file(&source).unwrap();
        let message = sender.queue_group_file(&a, &group.id, "note.txt", 5, &sha, &source, 2).unwrap();
        assert_eq!(message.kind, "file");
        assert_eq!(message.pending, 1);

        receiver.add_candidate(&a, "A").unwrap();
        receiver.confirm_candidate(&a).unwrap();
        let members = vec![a.clone(), b.clone()];
        let first = receiver.receive_group_file(&b, &a, &group.id, "team", &a, &members, &message.id, "note.txt", 5, &sha, 2, message.logical, 3).unwrap();
        let partial = match first {
            GroupFileDecision::Ready { offset: 0, partial, .. } => partial,
            other => panic!("expected a fresh transfer, got {other:?}"),
        };
        std::fs::create_dir_all(partial.parent().unwrap()).unwrap();
        std::fs::write(&partial, b"hel").unwrap();
        let resumed = receiver.receive_group_file(&b, &a, &group.id, "team", &a, &members, &message.id, "note.txt", 5, &sha, 2, message.logical, 4).unwrap();
        assert!(matches!(resumed, GroupFileDecision::Ready { offset: 3, .. }));
        receiver.complete_group_file_transfer(&message.id, 5).unwrap();
        let done = receiver.receive_group_file(&b, &a, &group.id, "team", &a, &members, &message.id, "note.txt", 5, &sha, 2, message.logical, 6).unwrap();
        assert!(matches!(done, GroupFileDecision::Complete));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unread_counts_follow_incoming_messages_and_clear_on_open() {
        let store = memory_store();
        let peer = "a".repeat(64);
        store.add_candidate(&peer, "peer").unwrap();
        store.confirm_candidate(&peer).unwrap();
        store.receive_direct(&peer, "msg-1", "hello", 10, 10).unwrap();
        store.receive_direct(&peer, "msg-2", "again", 11, 11).unwrap();
        let rows = store.list_conversations(&"b".repeat(64)).unwrap();
        assert_eq!(rows.iter().find(|item| item.kind == "direct").map(|item| item.unread), Some(2));
        store.open_direct(&peer).unwrap();
        let rows = store.list_conversations(&"b".repeat(64)).unwrap();
        assert_eq!(rows.iter().find(|item| item.kind == "direct").map(|item| item.unread), Some(0));
    }

    #[test]
    fn self_history_and_hidden_state_survive_reopen() {
        use std::{fs, time::{SystemTime, UNIX_EPOCH}};
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("storage-tests");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("self-{}-{}.sqlite", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        let me = "a".repeat(64);
        {
            let store = Store::open(&path).unwrap();
            store.send_self(&me, "before restart", 200).unwrap();
            store.hide_self_conversation().unwrap();
        }
        let store = Store::open(&path).unwrap();
        assert!(store.list_conversations(&me).unwrap().is_empty());
        assert_eq!(store.list_self_messages(20, 0).unwrap()[0].body, "before restart");
        assert_eq!(store.send_self(&me, "after restart", 100).unwrap().logical, 2);
        assert_eq!(store.list_conversations(&me).unwrap()[0].last_body, "after restart");
    }

    #[test]
    fn incoming_messages_require_trust_and_exact_retry_payload() {
        let store = memory_store();
        let alice = "a".repeat(64);
        let bob = "b".repeat(64);
        store.add_candidate(&alice, "Alice").unwrap();
        store.add_candidate(&bob, "Bob").unwrap();
        assert_eq!(store.save_incoming("m1", &alice, "hello", 42).unwrap(), IncomingResult::Unverified);
        store.confirm_candidate(&alice).unwrap();
        store.confirm_candidate(&bob).unwrap();
        assert_eq!(store.save_incoming("m1", &alice, "hello", 42).unwrap(), IncomingResult::Inserted);
        assert_eq!(store.save_incoming("m1", &alice, "hello", 42).unwrap(), IncomingResult::Duplicate);
        assert_eq!(store.save_incoming("m1", &alice, "changed", 42).unwrap(), IncomingResult::IdConflict);
        assert_eq!(store.save_incoming("m1", &bob, "hello", 42).unwrap(), IncomingResult::IdConflict);
        assert_eq!(store.save_incoming("m1", &alice, "hello", 43).unwrap(), IncomingResult::IdConflict);
        assert_eq!(store.list_messages(&alice, 20, 0).unwrap().len(), 1);
        assert!(store.save_incoming("", &alice, "body", 42).is_err());
        assert!(store.save_incoming("m2", &alice, &"x".repeat(32769), 42).is_err());
    }

    #[test]
    fn ack_cannot_mark_another_peers_message_delivered() {
        let store = memory_store();
        let alice = "a".repeat(64);
        let bob = "b".repeat(64);
        for peer in [&alice, &bob] { store.add_candidate(peer, "peer").unwrap(); store.confirm_candidate(peer).unwrap(); }
        assert!(store.save_outgoing("m1", &alice, "hello", 42).unwrap());
        assert!(!store.mark_delivered("m1", &bob).unwrap());
        assert!(store.mark_delivered("m1", &alice).unwrap());
        assert!(!store.mark_delivered("m1", &alice).unwrap());
        assert_eq!(store.list_messages(&alice, 1, 0).unwrap()[0].status, "delivered");
    }

    #[test]
    fn file_queue_and_receive_offer_track_state_and_resume() {
        use std::fs;
        let sender = memory_store();
        let receiver = memory_store();
        let me = "a".repeat(64);
        let peer = "b".repeat(64);
        let path = std::env::temp_dir().join(format!("qbird-queue-{}-{}.txt", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::write(&path, b"hello file").unwrap();
        sender.add_candidate(&peer, "Bob").unwrap();
        sender.confirm_candidate(&peer).unwrap();
        assert!(sender.queue_direct_file(&me, &me, "a.txt", 10, &"c".repeat(64), &path, 1).is_err());
        assert!(sender.queue_direct_file(&me, &peer, "a.txt", 10, &"z".repeat(64), &path, 1).is_err(), "hash must be 64 hex chars");
        let hash = crate::files::sha256_file(&path).unwrap();
        let queued = sender.queue_direct_file(&me, &peer, "../报告.txt", 10, &hash, &path, 5).unwrap();
        assert_eq!(queued.kind, "file");
        assert_eq!(queued.file_name.as_deref(), Some("报告.txt"));
        assert_eq!(queued.body, "报告.txt");
        assert_eq!(queued.file_status.as_deref(), Some("pending"));
        assert_eq!(queued.file_size, Some(10));
        assert!(sender.file_transfer(&queued.id).unwrap().unwrap().source_path.is_some());
        assert_eq!(sender.list_file_progress(&peer).unwrap().len(), 1);

        // The receiver's view is independent and only exists after a valid offer.
        assert!(receiver.receive_file_offer(&me, &queued.id, "a.txt", 10, &hash, 5, 1, 10).unwrap() == FileOfferDecision::Untrusted);
        receiver.add_candidate(&me, "Alice").unwrap();
        receiver.confirm_candidate(&me).unwrap();
        let decision = receiver.receive_file_offer(&me, &queued.id, "报告.txt", 10, &hash, 5, 1, 10).unwrap();
        let FileOfferDecision::Ready { offset, target, partial } = decision else { panic!("expected ready") };
        assert_eq!(offset, 0);
        assert!(target.ends_with("附件/报告.txt") || target.to_string_lossy().contains("报告.txt"));
        fs::create_dir_all(partial.parent().unwrap()).unwrap();
        fs::write(&partial, b"hello").unwrap();
        let resumed = receiver.receive_file_offer(&me, &queued.id, "报告.txt", 10, &hash, 5, 1, 11).unwrap();
        assert_eq!(resumed, FileOfferDecision::Ready { offset: 5, target: target.clone(), partial: partial.clone() });
        assert_eq!(receiver.list_direct(&me, 10, None).unwrap()[0].kind, "file");
        receiver.update_file_progress(&queued.id, 8, 12).unwrap();
        assert_eq!(receiver.file_transfer(&queued.id).unwrap().unwrap().transferred, 8);
        fs::write(&target, b"hello file").unwrap();
        assert!(receiver.complete_file_transfer(&queued.id, 13).unwrap());
        assert_eq!(receiver.attachment_path(&queued.id).unwrap().map(|(name, _)| name).as_deref(), Some("报告.txt"));
        assert_eq!(receiver.receive_file_offer(&me, &queued.id, "报告.txt", 10, &hash, 5, 1, 14).unwrap(), FileOfferDecision::Complete);
        assert_eq!(receiver.receive_file_offer(&me, &queued.id, "报告.txt", 10, &"d".repeat(64), 5, 1, 15).unwrap(), FileOfferDecision::Conflict);
        receiver.fail_file_transfer(&queued.id, "校验失败", 16).unwrap();
        assert_eq!(receiver.file_transfer(&queued.id).unwrap().unwrap().status, "failed");
        crate::files::remove_quietly(&path);
        crate::files::remove_quietly(&partial);
        crate::files::remove_quietly(&target);
    }

    #[test]
    fn file_queue_requires_verified_peer_and_valid_metadata() {
        let store = memory_store();
        let me = "a".repeat(64);
        let peer = "b".repeat(64);
        let path = std::env::temp_dir().join(format!("qbird-queue-b-{}-{}.txt", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::write(&path, b"x").unwrap();
        let hash = crate::files::sha256_file(&path).unwrap();
        assert!(store.queue_direct_file(&me, &peer, "a.txt", 1, &hash, &path, 1).is_err());
        store.add_candidate(&peer, "Bob").unwrap();
        store.confirm_candidate(&peer).unwrap();
        assert!(store.queue_direct_file(&me, &peer, "a.txt", 0, &hash, &path, 1).is_err());
        assert!(store.queue_direct_file(&me, &peer, "..", 1, &hash, &path, 1).is_err());
        assert!(store.queue_direct_file(&me, &peer, "a.txt", crate::protocol::MAX_FILE_BYTES + 1, &hash, &path, 1).is_err());
        assert!(store.receive_file_offer(&peer, "bad id", "a.txt", 1, &hash, 1, 1, 1).is_err());
        crate::files::remove_quietly(&path);
    }

    #[test]
    fn group_creation_requires_verified_members_and_queues_invites() {
        let store = memory_store();
        let me = "a".repeat(64);
        let bob = "b".repeat(64);
        let carol = "c".repeat(64);
        assert!(store.create_group(&me, "Team", &[bob.clone()], 1).is_err());
        store.add_candidate(&bob, "Bob").unwrap();
        store.confirm_candidate(&bob).unwrap();
        store.add_candidate(&carol, "Carol").unwrap();
        store.confirm_candidate(&carol).unwrap();
        assert!(store.create_group(&me, "  ", &[bob.clone()], 1).is_err());
        assert!(store.create_group(&me, "Team", &[me.clone()], 1).is_err());
        let group = store.create_group(&me, "  Team  ", &[bob.clone(), carol.clone()], 10).unwrap();
        assert_eq!(group.name, "Team");
        assert_eq!(group.member_count, 3);
        assert!(group.is_owner);
        assert_eq!(store.list_group_members(&group.id).unwrap().len(), 3);
        assert_eq!(store.list_group_members(&group.id).unwrap().iter().filter(|m| m.role == "owner").count(), 1);
        // Invites wait for an address before the retry loop can send them.
        assert_eq!(store.next_pending_control(None).unwrap(), None);
        store.set_contact_address(&bob, "127.0.0.1:1001").unwrap();
        store.set_contact_address(&carol, "127.0.0.1:1002").unwrap();
        let task = store.next_pending_control(None).unwrap().unwrap();
        assert_eq!(task.target, bob);
        assert_eq!(task.kind, "invite");
        assert!(task.payload.contains("Team"));
        assert!(store.ack_control(&task.id).unwrap());
        assert_eq!(store.next_pending_control(Some(&bob)).unwrap().unwrap().target, carol);
    }

    #[test]
    fn group_message_fans_out_and_acks_per_member() {
        let store = memory_store();
        let me = "a".repeat(64);
        let bob = "b".repeat(64);
        let carol = "c".repeat(64);
        for (peer, name) in [(&bob, "Bob"), (&carol, "Carol")] {
            store.add_candidate(peer, name).unwrap();
            store.confirm_candidate(peer).unwrap();
        }
        let group = store.create_group(&me, "Team", &[bob.clone(), carol.clone()], 1).unwrap();
        store.set_contact_address(&bob, "127.0.0.1:1").unwrap();
        store.set_contact_address(&carol, "127.0.0.1:2").unwrap();
        let message = store.queue_group(&me, &group.id, "hello", None, &[], 20).unwrap();
        assert_eq!(message.pending, 2);
        assert_eq!(store.list_group_messages(&group.id, 10, None).unwrap()[0].pending, 2);
        let dispatch = store.group_dispatch(&message.id).unwrap().unwrap();
        assert_eq!(dispatch.members, vec![me.clone(), bob.clone(), carol.clone()]);
        let first = store.next_pending_group_delivery(None).unwrap().unwrap();
        assert_eq!(first.message_id, message.id);
        assert!(store.ack_group(&first.member, &message.id).unwrap());
        assert!(!store.ack_group(&first.member, &message.id).unwrap());
        let second = store.next_pending_group_delivery(None).unwrap().unwrap();
        assert_ne!(second.member, first.member);
        assert!(store.ack_group(&second.member, &message.id).unwrap());
        assert_eq!(store.next_pending_group_delivery(None).unwrap(), None);
        assert_eq!(store.list_group_messages(&group.id, 10, None).unwrap()[0].pending, 0);
        // A non-member cannot post into the group.
        assert!(store.queue_group(&"d".repeat(64), &group.id, "nope", None, &[], 21).is_err());
    }

    #[test]
    fn group_receive_builds_membership_and_only_owner_may_rewrite_roster() {
        let store = memory_store();
        let me = "a".repeat(64);
        let owner = "b".repeat(64);
        let carol = "c".repeat(64);
        let stranger = "d".repeat(64);
        for (peer, name) in [(&owner, "Owner"), (&carol, "Carol"), (&stranger, "Stranger")] {
            store.add_candidate(peer, name).unwrap();
            store.confirm_candidate(peer).unwrap();
        }
        let gid = "1".repeat(32);
        assert_eq!(
            store.receive_group(&me, &owner, &gid, "Team", &owner, &[owner.clone(), me.clone()], "m1", "hi", 1, 5, None, None, None, &[], 100).unwrap(),
            IncomingResult::Inserted,
        );
        assert_eq!(
            store.receive_group(&me, &owner, &gid, "Team", &owner, &[owner.clone(), me.clone()], "m1", "hi", 1, 5, None, None, None, &[], 101).unwrap(),
            IncomingResult::Duplicate,
        );
        assert_eq!(
            store.receive_group(&me, &owner, &gid, "Team", &owner, &[owner.clone(), me.clone()], "m1", "changed", 1, 5, None, None, None, &[], 102).unwrap(),
            IncomingResult::IdConflict,
        );
        // Carol is not a member yet: her frame cannot add her to the roster.
        assert_eq!(
            store.receive_group(&me, &carol, &gid, "Team", &owner, &[owner.clone(), carol.clone(), me.clone()], "m2", "hey", 2, 6, None, None, None, &[], 103).unwrap(),
            IncomingResult::NotMember,
        );
        assert!(store.list_group_members(&gid).unwrap().iter().all(|member| member.member != carol));
        // Only the owner can extend the roster.
        assert_eq!(
            store.receive_group(&me, &owner, &gid, "Team", &owner, &[owner.clone(), carol.clone(), me.clone()], "m3", "join", 3, 7, None, None, None, &[], 104).unwrap(),
            IncomingResult::Inserted,
        );
        assert_eq!(store.list_group_members(&gid).unwrap().iter().filter(|member| member.active).count(), 3);
        // A roster without us means we were removed.
        assert_eq!(
            store.receive_group(&me, &owner, &gid, "Team", &owner, &[owner.clone(), carol.clone()], "m4", "bye", 4, 8, None, None, None, &[], 105).unwrap(),
            IncomingResult::NotMember,
        );
        assert_eq!(store.list_group_members(&gid).unwrap().iter().filter(|member| member.active).count(), 2);
        assert_eq!(store.list_group_messages(&gid, 10, None).unwrap().len(), 2);
    }

    /// Minimal header-only PNG: the app validates the signature and square dimensions.
    fn png_header(edge: u32) -> Vec<u8> {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13];
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&edge.to_be_bytes());
        bytes.extend_from_slice(&edge.to_be_bytes());
        bytes
    }

    #[test]
    fn image_batches_link_messages_into_one_bubble() {
        let store = memory_store();
        let me = "a".repeat(64);
        let peer = "b".repeat(64);
        store.add_candidate(&peer, "Bob").unwrap();
        store.confirm_candidate(&peer).unwrap();
        let first = store.queue_direct_file(&me, &peer, "a.png", 3, &"c".repeat(64), std::path::Path::new("C:/tmp/a.png"), 1).unwrap();
        let second = store.queue_direct_file(&me, &peer, "b.png", 4, &"d".repeat(64), std::path::Path::new("C:/tmp/b.png"), 1).unwrap();
        let batch = store.new_batch_id().unwrap();
        assert_eq!(batch.len(), 32);
        assert!(store.attach_image_batch(&first.id, &batch, "看这两张", 2).unwrap());
        assert!(store.attach_image_batch(&second.id, &batch, "看这两张", 2).unwrap());
        // A retransmit rewrites the same row instead of failing.
        assert!(store.attach_image_batch(&first.id, &batch, "改个说明", 3).unwrap());
        assert_eq!(store.image_batch_of(&first.id).unwrap(), Some((batch.clone(), "改个说明".to_owned())));
        assert_eq!(store.image_batch_of(&second.id).unwrap(), Some((batch.clone(), "看这两张".to_owned())));
        let listed = store.list_image_batches("direct", &peer).unwrap();
        assert_eq!(listed.len(), 2);
        assert!(listed.iter().all(|row| row.batch_id == batch));
        assert!(listed.iter().all(|row| row.anchor_id == first.id));
        // Unknown messages, bad ids and over-long captions are rejected.
        assert!(!store.attach_image_batch("missing", &batch, "", 1).unwrap());
        assert!(store.attach_image_batch(&first.id, "short", "", 1).is_err());
        assert!(store.attach_image_batch(&first.id, &batch, &"x".repeat(crate::protocol::MAX_BATCH_CAPTION + 1), 1).is_err());
        assert!(store.list_image_batches("self", &peer).is_err());
    }

    #[test]
    fn direct_and_group_image_quotes_persist_captions_and_reject_other_files() {
        let store = memory_store();
        let me = "a".repeat(64);
        let peer = "b".repeat(64);
        store.add_candidate(&peer, "Bob").unwrap();
        store.confirm_candidate(&peer).unwrap();

        let direct_image = store.queue_direct_file(&me, &peer, "photo.png", 3, &"c".repeat(64), Path::new("photo.png"), 1).unwrap();
        let batch = store.new_batch_id().unwrap();
        store.attach_image_batch(&direct_image.id, &batch, "直聊图片说明", 2).unwrap();
        let direct_reply = store.queue_direct_file_with_reply(&me, &peer, "reply.jpg", 4, &"d".repeat(64), Path::new("reply.jpg"), 3, Some(&direct_image.id)).unwrap();
        assert_eq!(direct_reply.reply_to.as_deref(), Some(direct_image.id.as_str()));
        assert_eq!(direct_reply.reply_sender.as_deref(), Some("我"));
        assert_eq!(direct_reply.reply_preview.as_deref(), Some("直聊图片说明"));
        let listed_direct = store.list_direct(&peer, 10, None).unwrap();
        let persisted_direct = listed_direct.iter().find(|message| message.id == direct_reply.id).unwrap();
        assert_eq!(persisted_direct.reply_preview.as_deref(), Some("直聊图片说明"));

        let document = store.queue_direct_file(&me, &peer, "notes.pdf", 5, &"e".repeat(64), Path::new("notes.pdf"), 4).unwrap();
        assert!(store.queue_direct_with_reply(&me, &peer, "not allowed", 5, Some(&document.id)).is_err());

        let incoming_reply = (Some("quoted-text"), Some("Alice"), Some("原消息"));
        assert!(matches!(store.receive_file_offer_with_reply(&peer, "incomingimage01", "incoming.png", 6,
            &"4".repeat(64), 11, 100, incoming_reply.0, incoming_reply.1, incoming_reply.2, 12).unwrap(),
            FileOfferDecision::Ready { .. }));
        assert!(matches!(store.receive_file_offer_with_reply(&peer, "incomingimage01", "incoming.png", 6,
            &"4".repeat(64), 11, 100, incoming_reply.0, incoming_reply.1, incoming_reply.2, 13).unwrap(),
            FileOfferDecision::Ready { .. }));
        assert!(matches!(store.receive_file_offer_with_reply(&peer, "incomingimage01", "incoming.png", 6,
            &"4".repeat(64), 11, 100, incoming_reply.0, incoming_reply.1, Some("被篡改"), 14).unwrap(),
            FileOfferDecision::Conflict));

        let (group_id, group_peer, group_other) = group_with_two_members(&store, &me);
        let group_image = store.queue_group_file(&me, &group_id, "group.png", 3, &"f".repeat(64), Path::new("group.png"), 6).unwrap();
        let group_batch = store.new_batch_id().unwrap();
        store.attach_image_batch(&group_image.id, &group_batch, "群聊图片说明", 7).unwrap();
        let group_batches = store.list_image_batches("group", &group_id).unwrap();
        assert!(group_batches.iter().filter(|row| row.batch_id == group_batch).all(|row| row.anchor_id == group_image.id));
        let group_reply = store.queue_group_file_with_reply(&me, &group_id, "reply.png", 4, &"1".repeat(64), Path::new("reply.png"), 8, Some(&group_image.id)).unwrap();
        assert_eq!(group_reply.reply_to.as_deref(), Some(group_image.id.as_str()));
        assert_eq!(group_reply.reply_preview.as_deref(), Some("群聊图片说明"));
        let listed_group = store.list_group_messages(&group_id, 10, None).unwrap();
        let persisted_group = listed_group.iter().find(|message| message.id == group_reply.id).unwrap();
        assert_eq!(persisted_group.reply_preview.as_deref(), Some("群聊图片说明"));

        let group_document = store.queue_group_file(&me, &group_id, "notes.pdf", 5, &"2".repeat(64), Path::new("notes.pdf"), 9).unwrap();
        assert!(store.queue_group_file_with_reply(&me, &group_id, "reply.png", 4, &"3".repeat(64), Path::new("reply.png"), 10, Some(&group_document.id)).is_err());

        let members = vec![me.clone(), group_peer.clone(), group_other];
        assert!(matches!(store.receive_group_file_with_reply(&me, &group_peer, &group_id, "Team", &me,
            &members, "groupincoming01", "incoming.png", 6, &"5".repeat(64), 11, 100,
            incoming_reply.0, incoming_reply.1, incoming_reply.2, 12).unwrap(), GroupFileDecision::Ready { .. }));
        assert!(matches!(store.receive_group_file_with_reply(&me, &group_peer, &group_id, "Team", &me,
            &members, "groupincoming01", "incoming.png", 6, &"5".repeat(64), 11, 100,
            incoming_reply.0, incoming_reply.1, incoming_reply.2, 13).unwrap(), GroupFileDecision::Ready { .. }));
        assert!(matches!(store.receive_group_file_with_reply(&me, &group_peer, &group_id, "Team", &me,
            &members, "groupincoming01", "incoming.png", 6, &"5".repeat(64), 11, 100,
            incoming_reply.0, incoming_reply.1, Some("被篡改"), 14).unwrap(), GroupFileDecision::Conflict));
    }

    #[test]
    fn avatar_authorization_requires_friend_or_active_shared_group() {
        let store = memory_store();
        let me = "a".repeat(64);
        let member = "b".repeat(64);
        let outsider = "c".repeat(64);
        store.add_candidate(&member, "Member").unwrap();
        store.confirm_candidate(&member).unwrap();
        let group = store.create_group(&me, "Team", &[member.clone()], 1).unwrap();
        store.connection.execute("UPDATE contacts SET trust_state = 'pending' WHERE fingerprint = ?1", [&member]).unwrap();
        assert!(store.avatar_authorized(&me, &member).unwrap());
        assert!(!store.avatar_authorized(&me, &outsider).unwrap());
        assert_eq!(store.avatar_group_peers(&me).unwrap(), vec![member.clone()]);
        assert!(!store.is_verified(&member).unwrap());
        assert!(!store.set_avatar_address(&me, &outsider, "127.0.0.1:41").unwrap());
        assert!(store.set_avatar_address(&me, &member, "0.0.0.0:41").is_err());
        assert!(store.set_avatar_address(&me, &member, "127.0.0.1:41").unwrap());
        assert_eq!(store.avatar_address(&me, &member).unwrap().as_deref(), Some("127.0.0.1:41"));
        assert_eq!(store.contact_address(&member).unwrap(), None);
        let png = png_header(128);
        assert!(!store.set_peer_avatar(&me, &outsider, Some(&png)).unwrap());
        assert!(store.set_peer_avatar(&me, &member, Some(b"bad png")).is_err());
        let mut oversized = png.clone();
        oversized.resize(crate::avatar::MAX_AVATAR_BYTES + 1, 0);
        assert!(store.set_peer_avatar(&me, &member, Some(&oversized)).is_err());
        assert!(store.set_peer_avatar(&me, &member, Some(&png)).unwrap());
        assert_eq!(store.peer_avatar(&me, &member).unwrap(), Some(png.clone()));
        assert!(store.set_peer_avatar(&me, &member, None).unwrap());
        assert_eq!(store.peer_avatar(&me, &member).unwrap(), None);
        assert!(store.set_peer_avatar(&me, &member, Some(&png)).unwrap());
        store.connection.execute("UPDATE group_members SET active = 0 WHERE group_id = ?1 AND member = ?2", params![group.id, member]).unwrap();
        assert!(!store.avatar_authorized(&me, &member).unwrap());
        assert_eq!(store.peer_avatar(&me, &member).unwrap(), None);
        assert_eq!(store.avatar_address(&me, &member).unwrap(), None);
        assert!(!store.set_peer_avatar(&me, &member, None).unwrap());
        assert!(store.avatar_group_peers(&me).unwrap().is_empty());
        store.connection.execute("UPDATE group_members SET active = 1 WHERE group_id = ?1 AND member = ?2", params![group.id, member]).unwrap();
        store.connection.execute("UPDATE groups SET active = 0 WHERE id = ?1", [&group.id]).unwrap();
        assert!(!store.avatar_authorized(&me, &member).unwrap());
        store.confirm_candidate(&member).unwrap();
        assert!(store.avatar_authorized(&me, &member).unwrap());
    }

    #[test]
    fn group_avatar_is_owner_only_and_members_accept_owner_snapshots() {
        let store = memory_store();
        let owner = "b".repeat(64);
        let me = "a".repeat(64);
        let stranger = "c".repeat(64);
        for peer in [&owner, &me, &stranger] {
            store.add_candidate(peer, "P").unwrap();
            store.confirm_candidate(peer).unwrap();
        }
        let group = store.create_group(&owner, "Team", &[me.clone()], 1).unwrap();
        let png = png_header(128);

        // Non-owners cannot set or clear the avatar.
        assert!(!store.set_group_avatar(&me, &group.id, Some(&png), 2).unwrap());
        // Malformed or oversized images are rejected.
        assert!(store.set_group_avatar(&owner, &group.id, Some(b"not a png"), 3).is_err());
        assert!(store.set_group_avatar(&owner, &group.id, Some(&vec![0u8; crate::protocol::MAX_GROUP_AVATAR_BYTES + 1]), 3).is_err());
        assert!(store.set_group_avatar(&owner, &group.id, Some(&png), 4).unwrap());
        assert_eq!(store.group_avatar(&group.id).unwrap().as_deref(), Some(png.as_slice()));

        // Members only accept the owner's snapshot.
        assert_eq!(store.receive_group_avatar(&me, &stranger, &group.id, Some(&png), 5).unwrap(), IncomingResult::Unverified);
        assert_eq!(store.receive_group_avatar(&me, &owner, &group.id, Some(&png), 6).unwrap(), IncomingResult::Inserted);
        assert!(store.set_group_avatar(&owner, &group.id, None, 7).unwrap());
        assert!(store.group_avatar(&group.id).unwrap().is_none());
    }

    #[test]
    fn group_join_requests_are_queued_approved_and_refused() {
        let store = memory_store();
        let owner = "b".repeat(64);
        let alice = "a".repeat(64);
        let bob = "c".repeat(64);
        for (index, peer) in [&alice, &owner, &bob].into_iter().enumerate() {
            store.add_candidate(peer, "P").unwrap();
            store.confirm_candidate(peer).unwrap();
            store.set_contact_address(peer, &format!("127.0.0.1:{}", 7000 + index)).unwrap();
        }
        let group = store.create_group(&owner, "研发一组", &[alice.clone()], 1).unwrap();
        let other = store.create_group(&owner, "市场组", &[alice.clone()], 1).unwrap();

        // Only groups the local device is a member of are reported, filtered by name.
        let found = store.search_known_groups(&owner, "研发").unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, group.id);
        assert_eq!(found[0].member_count, 2);
        assert!(store.search_known_groups(&alice, "不存在").unwrap().is_empty());

        // Bob is not a member: his application is stored once and repeated notices upsert.
        assert!(store.record_join_request(&owner, &bob, &group.id, "研发一组", 10).unwrap());
        assert!(store.record_join_request(&owner, &bob, &group.id, "研发一组", 11).unwrap());
        assert_eq!(store.list_join_requests(&owner, None).unwrap().len(), 1);
        // A member cannot apply again, nor can a non-owner record a request.
        assert!(!store.record_join_request(&owner, &alice, &group.id, "研发一组", 12).unwrap());
        assert!(!store.record_join_request(&bob, &alice, &group.id, "研发一组", 13).unwrap());
        let incoming = store.list_join_requests(&owner, None).unwrap();
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].applicant, bob);
        assert_eq!(store.list_join_requests(&owner, Some(&other.id)).unwrap().len(), 0);

        // Approving adds the member and queues the roster update for everyone.
        let (gid, applicant) = store.resolve_join_request(&owner, &incoming[0].id, true).unwrap().unwrap();
        assert_eq!((gid.as_str(), applicant.as_str()), (group.id.as_str(), bob.as_str()));
        assert!(store.add_group_member(&group.id, &owner, &bob, 14).unwrap());
        assert_eq!(store.list_group_members(&group.id).unwrap().iter().filter(|m| m.active).count(), 3);
        assert!(store.resolve_join_request(&owner, &incoming[0].id, true).unwrap().is_none());

        // Refusing queues a paired outbound notice and settles the applicant's own row.
        let carol = "d".repeat(64);
        store.add_candidate(&carol, "Carol").unwrap();
        store.confirm_candidate(&carol).unwrap();
        store.set_contact_address(&carol, "127.0.0.1:7010").unwrap();
        assert!(store.record_join_request(&owner, &carol, &group.id, "研发一组", 15).unwrap());
        let pending = store.list_join_requests(&owner, Some(&group.id)).unwrap();
        assert_eq!(pending.len(), 1);
        assert!(store.queue_join_reject(&owner, &group.id, &carol, 16).unwrap());
        let mut cursor: Option<String> = None;
        let mut rejection = None;
        while let Some(task) = store.next_pending_control(cursor.as_deref()).unwrap() {
            cursor = Some(task.target.clone());
            if task.kind == "join_reject" { rejection = Some(task); break; }
        }
        let rejection = rejection.expect("queued rejection");
        assert_eq!(rejection.target, carol);
    }

    #[test]
    fn outbound_join_request_is_tracked_and_settled_by_invite() {
        let store = memory_store();
        let owner = "b".repeat(64);
        let me = "a".repeat(64);
        for (index, peer) in [&owner, &me].into_iter().enumerate() {
            store.add_candidate(peer, "P").unwrap();
            store.confirm_candidate(peer).unwrap();
            store.set_contact_address(peer, &format!("127.0.0.1:{}", 7100 + index)).unwrap();
        }
        let group = store.create_group(&owner, "Team", &[], 1).unwrap();
        store.queue_join_request(&me, &group.id, "Team", &owner, 2).unwrap();
        // A second click re-queues the same application instead of failing.
        store.queue_join_request(&me, &group.id, "Team", &owner, 4).unwrap();
        assert_eq!(store.list_out_join_requests(&me).unwrap().len(), 1);
        assert_eq!(store.list_out_join_requests(&me).unwrap().len(), 1);
        let control = store.next_pending_control(None).unwrap().unwrap();
        assert_eq!(control.kind, "join");
        assert_eq!(control.target, owner);

        // Receiving the roster update settles the pending application (auto-join mode).
        store.set_auto_join_groups(true).unwrap();
        store.receive_group_control(&me, &owner, "invite", &group.id,
            &serde_json::json!({ "name": "Team", "owner": owner, "members": [owner.clone(), me.clone()] }).to_string(), 5).unwrap();
        assert!(store.list_out_join_requests(&me).unwrap().is_empty());
    }

    #[test]
    fn group_invite_waits_for_confirmation_and_decline_notifies_owner() {
        let owner_store = memory_store();
        let me_store = memory_store();
        let owner = "b".repeat(64);
        let me = "a".repeat(64);
        for (store, peer, name) in [(&owner_store, &me, "Me"), (&me_store, &owner, "Owner")] {
            store.add_candidate_with_username(peer, name, None).unwrap();
            store.confirm_candidate(peer).unwrap();
        }
        me_store.set_contact_address(&owner, "127.0.0.1:7000").unwrap();
        let group = owner_store.create_group(&owner, "Team", &[me.clone()], 1).unwrap();
        let payload = serde_json::json!({ "name": "Team", "owner": owner, "members": [owner.clone(), me.clone()] }).to_string();

        // Default: the invite becomes a local request and no group row is created.
        assert!(!me_store.auto_join_groups().unwrap());
        assert_eq!(me_store.receive_group_control(&me, &owner, "invite", &group.id, &payload, 5).unwrap(), IncomingResult::Inserted);
        assert_eq!(me_store.group_membership(&group.id, &me).unwrap(), None);
        assert!(me_store.list_groups(&me).unwrap().is_empty());
        let pending = me_store.pending_group_invitations(&me).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].name, "Team");
        assert_eq!(pending[0].owner_name.as_deref(), Some("Owner"));

        // Re-delivery of the same invite does not create duplicates.
        assert_eq!(me_store.receive_group_control(&me, &owner, "invite", &group.id, &payload, 6).unwrap(), IncomingResult::Inserted);
        assert_eq!(me_store.pending_group_invitations(&me).unwrap().len(), 1);

        // Accepting creates membership and keeps history reachable.
        let id = me_store.pending_group_invitations(&me).unwrap()[0].id.clone();
        assert_eq!(me_store.accept_group_invitation(&id, &me, 7).unwrap(), Some(group.id.clone()));
        assert_eq!(me_store.group_membership(&group.id, &me).unwrap(), Some(true));
        assert_eq!(me_store.list_groups(&me).unwrap().len(), 1);
        assert!(me_store.pending_group_invitations(&me).unwrap().is_empty());

        // A fresh invitation can be declined; the owner gets a leave notice instead.
        let other = owner_store.create_group(&owner, "Other", &[me.clone()], 8).unwrap();
        let payload = serde_json::json!({ "name": "Other", "owner": owner, "members": [owner.clone(), me.clone()] }).to_string();
        me_store.receive_group_control(&me, &owner, "invite", &other.id, &payload, 9).unwrap();
        let id = me_store.pending_group_invitations(&me).unwrap()[0].id.clone();
        assert!(me_store.decline_group_invitation(&id, &me, 10).unwrap());
        assert_eq!(me_store.group_membership(&other.id, &me).unwrap(), None);
        assert!(me_store.pending_group_invitations(&me).unwrap().is_empty());
        let control = me_store.next_pending_control(None).unwrap().unwrap();
        assert_eq!(control.kind, "leave");
        assert_eq!(control.target, owner);
    }

    #[test]
    fn auto_join_accepts_contacts_and_groups_by_preference() {
        let store = memory_store();
        assert!(store.auto_accept_contacts().unwrap(), "mutual add is the default");
        store.set_auto_accept_contacts(false).unwrap();
        assert!(!store.auto_accept_contacts().unwrap());
        store.set_auto_accept_contacts(true).unwrap();
        assert!(store.auto_accept_contacts().unwrap());

        let owner_store = memory_store();
        let owner = "b".repeat(64);
        let me = "a".repeat(64);
        for (store, peer) in [(&owner_store, &me), (&store, &owner)] {
            store.add_candidate(peer, "P").unwrap();
            store.confirm_candidate(peer).unwrap();
        }
        let group = owner_store.create_group(&owner, "Team", &[me.clone()], 1).unwrap();
        store.set_auto_join_groups(true).unwrap();
        let payload = serde_json::json!({ "name": "Team", "owner": owner, "members": [owner.clone(), me.clone()] }).to_string();
        store.receive_group_control(&me, &owner, "invite", &group.id, &payload, 2).unwrap();
        assert_eq!(store.group_membership(&group.id, &me).unwrap(), Some(true));
        assert!(store.pending_group_invitations(&me).unwrap().is_empty());
    }

    #[test]
    fn group_removal_clears_pending_rows_and_leave_notifies_owner() {
        let store = memory_store();
        let owner = "b".repeat(64);
        let me = "a".repeat(64);
        let carol = "c".repeat(64);
        for (peer, name) in [(&owner, "Owner"), (&carol, "Carol"), (&me, "Me")] {
            store.add_candidate(peer, name).unwrap();
            store.confirm_candidate(peer).unwrap();
        }
        let group = store.create_group(&owner, "Team", &[me.clone(), carol.clone()], 1).unwrap();
        store.set_contact_address(&me, "127.0.0.1:1").unwrap();
        store.set_contact_address(&carol, "127.0.0.1:2").unwrap();
        let message = store.queue_group(&owner, &group.id, "hi", None, &[], 2).unwrap();
        assert_eq!(message.pending, 2);
        assert!(store.remove_group_member(&group.id, &owner, &me, 3).unwrap());
        assert!(!store.remove_group_member(&group.id, &owner, &me, 3).unwrap());
        // Removed member has no pending delivery left.
        let mut cursor: Option<String> = None;
        let mut deliveries: Vec<String> = Vec::new();
        while let Some(delivery) = store.next_pending_group_delivery(cursor.as_deref()).unwrap() {
            cursor = Some(delivery.member.clone());
            deliveries.push(delivery.member);
        }
        assert!(!deliveries.contains(&me));
        assert!(deliveries.contains(&carol));
        // The owner cannot leave, and an already removed member has nothing to leave.
        assert!(!store.leave_group(&group.id, &owner, 4).unwrap());
        assert!(!store.leave_group(&group.id, &me, 5).unwrap());
        store.set_contact_address(&owner, "127.0.0.1:3").unwrap();
        assert!(store.leave_group(&group.id, &carol, 6).unwrap());
        let leave = store.next_pending_control(None).unwrap();
        assert!(leave.is_some());
    }

    #[test]
    fn group_conversation_is_listed_and_hides_until_new_message() {
        let store = memory_store();
        let me = "a".repeat(64);
        let bob = "b".repeat(64);
        store.add_candidate(&bob, "Bob").unwrap();
        store.confirm_candidate(&bob).unwrap();
        let group = store.create_group(&me, "Team", &[bob.clone()], 1).unwrap();
        let list = store.list_conversations(&me).unwrap();
        assert!(list.iter().any(|conversation| conversation.id == group.id && conversation.kind == "group"));
        assert!(store.hide_group(&group.id, &me).unwrap());
        assert!(!store.list_conversations(&me).unwrap().iter().any(|conversation| conversation.id == group.id));
        store.queue_group(&me, &group.id, "back", None, &[], 2).unwrap();
        assert!(store.list_conversations(&me).unwrap().iter().any(|conversation| conversation.id == group.id));
        assert!(store.open_group(&group.id, &me).unwrap());
        assert!(store.open_group(&group.id, &bob).unwrap());
        assert!(!store.open_group(&group.id, &"d".repeat(64)).unwrap());
    }

    fn group_with_two_members(store: &Store, me: &str) -> (String, String, String) {
        let bob = "b".repeat(64);
        let carol = "c".repeat(64);
        for (peer, name) in [(&bob, "Bob"), (&carol, "Carol")] {
            store.add_candidate(peer, name).unwrap();
            store.confirm_candidate(peer).unwrap();
        }
        let group = store.create_group(me, "Team", &[bob.clone(), carol.clone()], 1).unwrap();
        (group.id, bob, carol)
    }

    #[test]
    fn group_reply_and_mentions_roundtrip_with_validation() {
        let store = memory_store();
        let me = "a".repeat(64);
        let (group_id, bob, carol) = group_with_two_members(&store, &me);
        let original = store.queue_group(&me, &group_id, "第一行\n第二行", None, &[], 10).unwrap();
        let reply = store.queue_group(&me, &group_id, "收到", Some(&original.id), &[bob.clone()], 11).unwrap();
        assert_eq!(reply.reply_to.as_deref(), Some(original.id.as_str()));
        // Without a local contact row the quote uses a generic label, not a fingerprint.
        assert_eq!(reply.reply_sender.as_deref(), Some("群成员"));
        assert_eq!(reply.reply_preview.as_deref(), Some("第一行 第二行"));
        assert_eq!(reply.mentions, vec![bob.clone()]);
        let listed = store.list_group_messages(&group_id, 10, None).unwrap();
        let stored = listed.iter().find(|message| message.id == reply.id).unwrap();
        assert_eq!(stored.reply_preview.as_deref(), Some("第一行 第二行"));
        assert_eq!(stored.mentions, vec![bob.clone()]);
        assert_eq!(store.group_dispatch(&reply.id).unwrap().unwrap().mentions, vec![bob]);
        // Unknown quote, non-member mention and duplicate mentions are rejected.
        assert!(store.queue_group(&me, &group_id, "x", Some("missing"), &[], 12).is_err());
        assert!(store.queue_group(&me, &group_id, "x", None, &["d".repeat(64)], 12).is_err());
        assert!(store.queue_group(&me, &group_id, "x", None, &[carol.clone(), carol.clone()], 12).is_err());
    }

    #[test]
    fn group_owner_transfer_and_dissolve_follow_owner_rules() {
        let store = memory_store();
        let me = "a".repeat(64);
        let (group_id, bob, carol) = group_with_two_members(&store, &me);
        for (peer, port) in [(&bob, "127.0.0.1:1"), (&carol, "127.0.0.1:2")] {
            store.set_contact_address(peer, port).unwrap();
        }
        assert!(!store.transfer_group_owner(&group_id, &bob, &carol, 5).unwrap());
        assert!(!store.transfer_group_owner(&group_id, &me, &"d".repeat(64), 5).unwrap());
        assert!(store.transfer_group_owner(&group_id, &me, &bob, 5).unwrap());
        let members = store.list_group_members(&group_id).unwrap();
        assert_eq!(members.iter().find(|member| member.member == bob).unwrap().role, "owner");
        assert_eq!(members.iter().find(|member| member.member == me).unwrap().role, "member");
        assert!(store.list_groups(&bob).unwrap().iter().any(|group| group.id == group_id && group.is_owner));
        let mut statement = store.connection.prepare(
            "SELECT target, kind, payload FROM group_control WHERE group_id = ?1 AND status = 'pending' ORDER BY target").unwrap();
        let queued: Vec<(String, String, String)> = statement.query_map([&group_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        }).unwrap().collect::<Result<Vec<_>>>().unwrap();
        assert!(queued.iter().any(|(target, kind, payload)| target == &bob && kind == "transfer" && payload.contains(&bob)));
        assert!(queued.iter().any(|(target, kind, _)| target == &carol && kind == "transfer"));
        // The previous owner may no longer dissolve; the new owner can.
        assert!(!store.dissolve_group(&group_id, &me, 6).unwrap());
        assert!(store.dissolve_group(&group_id, &bob, 6).unwrap());
        assert!(store.list_groups(&me).unwrap().is_empty());
        assert!(store.list_groups(&bob).unwrap().is_empty());
        assert!(store.queue_group(&me, &group_id, "after", None, &[], 7).is_err());
    }

    #[test]
    fn inbound_transfer_and_dissolve_respect_local_owner() {
        let store = memory_store();
        let me = "e".repeat(64);
        let owner = "b".repeat(64);
        let new_owner = "c".repeat(64);
        for (peer, name) in [(&owner, "Owner"), (&new_owner, "Next")] {
            store.add_candidate(peer, name).unwrap();
            store.confirm_candidate(peer).unwrap();
        }
        let group_id = "2".repeat(32);
        let roster = vec![owner.clone(), new_owner.clone(), me.clone()];
        assert_eq!(
            store.receive_group(&me, &owner, &group_id, "Team", &owner, &roster, "g1", "hi", 1, 5, None, None, None, &[], 100).unwrap(),
            IncomingResult::Inserted);
        let payload = serde_json::json!({ "name": "Team", "owner": new_owner, "members": roster }).to_string();
        // A non-owner cannot hand the group over.
        assert_eq!(store.receive_group_control(&me, &new_owner, "transfer", &group_id, &payload, 101).unwrap(), IncomingResult::Unverified);
        assert_eq!(store.receive_group_control(&me, &owner, "transfer", &group_id, &payload, 101).unwrap(), IncomingResult::Inserted);
        assert_eq!(store.list_group_members(&group_id).unwrap().iter().find(|member| member.member == new_owner).unwrap().role, "owner");
        assert_eq!(store.receive_group_control(&me, &owner, "dissolve", &group_id, "{}", 102).unwrap(), IncomingResult::Unverified);
        assert_eq!(store.receive_group_control(&me, &new_owner, "dissolve", &group_id, "{}", 102).unwrap(), IncomingResult::Inserted);
        assert!(store.list_groups(&me).unwrap().is_empty());
        assert!(!store.open_group(&group_id, &me).unwrap());
    }

    #[test]
    fn search_matches_text_files_and_escapes_like_wildcards() {
        let store = memory_store();
        let me = "a".repeat(64);
        let peer = "b".repeat(64);
        store.add_candidate(&peer, "Bob").unwrap();
        store.confirm_candidate(&peer).unwrap();
        store.queue_direct(&me, &peer, "alpha 报告 100% done", 1).unwrap();
        store.queue_direct(&me, &peer, "plain text", 2).unwrap();
        let path = std::env::temp_dir().join(format!("qbird-search-{}-{}.pdf", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::write(&path, b"x").unwrap();
        let hash = crate::files::sha256_file(&path).unwrap();
        store.queue_direct_file(&me, &peer, "报告.pdf", 1, &hash, &path, 3).unwrap();
        assert_eq!(store.search_direct(&peer, "报告", "all", 50, 0).unwrap().len(), 2);
        assert_eq!(store.search_direct(&peer, "报告", "text", 50, 0).unwrap().len(), 1);
        assert_eq!(store.search_direct(&peer, "报告", "file", 50, 0).unwrap().len(), 1);
        assert_eq!(store.search_direct(&peer, "报告", "image", 50, 0).unwrap().len(), 0);
        assert_eq!(store.search_direct(&peer, "100%", "all", 50, 0).unwrap().len(), 1);
        assert_eq!(store.search_direct(&peer, "100_", "all", 50, 0).unwrap().len(), 0);
        assert!(store.search_direct(&peer, "x", "unknown", 50, 0).is_err());

        let (group_id, bob, _) = group_with_two_members(&store, &me);
        store.queue_group(&me, &group_id, "hello 报告", None, &[], 4).unwrap();
        store.queue_group(&me, &group_id, "second", None, &[], 5).unwrap();
        assert_eq!(store.search_group_messages(&group_id, "报告", "all", None, 50, 0).unwrap().len(), 1);
        assert_eq!(store.search_group_messages(&group_id, "", "all", Some(&bob), 50, 0).unwrap().len(), 0);
        assert_eq!(store.search_group_messages(&group_id, "", "all", None, 50, 0).unwrap().len(), 2);
        assert!(store.search_group_messages(&group_id, "", "file", None, 50, 0).unwrap().is_empty());
        crate::files::remove_quietly(&path);
    }
    #[test]
    fn storage_quota_defaults_bounds_and_blocks_oversized_offers() {
        let store = memory_store();
        assert_eq!(store.storage_status().unwrap(), StorageUsage { used: 0, quota: DEFAULT_STORAGE_QUOTA });
        assert!(store.storage_quota().unwrap() >= MIN_STORAGE_QUOTA);
        assert!(!store.set_storage_quota(MIN_STORAGE_QUOTA - 1).unwrap());
        assert!(!store.set_storage_quota(MAX_STORAGE_QUOTA + 1).unwrap());
        assert!(store.set_storage_quota(MIN_STORAGE_QUOTA).unwrap());
        assert_eq!(store.storage_quota().unwrap(), MIN_STORAGE_QUOTA);

        let me = "a".repeat(64);
        let peer = "b".repeat(64);
        store.add_candidate(&peer, "Bob").unwrap();
        store.confirm_candidate(&peer).unwrap();
        let hash = "c".repeat(64);
        // An offer larger than the remaining quota is refused before anything is written.
        assert_eq!(store.receive_file_offer(&peer, "f1", "a.bin", MIN_STORAGE_QUOTA + 1, &hash, 1, 1, 1).unwrap(), FileOfferDecision::NoSpace);
        let fits = store.receive_file_offer(&peer, "f2", "a.bin", 4096, &hash, 2, 2, 2).unwrap();
        assert!(matches!(fits, FileOfferDecision::Ready { offset: 0, .. }));
        assert_eq!(store.storage_status().unwrap().quota, MIN_STORAGE_QUOTA);
        // Usage counts partial files written under the attachments directory.
        let incoming = store.attachments_dir().join("in");
        std::fs::create_dir_all(&incoming).unwrap();
        std::fs::write(incoming.join("sample.part"), b"12345").unwrap();
        assert!(store.storage_used() >= 5);
        let _ = std::fs::remove_dir_all(store.attachments_dir());
        let _ = me;
    }
    #[test]
    fn contact_remark_category_and_profile_are_local_only() {
        let store = memory_store();
        let peer = "b".repeat(64);
        store.add_candidate_with_username(&peer, "Bob", Some("robert")).unwrap();
        store.confirm_candidate(&peer).unwrap();
        assert!(store.set_contact_profile(&peer, Some("项目对接人"), Some("研发部")).unwrap());
        let contact = store.list_contacts().unwrap().into_iter().find(|item| item.fingerprint == peer).unwrap();
        assert_eq!(contact.username.as_deref(), Some("robert"), "LAN username is display-only but must persist");
        assert_eq!(contact.remark.as_deref(), Some("项目对接人"));
        assert_eq!(contact.category.as_deref(), Some("研发部"));
        assert_eq!(contact.trust_state, "verified", "local notes must not change trust");
        assert!(store.set_contact_profile(&peer, Some("  "), None).unwrap());
        assert_eq!(store.list_contacts().unwrap()[0].remark, None);
        assert!(store.set_contact_profile(&"c".repeat(64), Some("x"), None).unwrap() == false);
        assert!(store.set_contact_profile(&peer, Some(&"x".repeat(65)), None).is_err());
        assert!(store.set_contact_profile(&peer, Some("对接"), Some("研发部")).unwrap());

        assert_eq!(store.get_profile().unwrap(), Profile { nickname: String::new(), email: String::new(), username: String::new() });
        assert!(store.set_profile("青鸟测试", "a@example.com").unwrap());
        assert_eq!(store.get_profile().unwrap(), Profile { nickname: "青鸟测试".into(), email: "a@example.com".into(), username: String::new() });
        assert!(!store.set_profile(&"x".repeat(65), "").unwrap());
        assert!(!store.set_profile("ok", "not-an-email").unwrap());
        assert_eq!(store.get_profile().unwrap().email, "a@example.com");
        assert!(store.set_username("bob_1").unwrap());
        assert_eq!(store.get_profile().unwrap().username, "bob_1");
        assert!(!store.set_username("Bad Name").unwrap());
        assert!(!store.set_username("a").unwrap());
        assert!(!store.set_username(&"x".repeat(21)).unwrap());
        // Letters, digits and underscores only, and the first character cannot be a digit.
        assert!(!store.set_username("1abc").unwrap());
        assert!(!store.set_username("bob-1").unwrap());
        assert!(!store.set_username("青鸟").unwrap());
        assert!(store.set_username("_Demo_01").unwrap());
        assert_eq!(store.get_profile().unwrap().username, "_Demo_01");
        assert!(!store.onboarding_done().unwrap());
        store.set_onboarding_done().unwrap();
        assert!(store.onboarding_done().unwrap());

        assert_eq!(store.search_contacts("研发", 20).unwrap().len(), 1);
        assert_eq!(store.search_contacts("Bob", 20).unwrap().len(), 1);
        assert_eq!(store.search_contacts("robert", 20).unwrap().len(), 1, "search must match the peer username");
        assert_eq!(store.search_contacts(&peer[..8], 20).unwrap().len(), 1);
        assert!(store.search_contacts("missing", 20).unwrap().is_empty());
        assert_eq!(store.search_contacts("", 20).unwrap().len(), 1);
        assert!(store.search_contacts("x", 0).is_err());
    }

    #[test]
    fn global_search_and_text_export_cover_visible_conversations() {
        let store = memory_store();
        let me = "a".repeat(64);
        let peer = "b".repeat(64);
        store.add_candidate(&peer, "Bob").unwrap();
        store.confirm_candidate(&peer).unwrap();
        store.queue_direct(&me, &peer, "单聊 里程碑", 10).unwrap();
        store.receive_direct(&peer, "d2", "回复 里程碑", 11, 5).unwrap();
        store.send_self(&me, "自聊 里程碑", 12).unwrap();
        let (group_id, _, _) = group_with_two_members(&store, &me);
        store.queue_group(&me, &group_id, "群聊 里程碑", None, &[], 13).unwrap();

        let hits = store.search_all(&me, "里程碑", "all", None, None, 100, 0).unwrap();
        assert_eq!(hits.len(), 4);
        assert!(hits.iter().any(|hit| hit.conversation_kind == "self"));
        assert!(hits.iter().any(|hit| hit.conversation_kind == "direct" && hit.conversation_name == "Bob"));
        assert!(hits.iter().any(|hit| hit.conversation_kind == "group" && hit.conversation_name == "Team"));
        assert_eq!(hits[0].created_at, 13);
        assert!(store.search_all(&me, "里程碑", "file", None, None, 100, 0).unwrap().is_empty());
        assert!(store.search_all(&me, "missing", "all", None, None, 100, 0).unwrap().is_empty());
        assert!(store.search_all(&me, "", "text", None, None, 100, 0).unwrap().len() >= 4);
        assert!(store.search_all(&me, "x", "all", None, None, 0, 0).is_err());
        assert!(store.search_all("bad", "x", "all", None, None, 10, 0).is_err());

    }

    #[test]
    fn global_search_filters_conversations_and_paginates_without_duplicates() {
        let store = memory_store();
        let me = "a".repeat(64);
        let peer = "b".repeat(64);
        let other = "c".repeat(64);
        for (fingerprint, name) in [(&peer, "Bob"), (&other, "Cara")] {
            store.add_candidate(fingerprint, name).unwrap();
            store.confirm_candidate(fingerprint).unwrap();
        }
        store.queue_direct(&me, &peer, "scope-mark one", 1).unwrap();
        store.queue_direct(&me, &peer, "scope-mark two", 2).unwrap();
        store.queue_direct(&me, &other, "scope-mark other", 3).unwrap();
        store.send_self(&me, "scope-mark self", 4).unwrap();
        let (group_id, _, _) = group_with_two_members(&store, &me);
        store.queue_group(&me, &group_id, "scope-mark group", None, &[], 5).unwrap();

        let first = store.search_all(&me, "scope-mark", "all", None, None, 2, 0).unwrap();
        let second = store.search_all(&me, "scope-mark", "all", None, None, 2, 2).unwrap();
        let third = store.search_all(&me, "scope-mark", "all", None, None, 2, 4).unwrap();
        let ids: std::collections::HashSet<_> = first.iter().chain(&second).chain(&third)
            .map(|hit| hit.message_id.as_str()).collect();
        assert_eq!((first.len(), second.len(), third.len(), ids.len()), (2, 2, 1, 5));

        let direct = store.search_all(&me, "scope-mark", "all", Some("direct"), Some(&peer), 100, 0).unwrap();
        assert_eq!(direct.len(), 2);
        assert!(direct.iter().all(|hit| hit.conversation_id == peer));
        let other_direct = store.search_all(&me, "scope-mark", "all", Some("direct"), Some(&other), 100, 0).unwrap();
        assert_eq!(other_direct.len(), 1);
        let own = store.search_all(&me, "scope-mark", "all", Some("self"), Some("self"), 100, 0).unwrap();
        assert_eq!(own.len(), 1);
        let group = store.search_all(&me, "scope-mark", "all", Some("group"), Some(&group_id), 100, 0).unwrap();
        assert_eq!(group.len(), 1);
        assert!(store.search_all(&me, "scope-mark", "all", Some("direct"), Some(&"d".repeat(64)), 100, 0).is_err());
        assert!(store.search_all(&me, "scope-mark", "all", Some("self"), Some(&peer), 100, 0).is_err());
        assert!(store.search_all(&me, "scope-mark", "all", None, None, 2, 100_001).is_err());
    }

    #[test]
    fn record_history_search_filters_dates_types_and_paginates_with_access_checks() {
        let store = memory_store();
        let me = "a".repeat(64);
        let peer = "b".repeat(64);
        store.add_candidate(&peer, "Bob").unwrap();
        store.confirm_candidate(&peer).unwrap();
        store.queue_direct(&me, &peer, "window direct one", 10).unwrap();
        store.queue_direct(&me, &peer, "window direct two", 20).unwrap();
        store.queue_direct(&me, &peer, "window direct three", 30).unwrap();
        store.queue_direct_file(&me, &peer, "photo.PNG", 3, &"c".repeat(64), Path::new("photo.png"), 40).unwrap();
        store.queue_direct_file(&me, &peer, "notes.txt", 4, &"d".repeat(64), Path::new("notes.txt"), 50).unwrap();

        let first = store.search_chat_history("direct", &peer, &me, "window", "all", None, None, "[recalled]", 2, 0).unwrap();
        let second = store.search_chat_history("direct", &peer, &me, "window", "all", None, None, "[recalled]", 2, 2).unwrap();
        assert_eq!(first.iter().map(|hit| hit.created_at).collect::<Vec<_>>(), vec![30, 20]);
        assert_eq!(second.iter().map(|hit| hit.created_at).collect::<Vec<_>>(), vec![10]);
        let dated = store.search_chat_history("direct", &peer, &me, "window", "text", Some(20), Some(20), "[recalled]", 10, 0).unwrap();
        assert_eq!(dated.len(), 1);
        assert_eq!(dated[0].created_at, 20);
        let images = store.search_chat_history("direct", &peer, &me, "", "image", None, None, "[recalled]", 10, 0).unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].file_name.as_deref(), Some("photo.PNG"));
        let files = store.search_chat_history("direct", &peer, &me, "", "file", None, None, "[recalled]", 10, 0).unwrap();
        assert_eq!(files.len(), 2);

        store.send_self(&me, "window self", 60).unwrap();
        let self_hits = store.search_chat_history("self", "self", &me, "window", "all", Some(60), Some(60), "[recalled]", 10, 0).unwrap();
        assert_eq!(self_hits.len(), 1);
        let (group_id, _, _) = group_with_two_members(&store, &me);
        store.queue_group(&me, &group_id, "window group", None, &[], 70).unwrap();
        let group_hits = store.search_chat_history("group", &group_id, &me, "window", "all", None, None, "[recalled]", 10, 0).unwrap();
        assert_eq!(group_hits.len(), 1);

        let pending_peer = "e".repeat(64);
        store.add_candidate(&pending_peer, "Pending").unwrap();
        assert!(store.search_chat_history("direct", &pending_peer, &me, "", "all", None, None, "[recalled]", 10, 0).is_err());
        store.connection.execute("UPDATE group_members SET active = 0 WHERE group_id = ?1 AND member = ?2", params![group_id, me]).unwrap();
        assert!(store.search_chat_history("group", &group_id, &me, "", "all", None, None, "[recalled]", 10, 0).is_err());
        assert!(store.search_chat_history("direct", &peer, &me, "", "all", Some(30), Some(20), "[recalled]", 10, 0).is_err());
    }

    #[test]
    fn fresh_database_reaches_latest_schema_and_accepts_offer_destinations() {
        let store = memory_store();
        let version: i64 = store.connection
            .pragma_query_value(None, "user_version", |row| row.get(0)).unwrap();
        assert_eq!(version, SCHEMA_VERSION, "a new database must start at the latest schema");
        // Regression: the v17 column migration used to run before the table existed,
        // so a fresh database rejected every group offer and one-shot destination.
        let peer = "b".repeat(64);
        let id = "m".repeat(32);
        store.record_file_offer_request(&peer, &id, "a.pdf", 10, 1, Some(&"c".repeat(32))).unwrap();
        assert!(store.resolve_file_offer(&id, true, Some("D:\\接收")).unwrap());
        assert_eq!(store.file_offer_target_dir(&id).unwrap(), Some(PathBuf::from("D:\\接收")));
    }

    #[test]
    fn v26_database_adds_direct_reply_columns_without_losing_messages() {
        use std::{fs, time::{SystemTime, UNIX_EPOCH}};
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("storage-tests");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("reply-v26-{}-{}.sqlite", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        {
            let store = Store::open(&path).unwrap();
            let me = "a".repeat(64);
            let peer = "b".repeat(64);
            store.add_candidate(&peer, "Bob").unwrap();
            store.confirm_candidate(&peer).unwrap();
            let existing = store.queue_direct(&me, &peer, "preserve me", 10).unwrap();
            store.connection.execute_batch(
                "ALTER TABLE direct_messages DROP COLUMN reply_to;
                 ALTER TABLE direct_messages DROP COLUMN reply_sender;
                 ALTER TABLE direct_messages DROP COLUMN reply_preview;
                 PRAGMA user_version = 26;",
            ).unwrap();
            assert_eq!(existing.body, "preserve me");
        }
        let store = Store::open(&path).unwrap();
        let version: i64 = store.connection.pragma_query_value(None, "user_version", |row| row.get(0)).unwrap();
        assert_eq!(version, 27);
        let columns: Vec<String> = {
            let mut statement = store.connection.prepare("PRAGMA table_info(direct_messages)").unwrap();
            statement.query_map([], |row| row.get(1)).unwrap().collect::<Result<Vec<_>>>().unwrap()
        };
        assert!(columns.iter().any(|column| column == "reply_to"));
        assert!(columns.iter().any(|column| column == "reply_sender"));
        assert!(columns.iter().any(|column| column == "reply_preview"));
        assert_eq!(store.list_direct(&"b".repeat(64), 10, None).unwrap().len(), 1);
        drop(store);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn v12_database_gains_offer_destination_columns() {
        use std::{fs, time::{SystemTime, UNIX_EPOCH}};
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("storage-tests");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("offers-{}-{}.sqlite", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        {
            let store = Store::open(&path).unwrap();
            store.connection.execute_batch(
                "CREATE TABLE offers_old (
                    message_id TEXT PRIMARY KEY NOT NULL,
                    peer TEXT NOT NULL,
                    name TEXT NOT NULL,
                    total INTEGER NOT NULL,
                    created_at INTEGER NOT NULL,
                    status TEXT NOT NULL DEFAULT 'pending',
                    target_dir TEXT
                 );
                 DROP TABLE file_offer_requests;
                 ALTER TABLE offers_old RENAME TO file_offer_requests;
                 PRAGMA user_version = 12;",
            ).unwrap();
        }
        let store = Store::open(&path).unwrap();
        let version: i64 = store.connection
            .pragma_query_value(None, "user_version", |row| row.get(0)).unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        let id = "m".repeat(32);
        store.record_file_offer_request(&"b".repeat(64), &id, "群文件.pdf", 10, 1, Some(&"c".repeat(32))).unwrap();
        assert_eq!(store.list_group_file_offer_requests(&"c".repeat(32)).unwrap().len(), 1);
        assert!(store.resolve_file_offer(&id, true, None).unwrap());
        drop(store);
        // Reopening an upgraded database must not be rejected as "too new".
        let store = Store::open(&path).unwrap();
        assert_eq!(store.file_offer_status(&id).unwrap().as_deref(), Some("accepted"));
        drop(store);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn file_accept_mode_defaults_from_legacy_flag_and_persists() {
        let store = memory_store();
        assert_eq!(store.file_accept_mode().unwrap(), FileAcceptMode::Always);
        store.set_setting("file_accept_confirm", Some("0")).unwrap();
        assert_eq!(store.file_accept_mode().unwrap(), FileAcceptMode::Auto);
        store.set_file_accept_mode(FileAcceptMode::Remember).unwrap();
        assert_eq!(store.file_accept_mode().unwrap(), FileAcceptMode::Remember);
        assert_eq!(store.get_setting("file_accept_confirm").unwrap().as_deref(), Some("1"));
        store.set_file_accept_mode(FileAcceptMode::Auto).unwrap();
        assert_eq!(store.get_setting("file_accept_confirm").unwrap().as_deref(), Some("0"));
        // An unreadable stored value falls back to the legacy boolean, never to "auto".
        store.set_setting("file_accept_mode", Some("bogus")).unwrap();
        assert_eq!(store.file_accept_mode().unwrap(), FileAcceptMode::Auto);
        store.set_setting("file_accept_confirm", Some("1")).unwrap();
        assert_eq!(store.file_accept_mode().unwrap(), FileAcceptMode::Always);
        for mode in [FileAcceptMode::Always, FileAcceptMode::Remember, FileAcceptMode::Auto] {
            assert_eq!(FileAcceptMode::parse(mode.as_str()), Some(mode));
        }
        assert_eq!(FileAcceptMode::parse("Ask"), None);
    }

    #[test]
    fn receive_directories_default_customize_and_reset_per_kind() {
        let store = memory_store();
        let attachments = store.attachments_dir();
        assert_eq!(store.recv_dir(RecvKind::Direct), attachments.join("in").join("direct"));
        assert_eq!(store.recv_dir(RecvKind::Group), attachments.join("in").join("group"));
        assert!(!store.recv_dir_custom(RecvKind::Direct));
        store.set_recv_dir(RecvKind::Direct, Some("D:\\接收\\单聊")).unwrap();
        assert!(store.recv_dir_custom(RecvKind::Direct));
        assert_eq!(store.recv_dir(RecvKind::Direct), PathBuf::from("D:\\接收\\单聊"));
        assert!(!store.recv_dir_custom(RecvKind::Group));
        // Clearing restores the built-in default instead of an empty path.
        store.set_recv_dir(RecvKind::Direct, Some("   ")).unwrap();
        assert!(!store.recv_dir_custom(RecvKind::Direct));
        assert_eq!(store.recv_dir(RecvKind::Direct), attachments.join("in").join("direct"));
        store.set_recv_dir(RecvKind::Group, Some("D:\\接收\\群")).unwrap();
        assert_eq!(store.recv_dir(RecvKind::Group), PathBuf::from("D:\\接收\\群"));
        store.set_recv_dir(RecvKind::Group, None).unwrap();
        assert_eq!(store.recv_dir(RecvKind::Group), attachments.join("in").join("group"));
    }

    #[test]
    fn quota_only_counts_the_attachment_store() {
        let store = memory_store();
        let attachments = store.attachments_dir();
        assert!(store.within_attachments(&attachments));
        assert!(store.within_attachments(&attachments.join("in").join("direct")));
        assert!(!store.within_attachments(&attachments.parent().unwrap().join("elsewhere")));
        assert!(!store.within_attachments(Path::new("D:\\接收")));
        // A sibling that only shares the prefix must not be treated as internal.
        let sibling = PathBuf::from(format!("{}2", attachments.to_string_lossy()));
        assert!(!store.within_attachments(&sibling));
    }

    #[test]
    fn remembered_sources_are_validated_deduplicated_and_cleared() {
        let store = memory_store();
        let peer = "b".repeat(64);
        let group = "c".repeat(32);
        assert!(store.remembered_file_sources().unwrap().is_empty());
        assert!(!store.is_file_source_remembered(&peer).unwrap());
        store.remember_file_source(&peer).unwrap();
        store.remember_file_source(&peer).unwrap();
        store.remember_file_source(&format!("group:{group}")).unwrap();
        assert_eq!(store.remembered_file_sources().unwrap(), vec![peer.clone(), format!("group:{group}")]);
        assert!(store.is_file_source_remembered(&peer).unwrap());
        assert!(store.is_file_source_remembered(&format!("group:{group}")).unwrap());
        assert!(!store.is_file_source_remembered(&"d".repeat(64)).unwrap());
        for bad in ["", "xyz", "group:123", "group:", &"B".repeat(64)] {
            assert!(store.remember_file_source(bad).is_err(), "{bad} must be refused");
        }
        assert!(!store.is_file_source_remembered("xyz").unwrap());
        // The list is bounded, so an attacker cannot grow settings without limit.
        for index in 0..205u64 {
            store.remember_file_source(&format!("{index:064x}")).unwrap();
        }
        let sources = store.remembered_file_sources().unwrap();
        assert_eq!(sources.len(), 200);
        assert!(!sources.contains(&"0".repeat(64)));
        assert!(sources.contains(&format!("{:064x}", 204u64)));
        store.forget_file_sources().unwrap();
        assert!(store.remembered_file_sources().unwrap().is_empty());
        assert!(!store.is_file_source_remembered(&peer).unwrap());
    }

    #[test]
    fn pending_offers_stay_per_conversation_and_keep_one_shot_target() {
        let store = memory_store();
        let peer = "b".repeat(64);
        let group = "c".repeat(32);
        let direct_id = "a".repeat(32);
        let group_id = "d".repeat(32);
        store.record_file_offer_request(&peer, &direct_id, "报告.pdf", 100, 10, None).unwrap();
        store.record_file_offer_request(&peer, &group_id, "群文件.pdf", 50, 11, Some(&group)).unwrap();
        // Repeated offers keep the original row instead of duplicating it.
        store.record_file_offer_request(&peer, &direct_id, "改名.pdf", 100, 12, None).unwrap();
        assert_eq!(store.file_offer_status(&direct_id).unwrap().as_deref(), Some("pending"));
        assert_eq!(store.file_offer_target_dir(&direct_id).unwrap(), None);
        let direct = store.list_file_offer_requests(&peer).unwrap();
        assert_eq!(direct.len(), 1);
        assert_eq!(direct[0].message_id, direct_id);
        assert_eq!(direct[0].name, "报告.pdf");
        assert_eq!(direct[0].group_id, None);
        let grouped = store.list_group_file_offer_requests(&group).unwrap();
        assert_eq!(grouped.len(), 1);
        assert_eq!(grouped[0].message_id, group_id);
        assert_eq!(grouped[0].group_id.as_deref(), Some(group.as_str()));
        assert!(store.resolve_file_offer(&direct_id, true, Some("D:\\接收")).unwrap());
        assert_eq!(store.file_offer_status(&direct_id).unwrap().as_deref(), Some("accepted"));
        assert_eq!(store.file_offer_target_dir(&direct_id).unwrap(), Some(PathBuf::from("D:\\接收")));
        assert!(store.list_file_offer_requests(&peer).unwrap().is_empty());
        // A resolved offer cannot be flipped again, so a late click cannot widen access.
        assert!(!store.resolve_file_offer(&direct_id, false, None).unwrap());
        assert_eq!(store.file_offer_status(&direct_id).unwrap().as_deref(), Some("accepted"));
        assert_eq!(store.file_offer_target_dir(&direct_id).unwrap(), Some(PathBuf::from("D:\\接收")));
        assert!(store.resolve_file_offer("bad id", true, None).is_err());
        assert!(store.file_offer_status("bad id").is_err());
        assert!(store.file_offer_target_dir("bad id").is_err());
        assert!(store.record_file_offer_request(&peer, &direct_id, "..", 1, 1, None).is_err());
        assert!(store.record_file_offer_request("bad", &"e".repeat(32), "a.pdf", 1, 1, None).is_err());
        assert!(store.record_file_offer_request(&peer, &"e".repeat(32), "a.pdf", 1, 1, Some("bad")).is_err());
    }

    #[test]
    fn recall_completed_attachments_keeps_local_files_and_marks_messages() {
        let store = memory_store();
        let me = "a".repeat(64);
        let (group_id, peer, other) = group_with_two_members(&store, &me);
        let dir = store.attachments_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("completed-attachment.txt");
        std::fs::write(&path, b"keep this file").unwrap();
        let path_string = path.to_string_lossy().to_string();

        store.receive_direct(&peer, "direct-file", "file", 1, 1).unwrap();
        store.connection.execute("UPDATE direct_messages SET kind = 'file' WHERE id = 'direct-file'", []).unwrap();
        store.connection.execute(
            "INSERT INTO file_transfers(message_id, peer, direction, target_path, total, transferred, status, updated_at) VALUES ('direct-file', ?1, 'incoming', ?2, 14, 14, 'completed', 1)",
            params![peer, path_string],
        ).unwrap();
        assert_eq!(store.receive_recall_direct(&me, &peer, "direct-file", 2).unwrap(), IncomingResult::Inserted);
        assert!(store.list_direct(&peer, 10, None).unwrap()[0].recalled_at.is_some());
        assert_eq!(store.file_transfer("direct-file").unwrap().unwrap().status, "completed");
        assert_eq!(std::fs::read(&path).unwrap(), b"keep this file");

        let roster = vec![me.clone(), peer.clone(), other];
        store.receive_group(&me, &peer, &group_id, "Team", &me, &roster, "group-file", "file", 1, 2, None, None, None, &[], 1).unwrap();
        store.connection.execute("UPDATE group_messages SET kind = 'file' WHERE id = 'group-file'", []).unwrap();
        store.connection.execute(
            "INSERT INTO group_file_transfers(message_id, group_id, peer, direction, target_path, total, transferred, status, updated_at) VALUES ('group-file', ?1, ?2, 'incoming', ?3, 14, 14, 'completed', 1)",
            params![group_id, peer, path_string],
        ).unwrap();
        let payload = serde_json::json!({ "message_id": "group-file" }).to_string();
        assert_eq!(store.receive_group_control(&me, &peer, "recall", &group_id, &payload, 2).unwrap(), IncomingResult::Inserted);
        assert!(store.list_group_messages(&group_id, 10, None).unwrap()[0].recalled_at.is_some());
        let status: String = store.connection.query_row("SELECT status FROM group_file_transfers WHERE message_id = 'group-file'", [], |row| row.get(0)).unwrap();
        assert_eq!(status, "completed");
        assert_eq!(std::fs::read(&path).unwrap(), b"keep this file");
    }

    #[test]
    fn recalled_inflight_files_cannot_be_marked_completed_or_overwritten() {
        let store = memory_store();
        let me = "a".repeat(64);
        let (group_id, peer, other) = group_with_two_members(&store, &me);
        store.receive_direct(&peer, "inflight-direct", "file", 1, 1).unwrap();
        store.connection.execute("UPDATE direct_messages SET kind = 'file' WHERE id = 'inflight-direct'", []).unwrap();
        store.connection.execute("INSERT INTO file_transfers(message_id, peer, direction, total, status, updated_at) VALUES ('inflight-direct', ?1, 'incoming', 10, 'transferring', 1)", [&peer]).unwrap();
        store.receive_recall_direct(&me, &peer, "inflight-direct", 2).unwrap();
        assert!(!store.complete_file_transfer("inflight-direct", 3).unwrap());
        store.fail_file_transfer("inflight-direct", "late error", 4).unwrap();
        let transfer = store.file_transfer("inflight-direct").unwrap().unwrap();
        assert_eq!((transfer.status.as_str(), transfer.error.as_deref()), ("failed", Some("recalled")));

        let roster = vec![me.clone(), peer.clone(), other];
        store.receive_group(&me, &peer, &group_id, "Team", &me, &roster, "inflight-group", "file", 1, 2, None, None, None, &[], 1).unwrap();
        store.connection.execute("UPDATE group_messages SET kind = 'file' WHERE id = 'inflight-group'", []).unwrap();
        store.connection.execute("INSERT INTO group_file_transfers(message_id, group_id, peer, direction, total, status, updated_at) VALUES ('inflight-group', ?1, ?2, 'incoming', 10, 'transferring', 1)", params![group_id, peer]).unwrap();
        store.receive_group_control(&me, &peer, "recall", &group_id, &serde_json::json!({"message_id":"inflight-group"}).to_string(), 2).unwrap();
        assert!(!store.complete_group_file_transfer("inflight-group", 3).unwrap());
        store.fail_group_file_transfer("inflight-group", "late error", 4).unwrap();
        let transfer = store.group_file_transfer("inflight-group").unwrap().unwrap();
        assert_eq!((transfer.status.as_str(), transfer.error.as_deref()), ("failed", Some("recalled")));
    }

    #[test]
    fn offline_recalls_keep_placeholders_and_queue_every_original_recipient() {
        let store = memory_store();
        let me = "a".repeat(64);
        let (group_id, bob, carol) = group_with_two_members(&store, &me);
        store.set_contact_address(&bob, "127.0.0.1:7101").unwrap();
        store.set_contact_address(&carol, "127.0.0.1:7102").unwrap();

        let direct = store.queue_direct(&me, &bob, "direct secret", 10).unwrap();
        assert_eq!(store.recall_direct(&me, &direct.id, 11).unwrap(), RecallResult::Recalled { delivered: false });
        let direct_row = store.list_direct(&bob, 10, None).unwrap().remove(0);
        assert_eq!(direct_row.id, direct.id);
        assert_eq!(direct_row.logical, direct.logical);
        assert_eq!(direct_row.created_at, direct.created_at);
        assert!(direct_row.recalled_at.is_some());
        assert_ne!(direct_row.body, "direct secret");
        assert!(store.pending_direct_by_id(&bob, &direct.id).unwrap().is_none());
        assert_eq!(store.next_pending_recall(None).unwrap().unwrap().message_id, direct.id);

        let group = store.queue_group(&me, &group_id, "group secret", None, &[], 20).unwrap();
        assert!(store.ack_group(&bob, &group.id).unwrap());
        assert_eq!(store.recall_group(&me, &group_id, &group.id, 21).unwrap(), RecallResult::Recalled { delivered: true });
        let row = store.list_group_messages(&group_id, 10, None).unwrap().remove(0);
        assert_eq!((row.id, row.logical, row.created_at), (group.id.clone(), group.logical, group.created_at));
        assert!(row.recalled_at.is_some());
        assert_ne!(row.body, "group secret");
        assert!(store.next_pending_group_delivery(None).unwrap().is_none());
        let mut statement = store.connection.prepare("SELECT target FROM group_control WHERE kind = 'recall' AND group_id = ?1 AND status = 'pending' ORDER BY target").unwrap();
        let targets: Vec<String> = statement.query_map([&group_id], |row| row.get(0)).unwrap().collect::<Result<Vec<_>>>().unwrap();
        assert_eq!(targets, vec![bob, carol]);
    }

    #[test]
    fn v23_recall_queue_migration_preserves_available_order_metadata() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("storage-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("recall-v23-{}-{}.sqlite", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let me = "a".repeat(64);
        let peer = "b".repeat(64);
        let message = {
            let store = Store::open(&path).unwrap();
            store.add_candidate(&peer, "Bob").unwrap();
            store.confirm_candidate(&peer).unwrap();
            store.set_contact_address(&peer, "127.0.0.1:7101").unwrap();
            let message = store.queue_direct(&me, &peer, "hello", 10).unwrap();
            store.recall_direct(&me, &message.id, 11).unwrap();
            store.connection.execute_batch("ALTER TABLE direct_recalls DROP COLUMN message_created_at;
                ALTER TABLE direct_recalls DROP COLUMN message_logical; PRAGMA user_version = 23;").unwrap();
            message
        };
        let store = Store::open(&path).unwrap();
        let recall = store.next_pending_recall(None).unwrap().unwrap();
        assert_eq!(recall.message_id, message.id);
        assert_eq!(recall.created_at, Some(message.created_at));
        assert_eq!(recall.logical, Some(message.logical));
    }

    #[test]
    fn direct_recall_placeholder_is_ordered_idempotent_and_cannot_revive() {
        let store = memory_store();
        let me = "a".repeat(64);
        let peer = "b".repeat(64);
        let stranger = "c".repeat(64);
        store.add_candidate(&peer, "Bob").unwrap();
        store.confirm_candidate(&peer).unwrap();
        assert_eq!(store.receive_direct_placeholder(&me, &stranger, "message1", 300, 5, 400).unwrap(), IncomingResult::Unverified);
        store.record_file_offer_request(&peer, "message1", "file.txt", 10, 300, None).unwrap();
        assert_eq!(store.receive_direct_placeholder(&me, &peer, "message1", 300, 5, 400).unwrap(), IncomingResult::Inserted);
        assert!(store.list_file_offer_requests(&peer).unwrap().is_empty());
        assert_eq!(store.receive_direct_placeholder(&me, &peer, "message1", 300, 5, 401).unwrap(), IncomingResult::Duplicate);
        assert_eq!(store.receive_direct_placeholder(&me, &peer, "message1", 301, 5, 401).unwrap(), IncomingResult::IdConflict);
        assert_eq!(store.receive_direct(&peer, "message1", "old secret", 300, 5).unwrap(), IncomingResult::IdConflict);
        let row = store.list_direct(&peer, 10, None).unwrap().remove(0);
        assert_eq!((row.id.as_str(), row.created_at, row.logical), ("message1", 300, 5));
        assert_eq!(row.body, " ");
        assert_eq!(row.recalled_at, Some(400));
        let next = store.queue_direct(&me, &peer, "after", 200).unwrap();
        assert!(next.logical > row.logical);
    }

    #[test]
    fn group_recall_placeholder_requires_membership_and_cannot_revive() {
        let store = memory_store();
        let me = "a".repeat(64);
        let (group_id, bob, carol) = group_with_two_members(&store, &me);
        let outsider = "d".repeat(64);
        store.add_candidate(&outsider, "Outsider").unwrap();
        store.confirm_candidate(&outsider).unwrap();
        assert_eq!(store.receive_group_placeholder(&me, &outsider, &group_id, "message2", 300, 5, 400).unwrap(), IncomingResult::NotMember);
        store.record_file_offer_request(&bob, "message2", "file.txt", 10, 300, Some(&group_id)).unwrap();
        assert_eq!(store.receive_group_placeholder(&me, &bob, &group_id, "message2", 300, 5, 400).unwrap(), IncomingResult::Inserted);
        assert!(store.list_group_file_offer_requests(&group_id).unwrap().is_empty());
        assert_eq!(store.receive_group_placeholder(&me, &bob, &group_id, "message2", 300, 5, 401).unwrap(), IncomingResult::Duplicate);
        assert_eq!(store.receive_group_placeholder(&me, &carol, &group_id, "message2", 300, 5, 401).unwrap(), IncomingResult::IdConflict);
        let roster = vec![me.clone(), bob.clone(), carol];
        assert_eq!(store.receive_group(&me, &bob, &group_id, "Team", &me, &roster, "message2", "old secret", 300, 5, None, None, None, &[], 402).unwrap(), IncomingResult::IdConflict);
        let row = store.list_group_messages(&group_id, 10, None).unwrap().remove(0);
        assert_eq!((row.id.as_str(), row.created_at, row.logical), ("message2", 300, 5));
        assert_eq!(row.body, " ");
        assert_eq!(row.recalled_at, Some(400));
    }

    #[test]
    fn generic_settings_roundtrip_and_delete() {
        let store = memory_store();
        assert_eq!(store.get_setting("lock_hash").unwrap(), None);
        assert!(store.set_setting("lock_hash", Some("abc")).unwrap());
        assert_eq!(store.get_setting("lock_hash").unwrap().as_deref(), Some("abc"));
        assert!(store.set_setting("lock_hash", Some("def")).unwrap());
        assert_eq!(store.get_setting("lock_hash").unwrap().as_deref(), Some("def"));
        assert!(store.set_setting("lock_hash", None).unwrap());
        assert_eq!(store.get_setting("lock_hash").unwrap(), None);
        assert!(store.set_setting("", Some("x")).is_err());
        assert!(store.get_setting(&"k".repeat(65)).is_err());
    }
}
