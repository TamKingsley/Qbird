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
    /// Advertised nickname and username. Untrusted display hints only; the fingerprint
    /// obtained by probing the address remains the identity.
    pub nickname: Option<String>,
    pub username: Option<String>,
}

type CandidateMap = BTreeMap<String, Candidate>;

#[derive(Clone)]
pub(crate) struct CandidateView {
    entries: Arc<Mutex<CandidateMap>>,
    own_name: Option<String>,
}

impl CandidateView {
    #[cfg(test)]
    pub(crate) fn test_view() -> Self {
        Self { entries: Arc::new(Mutex::new(BTreeMap::new())), own_name: None }
    }

    #[cfg(test)]
    pub(crate) fn test_set(&self, name: &str, address: Option<SocketAddr>) {
        let mut entries = self.entries.lock().unwrap();
        match address {
            Some(address) => update_candidate(&mut entries, name, address.port(), std::iter::once(address.ip()), None, None),
            None => { entries.remove(&name.to_ascii_lowercase()); },
        }
    }

    pub fn candidates(&self) -> Result<Vec<Candidate>, &'static str> {
        Ok(self.entries.lock().map_err(|_| "discovery unavailable")?.iter()
            .filter(|(name, _)| Some(name.as_str()) != self.own_name.as_deref())
            .map(|(_, candidate)| candidate.clone()).collect())
    }

    /// Keep the discovery lock while applying the result so removal cannot race the check.
    pub fn while_current<T>(&self, name: &str, address: &str, apply: impl FnOnce() -> T) -> Option<T> {
        let entries = self.entries.lock().ok()?;
        let key = name.to_ascii_lowercase();
        if Some(key.as_str()) == self.own_name.as_deref() { return None; }
        if !entries.get(&key).is_some_and(|entry| entry.addresses.iter().any(|a| a == address)) { return None; }
        Some(apply())
    }
}

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
                        let (nickname, username) = advertised_profile(service.get_properties());
                        if let Ok(mut entries) = worker_entries.lock() {
                            update_candidate(&mut entries, &service.fullname, service.port, addresses, nickname.as_deref(), username.as_deref());
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

    /// Only call once an authenticated Qbird listener is actually ready. The DNS-SD record
    /// carries display-only hints; it never carries the fingerprint or any trust claim.
    pub fn advertise_bound(&mut self, listener: &TcpListener, random_instance: &str, properties: &[(&str, &str)]) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if self.registered_name.is_some() || random_instance.is_empty() || random_instance.len() > 32
            || !random_instance.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid service instance").into());
        }
        let address = listener.local_addr()?;
        if !address.ip().is_unspecified() || !address.is_ipv4() || address.port() == 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "listener must be bound to IPv4 wildcard").into());
        }
        let hostname = format!("qbird-{random_instance}.local.");
        for (key, value) in properties {
            if key.is_empty() || key.len() > 32 || value.len() > 192 { return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid announcement").into()); }
        }
        let info = ServiceInfo::new(SERVICE_TYPE, random_instance, &hostname, "", address.port(), properties)?.enable_addr_auto();
        let fullname = info.get_fullname().to_string();
        self.daemon.register(info)?;
        self.registered_name = Some(fullname);
        Ok(())
    }

    /// Withdraws the local service record without stopping discovery.
    pub fn unadvertise(&mut self) {
        if let Some(name) = self.registered_name.take() {
            let _ = self.daemon.unregister(&name);
        }
    }

    pub fn is_advertised(&self) -> bool { self.registered_name.is_some() }

    /// Re-publishes on a known port, used when visibility is switched back on.
    pub fn advertise_port(&mut self, port: u16, random_instance: &str, properties: &[(&str, &str)])
        -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if self.registered_name.is_some() || port == 0 || random_instance.is_empty() || random_instance.len() > 32
            || !random_instance.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid service instance").into());
        }
        for (key, value) in properties {
            if key.is_empty() || key.len() > 32 || value.len() > 192 {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid announcement").into());
            }
        }
        let hostname = format!("qbird-{random_instance}.local.");
        let info = ServiceInfo::new(SERVICE_TYPE, random_instance, &hostname, "", port, properties)?.enable_addr_auto();
        let fullname = info.get_fullname().to_string();
        self.daemon.register(info)?;
        self.registered_name = Some(fullname);
        Ok(())
    }

    pub fn candidates(&self) -> Result<Vec<Candidate>, &'static str> {
        Ok(self.entries.lock().map_err(|_| "discovery unavailable")?.values().cloned().collect())
    }

    pub(crate) fn candidate_view(&self) -> CandidateView {
        CandidateView { entries: Arc::clone(&self.entries), own_name: self.registered_name.as_ref().map(|name| name.to_ascii_lowercase()) }
    }
}

/// Reads the optional display hints a peer put in its TXT record. Values are bounded and
/// copied only when they satisfy the same shape the local UI uses.
fn advertised_profile(properties: &mdns_sd::TxtProperties) -> (Option<String>, Option<String>) {
    let mut nickname = None;
    let mut username = None;
    for property in properties.iter() {
        let value = property.val_str().trim();
        match property.key() {
            "nick" if !value.is_empty() && value.chars().count() <= 64 && !value.chars().any(char::is_control) => {
                nickname = Some(value.to_owned());
            }
            "user" if !value.is_empty() && value.len() <= 20
                && value.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_') => {
                username = Some(value.to_owned());
            }
            _ => {}
        }
    }
    (nickname, username)
}

fn update_candidate(entries: &mut CandidateMap, name: &str, port: u16, ips: impl Iterator<Item = IpAddr>, nickname: Option<&str>, username: Option<&str>) {
    if name.is_empty() || name.len() > 255 { return; }
    let key = name.to_ascii_lowercase();
    if port == 0 { entries.remove(&key); return; }
    if entries.len() >= MAX_SERVICES && !entries.contains_key(&key) { return; }
    let mut addresses: Vec<String> = ips.filter(|ip| is_unicast(*ip))
        .take(MAX_ADDRESSES).map(|ip| SocketAddr::new(ip, port).to_string()).collect();
    addresses.sort();
    addresses.dedup();
    if addresses.is_empty() { entries.remove(&key); return; }
    entries.insert(key, Candidate {
        service_name: name.to_owned(),
        addresses,
        nickname: nickname.map(str::to_owned),
        username: username.map(str::to_owned),
    });
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
        update_candidate(&mut map, name, 42853, ips, Some("林小满"), Some("xiaoman"));
        assert_eq!(map[name].addresses, vec!["192.168.1.9:42853"]);
        assert_eq!(map[name].nickname.as_deref(), Some("林小满"));
        assert_eq!(map[name].username.as_deref(), Some("xiaoman"));
        update_candidate(&mut map, name, 0, std::iter::empty(), None, None);
        assert!(map.is_empty());
        update_candidate(&mut map, name, 9, std::iter::once(IpAddr::V4(Ipv4Addr::UNSPECIFIED)), None, None);
        assert!(map.is_empty());
        for i in 0..MAX_SERVICES + 3 {
            update_candidate(&mut map, &format!("peer-{i}"), 42, std::iter::once(IpAddr::V4(Ipv4Addr::LOCALHOST)), None, None);
        }
        assert_eq!(map.len(), MAX_SERVICES);
    }

    #[test]
    fn cannot_advertise_an_unbound_or_specific_interface() {
        let mut discovery = Discovery::browse().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        assert!(discovery.advertise_bound(&listener, "test", &[]).is_err());
        assert!(discovery.advertise_bound(&listener, "bad name", &[]).is_err());
    }

    #[test]
    #[ignore = "requires a working multicast-capable interface and firewall permission"]
    fn publication_resolution_and_goodbye() {
        use std::time::{Instant, SystemTime, UNIX_EPOCH};
        let reader = Discovery::browse().unwrap();
        let mut publisher = Discovery::browse().unwrap();
        let listener = TcpListener::bind("0.0.0.0:0").unwrap();
        let instance = format!("test-{:x}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos());
        publisher.advertise_bound(&listener, &instance, &[("nick", "青鸟"), ("user", "qbird")]).unwrap();
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
