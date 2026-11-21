//! Bounded retry loop for persisted direct messages, group messages and group notices.
//! Addresses are connection hints only and never grant trust.
use std::{collections::BTreeMap, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}, mpsc}, thread::{self, JoinHandle}, time::{Duration, Instant}};
use crate::{address, chat_transport, identity::Identity, storage::Store};

const POLL: Duration = Duration::from_millis(250);
const WORK_PAUSE: Duration = Duration::from_millis(10);
const MAX_BACKOFF_SECONDS: u64 = 60;
/// Caps memory used by failures; the oldest entry is evicted when full.
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

/// One key covers a direct peer, a group recipient, or a control target.
fn ready(backoff: &BTreeMap<String, Backoff>, key: &str) -> bool {
    backoff.get(key).is_none_or(|entry| Instant::now() >= entry.next)
}

fn record(backoff: &mut BTreeMap<String, Backoff>, key: String, success: bool) {
    if success { backoff.remove(&key); return; }
    let seconds = backoff.get(&key).map_or(5, |entry| (entry.seconds * 2).min(MAX_BACKOFF_SECONDS));
    if !backoff.contains_key(&key) && backoff.len() >= MAX_BACKOFF_PEERS {
        if let Some(oldest) = backoff.iter().min_by_key(|(_, entry)| entry.next).map(|(key, _)| key.clone()) {
            backoff.remove(&oldest);
        }
    }
    backoff.insert(key, Backoff { next: Instant::now() + Duration::from_secs(seconds), seconds });
}

pub struct RetryQueue {
    stop: mpsc::Sender<()>,
    worker: Option<JoinHandle<()>>,
}

impl RetryQueue {
    pub fn start(identity: Arc<Identity>, store: Arc<Mutex<Store>>, busy: Arc<AtomicBool>) -> std::io::Result<Self> {
        let (stop, receiver) = mpsc::channel();
        let worker = thread::Builder::new().name("qbird-direct-retry".into()).spawn(move || {
            let mut direct_cursor: Option<String> = None;
            let mut group_cursor: Option<String> = None;
            let mut control_cursor: Option<String> = None;
            let mut recall_cursor: Option<String> = None;
            let mut backoff: BTreeMap<String, Backoff> = BTreeMap::new();
            loop {
                if receiver.try_recv().is_ok() { break; }
                let mut worked = false;

                let direct = match store.lock() {
                    Ok(store) => match store.next_pending_delivery(direct_cursor.as_deref()) {
                        Ok(Some(item)) => Some(item),
                        Ok(None) => { direct_cursor = None; None },
                        Err(_) => None,
                    },
                    Err(_) => None,
                };
                if let Some(item) = direct {
                    direct_cursor = Some(item.peer.clone());
                    let key = format!("direct:{}", item.peer);
                    if ready(&backoff, &key) && !busy.swap(true, Ordering::AcqRel) {
                        let _gate = Gate(Arc::clone(&busy));
                        let success = address::parse_manual_address(&item.address).is_ok_and(|address| {
                            chat_transport::send_one(&identity, &store, &item.peer, address, &item.id).is_ok()
                        });
                        record(&mut backoff, key, success);
                        worked = true;
                    }
                }

                let group = match store.lock() {
                    Ok(store) => match store.next_pending_group_delivery(group_cursor.as_deref()) {
                        Ok(Some(item)) => Some(item),
                        Ok(None) => { group_cursor = None; None },
                        Err(_) => None,
                    },
                    Err(_) => None,
                };
                if let Some(item) = group {
                    group_cursor = Some(item.member.clone());
                    let key = format!("group:{}", item.member);
                    if ready(&backoff, &key) && !busy.swap(true, Ordering::AcqRel) {
                        let _gate = Gate(Arc::clone(&busy));
                        let success = address::parse_manual_address(&item.address).is_ok_and(|address| {
                            chat_transport::send_group_one(&identity, &store, &item.member, address, &item.message_id).is_ok()
                        });
                        record(&mut backoff, key, success);
                        worked = true;
                    }
                }

                let control = match store.lock() {
                    Ok(store) => match store.next_pending_control(control_cursor.as_deref()) {
                        Ok(Some(item)) => Some(item),
                        Ok(None) => { control_cursor = None; None },
                        Err(_) => None,
                    },
                    Err(_) => None,
                };
                if let Some(item) = control {
                    control_cursor = Some(item.target.clone());
                    let key = format!("control:{}", item.target);
                    if ready(&backoff, &key) && !busy.swap(true, Ordering::AcqRel) {
                        let _gate = Gate(Arc::clone(&busy));
                        let success = address::parse_manual_address(&item.address).is_ok_and(|address| {
                            chat_transport::send_control_one(&identity, &store, &item.target, address, &item).is_ok()
                        });
                        record(&mut backoff, key, success);
                        worked = true;
                    }
                }

                let recall = match store.lock() {
                    Ok(store) => match store.next_pending_recall(recall_cursor.as_deref()) {
                        Ok(Some(item)) => Some(item),
                        Ok(None) => { recall_cursor = None; None },
                        Err(_) => None,
                    },
                    Err(_) => None,
                };
                if let Some(item) = recall {
                    recall_cursor = Some(item.peer.clone());
                    let key = format!("recall:{}", item.peer);
                    if ready(&backoff, &key) && !busy.swap(true, Ordering::AcqRel) {
                        let _gate = Gate(Arc::clone(&busy));
                        let success = address::parse_manual_address(&item.address).is_ok_and(|address| {
                            chat_transport::send_recall_one(&identity, &store, &item.peer, address, &item.id, &item.message_id).is_ok()
                        });
                        record(&mut backoff, key, success);
                        worked = true;
                    }
                }

                let pause = if worked { WORK_PAUSE } else { POLL };
                if receiver.recv_timeout(pause).is_ok() { break; }
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
