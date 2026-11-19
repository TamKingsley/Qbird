//! Probe-only listener: proves key possession, but never confirms contacts or transmits chat.
use std::{collections::BTreeMap, io, net::TcpListener, sync::{Arc, Mutex, atomic::{AtomicBool, AtomicUsize, Ordering}}, thread::{self, JoinHandle}, time::Duration};
use rcgen::{KeyPair, PublicKeyData};
use serde::Serialize;
use sha2::{Digest, Sha256};
use crate::{discovery::Discovery, identity::Identity, probe};

const MAX_ACTIVE: usize = 8;
const MAX_PENDING: usize = 128;

type ServiceResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

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
    worker: Option<JoinHandle<()>>,
    // Keep discovery alive until the service is dropped.
    discovery: Discovery,
}

impl ProbeService {
    pub fn start(identity: Arc<Identity>, port: u16) -> ServiceResult<Self> {
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
        let active = Arc::new(AtomicUsize::new(0));
        let (thread_stopping, thread_requests, thread_running) = (Arc::clone(&stopping), Arc::clone(&requests), Arc::clone(&running));
        let worker = thread::spawn(move || {
            while !thread_stopping.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((socket, remote)) => {
                        if active.load(Ordering::Acquire) >= MAX_ACTIVE { drop(socket); continue; }
                        active.fetch_add(1, Ordering::AcqRel);
                        let (identity, requests, active, stopping) = (Arc::clone(&identity), Arc::clone(&thread_requests), Arc::clone(&active), Arc::clone(&thread_stopping));
                        thread::spawn(move || {
                            if let Ok(fingerprint) = probe::accept(&identity, socket) {
                                if !stopping.load(Ordering::Acquire) {
                                    if let Ok(mut requests) = requests.lock() {
                                        if requests.len() < MAX_PENDING || requests.contains_key(&fingerprint) {
                                            requests.insert(fingerprint.clone(), ProbeRequest { fingerprint, remote_ip: remote.ip().to_string() });
                                        }
                                    }
                                }
                            }
                            active.fetch_sub(1, Ordering::AcqRel);
                        });
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(30)),
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {},
                    Err(_) => break,
                }
            }
            thread_running.store(false, Ordering::Release);
        });
        Ok(Self { port: actual_port, stopping, running, requests, worker: Some(worker), discovery })
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
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
        // discovery is automatically shut down after this Drop returns.
        let _ = &self.discovery;
    }
}
