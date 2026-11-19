use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension, Result};
use serde::Serialize;

const SCHEMA_VERSION: i64 = 9;

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

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Contact {
    pub fingerprint: String,
    pub display_name: String,
    pub trust_state: String,
    /// Local-only note; never sent to peers and never changes identity.
    pub remark: Option<String>,
    /// Local-only category/group label used to organise the contact list.
    pub category: Option<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Profile {
    pub nickname: String,
    pub email: String,
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
    pub kind: String,
    pub file_name: Option<String>,
    pub file_size: Option<i64>,
    pub file_sha256: Option<String>,
    pub file_status: Option<String>,
    pub file_transferred: Option<i64>,
    pub file_error: Option<String>,
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

const MESSAGE_COLUMNS: &str = "m.id, m.peer, m.sender, m.body, m.created_at, m.logical, m.status, m.kind, m.file_name, m.file_size, m.file_sha256, f.status, f.transferred, f.error";

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
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct LocalMessage {
    pub id: String,
    pub conversation_id: String,
    pub sender: String,
    pub body: String,
    pub created_at: i64,
    pub logical: i64,
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
        Ok(Self { connection, data_dir, address_revision: std::cell::Cell::new(0) })
    }

    /// Records a candidate without replacing an existing trust decision.
    pub fn add_candidate(&self, fingerprint: &str, display_name: &str) -> Result<bool> {
        if !valid_fingerprint(fingerprint) || display_name.len() > 256 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        Ok(self.connection.execute(
            "INSERT INTO contacts (fingerprint, display_name, trust_state)
             VALUES (?1, ?2, 'pending') ON CONFLICT(fingerprint) DO NOTHING",
            params![fingerprint, display_name],
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
             WHERE m.peer = ?1 AND {category_filter}
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
             ON CONFLICT(peer) DO UPDATE SET hidden = 0", [peer],
        )? == 1)
    }

    pub fn hide_direct(&self, peer: &str) -> Result<bool> {
        if !valid_fingerprint(peer) { return Err(rusqlite::Error::InvalidQuery); }
        Ok(self.connection.execute("UPDATE direct_conversations SET hidden = 1 WHERE peer = ?1 AND hidden = 0", [peer])? == 1)
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

    pub fn is_verified(&self, fingerprint: &str) -> Result<bool> {
        let trust: Option<String> = self.connection.query_row(
            "SELECT trust_state FROM contacts WHERE fingerprint = ?1",
            [fingerprint],
            |row| row.get(0),
        ).optional()?;
        Ok(trust.as_deref() == Some("verified"))
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
            "SELECT fingerprint, display_name, trust_state, remark, category FROM contacts ORDER BY display_name, fingerprint LIMIT 1000"
        )?;
        let rows = statement.query_map([], |row| Ok(Contact {
            fingerprint: row.get(0)?, display_name: row.get(1)?, trust_state: row.get(2)?,
            remark: row.get(3)?, category: row.get(4)?,
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
        Ok(Profile { nickname: read("profile_nickname")?.unwrap_or_default(), email: read("profile_email")?.unwrap_or_default() })
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

    /// Contact list search across name, local remark, category and fingerprint.
    pub fn search_contacts(&self, query: &str, limit: u32) -> Result<Vec<Contact>> {
        if query.chars().count() > 64 || limit == 0 || limit > 200 || query.chars().any(char::is_control) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let pattern = like_pattern(query);
        let mut statement = self.connection.prepare(
            "SELECT fingerprint, display_name, trust_state, remark, category FROM contacts
             WHERE ?1 = '' OR display_name LIKE ?2 ESCAPE '\\' OR COALESCE(remark, '') LIKE ?2 ESCAPE '\\'
                OR COALESCE(category, '') LIKE ?2 ESCAPE '\\' OR fingerprint LIKE ?2 ESCAPE '\\'
             ORDER BY display_name, fingerprint LIMIT ?3"
        )?;
        let rows = statement.query_map(params![query, pattern, limit], |row| Ok(Contact {
            fingerprint: row.get(0)?, display_name: row.get(1)?, trust_state: row.get(2)?,
            remark: row.get(3)?, category: row.get(4)?,
        }))?;
        rows.collect()
    }

    /// Persist before attempting transmission; wall-clock time is for display only.
    pub fn queue_direct(&self, sender: &str, peer: &str, body: &str, created_at: i64) -> Result<DirectMessage> {
        if !valid_fingerprint(sender) || !valid_fingerprint(peer) || sender == peer || body.trim().is_empty() || body.len() > 32768 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let tx = self.connection.unchecked_transaction()?;
        let verified: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM contacts WHERE fingerprint = ?1 AND trust_state = 'verified')", [peer], |r| r.get(0))?;
        if !verified { return Err(rusqlite::Error::InvalidQuery); }
        tx.execute("INSERT INTO direct_conversations(peer, hidden) VALUES (?1, 0) ON CONFLICT(peer) DO UPDATE SET hidden = 0", [peer])?;
        if tx.execute("UPDATE logical_clock SET value = value + 1 WHERE singleton = 1 AND value < 9223372036854775807", [])? != 1 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let (id, logical): (String, i64) = tx.query_row("SELECT lower(hex(randomblob(16))), value FROM logical_clock WHERE singleton = 1", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let collision: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM local_messages WHERE id = ?1 UNION ALL SELECT 1 FROM messages WHERE id = ?1)", [&id], |r| r.get(0))?;
        if collision { return Err(rusqlite::Error::InvalidQuery); }
        tx.execute("INSERT INTO direct_messages(id, peer, sender, body, created_at, logical, status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending')", params![id, peer, sender, body, created_at, logical])?;
        tx.commit()?;
        Ok(DirectMessage {
            id, peer: peer.into(), sender: sender.into(), body: body.into(), created_at, logical, status: "pending".into(),
            kind: "text".into(), file_name: None, file_size: None, file_sha256: None, file_status: None, file_transferred: None, file_error: None,
        })
    }

    /// `peer` must be the fingerprint authenticated by the transport, never a payload claim.
    /// Only Inserted or Duplicate may be acknowledged after this transaction returns.
    pub fn receive_direct(&self, peer: &str, id: &str, body: &str, created_at: i64, logical: i64) -> Result<IncomingResult> {
        if !valid_fingerprint(peer) || id.is_empty() || id.len() > 128 || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') || body.is_empty() || body.len() > 32768 || logical <= 0 || logical == i64::MAX {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let tx = self.connection.unchecked_transaction()?;
        let verified: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM contacts WHERE fingerprint = ?1 AND trust_state = 'verified')", [peer], |r| r.get(0))?;
        if !verified { return Ok(IncomingResult::Unverified); }
        let existing: Option<(String, String, String, i64, i64, String)> = tx.query_row(
            "SELECT peer, sender, body, created_at, logical, status FROM direct_messages WHERE id = ?1", [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        ).optional()?;
        if let Some((old_peer, sender, old_body, time, old_logical, status)) = existing {
            return Ok(if old_peer == peer && sender == peer && old_body == body && time == created_at && old_logical == logical && status == "received" {
                IncomingResult::Duplicate
            } else { IncomingResult::IdConflict });
        }
        let collision: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM local_messages WHERE id = ?1 UNION ALL SELECT 1 FROM messages WHERE id = ?1)", [id], |r| r.get(0))?;
        if collision { return Ok(IncomingResult::IdConflict); }
        if tx.execute("UPDATE logical_clock SET value = max(value, ?1) + 1 WHERE singleton = 1 AND value < 9223372036854775807", [logical])? != 1 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        tx.execute("INSERT INTO direct_conversations(peer, hidden) VALUES (?1, 0) ON CONFLICT(peer) DO UPDATE SET hidden = 0", [peer])?;
        tx.execute("INSERT INTO direct_messages(id, peer, sender, body, created_at, logical, status) VALUES (?1, ?2, ?2, ?3, ?4, ?5, 'received')", params![id, peer, body, created_at, logical])?;
        tx.commit()?;
        Ok(IncomingResult::Inserted)
    }

    pub fn attachments_dir(&self) -> PathBuf { self.data_dir.join("attachments") }

    fn incoming_dir(&self) -> PathBuf { self.attachments_dir().join("in") }

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
            return Ok(RestoreReport { mode: "merge".into(), inserted: 0, skipped: 0, conflicts: conflicts as u64, snapshot_path });
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
                 INSERT INTO main.local_messages SELECT * FROM src.local_messages;
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
                 INSERT OR IGNORE INTO main.local_messages SELECT * FROM src.local_messages;
                 INSERT OR IGNORE INTO main.direct_conversations SELECT * FROM src.direct_conversations;
                 INSERT OR IGNORE INTO main.direct_messages SELECT * FROM src.direct_messages;
                 INSERT OR IGNORE INTO main.file_transfers SELECT * FROM src.file_transfers;
                 INSERT OR IGNORE INTO main.groups SELECT * FROM src.groups;
                 INSERT OR IGNORE INTO main.group_members SELECT * FROM src.group_members;
                 INSERT OR IGNORE INTO main.group_messages SELECT * FROM src.group_messages;
                 INSERT OR IGNORE INTO main.messages SELECT * FROM src.messages;",
            )?;
        }
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
        Ok(RestoreReport { mode: if overwrite { "overwrite".into() } else { "merge".into() }, inserted, skipped, conflicts: 0, snapshot_path })
    }

    /// Queue an outgoing file. The file stays at `source`; the receiver verifies the hash.
    pub fn queue_direct_file(&self, sender: &str, peer: &str, name: &str, size: u64, sha256: &str, source: &Path, created_at: i64) -> Result<DirectMessage> {
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
        tx.execute("INSERT INTO direct_conversations(peer, hidden) VALUES (?1, 0) ON CONFLICT(peer) DO UPDATE SET hidden = 0", [peer])?;
        if tx.execute("UPDATE logical_clock SET value = value + 1 WHERE singleton = 1 AND value < 9223372036854775807", [])? != 1 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let (id, logical): (String, i64) = tx.query_row("SELECT lower(hex(randomblob(16))), value FROM logical_clock WHERE singleton = 1", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let collision: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM messages WHERE id = ?1 UNION ALL SELECT 1 FROM local_messages WHERE id = ?1)", [&id], |r| r.get(0))?;
        if collision { return Err(rusqlite::Error::InvalidQuery); }
        tx.execute(
            "INSERT INTO direct_messages(id, peer, sender, body, created_at, logical, status, kind, file_name, file_size, file_sha256)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', 'file', ?4, ?7, ?8)",
            params![id, peer, sender, clean, created_at, logical, size, sha256],
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
        })
    }

    /// Validate an offer against trust, existing records and any partial file.
    /// `peer` must be authenticated by the transport, never taken from the payload.
    pub fn receive_file_offer(&self, peer: &str, id: &str, name: &str, size: u64, sha256: &str, created_at: i64, logical: i64, now: i64) -> Result<FileOfferDecision> {
        let clean = match crate::files::sanitize_name(name) { Some(clean) => clean, None => return Ok(FileOfferDecision::Conflict) };
        if !valid_fingerprint(peer) || !valid_message_id(id) || size == 0 || size > crate::protocol::MAX_FILE_BYTES
            || !valid_sha256(sha256) || logical <= 0 || logical == i64::MAX {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let size_i64 = i64::try_from(size).map_err(|_| rusqlite::Error::InvalidQuery)?;
        let tx = self.connection.unchecked_transaction()?;
        let verified: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM contacts WHERE fingerprint = ?1 AND trust_state = 'verified')", [peer], |r| r.get(0))?;
        if !verified { return Ok(FileOfferDecision::Untrusted); }
        let target = crate::files::incoming_target(&self.incoming_dir(), id, &clean);
        let partial = crate::files::incoming_partial(&self.incoming_dir(), id);
        let existing: Option<(String, String, Option<String>, Option<i64>, Option<String>, i64, i64)> = tx.query_row(
            "SELECT peer, sender, kind, file_size, file_sha256, created_at, logical FROM direct_messages WHERE id = ?1", [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
        ).optional()?;
        if let Some((old_peer, sender, kind, old_size, old_hash, old_created, old_logical)) = existing {
            if old_peer != peer || sender != peer || kind.as_deref() != Some("file") || old_size != Some(size_i64)
                || old_hash.as_deref() != Some(sha256) || old_created != created_at || old_logical != logical {
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
        if self.storage_used().saturating_add(size) > self.storage_quota()? {
            return Ok(FileOfferDecision::NoSpace);
        }
        if tx.execute("UPDATE logical_clock SET value = max(value, ?1) + 1 WHERE singleton = 1 AND value < 9223372036854775807", [logical])? != 1 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        tx.execute("INSERT INTO direct_conversations(peer, hidden) VALUES (?1, 0) ON CONFLICT(peer) DO UPDATE SET hidden = 0", [peer])?;
        tx.execute(
            "INSERT INTO direct_messages(id, peer, sender, body, created_at, logical, status, kind, file_name, file_size, file_sha256)
             VALUES (?1, ?2, ?2, ?3, ?4, ?5, 'received', 'file', ?3, ?6, ?7)",
            params![id, peer, clean, created_at, logical, size_i64, sha256],
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
            "UPDATE file_transfers SET status = 'completed', transferred = total, error = NULL, updated_at = ?2 WHERE message_id = ?1",
            params![id, now],
        )? == 1)
    }

    /// A failure keeps partial bytes so the next offer can resume from them.
    pub fn fail_file_transfer(&self, id: &str, error: &str, now: i64) -> Result<()> {
        if !valid_message_id(id) || error.is_empty() || error.len() > 200 { return Err(rusqlite::Error::InvalidQuery); }
        let tx = self.connection.unchecked_transaction()?;
        tx.execute(
            "UPDATE file_transfers SET status = 'failed', error = ?2, updated_at = ?3 WHERE message_id = ?1",
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

    pub fn list_conversations(&self, self_id: &str) -> Result<Vec<Conversation>> {
        if !valid_fingerprint(self_id) { return Err(rusqlite::Error::InvalidQuery); }
        let mut statement = self.connection.prepare(
            "SELECT c.id AS id, c.hidden AS hidden, m.body AS body, m.logical AS logical, 'self' AS kind FROM conversations c
             JOIN local_messages m ON m.id = (
               SELECT id FROM local_messages WHERE conversation_id = c.id
               ORDER BY logical DESC, sender DESC, id DESC LIMIT 1
             ) WHERE c.hidden = 0
             UNION ALL
             SELECT c.peer AS id, c.hidden AS hidden, m.body AS body, m.logical AS logical, 'direct' AS kind FROM direct_conversations c
             JOIN direct_messages m ON m.id = (
               SELECT id FROM direct_messages WHERE peer = c.peer
               ORDER BY logical DESC, sender DESC, id DESC LIMIT 1
             ) WHERE c.hidden = 0
             UNION ALL
             SELECT g.id AS id, gm.hidden AS hidden, COALESCE(m.body, '') AS body,
                    COALESCE(m.logical, 0) AS logical, 'group' AS kind
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

    pub fn open_group(&self, group_id: &str, self_id: &str) -> Result<bool> {
        if !valid_group_id(group_id) || !valid_fingerprint(self_id) { return Err(rusqlite::Error::InvalidQuery); }
        Ok(self.connection.execute(
            "UPDATE group_members SET hidden = 0 WHERE group_id = ?1 AND member = ?2 AND active = 1",
            params![group_id, self_id])? == 1)
    }

    pub fn hide_group(&self, group_id: &str, self_id: &str) -> Result<bool> {
        if !valid_group_id(group_id) || !valid_fingerprint(self_id) { return Err(rusqlite::Error::InvalidQuery); }
        Ok(self.connection.execute(
            "UPDATE group_members SET hidden = 1 WHERE group_id = ?1 AND member = ?2 AND active = 1",
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
                let quote_name: String = tx.query_row(
                    "SELECT COALESCE(display_name, substr(?1, 1, 8)) FROM contacts WHERE fingerprint = ?1",
                    [&quoted_sender], |row| row.get(0),
                ).unwrap_or_else(|_| quoted_sender[..8.min(quoted_sender.len())].to_owned());
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
                m.reply_to, m.reply_sender, m.reply_preview, m.mentions
             FROM group_messages m LEFT JOIN contacts c ON c.fingerprint = m.sender
             WHERE m.group_id = ?1 AND (?2 IS NULL OR (m.logical, m.sender, m.id) < (?2, ?3, ?4))
             ORDER BY m.logical DESC, m.sender DESC, m.id DESC LIMIT ?5"
        )?;
        let rows = statement.query_map(params![group_id, before.map(|c| c.logical), before.map(|c| c.sender.as_str()), before.map(|c| c.id.as_str()), limit],
            group_message_row)?;
        rows.collect()
    }

    /// Substring search within one group, newest first. Category `file`/`image` is
    /// reserved for group files and returns nothing until those exist.
    pub fn search_group_messages(&self, group_id: &str, query: &str, category: &str,
        member: Option<&str>, limit: u32, offset: u32) -> Result<Vec<GroupMessage>> {
        if !valid_group_id(group_id) || !valid_search(&query, category, limit, offset)
            || member.is_some_and(|value| !valid_fingerprint(value)) {
            return Err(rusqlite::Error::InvalidQuery);
        }
        if !matches!(category, "all" | "text") { return Ok(Vec::new()); }
        let pattern = like_pattern(query);
        let mut statement = self.connection.prepare(
            "SELECT m.id, m.group_id, m.sender, COALESCE(c.display_name, substr(m.sender, 1, 8)), m.body, m.created_at, m.logical,
                0, m.reply_to, m.reply_sender, m.reply_preview, m.mentions
             FROM group_messages m LEFT JOIN contacts c ON c.fingerprint = m.sender
             WHERE m.group_id = ?1
               AND (?2 = '' OR m.body LIKE ?3 ESCAPE '\\')
               AND (?4 IS NULL OR m.sender = ?4)
             ORDER BY m.logical DESC, m.sender DESC, m.id DESC LIMIT ?5 OFFSET ?6"
        )?;
        let rows = statement.query_map(
            params![group_id, query, pattern, member, limit, offset], group_message_row)?;
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

    /// Snapshot of a group message plus the member list used when sending it.
    pub fn group_dispatch(&self, message_id: &str) -> Result<Option<GroupDispatch>> {
        if message_id.is_empty() || message_id.len() > 128 { return Err(rusqlite::Error::InvalidQuery); }
        let found: Option<(String, String, String, String, String, i64, i64, Option<String>, Option<String>, Option<String>, String)> = self.connection.query_row(
            "SELECT m.id, m.group_id, g.name, g.owner, m.body, m.created_at, m.logical,
                m.reply_to, m.reply_sender, m.reply_preview, m.mentions
             FROM group_messages m JOIN groups g ON g.id = m.group_id WHERE m.id = ?1", [message_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?,
                row.get(7)?, row.get(8)?, row.get(9)?, row.get(10)?)),
        ).optional()?;
        let Some((message_id, group_id, group_name, owner, body, created_at, logical, reply_to, reply_sender, reply_preview, mentions)) = found else { return Ok(None); };
        let mut statement = self.connection.prepare(
            "SELECT member FROM group_members WHERE group_id = ?1 AND active = 1 ORDER BY member")?;
        let rows = statement.query_map([&group_id], |row| row.get(0))?;
        let members: Vec<String> = rows.collect::<Result<Vec<String>>>()?;
        Ok(Some(GroupDispatch { message_id, group_id, group_name, owner, members, body, created_at, logical,
            reply_to, reply_sender, reply_preview, mentions: parse_mentions(&mentions) }))
    }

    /// Applies one inbound group text. The member snapshot is only trusted from the owner;
    /// other members may create the group for us but cannot rewrite its member list.
    #[allow(clippy::too_many_arguments)]
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
        let existing: Option<(String, String, String, i64, i64, Option<String>, String)> = tx.query_row(
            "SELECT group_id, sender, body, created_at, logical, reply_to, mentions FROM group_messages WHERE id = ?1", [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)),
        ).optional()?;
        if let Some((old_group, sender, old_body, time, old_logical, old_reply, old_mentions)) = existing {
            let same = old_group == group_id && sender == peer && old_body == body && time == created_at
                && old_logical == logical && old_reply.as_deref() == reply_to
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
        tx.execute("UPDATE group_members SET hidden = 0 WHERE group_id = ?1 AND member = ?2", params![group_id, self_id])?;
        tx.commit()?;
        Ok(IncomingResult::Inserted)
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

    /// Applies an inbound invite / member update / removal / leave notice.
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
                tx.execute("UPDATE groups SET active = 0 WHERE id = ?1", [group_id])?;
                tx.execute("UPDATE group_members SET active = 0, hidden = 1 WHERE group_id = ?1", [group_id])?;
                tx.execute("DELETE FROM group_outbox WHERE group_id = ?1 AND status = 'pending'", [group_id])?;
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
        Ok(LocalMessage { id, conversation_id: "self".into(), sender: sender.into(), body: body.into(), created_at, logical })
    }

    /// Newest page first, ordered independently from device wall clock.
    pub fn list_self_messages(&self, limit: u32, offset: u32) -> Result<Vec<LocalMessage>> {
        if limit == 0 || limit > 200 || offset > 100_000 { return Err(rusqlite::Error::InvalidQuery); }
        let mut statement = self.connection.prepare(
            "SELECT id, conversation_id, sender, body, created_at, logical FROM local_messages
             WHERE conversation_id = 'self' ORDER BY logical DESC, sender DESC, id DESC LIMIT ?1 OFFSET ?2"
        )?;
        let rows = statement.query_map(params![limit, offset], |row| Ok(LocalMessage {
            id: row.get(0)?, conversation_id: row.get(1)?, sender: row.get(2)?, body: row.get(3)?, created_at: row.get(4)?, logical: row.get(5)?,
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
    })
}

/// One-line quote snippet: whitespace collapsed, trimmed and cut on a character boundary.
fn quote_preview(body: &str) -> String {
    let collapsed = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut preview: String = collapsed.chars().take(REPLY_PREVIEW_MAX).collect();
    if collapsed.chars().count() > REPLY_PREVIEW_MAX { preview.push('…'); }
    preview
}

fn parse_mentions(value: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(value).unwrap_or_default()
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
    })
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn valid_message_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

fn valid_fingerprint(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
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
        // Without a local contact row the quote falls back to the fingerprint prefix.
        assert_eq!(reply.reply_sender.as_deref(), Some("aaaaaaaa"));
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
        store.add_candidate(&peer, "Bob").unwrap();
        store.confirm_candidate(&peer).unwrap();
        assert!(store.set_contact_profile(&peer, Some("项目对接人"), Some("研发部")).unwrap());
        let contact = store.list_contacts().unwrap().into_iter().find(|item| item.fingerprint == peer).unwrap();
        assert_eq!(contact.remark.as_deref(), Some("项目对接人"));
        assert_eq!(contact.category.as_deref(), Some("研发部"));
        assert_eq!(contact.trust_state, "verified", "local notes must not change trust");
        assert!(store.set_contact_profile(&peer, Some("  "), None).unwrap());
        assert_eq!(store.list_contacts().unwrap()[0].remark, None);
        assert!(store.set_contact_profile(&"c".repeat(64), Some("x"), None).unwrap() == false);
        assert!(store.set_contact_profile(&peer, Some(&"x".repeat(65)), None).is_err());
        assert!(store.set_contact_profile(&peer, Some("对接"), Some("研发部")).unwrap());

        assert_eq!(store.get_profile().unwrap(), Profile { nickname: String::new(), email: String::new() });
        assert!(store.set_profile("青鸟测试", "a@example.com").unwrap());
        assert_eq!(store.get_profile().unwrap(), Profile { nickname: "青鸟测试".into(), email: "a@example.com".into() });
        assert!(!store.set_profile(&"x".repeat(65), "").unwrap());
        assert!(!store.set_profile("ok", "not-an-email").unwrap());
        assert_eq!(store.get_profile().unwrap().email, "a@example.com");

        assert_eq!(store.search_contacts("研发", 20).unwrap().len(), 1);
        assert_eq!(store.search_contacts("Bob", 20).unwrap().len(), 1);
        assert_eq!(store.search_contacts(&peer[..8], 20).unwrap().len(), 1);
        assert!(store.search_contacts("missing", 20).unwrap().is_empty());
        assert_eq!(store.search_contacts("", 20).unwrap().len(), 1);
        assert!(store.search_contacts("x", 0).is_err());
    }
}

