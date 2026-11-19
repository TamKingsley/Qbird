use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Result};
use serde::Serialize;

const SCHEMA_VERSION: i64 = 2;

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Contact {
    pub fingerprint: String,
    pub display_name: String,
    pub trust_state: String,
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
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        Self::initialize(Connection::open(path)?)
    }

    fn initialize(mut connection: Connection) -> Result<Self> {
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
        Ok(Self { connection })
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

    pub fn list_contacts(&self) -> Result<Vec<Contact>> {
        let mut statement = self.connection.prepare(
            "SELECT fingerprint, display_name, trust_state FROM contacts ORDER BY display_name, fingerprint LIMIT 1000"
        )?;
        let rows = statement.query_map([], |row| Ok(Contact {
            fingerprint: row.get(0)?, display_name: row.get(1)?, trust_state: row.get(2)?,
        }))?;
        rows.collect()
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
            "SELECT c.id, c.hidden, m.body, m.logical FROM conversations c
             JOIN local_messages m ON m.id = (
               SELECT id FROM local_messages WHERE conversation_id = c.id
               ORDER BY logical DESC, sender DESC, id DESC LIMIT 1
             ) WHERE c.hidden = 0 ORDER BY m.logical DESC, c.id"
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
}
