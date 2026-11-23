//! Bounded listener for first-contact probes and optional authenticated chat.
use std::{collections::BTreeMap, io, net::{Shutdown, TcpListener, TcpStream}, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, thread::{self, JoinHandle}, time::{Duration, Instant}};
use rcgen::{KeyPair, PublicKeyData};
use serde::Serialize;
use sha2::{Digest, Sha256};
use crate::{chat_transport, discovery::Discovery, identity::Identity, probe, storage::Store};

const MAX_ACTIVE: usize = 8;
const MAX_PENDING: usize = 128;

type ServiceResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
type Connections = Arc<Mutex<BTreeMap<usize, TcpStream>>>;

struct ConnectionGuard {
    id: usize,
    connections: Connections,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        if let Ok(mut connections) = self.connections.lock() { connections.remove(&self.id); }
    }
}

fn shutdown_connections(connections: &Connections) {
    if let Ok(connections) = connections.lock() {
        for socket in connections.values() { let _ = socket.shutdown(Shutdown::Both); }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ProbeRequest {
    pub fingerprint: String,
    pub remote_ip: String,
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::{fs, net::SocketAddr, path::Path, time::{Instant, SystemTime, UNIX_EPOCH}};

    fn identity(label: &str) -> Arc<Identity> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("service-tests");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{label}-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        Arc::new(Identity::load_or_create(&path).unwrap())
    }

    #[test]
    fn listener_reports_untrusted_probes_and_releases_port() {
        let server = identity("server");
        let client = identity("client");
        let service = ProbeService::start(Arc::clone(&server), 0).unwrap();
        let port = service.port();
        assert!(ProbeService::start(Arc::clone(&server), port).is_err());
        let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        assert_eq!(probe::connect(&client, addr).unwrap(), server.fingerprint());
        let deadline = Instant::now() + Duration::from_secs(3);
        while service.requests().unwrap().is_empty() && Instant::now() < deadline { thread::sleep(Duration::from_millis(20)); }
        let entries = service.requests().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].fingerprint, client.fingerprint());
        drop(service);
        assert!(TcpListener::bind(("0.0.0.0", port)).is_ok());
    }

    #[test]
    fn chat_listener_suppresses_automatic_probe_but_keeps_manual_requests() {
        let server = identity("notifications-server");
        let known = identity("notifications-known");
        let unknown = identity("notifications-unknown");
        let automatic = identity("notifications-automatic");
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("service-tests");
        let db = dir.join(format!("notifications-{}-{}.sqlite", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        let store = Arc::new(Mutex::new(Store::open(&db).unwrap()));
        store.lock().unwrap().add_candidate(&known.fingerprint(), "known").unwrap();
        store.lock().unwrap().confirm_candidate(&known.fingerprint()).unwrap();
        let service = ProbeService::start_with_chat(Arc::clone(&server), 0, store).unwrap();
        let addr: SocketAddr = format!("127.0.0.1:{}", service.port()).parse().unwrap();
        assert_eq!(probe::connect(&known, addr).unwrap(), server.fingerprint());
        assert_eq!(probe::connect_auto(&automatic, addr).unwrap(), server.fingerprint());
        assert_eq!(probe::connect(&unknown, addr).unwrap(), server.fingerprint());
        let deadline = Instant::now() + Duration::from_secs(3);
        while service.requests().unwrap().len() < 2 && Instant::now() < deadline { thread::sleep(Duration::from_millis(20)); }
        let requests = service.requests().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests.iter().any(|request| request.fingerprint == known.fingerprint()));
        assert!(requests.iter().any(|request| request.fingerprint == unknown.fingerprint()));
    }

    #[test]
    fn direct_add_automatically_syncs_and_respects_privacy() {
        let server = identity("direct-add-server");
        let client = identity("direct-add-client");
        let other = identity("direct-add-other");
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("service-tests");
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let server_store = Arc::new(Mutex::new(Store::open(&dir.join(format!("direct-server-{unique}.sqlite"))).unwrap()));
        let client_store = Arc::new(Mutex::new(Store::open(&dir.join(format!("direct-client-{unique}.sqlite"))).unwrap()));
        let service = ProbeService::start_with_chat(Arc::clone(&server), 0, Arc::clone(&server_store)).unwrap();
        let address: SocketAddr = format!("127.0.0.1:{}", service.port()).parse().unwrap();
        let announce = probe::ContactAnnounce { nickname: "Alice".into(), username: Some("Alice_01".into()), kind: None, address: None, reason: None, request_id: None };
        {
            let store = client_store.lock().unwrap();
            store.add_candidate(&server.fingerprint(), "Bob").unwrap();
            store.confirm_candidate(&server.fingerprint()).unwrap();
        }
        assert!(probe::announce_contact_pinned(&client, address, &announce, &other.fingerprint()).is_err());
        assert!(server_store.lock().unwrap().list_contacts().unwrap().is_empty());
        assert!(probe::announce_contact_pinned(&client, address, &announce, &server.fingerprint()).unwrap());
        assert!(probe::announce_contact_pinned(&client, address, &announce, &server.fingerprint()).unwrap(), "duplicate direct adds are idempotent");
        {
            let store = server_store.lock().unwrap();
            assert!(store.is_verified(&client.fingerprint()).unwrap());
            assert_eq!(store.list_contacts().unwrap().len(), 1);
            assert!(store.list_contact_requests("in").unwrap().is_empty());
        }
        let message = client_store.lock().unwrap().queue_direct(&client.fingerprint(), &server.fingerprint(), "hello", 1).unwrap();
        chat_transport::send_one(&client, &client_store, &server.fingerprint(), address, &message.id).unwrap();
        assert_eq!(server_store.lock().unwrap().list_direct(&client.fingerprint(), 10, None).unwrap()[0].body, "hello");

        server_store.lock().unwrap().set_auto_accept_contacts(false).unwrap();
        assert!(!probe::announce_contact(&other, address, &announce).unwrap());
        assert!(!server_store.lock().unwrap().is_verified(&other.fingerprint()).unwrap());
        server_store.lock().unwrap().set_auto_accept_contacts(true).unwrap();
        server_store.lock().unwrap().set_lan_visible(false).unwrap();
        assert!(!probe::announce_contact(&other, address, &announce).unwrap());
        assert!(!server_store.lock().unwrap().is_verified(&other.fingerprint()).unwrap());
    }

    #[test]
    fn contact_verification_requires_approval_and_correlates_retries() {
        use probe::{ContactAnnounce, ContactReply};
        let server = identity("verify-server");
        let client = identity("verify-client");
        let stranger = identity("verify-stranger");
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("service-tests");
        fs::create_dir_all(&dir).unwrap();
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let incoming = Arc::new(Mutex::new(Store::open(&dir.join(format!("verify-in-{stamp}.sqlite"))).unwrap()));
        let outgoing = Store::open(&dir.join(format!("verify-out-{stamp}.sqlite"))).unwrap();
        incoming.lock().unwrap().set_auto_accept_contacts(false).unwrap();
        let service = ProbeService::start_with_chat(Arc::clone(&server), 0, Arc::clone(&incoming)).unwrap();
        let addr: SocketAddr = format!("127.0.0.1:{}", service.port()).parse().unwrap();
        let server_key = server.fingerprint();
        let client_key = client.fingerprint();
        let mut message = ContactAnnounce { nickname: "Alice".into(), username: None, address: None,
            kind: Some("policy".into()), reason: None, request_id: None };
        assert_eq!(probe::contact_exchange(&client, addr, &message, &server_key).unwrap(), ContactReply::VerifyRequired);
        assert!(probe::contact_exchange(&client, addr, &message, &stranger.fingerprint()).is_err());
        message.kind = Some("request".into());
        message.reason = Some("Alice from the design team".into());
        let first = outgoing.create_contact_request("out", &server_key, "Bob", None, Some(&addr.to_string()), 1).unwrap().unwrap();
        message.request_id = Some(first.id.clone());
        assert_eq!(probe::contact_exchange(&client, addr, &message, &server_key).unwrap(), ContactReply::Pending);
        assert!(outgoing.list_contacts().unwrap().is_empty());
        let request = incoming.lock().unwrap().list_contact_requests("in").unwrap().remove(0);
        assert_eq!(incoming.lock().unwrap().get_setting(&format!("contact_reason_{}", request.id)).unwrap().as_deref(), Some("Alice from the design team"));
        assert!(incoming.lock().unwrap().list_contacts().unwrap().is_empty());
        incoming.lock().unwrap().resolve_contact_request(&request.id, false, 2).unwrap();
        message.kind = Some("status".into());
        message.reason = None;
        assert_eq!(probe::contact_exchange(&client, addr, &message, &server_key).unwrap(), ContactReply::Rejected);
        outgoing.resolve_contact_request(&first.id, false, 2).unwrap();
        let second = outgoing.create_contact_request("out", &server_key, "Bob", None, Some(&addr.to_string()), 3).unwrap().unwrap();
        message.kind = Some("request".into());
        message.reason = Some("Please try again".into());
        message.request_id = Some(second.id.clone());
        assert_eq!(probe::contact_exchange(&client, addr, &message, &server_key).unwrap(), ContactReply::Pending);
        message.kind = Some("status".into());
        message.reason = None;
        assert_eq!(probe::contact_exchange(&client, addr, &message, &server_key).unwrap(), ContactReply::Pending);
        message.request_id = Some(first.id.clone());
        assert_eq!(probe::contact_exchange(&client, addr, &message, &server_key).unwrap(), ContactReply::Rejected);
        let pending = incoming.lock().unwrap().pending_contact_request(&client_key, "in").unwrap().unwrap();
        incoming.lock().unwrap().resolve_contact_request(&pending.id, true, 4).unwrap();
        message.request_id = Some(second.id.clone());
        assert_eq!(probe::contact_exchange(&client, addr, &message, &server_key).unwrap(), ContactReply::Accepted);
        outgoing.resolve_contact_request(&second.id, true, 4).unwrap();
        assert!(incoming.lock().unwrap().is_verified(&client_key).unwrap());
        assert!(outgoing.is_verified(&server_key).unwrap());
    }

    #[test]
    fn stopping_closes_connections_waiting_for_preface_or_tls() {
        use std::{io::{Read, Write}, net::TcpStream};
        let server = identity("stop-server");
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("service-tests");
        fs::create_dir_all(&dir).unwrap();
        let db = dir.join(format!("stop-{}-{}.sqlite", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        let store = Arc::new(Mutex::new(Store::open(&db).unwrap()));
        let service = ProbeService::start_with_chat(server, 0, store).unwrap();
        let address = format!("127.0.0.1:{}", service.port());
        let mut idle = TcpStream::connect(&address).unwrap();
        let mut tls = TcpStream::connect(&address).unwrap();
        tls.write_all(chat_transport::CHAT_PREFACE).unwrap();
        idle.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        tls.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        thread::sleep(Duration::from_millis(200));
        let started = Instant::now();
        drop(service);
        assert!(started.elapsed() < Duration::from_secs(2), "listener stop took too long");
        for stream in [&mut idle, &mut tls] {
            let mut data = [0u8; 1];
            match stream.read(&mut data) {
                Ok(0) => {},
                Err(e) if matches!(e.kind(), io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted | io::ErrorKind::UnexpectedEof) => {},
                result => panic!("connection remained open after stop: {result:?}"),
            }
        }
    }

    #[test]
    fn listener_bounds_connections_and_reuses_released_slots() {
        use std::io::Read;
        let service = ProbeService::start(identity("bounded"), 0).unwrap();
        let address = format!("127.0.0.1:{}", service.port());
        let mut clients: Vec<TcpStream> = (0..MAX_ACTIVE).map(|_| TcpStream::connect(&address).unwrap()).collect();
        let deadline = Instant::now() + Duration::from_secs(3);
        while service.connections.lock().unwrap().len() < MAX_ACTIVE && Instant::now() < deadline { thread::sleep(Duration::from_millis(10)); }
        assert_eq!(service.connections.lock().unwrap().len(), MAX_ACTIVE);
        let mut excess = TcpStream::connect(&address).unwrap();
        excess.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut buffer = [0u8; 1];
        match excess.read(&mut buffer) {
            Ok(0) => {},
            Err(e) if matches!(e.kind(), io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted) => {},
            result => panic!("excess connection was not closed: {result:?}"),
        }
        drop(clients.pop());
        let deadline = Instant::now() + Duration::from_secs(3);
        while service.connections.lock().unwrap().len() == MAX_ACTIVE && Instant::now() < deadline { thread::sleep(Duration::from_millis(10)); }
        assert_eq!(service.connections.lock().unwrap().len(), MAX_ACTIVE - 1);
        clients.push(TcpStream::connect(&address).unwrap());
        let deadline = Instant::now() + Duration::from_secs(3);
        while service.connections.lock().unwrap().len() < MAX_ACTIVE && Instant::now() < deadline { thread::sleep(Duration::from_millis(10)); }
        assert_eq!(service.connections.lock().unwrap().len(), MAX_ACTIVE);
    }

    #[test]
    #[ignore = "helper process for two_process_discovery_and_probe"]
    fn listener_child() {
        use std::io::{BufRead, Write};
        let server = identity("child");
        let service = ProbeService::start(Arc::clone(&server), 0).unwrap();
        println!("QBIRD_TEST_PORT={} FINGERPRINT={}", service.port(), server.fingerprint());
        std::io::stdout().flush().unwrap();
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while service.requests().unwrap().is_empty() && Instant::now() < deadline { thread::sleep(Duration::from_millis(20)); }
        assert_eq!(service.requests().unwrap().len(), 1);
    }

    #[test]
    #[ignore = "requires multicast-capable interface and launches a local helper process"]
    fn two_process_discovery_and_probe() {
        use std::{io::{BufRead, BufReader, Write}, process::{Command, Stdio}};
        let browser = Discovery::browse().unwrap();
        let client = identity("parent");
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "probe_service::tests::listener_child", "--ignored", "--nocapture"])
            .stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        let (port, expected) = loop {
            line.clear();
            assert!(output.read_line(&mut line).unwrap() > 0, "helper exited before starting");
            if let Some(start) = line.find("QBIRD_TEST_PORT=") {
                let mut parts = line[start..].split_whitespace();
                let port: u16 = parts.next().unwrap().trim_start_matches("QBIRD_TEST_PORT=").parse().unwrap();
                let expected = parts.next().unwrap().trim_start_matches("FINGERPRINT=").to_owned();
                break (port, expected);
            }
        };
        let deadline = Instant::now() + Duration::from_secs(15);
        let suffix = format!(":{port}");
        while !browser.candidates().unwrap().iter().any(|item| item.addresses.iter().any(|addr| addr.ends_with(&suffix))) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(100));
        }
        assert!(browser.candidates().unwrap().iter().any(|item| item.addresses.iter().any(|addr| addr.ends_with(&suffix))), "helper service was not discovered");
        let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        assert_ne!(client.fingerprint(), expected);
        assert_eq!(probe::connect(&client, addr).unwrap(), expected);
        writeln!(child.stdin.as_mut().unwrap(), "done").unwrap();
        drop(child.stdin.take());
        assert!(child.wait().unwrap().success());
    }
}

/// Display-only identity hints published over DNS-SD. Never includes the fingerprint.
#[derive(Clone, Debug, Default)]
pub struct LanAnnounce {
    pub nickname: String,
    pub username: String,
}

impl LanAnnounce {
    fn properties(&self) -> Vec<(&str, &str)> {
        let mut properties: Vec<(&str, &str)> = Vec::with_capacity(3);
        properties.push(("v", "2"));
        let nickname = self.nickname.trim();
        if !nickname.is_empty() { properties.push(("nick", nickname)); }
        let username = self.username.trim();
        if !username.is_empty() { properties.push(("user", username)); }
        properties
    }
}

fn announce_from_store(store: &Arc<Mutex<Store>>) -> LanAnnounce {
    store.lock().ok()
        .and_then(|store| store.get_profile().ok())
        .map(|profile| LanAnnounce { nickname: profile.nickname, username: profile.username })
        .unwrap_or_default()
}

pub struct ProbeService {
    port: u16,
    stopping: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    requests: Arc<Mutex<BTreeMap<String, ProbeRequest>>>,
    connections: Connections,
    worker: Option<JoinHandle<()>>,
    address_worker: Option<JoinHandle<()>>,
    // Keep discovery alive until the service is dropped.
    discovery: Discovery,
    /// Advertised identity, kept so visibility can be toggled without rebinding.
    instance: String,
    properties: Vec<(String, String)>,
}

impl ProbeService {
    pub fn start(identity: Arc<Identity>, port: u16) -> ServiceResult<Self> {
        Self::start_inner(identity, port, None, LanAnnounce::default())
    }

    pub fn start_announcing(identity: Arc<Identity>, port: u16, announce: LanAnnounce, store: Arc<Mutex<Store>>) -> ServiceResult<Self> {
        Self::start_inner(identity, port, Some(store), announce)
    }

    pub fn start_with_chat(identity: Arc<Identity>, port: u16, store: Arc<Mutex<Store>>) -> ServiceResult<Self> {
        let announce = announce_from_store(&store);
        Self::start_inner(identity, port, Some(store), announce)
    }

    fn start_inner(identity: Arc<Identity>, port: u16, store: Option<Arc<Mutex<Store>>>, announce: LanAnnounce) -> ServiceResult<Self> {
        let listener = TcpListener::bind(("0.0.0.0", port))?;
        listener.set_nonblocking(true)?;
        let actual_port = listener.local_addr()?.port();
        let random_key = KeyPair::generate()?;
        let digest = Sha256::digest(random_key.subject_public_key_info());
        let instance: String = digest[..12].iter().map(|b| format!("{b:02x}")).collect();
        let mut discovery = Discovery::browse()?;
        let properties = announce.properties().into_iter().map(|(key, value)| (key.to_owned(), value.to_owned())).collect::<Vec<_>>();
        discovery.advertise_bound(&listener, &instance, &announce.properties())?;
        let stopping = Arc::new(AtomicBool::new(false));
        let running = Arc::new(AtomicBool::new(true));
        let requests: Arc<Mutex<BTreeMap<String, ProbeRequest>>> = Arc::new(Mutex::new(BTreeMap::new()));
        let connections: Connections = Arc::new(Mutex::new(BTreeMap::new()));
        let address_identity = Arc::clone(&identity);
        let address_store = store.clone();
        let thread_connections = Arc::clone(&connections);
        let (thread_stopping, thread_requests, thread_running) = (Arc::clone(&stopping), Arc::clone(&requests), Arc::clone(&running));
        let worker = thread::spawn(move || {
            let mut children: Vec<JoinHandle<()>> = Vec::new();
            while !thread_stopping.load(Ordering::Acquire) {
                let mut index = 0;
                while index < children.len() {
                    if children[index].is_finished() { let _ = children.swap_remove(index).join(); }
                    else { index += 1; }
                }
                match listener.accept() {
                    Ok((socket, remote)) => {
                        let cloned = match socket.try_clone() { Ok(socket) => socket, Err(_) => continue };
                        let id = {
                            let Ok(mut connections) = thread_connections.lock() else { continue };
                            if thread_stopping.load(Ordering::Acquire) || connections.len() >= MAX_ACTIVE { continue; }
                            let Some(id) = (0..MAX_ACTIVE).find(|id| !connections.contains_key(id)) else { continue };
                            connections.insert(id, cloned);
                            id
                        };
                        let guard = ConnectionGuard { id, connections: Arc::clone(&thread_connections) };
                        let (identity, requests, stopping) = (Arc::clone(&identity), Arc::clone(&thread_requests), Arc::clone(&thread_stopping));
                        let store = store.clone();
                        let child = thread::Builder::new().spawn(move || {
                            let _guard = guard;
                            let mut socket = socket;
                            let mut first = [0u8; 1];
                            let deadline = Instant::now() + Duration::from_secs(5);
                            let mut detected = false;
                            if socket.set_nonblocking(true).is_ok() {
                                while !stopping.load(Ordering::Acquire) && Instant::now() < deadline {
                                    match socket.peek(&mut first) {
                                        Ok(1) => { detected = true; break; }
                                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(10)),
                                        Err(e) if e.kind() == io::ErrorKind::Interrupted => {},
                                        _ => break,
                                    }
                                }
                            }
                            if detected && !stopping.load(Ordering::Acquire) {
                                if first[0] == 0x16 {
                                    if let Ok((fingerprint, kind, mut connection)) = probe::accept_classified(&identity, &mut socket) {
                                        match kind {
                                            probe::ProbeKind::Contact => {
                                                // A peer asks to become a contact. The fingerprint always
                                                // comes from TLS; the body only carries display hints and
                                                // the requester's return address.
                                                if let Some(store) = &store {
                                                    let reply = match probe::read_contact_announce(&mut connection, &mut socket) {
                                                        Ok(announce) => {
                                                            let now = std::time::SystemTime::now()
                                                                .duration_since(std::time::UNIX_EPOCH)
                                                                .map(|value| value.as_secs() as i64).unwrap_or(0);
                                                            store.lock().ok().map(|store| {
                                                                use probe::ContactReply as Reply;
                                                                match announce.kind.as_deref().unwrap_or("announce") {
                                                                    "accept" | "reject" => {
                                                                        let Some(id) = announce.request_id.as_deref() else { return Reply::Unavailable; };
                                                                        let request = store.contact_request(id).ok().flatten();
                                                                        if !request.is_some_and(|r| r.peer == fingerprint && r.direction == "out" && r.status == "pending") { return Reply::Unavailable; }
                                                                        if store.resolve_contact_request(id, announce.kind.as_deref() == Some("accept"), now).ok().flatten().is_some() { Reply::Accepted } else { Reply::Unavailable }
                                                                    }
                                                                    "status" => {
                                                                        if store.is_verified(&fingerprint).unwrap_or(false) { return Reply::Accepted; }
                                                                        let Some(token) = announce.request_id.as_deref() else { return Reply::Unavailable; };
                                                                        match store.list_contact_requests("in").ok().and_then(|items| items.into_iter().find(|item| item.peer == fingerprint && store.get_setting(&format!("contact_token_{}", item.id)).ok().flatten().as_deref() == Some(token))) {
                                                                            Some(item) if item.status == "accepted" => Reply::Accepted,
                                                                            Some(item) if item.status == "rejected" => Reply::Rejected,
                                                                            Some(_) => Reply::Pending,
                                                                            None => Reply::Unavailable,
                                                                        }
                                                                    }
                                                                    "policy" => {
                                                                        if !store.lan_visible().unwrap_or(false) { Reply::Unavailable }
                                                                        else if store.auto_accept_contacts().unwrap_or(false) { Reply::Accepted }
                                                                        else { Reply::VerifyRequired }
                                                                    }
                                                                    "announce" | "request" => {
                                                                        if !store.lan_visible().unwrap_or(false) { return Reply::Unavailable; }
                                                                        let nickname = if announce.nickname.trim().is_empty() { fingerprint[..8].to_string() } else { announce.nickname.clone() };
                                                                        if store.is_verified(&fingerprint).unwrap_or(false) { return Reply::Accepted; }
                                                                        if announce.kind.as_deref() == Some("request") && announce.request_id.is_none() { return Reply::Unavailable; }
                                                                        if store.auto_accept_contacts().unwrap_or(false) {
                                                                            if store.add_candidate_with_username(&fingerprint, &nickname, announce.username.as_deref()).is_err()
                                                                                || !store.confirm_candidate(&fingerprint).unwrap_or(false) { return Reply::Unavailable; }
                                                                            if let Some(address) = announce.address.as_deref() { let _ = store.set_contact_address(&fingerprint, address); }
                                                                            Reply::Accepted
                                                                        } else {
                                                                            let Some(reason) = announce.reason.as_deref().filter(|s| !s.trim().is_empty()) else { return Reply::VerifyRequired; };
                                                                            let Some(token) = announce.request_id.as_deref() else { return Reply::Unavailable; };
                                                                            match store.create_contact_request("in", &fingerprint, &nickname, announce.username.as_deref(), announce.address.as_deref(), now) {
                                                                                Ok(Some(request)) if store.set_setting(&format!("contact_reason_{}", request.id), Some(reason)).is_ok() && store.set_setting(&format!("contact_token_{}", request.id), Some(token)).is_ok() => Reply::Pending,
                                                                                _ => Reply::Unavailable,
                                                                            }
                                                                        }
                                                                    }
                                                                    _ => Reply::Unavailable,
                                                                }
                                                            }).unwrap_or(probe::ContactReply::Unavailable)
                                                        }
                                                        Err(_) => probe::ContactReply::Unavailable,
                                                    };
                                                    let _ = probe::write_contact_ack(&mut connection, &mut socket, reply);
                                                }
                                            }
                                            probe::ProbeKind::Automatic => {}
                                            probe::ProbeKind::Manual => {
                                                if !stopping.load(Ordering::Acquire) {
                                                    if let Ok(mut requests) = requests.lock() {
                                                        if requests.len() < MAX_PENDING || requests.contains_key(&fingerprint) {
                                                            requests.insert(fingerprint.clone(), ProbeRequest { fingerprint, remote_ip: remote.ip().to_string() });
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                } else if first[0] == chat_transport::CHAT_PREFACE[0] {
                                    if let Some(store) = store { let _ = chat_transport::accept_one(&identity, &store, socket); }
                                }
                            }
                        });
                        if let Ok(child) = child { children.push(child); }
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(30)),
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {},
                    Err(_) => break,
                }
            }
            shutdown_connections(&thread_connections);
            for child in children { let _ = child.join(); }
            thread_stopping.store(true, Ordering::Release);
            thread_running.store(false, Ordering::Release);
        });
        let mut service = Self { port: actual_port, stopping, running, requests, connections, worker: Some(worker), address_worker: None, discovery, instance, properties };
        if let Some(store) = address_store {
            service.address_worker = Some(crate::auto_address::start(address_identity, store, service.discovery.candidate_view(), Arc::clone(&service.stopping))?);
        }
        Ok(service)
    }

    /// Hides from LAN discovery without stopping the listener, so confirmed
    /// contacts keep working while new peers cannot find this device.
    pub fn set_visible(&mut self, visible: bool) {
        if visible {
            if self.discovery.is_advertised() { return; }
            let props: Vec<(&str, &str)> = self.properties.iter().map(|(key, value)| (key.as_str(), value.as_str())).collect();
            let _ = self.discovery.advertise_port(self.port, &self.instance, &props);
        } else {
            self.discovery.unadvertise();
        }
    }

    pub fn port(&self) -> u16 { self.port }

    pub fn is_running(&self) -> bool { self.running.load(Ordering::Acquire) }

    pub fn requests(&self) -> Result<Vec<ProbeRequest>, &'static str> {
        Ok(self.requests.lock().map_err(|_| "probe unavailable")?.values().cloned().collect())
    }
}

impl Drop for ProbeService {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        shutdown_connections(&self.connections);
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
        if let Some(worker) = self.address_worker.take() { let _ = worker.join(); }
        // discovery is automatically shut down after this Drop returns.
        let _ = &self.discovery;
    }
}
