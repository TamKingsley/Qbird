//! Untrusted LAN discovery only. Never use mDNS data as an identity or trust decision.
use std::{collections::BTreeMap, io, net::{IpAddr, SocketAddr, TcpListener}, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, thread::{self, JoinHandle}, time::Duration};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::Serialize;
use crate::address::is_unicast;

const SERVICE_TYPE: &str = "_qbird._tcp.local.";
const MAX_SERVICES: usize = 256;
const MAX_ADDRESSES: usize = 16;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Candidate {
    /// An unauthenticated DNS-SD service name, not a verified user name.
    pub service_name: String,
    pub addresses: Vec<String>,
}

type CandidateMap = BTreeMap<String, Candidate>;

pub struct Discovery {
    daemon: ServiceDaemon,
    entries: Arc<Mutex<CandidateMap>>,
    stopping: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    registered_name: Option<String>,
}

impl Discovery {
    pub fn browse() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let daemon = ServiceDaemon::new()?;
        let receiver = daemon.browse(SERVICE_TYPE)?;
        let entries = Arc::new(Mutex::new(CandidateMap::new()));
        let stopping = Arc::new(AtomicBool::new(false));
        let (worker_entries, worker_stopping) = (Arc::clone(&entries), Arc::clone(&stopping));
        let worker = thread::spawn(move || {
            while !worker_stopping.load(Ordering::Acquire) {
                match receiver.recv_timeout(Duration::from_millis(200)) {
                    Ok(ServiceEvent::ServiceResolved(service)) => {
                        let addresses = service.get_addresses_v4().into_iter().map(IpAddr::V4);
                        if let Ok(mut entries) = worker_entries.lock() {
                            update_candidate(&mut entries, &service.fullname, service.port, addresses);
                        }
                    }
                    Ok(ServiceEvent::ServiceRemoved(_, fullname)) => {
                        if let Ok(mut entries) = worker_entries.lock() { entries.remove(&fullname.to_ascii_lowercase()); }
                    }
                    Ok(_) | Err(mdns_sd::RecvTimeoutError::Timeout) => {}
                    Err(mdns_sd::RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        Ok(Self { daemon, entries, stopping, worker: Some(worker), registered_name: None })
    }

    /// Only call once an authenticated Qbird listener is actually ready. No identity is advertised.
    pub fn advertise_bound(&mut self, listener: &TcpListener, random_instance: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if self.registered_name.is_some() || random_instance.is_empty() || random_instance.len() > 32
            || !random_instance.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid service instance").into());
        }
        let address = listener.local_addr()?;
        if !address.ip().is_unspecified() || !address.is_ipv4() || address.port() == 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "listener must be bound to IPv4 wildcard").into());
        }
        let hostname = format!("qbird-{random_instance}.local.");
        let properties: &[(&str, &str)] = &[];
        let info = ServiceInfo::new(SERVICE_TYPE, random_instance, &hostname, "", address.port(), properties)?.enable_addr_auto();
        let fullname = info.get_fullname().to_string();
        self.daemon.register(info)?;
        self.registered_name = Some(fullname);
        Ok(())
    }

    pub fn candidates(&self) -> Result<Vec<Candidate>, &'static str> {
        Ok(self.entries.lock().map_err(|_| "discovery unavailable")?.values().cloned().collect())
    }
}

fn update_candidate(entries: &mut CandidateMap, name: &str, port: u16, ips: impl Iterator<Item = IpAddr>) {
    if name.is_empty() || name.len() > 255 { return; }
    let key = name.to_ascii_lowercase();
    if port == 0 { entries.remove(&key); return; }
    if entries.len() >= MAX_SERVICES && !entries.contains_key(&key) { return; }
    let mut addresses: Vec<String> = ips.filter(|ip| is_unicast(*ip))
        .take(MAX_ADDRESSES).map(|ip| SocketAddr::new(ip, port).to_string()).collect();
    addresses.sort();
    addresses.dedup();
    if addresses.is_empty() { entries.remove(&key); return; }
    entries.insert(key, Candidate { service_name: name.to_owned(), addresses });
}

impl Drop for Discovery {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        if let Some(name) = &self.registered_name {
            if let Ok(receipt) = self.daemon.unregister(name) { let _ = receipt.recv_timeout(Duration::from_secs(2)); }
        }
        let _ = self.daemon.stop_browse(SERVICE_TYPE);
        if let Ok(receipt) = self.daemon.shutdown() { let _ = receipt.recv_timeout(Duration::from_secs(2)); }
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn candidates_are_bounded_deduplicated_and_never_trusted() {
        let mut map = CandidateMap::new();
        let name = "example._qbird._tcp.local.";
        let ips = [Ipv4Addr::new(192, 168, 1, 9), Ipv4Addr::new(192, 168, 1, 9), Ipv4Addr::UNSPECIFIED].into_iter().map(IpAddr::V4);
        update_candidate(&mut map, name, 42853, ips);
        assert_eq!(map[name].addresses, vec!["192.168.1.9:42853"]);
        update_candidate(&mut map, name, 0, std::iter::empty());
        assert!(map.is_empty());
        update_candidate(&mut map, name, 9, std::iter::once(IpAddr::V4(Ipv4Addr::UNSPECIFIED)));
        assert!(map.is_empty());
        for i in 0..MAX_SERVICES + 3 {
            update_candidate(&mut map, &format!("peer-{i}"), 42, std::iter::once(IpAddr::V4(Ipv4Addr::LOCALHOST)));
        }
        assert_eq!(map.len(), MAX_SERVICES);
    }

    #[test]
    fn cannot_advertise_an_unbound_or_specific_interface() {
        let mut discovery = Discovery::browse().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        assert!(discovery.advertise_bound(&listener, "test").is_err());
        assert!(discovery.advertise_bound(&listener, "bad name").is_err());
    }

    #[test]
    #[ignore = "requires a working multicast-capable interface and firewall permission"]
    fn publication_resolution_and_goodbye() {
        use std::time::{Instant, SystemTime, UNIX_EPOCH};
        let reader = Discovery::browse().unwrap();
        let mut publisher = Discovery::browse().unwrap();
        let listener = TcpListener::bind("0.0.0.0:0").unwrap();
        let instance = format!("test-{:x}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos());
        publisher.advertise_bound(&listener, &instance).unwrap();
        let found = |d: &Discovery| d.candidates().unwrap().iter().any(|candidate| candidate.service_name.starts_with(&instance) && candidate.addresses.iter().any(|address| address.ends_with(&format!(":{}", listener.local_addr().unwrap().port()))));
        let deadline = Instant::now() + Duration::from_secs(15);
        while !found(&reader) && Instant::now() < deadline { thread::sleep(Duration::from_millis(100)); }
        assert!(found(&reader), "published service was not resolved");
        drop(publisher);
        let deadline = Instant::now() + Duration::from_secs(8);
        while found(&reader) && Instant::now() < deadline { thread::sleep(Duration::from_millis(100)); }
        assert!(!found(&reader), "service remained after unregister");
    }
}
