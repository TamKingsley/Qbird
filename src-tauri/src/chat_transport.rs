//! Direct-message and file transport primitives. Content is never sent outside TLS.
use std::{fs::{self, File, OpenOptions}, io::{self, Read, Seek, SeekFrom, Write}, net::{SocketAddr, TcpStream}, path::PathBuf, sync::{Arc, Mutex}, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};
use rustls::{ClientConnection, ServerConnection, StreamOwned, pki_types::ServerName};
use crate::{certificate, files, identity::Identity, protocol::{self, Payload, ReadyGate}, storage::{DirectMessage, FileOfferDecision, GroupFileDecision, GroupDispatch, IncomingResult, Store}, tls};

pub const CHAT_PREFACE: &[u8] = b"QBIRD-CHAT/1\n";
const SESSION_TIMEOUT: Duration = Duration::from_secs(15);
const CHUNK_TIMEOUT: Duration = Duration::from_secs(60);
/// Raw chunk frames are length-prefixed and never pass through JSON.
pub const CHUNK_SIZE: usize = 256 * 1024;
const PROGRESS_STEP: u64 = 1024 * 1024;

/// Apply an absolute deadline to every underlying socket operation. Each long
/// operation (file chunks) resets it, so a stalled peer still fails promptly.
pub struct DeadlineStream {
    socket: TcpStream,
    deadline: Instant,
}

impl DeadlineStream {
    pub fn new(socket: TcpStream) -> io::Result<Self> {
        socket.set_nonblocking(false)?;
        Ok(Self { socket, deadline: Instant::now() + SESSION_TIMEOUT })
    }

    fn remaining(&self) -> io::Result<Duration> {
        self.deadline.checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "session timed out"))
    }

    pub fn touch(&mut self) { self.deadline = Instant::now() + SESSION_TIMEOUT; }
    fn touch_chunk(&mut self) { self.deadline = Instant::now() + CHUNK_TIMEOUT; }
}

impl Read for DeadlineStream {
    fn read(&mut self, data: &mut [u8]) -> io::Result<usize> {
        self.socket.set_read_timeout(Some(self.remaining()?))?;
        self.socket.read(data)
    }
}

impl Write for DeadlineStream {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.socket.set_write_timeout(Some(self.remaining()?))?;
        self.socket.write(data)
    }
    fn flush(&mut self) -> io::Result<()> { self.socket.flush() }
}

/// Both endpoints announce readiness after TLS starts; no content is sent first.
pub fn exchange_ready(stream: &mut (impl Read + Write)) -> io::Result<ReadyGate> {
    let mut gate = ReadyGate::default();
    let ready = Payload::Ready {};
    gate.outbound(&ready)?;
    protocol::write_frame(&mut *stream, &ready)?;
    let incoming = protocol::read_frame(&mut *stream)?;
    gate.inbound(&incoming)?;
    if incoming != ready { return Err(io::Error::new(io::ErrorKind::InvalidData, "ready expected")); }
    Ok(gate)
}

type ChatResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
fn invalid(message: &'static str) -> io::Error { io::Error::new(io::ErrorKind::InvalidData, message) }

fn unix_now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_secs()).ok()
        .and_then(|seconds| i64::try_from(seconds).ok()).unwrap_or(0)
}

/// Raw chunk framing: a zero-length frame terminates the stream.
fn write_chunk(stream: &mut impl Write, data: &[u8]) -> io::Result<()> {
    if data.len() > CHUNK_SIZE { return Err(invalid("chunk too large")); }
    stream.write_all(&(data.len() as u32).to_be_bytes())?;
    if !data.is_empty() { stream.write_all(data)?; }
    stream.flush()
}

fn read_chunk(stream: &mut impl Read, buffer: &mut [u8]) -> io::Result<usize> {
    let mut header = [0u8; 4];
    stream.read_exact(&mut header)?;
    let length = u32::from_be_bytes(header) as usize;
    if length > CHUNK_SIZE || length > buffer.len() { return Err(invalid("invalid chunk length")); }
    if length > 0 { stream.read_exact(&mut buffer[..length])?; }
    Ok(length)
}

#[cfg(all(test, windows))]
#[path = "chat_transport_tests.rs"]
mod tests;

/// Handle all messages of one incoming authenticated session, including files.
pub fn accept_one(identity: &Identity, store: &Mutex<Store>, socket: TcpStream) -> ChatResult<()> {
    let trusted = store.lock().map_err(|_| invalid("store unavailable"))?.list_contacts()?
        .into_iter().filter(|contact| contact.trust_state == "verified").map(|contact| contact.fingerprint).collect();
    let config = Arc::new(tls::server_config(identity, trusted)?);
    let mut socket = DeadlineStream::new(socket)?;
    let mut preface = vec![0; CHAT_PREFACE.len()];
    socket.read_exact(&mut preface)?;
    if preface != CHAT_PREFACE { return Err(invalid("invalid chat preface").into()); }
    let mut stream = StreamOwned::new(ServerConnection::new(config)?, socket);
    while stream.conn.is_handshaking() { stream.conn.complete_io(&mut stream.sock)?; }
    let chain = stream.conn.peer_certificates().ok_or_else(|| invalid("missing client certificate"))?;
    if chain.len() != 1 { return Err(invalid("invalid certificate chain").into()); }
    let peer = certificate::fingerprint(chain[0].as_ref()).map_err(invalid)?;
    let self_id = identity.fingerprint();
    let mut gate = exchange_ready(&mut stream)?;
    loop {
        stream.sock.touch();
        let incoming = match protocol::read_frame(&mut stream) {
            Ok(payload) => payload,
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(error) if error.kind() == io::ErrorKind::ConnectionReset => break,
            Err(error) => return Err(error.into()),
        };
        gate.inbound(&incoming)?;
        match incoming {
            Payload::Text { id, body, created_at, logical } => {
                let result = store.lock().map_err(|_| invalid("store unavailable"))?.receive_direct(&peer, &id, &body, created_at, logical)?;
                if !matches!(result, crate::storage::IncomingResult::Inserted | crate::storage::IncomingResult::Duplicate) {
                    return Err(invalid("message rejected").into());
                }
                let ack = Payload::Ack { id };
                gate.outbound(&ack)?;
                protocol::write_frame(&mut stream, &ack)?;
            }
            Payload::FileOffer { id, name, size, sha256, created_at, logical } => {
                receive_file(&mut stream, &mut gate, store, &peer, FileOffer { id, name, size, sha256, created_at, logical })?;
            }
            Payload::GroupText { id, group_id, name, owner, members, body, created_at, logical, reply_to, reply_sender, reply_preview, mentions } => {
                let result = store.lock().map_err(|_| invalid("store unavailable"))?
                    .receive_group(&self_id, &peer, &group_id, &name, &owner, &members, &id, &body, created_at, logical,
                        reply_to.as_deref(), reply_sender.as_deref(), reply_preview.as_deref(), &mentions, unix_now())?;
                // NotMember is acknowledged so a removed member stops being retried.
                if !matches!(result, IncomingResult::Inserted | IncomingResult::Duplicate | IncomingResult::NotMember) {
                    return Err(invalid("group message rejected").into());
                }
                acknowledge(&mut stream, &mut gate, &id)?;
            }
            Payload::GroupFileOffer { id, group_id, name, owner, members, file_name, size, sha256, created_at, logical } => {
                receive_group_file(&mut stream, &mut gate, store, &self_id, &peer, GroupFilePayload {
                    id, group_id, name, owner, members, file_name, size, sha256, created_at, logical,
                })?;
            }
            Payload::GroupUpdate { id, group_id, name, owner, members } => {
                let payload = serde_json::json!({ "name": name, "owner": owner, "members": members }).to_string();
                let result = store.lock().map_err(|_| invalid("store unavailable"))?
                    .receive_group_control(&self_id, &peer, "invite", &group_id, &payload, unix_now())?;
                if !matches!(result, IncomingResult::Inserted | IncomingResult::Duplicate | IncomingResult::NotMember) {
                    return Err(invalid("group update rejected").into());
                }
                acknowledge(&mut stream, &mut gate, &id)?;
            }
            Payload::GroupRemove { id, group_id } => {
                let result = store.lock().map_err(|_| invalid("store unavailable"))?
                    .receive_group_control(&self_id, &peer, "remove", &group_id, "{}", unix_now())?;
                if !matches!(result, IncomingResult::Inserted | IncomingResult::Duplicate | IncomingResult::NotMember) {
                    return Err(invalid("group removal rejected").into());
                }
                acknowledge(&mut stream, &mut gate, &id)?;
            }
            Payload::GroupLeave { id, group_id, member } => {
                let payload = serde_json::json!({ "member": member }).to_string();
                let result = store.lock().map_err(|_| invalid("store unavailable"))?
                    .receive_group_control(&self_id, &peer, "leave", &group_id, &payload, unix_now())?;
                if !matches!(result, IncomingResult::Inserted | IncomingResult::Duplicate | IncomingResult::NotMember) {
                    return Err(invalid("group leave rejected").into());
                }
                acknowledge(&mut stream, &mut gate, &id)?;
            }
            Payload::GroupTransfer { id, group_id, name, owner, members } => {
                let payload = serde_json::json!({ "name": name, "owner": owner, "members": members }).to_string();
                let result = store.lock().map_err(|_| invalid("store unavailable"))?
                    .receive_group_control(&self_id, &peer, "transfer", &group_id, &payload, unix_now())?;
                if !matches!(result, IncomingResult::Inserted | IncomingResult::Duplicate | IncomingResult::NotMember) {
                    return Err(invalid("group transfer rejected").into());
                }
                acknowledge(&mut stream, &mut gate, &id)?;
            }
            Payload::GroupDissolve { id, group_id } => {
                let result = store.lock().map_err(|_| invalid("store unavailable"))?
                    .receive_group_control(&self_id, &peer, "dissolve", &group_id, "{}", unix_now())?;
                if !matches!(result, IncomingResult::Inserted | IncomingResult::Duplicate | IncomingResult::NotMember) {
                    return Err(invalid("group dissolve rejected").into());
                }
                acknowledge(&mut stream, &mut gate, &id)?;
            }
            _ => return Err(invalid("unexpected payload").into()),
        }
    }
    Ok(())
}

struct FileOffer {
    id: String,
    name: String,
    size: u64,
    sha256: String,
    created_at: i64,
    logical: i64,
}

fn reject(stream: &mut StreamOwned<ServerConnection, DeadlineStream>, gate: &mut ReadyGate, id: &str, reason: &'static str) -> io::Result<()> {
    let payload = Payload::FileReject { id: id.to_owned(), reason: reason.to_owned() };
    gate.outbound(&payload)?;
    protocol::write_frame(&mut *stream, &payload)
}

fn receive_file(stream: &mut StreamOwned<ServerConnection, DeadlineStream>, gate: &mut ReadyGate, store: &Mutex<Store>, peer: &str, offer: FileOffer) -> ChatResult<()> {
    let FileOffer { id, name, size, sha256, created_at, logical } = offer;
    {
        let store = store.lock().map_err(|_| invalid("store unavailable"))?;
        if store.file_accept_confirm()? {
            match store.file_offer_status(&id)?.as_deref() {
                Some("accepted") => {}
                Some("rejected") => { drop(store); let _ = reject(stream, gate, &id, "declined"); return Err(invalid("declined").into()); }
                _ => {
                    store.record_file_offer_request(peer, &id, &name, size, created_at)?;
                    drop(store);
                    let _ = reject(stream, gate, &id, "defer");
                    return Err(invalid("waiting for local confirmation").into());
                }
            }
        }
    }
    let decision = store.lock().map_err(|_| invalid("store unavailable"))?
        .receive_file_offer(peer, &id, &name, size, &sha256, created_at, logical, unix_now())?;
    let (offset, target, partial) = match decision {
        FileOfferDecision::Untrusted => { let _ = reject(stream, gate, &id, "untrusted"); return Err(invalid("untrusted file offer").into()); }
        FileOfferDecision::Conflict => { let _ = reject(stream, gate, &id, "conflict"); return Err(invalid("file offer conflict").into()); }
        FileOfferDecision::NoSpace => { let _ = reject(stream, gate, &id, "no_space"); return Err(invalid("storage quota reached").into()); }
        FileOfferDecision::Complete => {
            let dir = store.lock().map_err(|_| invalid("store unavailable"))?.attachments_dir();
            let target = files::incoming_target(&dir.join("in"), &id, &files::sanitize_name(&name).ok_or_else(|| invalid("invalid name"))?);
            (size, target, files::incoming_partial(&dir.join("in"), &id))
        }
        FileOfferDecision::Ready { offset, target, partial } => (offset, target, partial),
    };
    let accept = Payload::FileAccept { id: id.clone(), offset };
    gate.outbound(&accept)?;
    protocol::write_frame(&mut *stream, &accept)?;

    // Consume any chunk frames (a completed transfer only expects the terminator).
    let mut buffer = vec![0u8; CHUNK_SIZE];
    let mut file = if offset == size && target.is_file() {
        None
    } else {
        if let Some(parent) = partial.parent() { fs::create_dir_all(parent)?; }
        let handle = if offset == 0 {
            File::create(&partial)?
        } else {
            let existing = fs::metadata(&partial).map(|m| m.len()).unwrap_or(0);
            if existing != offset {
                let _ = store.lock().map_err(|_| invalid("store unavailable"))?.fail_file_transfer(&id, "断点不一致", unix_now());
                return Err(invalid("partial file mismatch").into());
            }
            OpenOptions::new().append(true).open(&partial)?
        };
        Some(handle)
    };
    let mut received = offset;
    let mut reported = offset;
    loop {
        stream.sock.touch_chunk();
        let read = read_chunk(&mut *stream, &mut buffer)?;
        if read == 0 { break; }
        if let Some(handle) = file.as_mut() {
            handle.write_all(&buffer[..read])?;
        }
        received += read as u64;
        if received > size { 
            let _ = store.lock().map_err(|_| invalid("store unavailable"))?.fail_file_transfer(&id, "文件大小超出声明", unix_now());
            return Err(invalid("file larger than offered").into());
        }
        if received - reported >= PROGRESS_STEP {
            reported = received;
            if let Ok(store) = store.lock() { let _ = store.update_file_progress(&id, received, unix_now()); }
        }
    }
    if received != size {
        let _ = store.lock().map_err(|_| invalid("store unavailable"))?.fail_file_transfer(&id, "连接中断，等待续传", unix_now());
        return Err(invalid("incomplete file").into());
    }
    if let Some(handle) = file.as_ref() { handle.sync_all()?; }
    let verified = if received == offset && offset == size { files::sha256_file(&target) } else { files::sha256_file(&partial) };
    match verified {
        Ok(hash) if hash == sha256 => {}
        _ => {
            files::remove_quietly(&partial);
            let _ = store.lock().map_err(|_| invalid("store unavailable"))?.fail_file_transfer(&id, "完整性校验失败", unix_now());
            return Err(invalid("file checksum mismatch").into());
        }
    }
    if file.is_some() {
        if target.exists() { files::remove_quietly(&target); }
        fs::rename(&partial, &target)?;
    }
    {
        let store = store.lock().map_err(|_| invalid("store unavailable"))?;
        store.complete_file_transfer(&id, unix_now())?;
    }
    let complete = Payload::FileComplete { id };
    gate.outbound(&complete)?;
    protocol::write_frame(&mut *stream, &complete)?;
    Ok(())
}

struct GroupFilePayload {
    id: String,
    group_id: String,
    name: String,
    owner: String,
    members: Vec<String>,
    file_name: String,
    size: u64,
    sha256: String,
    created_at: i64,
    logical: i64,
}

/// Mirrors `receive_file` but records progress in the group transfer table and
/// validates membership through `receive_group_file`.
fn receive_group_file(stream: &mut StreamOwned<ServerConnection, DeadlineStream>, gate: &mut ReadyGate,
    store: &Mutex<Store>, self_id: &str, peer: &str, offer: GroupFilePayload) -> ChatResult<()> {
    let GroupFilePayload { id, group_id, name, owner, members, file_name, size, sha256, created_at, logical } = offer;
    {
        let store = store.lock().map_err(|_| invalid("store unavailable"))?;
        if store.file_accept_confirm()? {
            match store.file_offer_status(&id)?.as_deref() {
                Some("accepted") => {}
                Some("rejected") => { drop(store); let _ = reject(stream, gate, &id, "declined"); return Err(invalid("declined").into()); }
                _ => {
                    store.record_file_offer_request(peer, &id, &file_name, size, created_at)?;
                    drop(store);
                    let _ = reject(stream, gate, &id, "defer");
                    return Err(invalid("waiting for local confirmation").into());
                }
            }
        }
    }
    let decision = store.lock().map_err(|_| invalid("store unavailable"))?
        .receive_group_file(self_id, peer, &group_id, &name, &owner, &members, &id, &file_name, size, &sha256, created_at, logical, unix_now())?;
    let (offset, target, partial) = match decision {
        GroupFileDecision::NotMember => { let _ = reject(stream, gate, &id, "untrusted"); return Err(invalid("not a group member").into()); }
        GroupFileDecision::Conflict => { let _ = reject(stream, gate, &id, "conflict"); return Err(invalid("group file conflict").into()); }
        GroupFileDecision::NoSpace => { let _ = reject(stream, gate, &id, "no_space"); return Err(invalid("storage quota reached").into()); }
        GroupFileDecision::Complete => {
            let clean = files::sanitize_name(&file_name).ok_or_else(|| invalid("invalid name"))?;
            let dir = store.lock().map_err(|_| invalid("store unavailable"))?.attachments_dir();
            (size, files::incoming_target(&dir.join("in"), &id, &clean), files::incoming_partial(&dir.join("in"), &id))
        }
        GroupFileDecision::Ready { offset, target, partial } => (offset, target, partial),
    };
    let accept = Payload::FileAccept { id: id.clone(), offset };
    gate.outbound(&accept)?;
    protocol::write_frame(&mut *stream, &accept)?;

    let mut buffer = vec![0u8; CHUNK_SIZE];
    let mut file = if offset == size && target.is_file() {
        None
    } else {
        if let Some(parent) = partial.parent() { fs::create_dir_all(parent)?; }
        let handle = if offset == 0 {
            File::create(&partial)?
        } else {
            let existing = fs::metadata(&partial).map(|m| m.len()).unwrap_or(0);
            if existing != offset {
                let _ = store.lock().map_err(|_| invalid("store unavailable"))?.fail_group_file_transfer(&id, "断点不一致", unix_now());
                return Err(invalid("partial file mismatch").into());
            }
            OpenOptions::new().append(true).open(&partial)?
        };
        Some(handle)
    };
    let mut received = offset;
    let mut reported = offset;
    loop {
        stream.sock.touch_chunk();
        let read = read_chunk(&mut *stream, &mut buffer)?;
        if read == 0 { break; }
        if let Some(handle) = file.as_mut() { handle.write_all(&buffer[..read])?; }
        received += read as u64;
        if received > size {
            let _ = store.lock().map_err(|_| invalid("store unavailable"))?.fail_group_file_transfer(&id, "文件大小超出声明", unix_now());
            return Err(invalid("file larger than offered").into());
        }
        if received - reported >= PROGRESS_STEP {
            reported = received;
            if let Ok(store) = store.lock() { let _ = store.update_group_file_progress(&id, received, unix_now()); }
        }
    }
    if received != size {
        let _ = store.lock().map_err(|_| invalid("store unavailable"))?.pause_group_file_transfer(&id, "连接中断，等待续传", unix_now());
        return Err(invalid("incomplete file").into());
    }
    if let Some(handle) = file.as_ref() { handle.sync_all()?; }
    let verified = if received == offset && offset == size { files::sha256_file(&target) } else { files::sha256_file(&partial) };
    match verified {
        Ok(hash) if hash == sha256 => {}
        _ => {
            files::remove_quietly(&partial);
            let _ = store.lock().map_err(|_| invalid("store unavailable"))?.fail_group_file_transfer(&id, "完整性校验失败", unix_now());
            return Err(invalid("file checksum mismatch").into());
        }
    }
    if file.is_some() {
        if target.exists() { files::remove_quietly(&target); }
        fs::rename(&partial, &target)?;
    }
    {
        let store = store.lock().map_err(|_| invalid("store unavailable"))?;
        store.complete_group_file_transfer(&id, unix_now())?;
    }
    let complete = Payload::FileComplete { id };
    gate.outbound(&complete)?;
    protocol::write_frame(&mut *stream, &complete)?;
    Ok(())
}

/// Sends one queued group file to a single member with the same chunk rules as a direct file.
fn send_group_file(identity: &Identity, store: &Mutex<Store>, member: &str, address: SocketAddr, dispatch: &GroupDispatch) -> ChatResult<()> {
    let message_id = dispatch.message_id.clone();
    let transfer = store.lock().map_err(|_| invalid("store unavailable"))?.group_file_transfer(&message_id)?
        .ok_or_else(|| invalid("group file record missing"))?;
    let source = store.lock().map_err(|_| invalid("store unavailable"))?.group_file_source(&message_id)?
        .ok_or_else(|| invalid("group file source missing"))?;
    let source_path = PathBuf::from(&source);
    let sha256 = dispatch.file_sha256.clone().ok_or_else(|| invalid("file hash missing"))?;
    let size = u64::try_from(transfer.total).map_err(|_| invalid("invalid file size"))?;
    match files::file_size(&source_path) {
        Ok(actual) if actual == size => {}
        _ => {
            if let Ok(store) = store.lock() { let _ = store.fail_group_file_transfer(&message_id, "文件已移动或大小已变化", unix_now()); }
            return Err(invalid("source file changed").into());
        }
    }
    let attempt = (|| -> ChatResult<()> {
        let (mut stream, mut gate) = connect(identity, member, address)?;
        let offer = Payload::GroupFileOffer {
            id: message_id.clone(), group_id: dispatch.group_id.clone(), name: dispatch.group_name.clone(),
            owner: dispatch.owner.clone(), members: dispatch.members.clone(),
            file_name: dispatch.file_name.clone().unwrap_or_else(|| "file".into()),
            size, sha256: sha256.clone(), created_at: dispatch.created_at, logical: dispatch.logical,
        };
        gate.outbound(&offer)?;
        protocol::write_frame(&mut stream, &offer)?;
        let reply = protocol::read_frame(&mut stream)?;
        gate.inbound(&reply)?;
        let offset = match reply {
            Payload::FileReject { id, reason } if id == message_id => {
                if reason == "defer" {
                    if let Ok(store) = store.lock() { let _ = store.pause_group_file_transfer(&message_id, "等待对方确认接收", unix_now()); }
                    return Err(invalid("waiting for peer confirmation").into());
                }
                let text = reject_reason_text(&reason);
                if let Ok(store) = store.lock() { let _ = store.fail_group_file_transfer(&message_id, text, unix_now()); }
                return Err(invalid("group file rejected").into());
            }
            Payload::FileAccept { id, offset } if id == message_id && offset <= size => offset,
            _ => return Err(invalid("invalid file reply").into()),
        };
        let mut file = File::open(&source_path)?;
        if offset > 0 { file.seek(SeekFrom::Start(offset))?; }
        let mut buffer = vec![0u8; CHUNK_SIZE];
        let mut sent = offset;
        let mut reported = offset;
        if let Ok(store) = store.lock() { let _ = store.update_group_file_progress(&message_id, offset, unix_now()); }
        while sent < size {
            let wanted = std::cmp::min(CHUNK_SIZE as u64, size - sent) as usize;
            let read = file.read(&mut buffer[..wanted])?;
            if read == 0 { return Err(invalid("source file ended early").into()); }
            stream.sock.touch_chunk();
            write_chunk(&mut stream, &buffer[..read])?;
            sent += read as u64;
            if sent - reported >= PROGRESS_STEP {
                reported = sent;
                if let Ok(store) = store.lock() { let _ = store.update_group_file_progress(&message_id, sent, unix_now()); }
            }
        }
        stream.sock.touch_chunk();
        write_chunk(&mut stream, &[])?;
        let reply = protocol::read_frame(&mut stream)?;
        gate.inbound(&reply)?;
        if reply != (Payload::FileComplete { id: message_id.clone() }) { return Err(invalid("file not confirmed").into()); }
        let store = store.lock().map_err(|_| invalid("store unavailable"))?;
        store.complete_group_file_transfer(&message_id, unix_now())?;
        store.ack_group(member, &message_id)?;
        Ok(())
    })();
    match attempt {
        Ok(()) => Ok(()),
        Err(error) => {
            if let Ok(store) = store.lock() {
                if store.group_file_transfer(&message_id).ok().flatten().is_some_and(|transfer| transfer.status != "failed") {
                    let _ = store.pause_group_file_transfer(&message_id, "连接中断，等待重试", unix_now());
                }
            }
            Err(error)
        }
    }
}

fn acknowledge(stream: &mut StreamOwned<ServerConnection, DeadlineStream>, gate: &mut ReadyGate, id: &str) -> io::Result<()> {
    let ack = Payload::Ack { id: id.to_owned() };
    gate.outbound(&ack)?;
    protocol::write_frame(&mut *stream, &ack)
}

/// Deliver one queued group message to a single member; only that member's outbox row is cleared.
pub fn send_group_one(identity: &Identity, store: &Mutex<Store>, member: &str, address: SocketAddr, message_id: &str) -> ChatResult<()> {
    if address.port() == 0 || !crate::address::is_unicast(address.ip()) { return Err(invalid("invalid address").into()); }
    let dispatch = store.lock().map_err(|_| invalid("store unavailable"))?.group_dispatch(message_id)?
        .ok_or_else(|| invalid("group message not found"))?;
    if dispatch.kind == "file" { return send_group_file(identity, store, member, address, &dispatch); }
    let (mut stream, mut gate) = connect(identity, member, address)?;
    let text = Payload::GroupText {
        id: dispatch.message_id.clone(), group_id: dispatch.group_id, name: dispatch.group_name,
        owner: dispatch.owner, members: dispatch.members, body: dispatch.body,
        created_at: dispatch.created_at, logical: dispatch.logical,
        reply_to: dispatch.reply_to, reply_sender: dispatch.reply_sender,
        reply_preview: dispatch.reply_preview, mentions: dispatch.mentions,
    };
    gate.outbound(&text)?;
    protocol::write_frame(&mut stream, &text)?;
    let reply = protocol::read_frame(&mut stream)?;
    gate.inbound(&reply)?;
    if reply != (Payload::Ack { id: dispatch.message_id.clone() }) { return Err(invalid("invalid group acknowledgement").into()); }
    if !store.lock().map_err(|_| invalid("store unavailable"))?.ack_group(member, &dispatch.message_id)? {
        return Err(invalid("group delivery state changed").into());
    }
    Ok(())
}

/// Deliver one invite / roster update / removal / leave notice.
pub fn send_control_one(identity: &Identity, store: &Mutex<Store>, target: &str, address: SocketAddr, task: &crate::storage::GroupControlTask) -> ChatResult<()> {
    if address.port() == 0 || !crate::address::is_unicast(address.ip()) { return Err(invalid("invalid address").into()); }
    let payload = match task.kind.as_str() {
        "invite" | "members" => {
            let value: serde_json::Value = serde_json::from_str(&task.payload)?;
            let name = value.get("name").and_then(|v| v.as_str()).ok_or_else(|| invalid("invalid control payload"))?.to_owned();
            let owner = value.get("owner").and_then(|v| v.as_str()).ok_or_else(|| invalid("invalid control payload"))?.to_owned();
            let members: Vec<String> = value.get("members").and_then(|v| v.as_array())
                .ok_or_else(|| invalid("invalid control payload"))?
                .iter().filter_map(|v| v.as_str().map(str::to_owned)).collect();
            Payload::GroupUpdate { id: task.id.clone(), group_id: task.group_id.clone(), name, owner, members }
        }
        "transfer" => {
            let value: serde_json::Value = serde_json::from_str(&task.payload)?;
            let name = value.get("name").and_then(|v| v.as_str()).ok_or_else(|| invalid("invalid control payload"))?.to_owned();
            let owner = value.get("owner").and_then(|v| v.as_str()).ok_or_else(|| invalid("invalid control payload"))?.to_owned();
            let members: Vec<String> = value.get("members").and_then(|v| v.as_array())
                .ok_or_else(|| invalid("invalid control payload"))?
                .iter().filter_map(|v| v.as_str().map(str::to_owned)).collect();
            Payload::GroupTransfer { id: task.id.clone(), group_id: task.group_id.clone(), name, owner, members }
        }
        "dissolve" => Payload::GroupDissolve { id: task.id.clone(), group_id: task.group_id.clone() },
        "remove" => Payload::GroupRemove { id: task.id.clone(), group_id: task.group_id.clone() },
        "leave" => {
            let value: serde_json::Value = serde_json::from_str(&task.payload)?;
            let member = value.get("member").and_then(|v| v.as_str()).ok_or_else(|| invalid("invalid control payload"))?.to_owned();
            Payload::GroupLeave { id: task.id.clone(), group_id: task.group_id.clone(), member }
        }
        _ => return Err(invalid("unknown control kind").into()),
    };
    let (mut stream, mut gate) = connect(identity, target, address)?;
    gate.outbound(&payload)?;
    protocol::write_frame(&mut stream, &payload)?;
    let reply = protocol::read_frame(&mut stream)?;
    gate.inbound(&reply)?;
    if reply != (Payload::Ack { id: task.id.clone() }) { return Err(invalid("invalid control acknowledgement").into()); }
    if !store.lock().map_err(|_| invalid("store unavailable"))?.ack_control(&task.id)? {
        return Err(invalid("control state changed").into());
    }
    Ok(())
}

/// Send one persisted pending message; a lost acknowledgement leaves it retryable.
pub fn send_one(identity: &Identity, store: &Mutex<Store>, peer: &str, address: SocketAddr, id: &str) -> ChatResult<()> {
    if address.port() == 0 || !crate::address::is_unicast(address.ip()) { return Err(invalid("invalid address").into()); }
    let message = store.lock().map_err(|_| invalid("store unavailable"))?.pending_direct_by_id(peer, id)?
        .ok_or_else(|| invalid("pending message not found"))?;
    if message.kind == "file" { send_file(identity, store, peer, address, &message) } else { send_text(identity, store, peer, address, &message) }
}

fn connect(identity: &Identity, peer: &str, address: SocketAddr) -> ChatResult<(StreamOwned<ClientConnection, DeadlineStream>, ReadyGate)> {
    let config = Arc::new(tls::client_config(identity, peer.to_owned())?);
    let socket = TcpStream::connect_timeout(&address, Duration::from_secs(5))?;
    let mut socket = DeadlineStream::new(socket)?;
    socket.write_all(CHAT_PREFACE)?;
    let name = ServerName::try_from("qbird.invalid")?.to_owned();
    let mut stream = StreamOwned::new(ClientConnection::new(config, name)?, socket);
    while stream.conn.is_handshaking() { stream.conn.complete_io(&mut stream.sock)?; }
    let gate = exchange_ready(&mut stream)?;
    Ok((stream, gate))
}

fn send_text(identity: &Identity, store: &Mutex<Store>, peer: &str, address: SocketAddr, message: &DirectMessage) -> ChatResult<()> {
    let (mut stream, mut gate) = connect(identity, peer, address)?;
    let text = Payload::Text { id: message.id.clone(), body: message.body.clone(), created_at: message.created_at, logical: message.logical };
    gate.outbound(&text)?;
    protocol::write_frame(&mut stream, &text)?;
    let reply = protocol::read_frame(&mut stream)?;
    gate.inbound(&reply)?;
    if reply != (Payload::Ack { id: message.id.clone() }) { return Err(invalid("invalid acknowledgement").into()); }
    if !store.lock().map_err(|_| invalid("store unavailable"))?.ack_direct(peer, &message.id)? { return Err(invalid("message state changed").into()); }
    Ok(())
}

fn reject_reason_text(reason: &str) -> &'static str {
    match reason {
        "untrusted" => "对方未确认本机身份",
        "conflict" => "对方存在同 ID 的不同内容",
        "too_large" => "文件超出对方限制",
        "no_space" => "对方存储空间不足",
        "defer" => "等待对方确认接收",
        "declined" => "对方拒绝接收文件",
        _ => "对方拒绝了文件",
    }
}

fn send_file(identity: &Identity, store: &Mutex<Store>, peer: &str, address: SocketAddr, message: &DirectMessage) -> ChatResult<()> {
    let transfer = store.lock().map_err(|_| invalid("store unavailable"))?.file_transfer(&message.id)?
        .ok_or_else(|| invalid("file transfer record missing"))?;
    let source = transfer.source_path.clone().ok_or_else(|| invalid("file source missing"))?;
    let source_path = PathBuf::from(&source);
    let sha256 = message.file_sha256.clone().ok_or_else(|| invalid("file hash missing"))?;
    let size = u64::try_from(transfer.total).map_err(|_| invalid("invalid file size"))?;
    let permanent = |reason: &'static str| -> ChatResult<()> {
        if let Ok(store) = store.lock() { let _ = store.fail_file_transfer(&message.id, reason, unix_now()); }
        Err(invalid(reason).into())
    };
    match files::file_size(&source_path) {
        Ok(actual) if actual == size => {}
        _ => return permanent("文件已移动或大小已变化"),
    }
    let attempt = (|| -> ChatResult<()> {
        let (mut stream, mut gate) = connect(identity, peer, address)?;
        let offer = Payload::FileOffer {
            id: message.id.clone(), name: message.file_name.clone().unwrap_or_else(|| "file".into()),
            size, sha256: sha256.clone(), created_at: message.created_at, logical: message.logical,
        };
        gate.outbound(&offer)?;
        protocol::write_frame(&mut stream, &offer)?;
        let reply = protocol::read_frame(&mut stream)?;
        gate.inbound(&reply)?;
        let offset = match reply {
            Payload::FileReject { id, reason } if id == message.id => {
                if reason == "defer" {
                    if let Ok(store) = store.lock() { let _ = store.pause_file_transfer(&message.id, "等待对方确认接收", unix_now()); }
                    return Err(invalid("waiting for peer confirmation").into());
                }
                let text = reject_reason_text(&reason);
                if let Ok(store) = store.lock() { let _ = store.fail_file_transfer(&message.id, text, unix_now()); }
                return Err(invalid("file rejected").into());
            }
            Payload::FileAccept { id, offset } if id == message.id && offset <= size => offset,
            _ => return Err(invalid("invalid file reply").into()),
        };
        let mut file = File::open(&source_path)?;
        if offset > 0 { file.seek(SeekFrom::Start(offset))?; }
        let mut buffer = vec![0u8; CHUNK_SIZE];
        let mut sent = offset;
        let mut reported = offset;
        if let Ok(store) = store.lock() { let _ = store.update_file_progress(&message.id, offset, unix_now()); }
        while sent < size {
            let wanted = std::cmp::min(CHUNK_SIZE as u64, size - sent) as usize;
            let read = file.read(&mut buffer[..wanted])?;
            if read == 0 { return Err(invalid("source file ended early").into()); }
            stream.sock.touch_chunk();
            write_chunk(&mut stream, &buffer[..read])?;
            sent += read as u64;
            if sent - reported >= PROGRESS_STEP {
                reported = sent;
                if let Ok(store) = store.lock() { let _ = store.update_file_progress(&message.id, sent, unix_now()); }
            }
        }
        stream.sock.touch_chunk();
        write_chunk(&mut stream, &[])?;
        let reply = protocol::read_frame(&mut stream)?;
        gate.inbound(&reply)?;
        if reply != (Payload::FileComplete { id: message.id.clone() }) { return Err(invalid("file not confirmed").into()); }
        let store = store.lock().map_err(|_| invalid("store unavailable"))?;
        store.complete_file_transfer(&message.id, unix_now())?;
        store.ack_direct(peer, &message.id)?;
        Ok(())
    })();
    match attempt {
        Ok(()) => Ok(()),
        Err(error) => {
            // Only transient transport failures stay retryable; permanent ones were already recorded.
            if let Ok(store) = store.lock() {
                if store.file_transfer(&message.id).ok().flatten().is_some_and(|transfer| transfer.status != "failed") {
                    let _ = store.pause_file_transfer(&message.id, "连接中断，等待重试", unix_now());
                }
            }
            Err(error)
        }
    }
}
