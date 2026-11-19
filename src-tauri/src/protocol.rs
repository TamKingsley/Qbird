//! Framing and application readiness only; always use inside an authenticated TLS channel.
use std::io::{self, Read, Write};
use serde::{Deserialize, Serialize};

const VERSION: u8 = 1;
const MAX_FRAME: usize = 200_000;
const MAX_BODY: usize = 32_768;
/// Upper bound shared by the offer, the stored transfer and the wire metadata.
pub const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_FILE_NAME: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum Payload {
    Ready {},
    Text { id: String, body: String, created_at: i64, logical: i64 },
    Ack { id: String },
    /// Metadata only; the bytes follow as raw chunk frames after an accepted offer.
    FileOffer { id: String, name: String, size: u64, sha256: String, created_at: i64, logical: i64 },
    FileAccept { id: String, offset: u64 },
    FileReject { id: String, reason: String },
    FileComplete { id: String },
    /// One group message. `members` is the sender's local snapshot; receivers accept it
    /// only from the group owner, so a member cannot rewrite the roster. `reply_to`/`reply_preview`
    /// carry a short quote so the UI can show it before the original is paged in.
    GroupText {
        id: String, group_id: String, name: String, owner: String, members: Vec<String>,
        body: String, created_at: i64, logical: i64,
        #[serde(default)] reply_to: Option<String>,
        #[serde(default)] reply_sender: Option<String>,
        #[serde(default)] reply_preview: Option<String>,
        #[serde(default)] mentions: Vec<String>,
    },
    /// A group file message. Bytes follow the same accept / raw chunk / complete
    /// exchange as a direct file offer; the receiver only trusts the roster from the owner.
    GroupFileOffer {
        id: String, group_id: String, name: String, owner: String, members: Vec<String>,
        file_name: String, size: u64, sha256: String, created_at: i64, logical: i64,
    },
    /// Owner handover, signed by the current owner like a roster update.
    GroupTransfer { id: String, group_id: String, name: String, owner: String, members: Vec<String> },
    /// The owner dissolved the group; members keep local history but leave the roster.
    GroupDissolve { id: String, group_id: String },
    /// Invite or member-list update; carries the same owner-controlled snapshot.
    GroupUpdate { id: String, group_id: String, name: String, owner: String, members: Vec<String> },
    /// The owner removed the receiver from the group.
    GroupRemove { id: String, group_id: String },
    /// A member left voluntarily and tells the owner.
    GroupLeave { id: String, group_id: String, member: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Frame {
    version: u8,
    payload: Payload,
}

fn invalid(reason: &'static str) -> io::Error { io::Error::new(io::ErrorKind::InvalidData, reason) }

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub const MAX_GROUP_MEMBERS: usize = 256;

fn valid_group_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn valid_members(owner: &str, members: &[String]) -> bool {
    !members.is_empty()
        && members.len() <= MAX_GROUP_MEMBERS
        && members.iter().all(|member| {
            member.len() == 64 && member.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
        && members.iter().any(|member| member == owner)
}

fn valid_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_FILE_NAME
        && !name.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
}

/// Reasons are fixed codes so a peer cannot inject display text into diagnostics.
pub fn valid_reject_reason(reason: &str) -> bool {
    matches!(reason, "untrusted" | "conflict" | "too_large" | "invalid" | "no_space" | "defer" | "declined")
}

fn validate(payload: &Payload) -> io::Result<()> {
    let valid_id = |id: &str| !id.is_empty() && id.len() <= 128 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    match payload {
        Payload::Ready {} => Ok(()),
        Payload::Text { id, body, logical, .. } if valid_id(id) && !body.is_empty() && body.len() <= MAX_BODY && *logical > 0 => Ok(()),
        Payload::Ack { id } if valid_id(id) => Ok(()),
        Payload::FileOffer { id, name, size, sha256, logical, .. }
            if valid_id(id) && valid_file_name(name) && *size > 0 && *size <= MAX_FILE_BYTES && valid_sha256(sha256) && *logical > 0 => Ok(()),
        Payload::FileAccept { id, offset } if valid_id(id) && *offset <= MAX_FILE_BYTES => Ok(()),
        Payload::FileReject { id, reason } if valid_id(id) && valid_reject_reason(reason) => Ok(()),
        Payload::FileComplete { id } if valid_id(id) => Ok(()),
        Payload::GroupText { id, group_id, name, owner, members, body, logical, reply_to, reply_sender, reply_preview, mentions, .. }
            if valid_id(id) && valid_group_id(group_id) && !name.is_empty() && name.len() <= 128
                && valid_members(owner, members) && !body.is_empty() && body.len() <= MAX_BODY && *logical > 0
                && reply_to.as_deref().is_none_or(valid_id)
                && reply_sender.as_deref().is_none_or(|value| !value.is_empty() && value.chars().count() <= 128 && !value.chars().any(char::is_control))
                && reply_preview.as_deref().is_none_or(|value| value.chars().count() <= 320)
                && (reply_to.is_some() == reply_preview.is_some())
                && mentions.len() <= MAX_GROUP_MEMBERS
                && mentions.iter().all(|member| members.contains(member)) => Ok(()),
        Payload::GroupFileOffer { id, group_id, name, owner, members, file_name, size, sha256, logical, .. }
            if valid_id(id) && valid_group_id(group_id) && !name.is_empty() && name.len() <= 128
                && valid_members(owner, members) && valid_file_name(file_name)
                && *size > 0 && *size <= MAX_FILE_BYTES && valid_sha256(sha256) && *logical > 0 => Ok(()),
        Payload::GroupTransfer { id, group_id, name, owner, members }
            if valid_id(id) && valid_group_id(group_id) && !name.is_empty() && name.len() <= 128
                && valid_members(owner, members) => Ok(()),
        Payload::GroupDissolve { id, group_id } if valid_id(id) && valid_group_id(group_id) => Ok(()),
        Payload::GroupUpdate { id, group_id, name, owner, members }
            if valid_id(id) && valid_group_id(group_id) && !name.is_empty() && name.len() <= 128
                && valid_members(owner, members) => Ok(()),
        Payload::GroupRemove { id, group_id } if valid_id(id) && valid_group_id(group_id) => Ok(()),
        Payload::GroupLeave { id, group_id, member }
            if valid_id(id) && valid_group_id(group_id)
                && member.len() == 64 && member.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) => Ok(()),
        _ => Err(invalid("invalid message payload")),
    }
}

pub fn write_frame(mut writer: impl Write, payload: &Payload) -> io::Result<()> {
    validate(payload)?;
    let data = serde_json::to_vec(&Frame { version: VERSION, payload: payload.clone() }).map_err(|_| invalid("encoding failed"))?;
    if data.is_empty() || data.len() > MAX_FRAME { return Err(invalid("frame too large")); }
    writer.write_all(&(data.len() as u32).to_be_bytes())?;
    writer.write_all(&data)?;
    writer.flush()
}

/// The underlying transport must enforce bounded read/write timeouts.
pub fn read_frame(mut reader: impl Read) -> io::Result<Payload> {
    let mut header = [0; 4];
    reader.read_exact(&mut header)?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 || length > MAX_FRAME { return Err(invalid("invalid frame length")); }
    let mut data = vec![0; length];
    reader.read_exact(&mut data)?;
    let frame: Frame = serde_json::from_slice(&data).map_err(|_| invalid("invalid frame"))?;
    if frame.version != VERSION { return Err(invalid("unsupported protocol version")); }
    validate(&frame.payload)?;
    Ok(frame.payload)
}

/// Construct only after TLS has authenticated a locally verified peer. Both sides
/// must exchange Ready before chat or ACK frames may be sent or processed.
#[derive(Default)]
pub struct ReadyGate {
    local_ready: bool,
    remote_ready: bool,
}

impl ReadyGate {
    pub fn outbound(&mut self, payload: &Payload) -> io::Result<()> {
        validate(payload)?;
        match payload {
            Payload::Ready {} if !self.local_ready => { self.local_ready = true; Ok(()) }
            Payload::Ready {} => Err(invalid("duplicate ready")),
            _ if self.local_ready && self.remote_ready => Ok(()),
            _ => Err(invalid("peer not ready")),
        }
    }

    pub fn inbound(&mut self, payload: &Payload) -> io::Result<()> {
        validate(payload)?;
        match payload {
            Payload::Ready {} if !self.remote_ready => { self.remote_ready = true; Ok(()) }
            Payload::Ready {} => Err(invalid("duplicate ready")),
            _ if self.local_ready && self.remote_ready => Ok(()),
            _ => Err(invalid("peer not ready")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn frames_roundtrip_and_enforce_length_version_and_fields() {
        let text = Payload::Text { id: "message-123".into(), body: "你好\nworld".into(), created_at: 42, logical: 1 };
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &text).unwrap();
        assert_eq!(read_frame(Cursor::new(&bytes)).unwrap(), text);
        assert!(read_frame(Cursor::new([0xff, 0xff, 0xff, 0xff])).is_err());
        assert!(read_frame(Cursor::new([0, 0, 0, 0])).is_err());
        assert!(read_frame(Cursor::new(&bytes[..bytes.len() - 1])).is_err());
        let raw = br#"{"version":2,"payload":{"type":"Ready"}}"#;
        let mut invalid_version = (raw.len() as u32).to_be_bytes().to_vec();
        invalid_version.extend_from_slice(raw);
        assert!(read_frame(Cursor::new(invalid_version)).is_err());
        let raw = br#"{"version":1,"payload":{"type":"Ready","injected":true}}"#;
        let mut extra = (raw.len() as u32).to_be_bytes().to_vec();
        extra.extend_from_slice(raw);
        assert!(read_frame(Cursor::new(extra)).is_err());
        assert!(write_frame(Vec::new(), &Payload::Text { id: "1".into(), body: "x".repeat(MAX_BODY + 1), created_at: 1, logical: 1 }).is_err());
        assert!(write_frame(Vec::new(), &Payload::Text { id: "1".into(), body: "hello".into(), created_at: 1, logical: 0 }).is_err());
        assert!(write_frame(Vec::new(), &Payload::Ack { id: "bad id".into() }).is_err());
    }

    #[test]
    fn file_frames_enforce_metadata_bounds() {
        let offer = Payload::FileOffer {
            id: "m1".into(), name: "报告.pdf".into(), size: 1024, sha256: "a".repeat(64), created_at: 1, logical: 1,
        };
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &offer).unwrap();
        assert_eq!(read_frame(Cursor::new(&bytes)).unwrap(), offer);
        let invalid_offers = [
            Payload::FileOffer { id: "m1".into(), name: "../etc".into(), size: 1, sha256: "a".repeat(64), created_at: 1, logical: 1 },
            Payload::FileOffer { id: "m1".into(), name: "a".into(), size: 0, sha256: "a".repeat(64), created_at: 1, logical: 1 },
            Payload::FileOffer { id: "m1".into(), name: "a".into(), size: MAX_FILE_BYTES + 1, sha256: "a".repeat(64), created_at: 1, logical: 1 },
            Payload::FileOffer { id: "m1".into(), name: "a".into(), size: 1, sha256: "g".repeat(64), created_at: 1, logical: 1 },
            Payload::FileOffer { id: "m1".into(), name: "a".into(), size: 1, sha256: "a".repeat(64), created_at: 1, logical: 0 },
        ];
        for payload in invalid_offers { assert!(write_frame(Vec::new(), &payload).is_err()); }
        assert!(write_frame(Vec::new(), &Payload::FileAccept { id: "m1".into(), offset: MAX_FILE_BYTES + 1 }).is_err());
        assert!(write_frame(Vec::new(), &Payload::FileReject { id: "m1".into(), reason: "crafted text".into() }).is_err());
        assert!(write_frame(Vec::new(), &Payload::FileReject { id: "m1".into(), reason: "no_space".into() }).is_ok());
    }

    #[test]
    fn both_ready_messages_are_required_before_application_data() {
        let mut gate = ReadyGate::default();
        let text = Payload::Text { id: "m1".into(), body: "hello".into(), created_at: 1, logical: 1 };
        assert!(gate.outbound(&text).is_err());
        assert!(gate.inbound(&text).is_err());
        gate.outbound(&Payload::Ready {}).unwrap();
        assert!(gate.outbound(&text).is_err());
        gate.inbound(&Payload::Ready {}).unwrap();
        assert!(gate.inbound(&Payload::Ready {}).is_err());
        assert!(gate.outbound(&text).is_ok());
        assert!(gate.inbound(&Payload::Ack { id: "m1".into() }).is_ok());
    }

    #[test]
    fn group_file_offer_roundtrips_and_validates() {
        let offer = Payload::GroupFileOffer {
            id: "m1".into(), group_id: "a".repeat(32), name: "team".into(),
            owner: "b".repeat(64), members: vec!["b".repeat(64)],
            file_name: "a.txt".into(), size: 5, sha256: "c".repeat(64), created_at: 1, logical: 1,
        };
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &offer).unwrap();
        assert_eq!(read_frame(Cursor::new(&bytes)).unwrap(), offer);
        let mut bad = offer.clone();
        if let Payload::GroupFileOffer { size, .. } = &mut bad { *size = 0; }
        assert!(write_frame(Vec::new(), &bad).is_err());
        let mut bad_name = offer.clone();
        if let Payload::GroupFileOffer { file_name, .. } = &mut bad_name { *file_name = "../x".into(); }
        assert!(write_frame(Vec::new(), &bad_name).is_err());
    }

    #[test]
    fn group_frames_enforce_group_id_ownership_and_member_bounds() {
        let owner = "a".repeat(64);
        let member = "b".repeat(64);
        let group_id = "1".repeat(32);
        let text = Payload::GroupText {
            id: "m1".into(), group_id: group_id.clone(), name: "Team".into(), owner: owner.clone(),
            members: vec![owner.clone(), member.clone()], body: "hi".into(), created_at: 1, logical: 1,
            reply_to: None, reply_sender: None, reply_preview: None, mentions: Vec::new(),
        };
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &text).unwrap();
        assert_eq!(read_frame(Cursor::new(&bytes)).unwrap(), text);

        let base = |members: Vec<String>| Payload::GroupText {
            id: "m1".into(), group_id: group_id.clone(), name: "Team".into(), owner: owner.clone(),
            members, body: "hi".into(), created_at: 1, logical: 1,
            reply_to: None, reply_sender: None, reply_preview: None, mentions: Vec::new(),
        };
        // The owner must appear in the member snapshot.
        assert!(write_frame(Vec::new(), &base(vec![member.clone()])).is_err());
        // Duplicate or malformed fingerprints are rejected by receive, empty list here.
        assert!(write_frame(Vec::new(), &base(vec![])).is_err());
        // Group ids are exactly 32 hex characters.
        let mut short = text.clone();
        if let Payload::GroupText { group_id, .. } = &mut short { *group_id = "short".into(); }
        assert!(write_frame(Vec::new(), &short).is_err());
        // Too many members exceeds the frame contract.
        let crowd: Vec<String> = (0..=MAX_GROUP_MEMBERS).map(|i| format!("{:064x}", i)).collect();
        assert!(write_frame(Vec::new(), &base(crowd)).is_err());

        let update = Payload::GroupUpdate { id: "u1".into(), group_id: group_id.clone(), name: "Team".into(), owner: owner.clone(), members: vec![owner.clone(), member.clone()] };
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &update).unwrap();
        assert_eq!(read_frame(Cursor::new(&bytes)).unwrap(), update);

        assert!(write_frame(Vec::new(), &Payload::GroupRemove { id: "r1".into(), group_id: group_id.clone() }).is_ok());
        assert!(write_frame(Vec::new(), &Payload::GroupRemove { id: "r1".into(), group_id: "x".repeat(31) }).is_err());
        assert!(write_frame(Vec::new(), &Payload::GroupLeave { id: "l1".into(), group_id: group_id.clone(), member: member.clone() }).is_ok());
        assert!(write_frame(Vec::new(), &Payload::GroupLeave { id: "l1".into(), group_id, member: "not-a-fingerprint".into() }).is_err());

        // Quotes must pair an id with a preview, and mentions must stay inside the roster.
        let reply = Payload::GroupText {
            id: "m2".into(), group_id: "1".repeat(32), name: "Team".into(), owner: owner.clone(),
            members: vec![owner.clone(), member.clone()], body: "reply".into(), created_at: 2, logical: 2,
            reply_to: Some("m1".into()), reply_sender: Some("Owner".into()), reply_preview: Some("hi".into()),
            mentions: vec![member.clone()],
        };
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &reply).unwrap();
        assert_eq!(read_frame(Cursor::new(&bytes)).unwrap(), reply);
        let mut missing_preview = reply.clone();
        if let Payload::GroupText { reply_preview, .. } = &mut missing_preview { *reply_preview = None; }
        assert!(write_frame(Vec::new(), &missing_preview).is_err());
        let mut outsider = reply.clone();
        if let Payload::GroupText { mentions, .. } = &mut outsider { *mentions = vec!["c".repeat(64)]; }
        assert!(write_frame(Vec::new(), &outsider).is_err());

        let transfer = Payload::GroupTransfer {
            id: "t1".into(), group_id: "1".repeat(32), name: "Team".into(), owner: member.clone(),
            members: vec![owner.clone(), member.clone()],
        };
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &transfer).unwrap();
        assert_eq!(read_frame(Cursor::new(&bytes)).unwrap(), transfer);
        assert!(write_frame(Vec::new(), &Payload::GroupDissolve { id: "d1".into(), group_id: "short".into() }).is_err());
        assert!(write_frame(Vec::new(), &Payload::GroupDissolve { id: "d1".into(), group_id: "1".repeat(32) }).is_ok());
    }
}
