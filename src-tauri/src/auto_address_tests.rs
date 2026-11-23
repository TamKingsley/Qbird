use super::*;
use std::{fs, net::TcpListener, path::Path, time::{SystemTime, UNIX_EPOCH}};

fn test_path(label: &str) -> std::path::PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("auto-address-tests");
    fs::create_dir_all(&dir).unwrap();
    dir.join(format!("{label}-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()))
}

fn probe_one(client: &Identity, server: Identity, store: &Mutex<Store>, keep: bool, stopping: &AtomicBool) -> (bool, String) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let socket = listener.local_addr().unwrap();
    let address = socket.to_string();
    let view = CandidateView::test_view();
    view.test_set("candidate", Some(socket));
    if !keep { view.test_set("candidate", None); }
    let worker = thread::spawn(move || probe::accept(&server, listener.accept().unwrap().0).unwrap());
    let accepted = try_candidate(client, store, &view, "candidate", &address, stopping);
    worker.join().unwrap();
    (accepted, address)
}

#[test]
fn only_proven_verified_fingerprint_updates_address() {
    let client = Identity::load_or_create(&test_path("client")).unwrap();
    let bob = Identity::load_or_create(&test_path("bob")).unwrap();
    let bob_id = bob.fingerprint();
    let mallory = Identity::load_or_create(&test_path("mallory")).unwrap();
    let mallory_id = mallory.fingerprint();
    let store = Mutex::new(Store::open(&test_path("store.sqlite")).unwrap());
    {
        let db = store.lock().unwrap();
        db.add_candidate(&bob_id, "Bob").unwrap();
        db.confirm_candidate(&bob_id).unwrap();
        db.add_candidate(&mallory_id, "Pending").unwrap();
        db.set_contact_address(&bob_id, "127.0.0.1:1234").unwrap();
    }
    let stop = AtomicBool::new(false);
    let (saved, _) = probe_one(&client, mallory, &store, true, &stop);
    assert!(!saved);
    assert!(!store.lock().unwrap().is_verified(&mallory_id).unwrap());
    assert_eq!(store.lock().unwrap().contact_address(&bob_id).unwrap().as_deref(), Some("127.0.0.1:1234"));
    let (saved, address) = probe_one(&client, bob, &store, true, &stop);
    assert!(saved);
    assert_eq!(store.lock().unwrap().contact_address(&bob_id).unwrap().as_deref(), Some(address.as_str()));
    assert!(store.lock().unwrap().is_verified(&bob_id).unwrap());
}

#[test]
fn group_member_probe_is_not_a_friend_and_stops_after_leaving() {
    let client = Identity::load_or_create(&test_path("group-client")).unwrap();
    let server_path = test_path("group-member");
    let server = Identity::load_or_create(&server_path).unwrap();
    let peer = server.fingerprint();
    let me = client.fingerprint();
    let owner = "a".repeat(64);
    let group = "b".repeat(32);
    let store = Mutex::new(Store::open(&test_path("group.sqlite")).unwrap());
    {
        let db = store.lock().unwrap();
        db.add_candidate(&owner, "owner").unwrap();
        db.confirm_candidate(&owner).unwrap();
        db.set_auto_join_groups(true).unwrap();
        let payload = serde_json::json!({"name":"Team", "owner":owner, "members":[owner, me, peer]}).to_string();
        db.receive_group_control(&me, &owner, "invite", &group, &payload, 1).unwrap();
    }
    let stop = AtomicBool::new(false);
    let (saved, address) = probe_one(&client, server, &store, true, &stop);
    assert!(saved);
    assert_eq!(store.lock().unwrap().avatar_address(&me, &peer).unwrap().as_deref(), Some(address.as_str()));
    assert!(!store.lock().unwrap().is_verified(&peer).unwrap());
    store.lock().unwrap().receive_group_control(&me, &owner, "remove", &group, "{}", 2).unwrap();
    let server = Identity::load_or_create(&server_path).unwrap();
    assert!(!probe_one(&client, server, &store, true, &stop).0);
    assert_eq!(store.lock().unwrap().avatar_address(&me, &peer).unwrap(), None);
}

#[test]
fn removed_candidate_and_stopping_service_do_not_write_address() {
    let client = Identity::load_or_create(&test_path("client")).unwrap();
    let server_path = test_path("server");
    let server = Identity::load_or_create(&server_path).unwrap();
    let peer = server.fingerprint();
    let store = Mutex::new(Store::open(&test_path("store.sqlite")).unwrap());
    store.lock().unwrap().add_candidate(&peer, "Bob").unwrap();
    store.lock().unwrap().confirm_candidate(&peer).unwrap();
    let stop = AtomicBool::new(false);
    assert!(!probe_one(&client, server, &store, false, &stop).0);
    stop.store(true, Ordering::Release);
    let server = Identity::load_or_create(&server_path).unwrap();
    assert!(!probe_one(&client, server, &store, true, &stop).0);
    assert_eq!(store.lock().unwrap().contact_address(&peer).unwrap(), None);
}

#[test]
fn manual_address_during_probe_is_not_overwritten() {
    use std::sync::mpsc;
    let client = Arc::new(Identity::load_or_create(&test_path("race-client")).unwrap());
    let server = Identity::load_or_create(&test_path("race-server")).unwrap();
    let peer = server.fingerprint();
    let store = Arc::new(Mutex::new(Store::open(&test_path("race.sqlite")).unwrap()));
    store.lock().unwrap().add_candidate(&peer, "Bob").unwrap();
    store.lock().unwrap().confirm_candidate(&peer).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let socket = listener.local_addr().unwrap();
    let view = CandidateView::test_view();
    view.test_set("candidate", Some(socket));
    let (accepted, received) = mpsc::channel();
    let (proceed, waiting) = mpsc::channel();
    let server_worker = thread::spawn(move || {
        let stream = listener.accept().unwrap().0;
        accepted.send(()).unwrap();
        waiting.recv_timeout(Duration::from_secs(2)).unwrap();
        probe::accept(&server, stream).unwrap();
    });
    let client_store = Arc::clone(&store);
    let worker = thread::spawn(move || try_candidate(&client, &client_store, &view, "candidate", &socket.to_string(), &AtomicBool::new(false)));
    received.recv_timeout(Duration::from_secs(2)).unwrap();
    store.lock().unwrap().set_contact_address(&peer, "127.0.0.1:43210").unwrap();
    proceed.send(()).unwrap();
    assert!(!worker.join().unwrap());
    server_worker.join().unwrap();
    assert_eq!(store.lock().unwrap().contact_address(&peer).unwrap().as_deref(), Some("127.0.0.1:43210"));
}

#[test]
fn stalled_handshake_has_bounded_stop_and_no_address_write() {
    use std::{sync::mpsc, time::Instant};
    let client = Arc::new(Identity::load_or_create(&test_path("stall-client")).unwrap());
    let peer = "b".repeat(64);
    let store = Arc::new(Mutex::new(Store::open(&test_path("stall.sqlite")).unwrap()));
    store.lock().unwrap().add_candidate(&peer, "Bob").unwrap();
    store.lock().unwrap().confirm_candidate(&peer).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let view = CandidateView::test_view();
    view.test_set("stall", Some(listener.local_addr().unwrap()));
    let (accepted, received) = mpsc::channel();
    let (release, waiting) = mpsc::channel();
    let server_worker = thread::spawn(move || {
        let _stream = listener.accept().unwrap().0;
        accepted.send(()).unwrap();
        let _ = waiting.recv_timeout(Duration::from_secs(8));
    });
    let stopping = Arc::new(AtomicBool::new(false));
    let worker = start(client, Arc::clone(&store), view, Arc::clone(&stopping)).unwrap();
    received.recv_timeout(Duration::from_secs(2)).unwrap();
    let start = Instant::now();
    stopping.store(true, Ordering::Release);
    worker.join().unwrap();
    assert!(start.elapsed() < Duration::from_secs(7));
    release.send(()).unwrap();
    server_worker.join().unwrap();
    assert_eq!(store.lock().unwrap().contact_address(&peer).unwrap(), None);
}

#[test]
fn worker_discovers_verified_address_and_retry_delivers_queued_message() {
    use std::time::Instant;
    let alice = Arc::new(Identity::load_or_create(&test_path("alice")).unwrap());
    let bob = Arc::new(Identity::load_or_create(&test_path("bob")).unwrap());
    let alice_id = alice.fingerprint();
    let bob_id = bob.fingerprint();
    let alice_store = Arc::new(Mutex::new(Store::open(&test_path("alice.sqlite")).unwrap()));
    let bob_store = Arc::new(Mutex::new(Store::open(&test_path("bob.sqlite")).unwrap()));
    for (db, peer) in [(&alice_store, &bob_id), (&bob_store, &alice_id)] {
        let db = db.lock().unwrap();
        db.add_candidate(peer, "contact").unwrap();
        db.confirm_candidate(peer).unwrap();
    }
    let service = crate::probe_service::ProbeService::start_with_chat(Arc::clone(&bob), 0, Arc::clone(&bob_store)).unwrap();
    let address = format!("127.0.0.1:{}", service.port());
    let message = alice_store.lock().unwrap().queue_direct(&alice_id, &bob_id, "discovered delivery", 1).unwrap();
    let view = CandidateView::test_view();
    view.test_set("bob", Some(address.parse().unwrap()));
    let stopping = Arc::new(AtomicBool::new(false));
    let worker = start(Arc::clone(&alice), Arc::clone(&alice_store), view, Arc::clone(&stopping)).unwrap();
    let retry = crate::retry_queue::RetryQueue::start(Arc::clone(&alice), Arc::clone(&alice_store), Arc::new(AtomicBool::new(false))).unwrap();
    let deadline = Instant::now() + Duration::from_secs(12);
    while alice_store.lock().unwrap().pending_direct_by_id(&bob_id, &message.id).unwrap().is_some() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(alice_store.lock().unwrap().contact_address(&bob_id).unwrap().as_deref(), Some(address.as_str()));
    assert!(alice_store.lock().unwrap().pending_direct_by_id(&bob_id, &message.id).unwrap().is_none());
    assert_eq!(bob_store.lock().unwrap().list_direct(&alice_id, 10, None).unwrap()[0].body, "discovered delivery");
    stopping.store(true, Ordering::Release);
    worker.join().unwrap();
    drop(retry);
    drop(service);
}

#[test]
#[ignore = "requires multicast-capable interface and firewall permission"]
fn real_mdns_discovery_delivers_without_manual_address() {
    use std::time::Instant;
    let alice = Arc::new(Identity::load_or_create(&test_path("mdns-alice")).unwrap());
    let bob = Arc::new(Identity::load_or_create(&test_path("mdns-bob")).unwrap());
    let alice_id = alice.fingerprint();
    let bob_id = bob.fingerprint();
    let alice_store = Arc::new(Mutex::new(Store::open(&test_path("mdns-alice.sqlite")).unwrap()));
    let bob_store = Arc::new(Mutex::new(Store::open(&test_path("mdns-bob.sqlite")).unwrap()));
    for (db, peer) in [(&alice_store, &bob_id), (&bob_store, &alice_id)] {
        let db = db.lock().unwrap();
        db.add_candidate(peer, "contact").unwrap();
        db.confirm_candidate(peer).unwrap();
    }
    let bob_service = crate::probe_service::ProbeService::start_with_chat(Arc::clone(&bob), 0, Arc::clone(&bob_store)).unwrap();
    let alice_service = crate::probe_service::ProbeService::start_with_chat(Arc::clone(&alice), 0, Arc::clone(&alice_store)).unwrap();
    let message = alice_store.lock().unwrap().queue_direct(&alice_id, &bob_id, "real mdns delivery", 1).unwrap();
    let retry = crate::retry_queue::RetryQueue::start(Arc::clone(&alice), Arc::clone(&alice_store), Arc::new(AtomicBool::new(false))).unwrap();
    let deadline = Instant::now() + Duration::from_secs(35);
    while alice_store.lock().unwrap().pending_direct_by_id(&bob_id, &message.id).unwrap().is_some() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(100));
    }
    assert!(alice_store.lock().unwrap().contact_address(&bob_id).unwrap().is_some(), "mDNS address was not verified");
    assert!(alice_store.lock().unwrap().pending_direct_by_id(&bob_id, &message.id).unwrap().is_none(), "message was not delivered");
    assert_eq!(bob_store.lock().unwrap().list_direct(&alice_id, 10, None).unwrap()[0].body, "real mdns delivery");
    assert!(bob_service.requests().unwrap().is_empty(), "automatic discovery must not generate contact requests");
    drop(retry);
    drop(alice_service);
    drop(bob_service);
}
