use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension, Result};
use serde::Serialize;

const SCHEMA_VERSION: i64 = 6;

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Contact {
    pub fingerprint: String,
    pub display_name: String,
    pub trust_state: String,
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
            "SELECT fingerprint, display_name, trust_state FROM contacts ORDER BY display_name, fingerprint LIMIT 1000"
        )?;
        let rows = statement.query_map([], |row| Ok(Contact {
            fingerprint: row.get(0)?, display_name: row.get(1)?, trust_state: row.get(2)?,
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

    pub fn list_conversations(&self) -> Result<Vec<Conversation>> {
        let mut statement = self.connection.prepare(
            "SELECT c.id AS id, c.hidden AS hidden, m.body AS body, m.logical AS logical FROM conversations c
             JOIN local_messages m ON m.id = (
               SELECT id FROM local_messages WHERE conversation_id = c.id
               ORDER BY logical DESC, sender DESC, id DESC LIMIT 1
             ) WHERE c.hidden = 0
             UNION ALL
             SELECT c.peer AS id, c.hidden AS hidden, m.body AS body, m.logical AS logical FROM direct_conversations c
             JOIN direct_messages m ON m.id = (
               SELECT id FROM direct_messages WHERE peer = c.peer
               ORDER BY logical DESC, sender DESC, id DESC LIMIT 1
             ) WHERE c.hidden = 0 ORDER BY logical DESC, id"
        )?;
        let rows = statement.query_map([], |row| Ok(Conversation {
            id: row.get(0)?, hidden: row.get(1)?, last_body: row.get(2)?, last_logical: row.get(3)?,
        }))?;
        rows.collect()
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
        let list = store.list_conversations().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, peer);
        assert_eq!(list[1].id, "self");
        assert!(store.hide_direct(&peer).unwrap());
        assert_eq!(store.list_conversations().unwrap().len(), 1);
        assert_eq!(store.receive_direct(&peer, "new", "hello", 1, 5).unwrap(), IncomingResult::Inserted);
        assert_eq!(store.list_conversations().unwrap()[0].id, peer);
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
        assert!(store.list_conversations().unwrap().is_empty());
        assert!(store.send_self(&sender, " ", 20).is_err());
        assert!(store.send_self(&sender, &"x".repeat(32769), 20).is_err());
        let first = store.send_self(&sender, "first", 200).unwrap();
        assert!(store.hide_self_conversation().unwrap());
        assert!(store.list_conversations().unwrap().is_empty());
        assert_eq!(store.list_self_messages(20, 0).unwrap().len(), 1);
        let second = store.send_self(&sender, "second", 100).unwrap();
        assert_ne!(first.id, second.id);
        assert_eq!((first.logical, second.logical), (1, 2));
        assert_eq!(store.list_self_messages(20, 0).unwrap()[0].body, "second");
        assert_eq!(store.list_conversations().unwrap()[0].last_body, "second");
        assert!(store.hide_self_conversation().unwrap());
        store.open_self_conversation().unwrap();
        assert_eq!(store.list_conversations().unwrap().len(), 1);
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
        assert!(store.list_conversations().unwrap().is_empty());
        assert_eq!(store.list_self_messages(20, 0).unwrap()[0].body, "before restart");
        assert_eq!(store.send_self(&me, "after restart", 100).unwrap().logical, 2);
        assert_eq!(store.list_conversations().unwrap()[0].last_body, "after restart");
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
}
