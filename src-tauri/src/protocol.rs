//! Framing and application readiness only; always use inside an authenticated TLS channel.
use std::io::{self, Read, Write};
use serde::{Deserialize, Serialize};

const VERSION: u8 = 1;
const MAX_FRAME: usize = 200_000;
const MAX_BODY: usize = 32_768;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum Payload {
    Ready {},
    Text { id: String, body: String, created_at: i64, logical: i64 },
    Ack { id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Frame {
    version: u8,
    payload: Payload,
}

fn invalid(reason: &'static str) -> io::Error { io::Error::new(io::ErrorKind::InvalidData, reason) }

fn validate(payload: &Payload) -> io::Result<()> {
    let valid_id = |id: &str| !id.is_empty() && id.len() <= 128 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    match payload {
        Payload::Ready {} => Ok(()),
        Payload::Text { id, body, logical, .. } if valid_id(id) && !body.is_empty() && body.len() <= MAX_BODY && *logical > 0 => Ok(()),
        Payload::Ack { id } if valid_id(id) => Ok(()),
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
}
