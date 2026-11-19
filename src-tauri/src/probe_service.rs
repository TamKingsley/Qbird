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
}

impl ProbeService {
    pub fn start(identity: Arc<Identity>, port: u16) -> ServiceResult<Self> {
        Self::start_inner(identity, port, None)
    }

    pub fn start_with_chat(identity: Arc<Identity>, port: u16, store: Arc<Mutex<Store>>) -> ServiceResult<Self> {
        Self::start_inner(identity, port, Some(store))
    }

    fn start_inner(identity: Arc<Identity>, port: u16, store: Option<Arc<Mutex<Store>>>) -> ServiceResult<Self> {
        let listener = TcpListener::bind(("0.0.0.0", port))?;
        listener.set_nonblocking(true)?;
        let actual_port = listener.local_addr()?.port();
        let random_key = KeyPair::generate()?;
        let digest = Sha256::digest(random_key.subject_public_key_info());
        let instance: String = digest[..12].iter().map(|b| format!("{b:02x}")).collect();
        let mut discovery = Discovery::browse()?;
        discovery.advertise_bound(&listener, &instance)?;
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
                                    if let Ok((fingerprint, automatic)) = probe::accept_classified(&identity, &mut socket) {
                                        if !automatic && !stopping.load(Ordering::Acquire) {
                                            if let Ok(mut requests) = requests.lock() {
                                                if requests.len() < MAX_PENDING || requests.contains_key(&fingerprint) {
                                                    requests.insert(fingerprint.clone(), ProbeRequest { fingerprint, remote_ip: remote.ip().to_string() });
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
        let mut service = Self { port: actual_port, stopping, running, requests, connections, worker: Some(worker), address_worker: None, discovery };
        if let Some(store) = address_store {
            service.address_worker = Some(crate::auto_address::start(address_identity, store, service.discovery.candidate_view(), Arc::clone(&service.stopping))?);
        }
        Ok(service)
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
