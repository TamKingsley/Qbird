//! Bounded retry loop for persisted direct messages. Addresses never grant trust.
use std::{collections::BTreeMap, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}, mpsc}, thread::{self, JoinHandle}, time::{Duration, Instant}};
use crate::{address, chat_transport, identity::Identity, storage::Store};

const POLL: Duration = Duration::from_millis(250);
const MAX_BACKOFF_SECONDS: u64 = 60;
const MAX_BACKOFF_PEERS: usize = 1024;

#[cfg(all(test, windows))]
#[path = "retry_queue_tests.rs"]
mod tests;

struct Gate(Arc<AtomicBool>);
impl Drop for Gate {
    fn drop(&mut self) { self.0.store(false, Ordering::Release); }
}

struct Backoff {
    next: Instant,
    seconds: u64,
}

pub struct RetryQueue {
    stop: mpsc::Sender<()>,
    worker: Option<JoinHandle<()>>,
}

impl RetryQueue {
    pub fn start(identity: Arc<Identity>, store: Arc<Mutex<Store>>, busy: Arc<AtomicBool>) -> std::io::Result<Self> {
        let (stop, receiver) = mpsc::channel();
        let worker = thread::Builder::new().name("qbird-direct-retry".into()).spawn(move || {
            let mut cursor: Option<String> = None;
            let mut backoff: BTreeMap<String, Backoff> = BTreeMap::new();
            loop {
                if receiver.try_recv().is_ok() { break; }
                let item = match store.lock() {
                    Ok(store) => match store.next_pending_delivery(cursor.as_deref()) {
                        Ok(Some(item)) => Some(item),
                        Ok(None) => { cursor = None; None },
                        Err(_) => None,
                    },
                    Err(_) => None,
                };
                if let Some(item) = item {
                    cursor = Some(item.peer.clone());
                    let now = Instant::now();
                    let ready = backoff.get(&item.peer).is_none_or(|entry| now >= entry.next);
                    if ready && !busy.swap(true, Ordering::AcqRel) {
                        let _gate = Gate(Arc::clone(&busy));
                        let success = address::parse_manual_address(&item.address).is_ok_and(|address| {
                            chat_transport::send_one(&identity, &store, &item.peer, address, &item.id).is_ok()
                        });
                        if success {
                            backoff.remove(&item.peer);
                        } else {
                            let seconds = backoff.get(&item.peer).map_or(5, |entry| (entry.seconds * 2).min(MAX_BACKOFF_SECONDS));
                            if !backoff.contains_key(&item.peer) && backoff.len() >= MAX_BACKOFF_PEERS {
                                if let Some(oldest) = backoff.iter().min_by_key(|(_, entry)| entry.next).map(|(peer, _)| peer.clone()) {
                                    backoff.remove(&oldest);
                                }
                            }
                            backoff.insert(item.peer, Backoff { next: Instant::now() + Duration::from_secs(seconds), seconds });
                        }
                    }
                }
                if receiver.recv_timeout(POLL).is_ok() { break; }
            }
        })?;
        Ok(Self { stop, worker: Some(worker) })
    }
}

impl Drop for RetryQueue {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
    }
}
