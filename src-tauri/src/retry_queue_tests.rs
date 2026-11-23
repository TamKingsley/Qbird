use super::*;
use std::{fs, net::TcpListener, path::Path, sync::atomic::AtomicU64, time::{SystemTime, UNIX_EPOCH}};
use crate::storage::IncomingResult;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Side {
    identity: Arc<Identity>,
    store: Arc<Mutex<Store>>,
    db: std::path::PathBuf,
}

#[test]
fn offline_recall_survives_restart_and_retries_without_sending_original() {
    let mut alice = side("alice-offline-recall");
    let bob = side("bob-offline-recall");
    trust(&alice, &bob);
    trust(&bob, &alice);
    let (alice_id, bob_id) = (alice.identity.fingerprint(), bob.identity.fingerprint());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let message = {
        let store = alice.store.lock().unwrap();
        store.set_contact_address(&bob_id, &address.to_string()).unwrap();
        let message = store.queue_direct(&alice_id, &bob_id, "never deliver this secret", 100).unwrap();
        assert_eq!(store.recall_direct(&alice_id, &message.id, 101).unwrap(), crate::storage::RecallResult::Recalled { delivered: false });
        message
    };
    // Simulate a failed connection before the application is restarted.
    let failed = thread::spawn(move || { let (_socket, _) = listener.accept().unwrap(); });
    let queue = RetryQueue::start(Arc::clone(&alice.identity), Arc::clone(&alice.store), Arc::new(AtomicBool::new(false))).unwrap();
    failed.join().unwrap();
    drop(queue);
    assert!(alice.store.lock().unwrap().next_pending_recall(None).unwrap().is_some());
    assert_eq!(alice.store.lock().unwrap().delete_messages("direct", &bob_id, &[message.id.clone()]).unwrap(), 1);
    drop(alice.store);
    alice.store = Arc::new(Mutex::new(Store::open(&alice.db).unwrap()));
    let listener = TcpListener::bind(address).unwrap();
    listener.set_nonblocking(true).unwrap();
    let (identity, store) = (Arc::clone(&bob.identity), Arc::clone(&bob.store));
    let worker = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            match listener.accept() {
                Ok((socket, _)) => return chat_transport::accept_one(&identity, &store, socket).is_ok(),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
                _ => return false,
            }
        }
    });
    let queue = RetryQueue::start(Arc::clone(&alice.identity), Arc::clone(&alice.store), Arc::new(AtomicBool::new(false))).unwrap();
    let deadline = Instant::now() + Duration::from_secs(12);
    while alice.store.lock().unwrap().next_pending_recall(None).unwrap().is_some() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }
    assert!(worker.join().unwrap());
    assert!(alice.store.lock().unwrap().next_pending_recall(None).unwrap().is_none());
    let received = bob.store.lock().unwrap().list_direct(&alice_id, 10, None).unwrap();
    assert_eq!(received.len(), 1);
    assert_eq!((&received[0].id, received[0].created_at, received[0].logical), (&message.id, message.created_at, message.logical));
    assert!(received[0].recalled_at.is_some());
    assert_ne!(received[0].body, "never deliver this secret");
    drop(queue);
}

fn side(label: &str) -> Side {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("retry-tests")
        .join(format!("{label}-{}-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir_all(&dir).unwrap();
    let db = dir.join("qbird.sqlite3");
    Side { identity: Arc::new(Identity::load_or_create(&dir.join("identity.bin")).unwrap()), store: Arc::new(Mutex::new(Store::open(&db).unwrap())), db }
}

fn trust(side: &Side, peer: &Side) {
    let fingerprint = peer.identity.fingerprint();
    let store = side.store.lock().unwrap();
    store.add_candidate(&fingerprint, "peer").unwrap();
    store.confirm_candidate(&fingerprint).unwrap();
}

#[test]
fn offline_queue_retries_after_peer_returns_and_deduplicates() {
    let mut alice = side("alice");
    let bob = side("bob");
    trust(&alice, &bob);
    trust(&bob, &alice);
    let alice_id = alice.identity.fingerprint();
    let bob_id = bob.identity.fingerprint();
    let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reservation.local_addr().unwrap();
    let message = {
        let store = alice.store.lock().unwrap();
        store.set_contact_address(&bob_id, &address.to_string()).unwrap();
        store.queue_direct(&alice_id, &bob_id, "retry after reconnect", 7).unwrap()
    };
    // Simulate a received message whose acknowledgement was lost.
    assert_eq!(bob.store.lock().unwrap().receive_direct(&alice_id, &message.id, &message.body, message.created_at, message.logical).unwrap(), IncomingResult::Inserted);
    // Reopen the database to exercise recovery of address and pending status.
    drop(alice.store);
    alice.store = Arc::new(Mutex::new(Store::open(&alice.db).unwrap()));
    reservation.set_nonblocking(true).unwrap();
    let failed_attempt = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            match reservation.accept() {
                Ok((_socket, _)) => return true, // close without completing TLS or acknowledging
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
                _ => return false,
            }
        }
    });
    let queue = RetryQueue::start(Arc::clone(&alice.identity), Arc::clone(&alice.store), Arc::new(AtomicBool::new(false))).unwrap();
    assert!(failed_attempt.join().unwrap(), "the first attempt never reached the peer");
    assert!(alice.store.lock().unwrap().pending_direct_by_id(&bob_id, &message.id).unwrap().is_some());
    let listener = TcpListener::bind(address).unwrap();
    listener.set_nonblocking(true).unwrap();
    let (identity, store) = (Arc::clone(&bob.identity), Arc::clone(&bob.store));
    let worker = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            match listener.accept() {
                Ok((socket, _)) => return chat_transport::accept_one(&identity, &store, socket).is_ok(),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
                _ => return false,
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(12);
    while alice.store.lock().unwrap().pending_direct_by_id(&bob_id, &message.id).unwrap().is_some() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }
    assert!(alice.store.lock().unwrap().pending_direct_by_id(&bob_id, &message.id).unwrap().is_none());
    assert!(worker.join().unwrap());
    assert_eq!(bob.store.lock().unwrap().list_direct(&alice_id, 10, None).unwrap().len(), 1);
    drop(queue);
}

#[test]
fn unavailable_peer_does_not_prevent_another_delivery() {
    let alice = side("alice-fair");
    let bob = side("bob-fair");
    let offline = side("offline-fair");
    trust(&alice, &bob);
    trust(&alice, &offline);
    trust(&bob, &alice);
    let alice_id = alice.identity.fingerprint();
    let bob_id = bob.identity.fingerprint();
    let offline_id = offline.identity.fingerprint();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let reserved = TcpListener::bind("127.0.0.1:0").unwrap();
    let offline_address = reserved.local_addr().unwrap();
    drop(reserved);
    let (online_message, pending_message) = {
        let store = alice.store.lock().unwrap();
        store.set_contact_address(&bob_id, &address.to_string()).unwrap();
        store.set_contact_address(&offline_id, &offline_address.to_string()).unwrap();
        (store.queue_direct(&alice_id, &bob_id, "online", 1).unwrap(), store.queue_direct(&alice_id, &offline_id, "offline", 2).unwrap())
    };
    listener.set_nonblocking(true).unwrap();
    let (identity, store) = (Arc::clone(&bob.identity), Arc::clone(&bob.store));
    let receiver = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match listener.accept() {
                Ok((socket, _)) => return chat_transport::accept_one(&identity, &store, socket).is_ok(),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
                _ => return false,
            }
        }
    });
    let queue = RetryQueue::start(Arc::clone(&alice.identity), Arc::clone(&alice.store), Arc::new(AtomicBool::new(false))).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while alice.store.lock().unwrap().pending_direct_by_id(&bob_id, &online_message.id).unwrap().is_some() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }
    assert!(alice.store.lock().unwrap().pending_direct_by_id(&bob_id, &online_message.id).unwrap().is_none());
    assert!(alice.store.lock().unwrap().pending_direct_by_id(&offline_id, &pending_message.id).unwrap().is_some());
    assert!(receiver.join().unwrap());
    assert_eq!(bob.store.lock().unwrap().list_direct(&alice_id, 10, None).unwrap()[0].body, "online");
    drop(queue);
}
