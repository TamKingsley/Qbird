//! Discovery addresses are hints; only a proven, already verified fingerprint may be stored.
use std::{sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, thread::{self, JoinHandle}, time::Duration};
use crate::{address, discovery::CandidateView, identity::Identity, probe, storage::Store};

#[cfg(all(test, windows))]
#[path = "auto_address_tests.rs"]
mod tests;

fn try_candidate(identity: &Identity, store: &Mutex<Store>, view: &CandidateView, name: &str, address: &str, stopping: &AtomicBool) -> bool {
    let revision = match store.lock() { Ok(store) => store.address_revision(), Err(_) => return false };
    let socket = match address::parse_manual_address(address) { Ok(socket) => socket, Err(_) => return false };
    let peer = match probe::connect_auto(identity, socket) { Ok(peer) => peer, Err(_) => return false };
    if peer == identity.fingerprint() { return false; }
    view.while_current(name, address, || {
        if stopping.load(Ordering::Acquire) { return false; }
        match store.lock() {
            Ok(store) if !stopping.load(Ordering::Acquire) => store.set_discovered_address(&peer, address, revision).unwrap_or(false),
            _ => false,
        }
    }).unwrap_or(false)
}

pub(crate) fn start(identity: Arc<Identity>, store: Arc<Mutex<Store>>, view: CandidateView, stopping: Arc<AtomicBool>) -> std::io::Result<JoinHandle<()>> {
    thread::Builder::new().name("qbird-address-discovery".into()).spawn(move || {
        let mut cursor: Option<(String, String)> = None;
        while !stopping.load(Ordering::Acquire) {
            let mut choices: Vec<(String, String)> = view.candidates().unwrap_or_default().into_iter()
                .flat_map(|candidate| candidate.addresses.into_iter().map(move |address| (candidate.service_name.clone(), address)))
                .collect();
            choices.sort();
            choices.dedup();
            let next = cursor.as_ref().and_then(|last| choices.iter().find(|choice| *choice > last))
                .or_else(|| choices.first()).cloned();
            if let Some((name, address)) = next {
                cursor = Some((name.clone(), address.clone()));
                let _ = try_candidate(&identity, &store, &view, &name, &address, &stopping);
            }
            // Limit active probing to one candidate every five seconds, with responsive idle stop.
            for _ in 0..50 {
                if stopping.load(Ordering::Acquire) { return; }
                thread::sleep(Duration::from_millis(100));
            }
        }
    })
}
