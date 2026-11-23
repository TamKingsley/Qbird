use super::*;
use crate::storage::IncomingResult;
use std::{fs, net::TcpListener, path::Path, sync::atomic::{AtomicU64, Ordering}, thread, time::{SystemTime, UNIX_EPOCH}};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Side {
    identity: Arc<Identity>,
    store: Arc<Mutex<Store>>,
}

#[test]
fn profile_avatar_sync_allows_only_active_members_without_granting_chat() {
    let alice = side("avatar-alice");
    let bob = side("avatar-bob");
    let (a, b) = (alice.identity.fingerprint(), bob.identity.fingerprint());
    let owner = "a".repeat(64);
    let group = "b".repeat(32);
    for side in [&alice, &bob] {
        let db = side.store.lock().unwrap();
        db.add_candidate(&owner, "owner").unwrap();
        db.confirm_candidate(&owner).unwrap();
        db.set_auto_join_groups(true).unwrap();
        let payload = serde_json::json!({"name":"Team", "owner":owner, "members":[owner, a, b]}).to_string();
        db.receive_group_control(&side.identity.fingerprint(), &owner, "invite", &group, &payload, 1).unwrap();
    }
    let mut png = vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10, 0, 0, 0, 13];
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&128u32.to_be_bytes());
    png.extend_from_slice(&128u32.to_be_bytes());
    png.resize(crate::avatar::MAX_AVATAR_BYTES, 0);
    crate::avatar::save(bob.store.lock().unwrap().data_dir(), &png).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (identity, store) = (Arc::clone(&bob.identity), Arc::clone(&bob.store));
    let worker = thread::spawn(move || accept_one(&identity, &store, listener.accept().unwrap().0).unwrap());
    fetch_profile_avatar(&alice.identity, &alice.store, &b, addr).unwrap();
    worker.join().unwrap();
    assert_eq!(alice.store.lock().unwrap().peer_avatar(&a, &b).unwrap(), Some(png));
    assert!(!alice.store.lock().unwrap().is_verified(&b).unwrap());

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (identity, store) = (Arc::clone(&bob.identity), Arc::clone(&bob.store));
    let worker = thread::spawn(move || accept_one(&identity, &store, listener.accept().unwrap().0).is_err());
    let (mut stream, mut gate) = connect(&alice.identity, &b, addr).unwrap();
    let payload = Payload::Text { id: "attack".into(), body: "no".into(), created_at: 1, logical: 1 };
    gate.outbound(&payload).unwrap();
    protocol::write_frame(&mut stream, &payload).unwrap();
    assert!(protocol::read_frame(&mut stream).is_err());
    assert!(worker.join().unwrap());
    assert!(!bob.store.lock().unwrap().is_verified(&a).unwrap());

    alice.store.lock().unwrap().receive_group_control(&a, &owner, "remove", &group, "{}", 2).unwrap();
    assert!(alice.store.lock().unwrap().peer_avatar(&a, &b).unwrap().is_none());
    assert!(fetch_profile_avatar(&alice.identity, &alice.store, &b, addr).is_err());
}

/// Hold the application-ready response until the test recalls the queued message.
fn delayed_ready(receiver: &Side, sender: &str) -> (SocketAddr, std::sync::mpsc::Receiver<()>, std::sync::mpsc::Sender<()>, thread::JoinHandle<bool>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let identity = Arc::clone(&receiver.identity);
    let trusted = sender.to_owned();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let worker = thread::spawn(move || {
        let config = Arc::new(crate::tls::server_config(&identity, vec![trusted]).unwrap());
        let mut socket = DeadlineStream::new(listener.accept().unwrap().0).unwrap();
        let mut preface = vec![0; CHAT_PREFACE.len()];
        socket.read_exact(&mut preface).unwrap();
        assert_eq!(preface, CHAT_PREFACE);
        let mut stream = StreamOwned::new(ServerConnection::new(config).unwrap(), socket);
        while stream.conn.is_handshaking() { stream.conn.complete_io(&mut stream.sock).unwrap(); }
        assert_eq!(protocol::read_frame(&mut stream).unwrap(), Payload::Ready {});
        ready_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        protocol::write_frame(&mut stream, &Payload::Ready {}).unwrap();
        protocol::read_frame(&mut stream).is_err()
    });
    (address, ready_rx, release_tx, worker)
}

#[test]
fn recall_during_connection_stops_direct_and_group_originals() {
    let alice = side("alice-recall-connect");
    let bob = side("bob-recall-connect");
    trust(&alice, &bob); trust(&bob, &alice);
    let (alice_id, bob_id) = (alice.identity.fingerprint(), bob.identity.fingerprint());
    let direct = alice.store.lock().unwrap().queue_direct(&alice_id, &bob_id, "private direct", 1).unwrap();
    let (address, ready, release, server) = delayed_ready(&bob, &alice_id);
    let (identity, store, peer, id) = (Arc::clone(&alice.identity), Arc::clone(&alice.store), bob_id.clone(), direct.id.clone());
    let sender = thread::spawn(move || send_one(&identity, &store, &peer, address, &id).is_err());
    ready.recv_timeout(Duration::from_secs(10)).unwrap();
    alice.store.lock().unwrap().recall_direct(&alice_id, &direct.id, 2).unwrap();
    release.send(()).unwrap();
    assert!(sender.join().unwrap());
    assert!(server.join().unwrap());
    assert!(bob.store.lock().unwrap().list_direct(&alice_id, 10, None).unwrap().is_empty());

    let group = alice.store.lock().unwrap().create_group(&alice_id, "Team", &[bob_id.clone()], 3).unwrap();
    let message = alice.store.lock().unwrap().queue_group(&alice_id, &group.id, "private group", None, &[], 4).unwrap();
    let (address, ready, release, server) = delayed_ready(&bob, &alice_id);
    let (identity, store, peer, id) = (Arc::clone(&alice.identity), Arc::clone(&alice.store), bob_id.clone(), message.id.clone());
    let sender = thread::spawn(move || send_group_one(&identity, &store, &peer, address, &id).is_err());
    ready.recv_timeout(Duration::from_secs(10)).unwrap();
    alice.store.lock().unwrap().recall_group(&alice_id, &group.id, &message.id, 5).unwrap();
    release.send(()).unwrap();
    assert!(sender.join().unwrap());
    assert!(server.join().unwrap());
    assert!(bob.store.lock().unwrap().list_group_messages(&group.id, 10, None).unwrap().is_empty());

    let path = file_fixture("recall-connect-file", b"file contents");
    let hash = crate::files::sha256_file(&path).unwrap();
    let direct_file = alice.store.lock().unwrap().queue_direct_file(&alice_id, &bob_id, "file.txt", 13, &hash, &path, 6).unwrap();
    let (address, ready, release, server) = delayed_ready(&bob, &alice_id);
    let (identity, store, peer, id) = (Arc::clone(&alice.identity), Arc::clone(&alice.store), bob_id.clone(), direct_file.id.clone());
    let sender = thread::spawn(move || send_one(&identity, &store, &peer, address, &id).is_err());
    ready.recv_timeout(Duration::from_secs(10)).unwrap();
    alice.store.lock().unwrap().recall_direct(&alice_id, &direct_file.id, 7).unwrap();
    release.send(()).unwrap();
    assert!(sender.join().unwrap());
    assert!(server.join().unwrap());

    let group_file = alice.store.lock().unwrap().queue_group_file(&alice_id, &group.id, "file.txt", 13, &hash, &path, 8).unwrap();
    let (address, ready, release, server) = delayed_ready(&bob, &alice_id);
    let (identity, store, peer, id) = (Arc::clone(&alice.identity), Arc::clone(&alice.store), bob_id.clone(), group_file.id.clone());
    let sender = thread::spawn(move || send_group_one(&identity, &store, &peer, address, &id).is_err());
    ready.recv_timeout(Duration::from_secs(10)).unwrap();
    alice.store.lock().unwrap().recall_group(&alice_id, &group.id, &group_file.id, 9).unwrap();
    release.send(()).unwrap();
    assert!(sender.join().unwrap());
    assert!(server.join().unwrap());
}

#[test]
fn group_recall_reaches_received_and_offline_members() {
    let alice = side("alice-recall-group");
    let bob = side("bob-recall-group");
    let carol = side("carol-recall-group");
    trust(&alice, &bob); trust(&bob, &alice);
    trust(&alice, &carol); trust(&carol, &alice);
    bob.store.lock().unwrap().set_auto_join_groups(true).unwrap();
    carol.store.lock().unwrap().set_auto_join_groups(true).unwrap();
    let (alice_id, bob_id, carol_id) = (alice.identity.fingerprint(), bob.identity.fingerprint(), carol.identity.fingerprint());
    let group = alice.store.lock().unwrap().create_group(&alice_id, "Team", &[bob_id.clone(), carol_id.clone()], 1).unwrap();
    for (peer, peer_id) in [(&bob, &bob_id), (&carol, &carol_id)] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        alice.store.lock().unwrap().set_contact_address(peer_id, &address.to_string()).unwrap();
        let task = {
            let store = alice.store.lock().unwrap();
            let first = store.next_pending_control(None).unwrap().unwrap();
            if first.target == *peer_id { first } else { store.next_pending_control(Some(&first.target)).unwrap().unwrap() }
        };
        let (identity, store) = (Arc::clone(&peer.identity), Arc::clone(&peer.store));
        let worker = thread::spawn(move || accept_one(&identity, &store, listener.accept().unwrap().0).unwrap());
        send_control_one(&alice.identity, &alice.store, peer_id, address, &task).unwrap();
        worker.join().unwrap();
    }
    let message = alice.store.lock().unwrap().queue_group(&alice_id, &group.id, "group secret", None, &[], 20).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (identity, store) = (Arc::clone(&bob.identity), Arc::clone(&bob.store));
    let worker = thread::spawn(move || accept_one(&identity, &store, listener.accept().unwrap().0).unwrap());
    send_group_one(&alice.identity, &alice.store, &bob_id, address, &message.id).unwrap();
    worker.join().unwrap();
    assert_eq!(alice.store.lock().unwrap().recall_group(&alice_id, &group.id, &message.id, 21).unwrap(), crate::storage::RecallResult::Recalled { delivered: true });
    for (peer, peer_id) in [(&bob, &bob_id), (&carol, &carol_id)] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        alice.store.lock().unwrap().set_contact_address(peer_id, &address.to_string()).unwrap();
        let task = {
            let store = alice.store.lock().unwrap();
            let first = store.next_pending_control(None).unwrap().unwrap();
            if first.target == *peer_id { first } else { store.next_pending_control(Some(&first.target)).unwrap().unwrap() }
        };
        assert_eq!(task.kind, "recall");
        let (identity, store) = (Arc::clone(&peer.identity), Arc::clone(&peer.store));
        let worker = thread::spawn(move || accept_one(&identity, &store, listener.accept().unwrap().0).unwrap());
        send_control_one(&alice.identity, &alice.store, peer_id, address, &task).unwrap();
        worker.join().unwrap();
        let received = peer.store.lock().unwrap().list_group_messages(&group.id, 10, None).unwrap();
        assert_eq!(received.len(), 1);
        assert_eq!((&received[0].id, received[0].created_at, received[0].logical), (&message.id, message.created_at, message.logical));
        assert!(received[0].recalled_at.is_some());
        assert_ne!(received[0].body, "group secret");
    }
    assert!(alice.store.lock().unwrap().next_pending_control(None).unwrap().is_none());
}

#[test]
fn recall_waits_until_pending_group_invitation_is_accepted() {
    let alice = side("alice-pending-invite-recall");
    let bob = side("bob-pending-invite-recall");
    trust(&alice, &bob); trust(&bob, &alice);
    let (alice_id, bob_id) = (alice.identity.fingerprint(), bob.identity.fingerprint());
    let group = alice.store.lock().unwrap().create_group(&alice_id, "Team", &[bob_id.clone()], 1).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    alice.store.lock().unwrap().set_contact_address(&bob_id, &address.to_string()).unwrap();
    let invite = alice.store.lock().unwrap().next_pending_control(None).unwrap().unwrap();
    let (identity, store) = (Arc::clone(&bob.identity), Arc::clone(&bob.store));
    let worker = thread::spawn(move || accept_one(&identity, &store, listener.accept().unwrap().0).unwrap());
    send_control_one(&alice.identity, &alice.store, &bob_id, address, &invite).unwrap();
    worker.join().unwrap();
    let message = alice.store.lock().unwrap().queue_group(&alice_id, &group.id, "secret", None, &[], 2).unwrap();
    alice.store.lock().unwrap().recall_group(&alice_id, &group.id, &message.id, 3).unwrap();
    let task = alice.store.lock().unwrap().next_pending_control(None).unwrap().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (identity, store) = (Arc::clone(&bob.identity), Arc::clone(&bob.store));
    let worker = thread::spawn(move || accept_one(&identity, &store, listener.accept().unwrap().0).is_err());
    assert!(send_control_one(&alice.identity, &alice.store, &bob_id, address, &task).is_err());
    assert!(worker.join().unwrap());
    assert!(alice.store.lock().unwrap().next_pending_control(None).unwrap().is_some());
    let invite_id = bob.store.lock().unwrap().pending_group_invitations(&bob_id).unwrap()[0].id.clone();
    assert_eq!(bob.store.lock().unwrap().accept_group_invitation(&invite_id, &bob_id, 4).unwrap(), Some(group.id.clone()));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (identity, store) = (Arc::clone(&bob.identity), Arc::clone(&bob.store));
    let worker = thread::spawn(move || accept_one(&identity, &store, listener.accept().unwrap().0).unwrap());
    send_control_one(&alice.identity, &alice.store, &bob_id, address, &task).unwrap();
    worker.join().unwrap();
    assert!(alice.store.lock().unwrap().next_pending_control(None).unwrap().is_none());
    let received = bob.store.lock().unwrap().list_group_messages(&group.id, 10, None).unwrap();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].id, message.id);
    assert!(received[0].recalled_at.is_some());
}

#[test]
fn can_deliver_a_message_beyond_the_first_pending_page() {
    let alice = side("alice-long-queue");
    let bob = side("bob-long-queue");
    trust(&alice, &bob);
    trust(&bob, &alice);
    let alice_id = alice.identity.fingerprint();
    let bob_id = bob.identity.fingerprint();
    let message = {
        let store = alice.store.lock().unwrap();
        for _ in 0..200 { store.queue_direct(&alice_id, &bob_id, "older", 1).unwrap(); }
        store.queue_direct(&alice_id, &bob_id, "after first page", 1).unwrap()
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (identity, store) = (Arc::clone(&bob.identity), Arc::clone(&bob.store));
    let worker = thread::spawn(move || accept_one(&identity, &store, listener.accept().unwrap().0).unwrap());
    send_one(&alice.identity, &alice.store, &bob_id, address, &message.id).unwrap();
    worker.join().unwrap();
    assert_eq!(bob.store.lock().unwrap().list_direct(&alice_id, 10, None).unwrap()[0].body, "after first page");
    assert!(alice.store.lock().unwrap().pending_direct_by_id(&bob_id, &message.id).unwrap().is_none());
    assert_eq!(alice.store.lock().unwrap().pending_direct(&bob_id, 200).unwrap().len(), 200);
}

#[test]
fn shared_listener_routes_probes_and_verified_chat() {
    let alice = side("alice-shared");
    let bob = side("bob-shared");
    trust(&alice, &bob);
    trust(&bob, &alice);
    let alice_id = alice.identity.fingerprint();
    let bob_id = bob.identity.fingerprint();
    let service = crate::probe_service::ProbeService::start_with_chat(Arc::clone(&bob.identity), 0, Arc::clone(&bob.store)).unwrap();
    let address = format!("127.0.0.1:{}", service.port()).parse().unwrap();
    let message = alice.store.lock().unwrap().queue_direct(&alice_id, &bob_id, "shared port", 9).unwrap();
    send_one(&alice.identity, &alice.store, &bob_id, address, &message.id).unwrap();
    assert_eq!(bob.store.lock().unwrap().list_direct(&alice_id, 10, None).unwrap()[0].body, "shared port");
    assert!(alice.store.lock().unwrap().pending_direct(&bob_id, 10).unwrap().is_empty());
    assert!(service.requests().unwrap().is_empty(), "chat must not create a probe request");
    assert_eq!(crate::probe::connect(&alice.identity, address).unwrap(), bob_id);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while service.requests().unwrap().is_empty() && std::time::Instant::now() < deadline {
        thread::sleep(std::time::Duration::from_millis(20));
    }
    let requests = service.requests().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].fingerprint, alice_id);
}

#[test]
fn changed_server_identity_and_missing_client_trust_leave_message_pending() {
    let alice = side("alice-bad");
    let bob = side("bob-bad");
    let impostor = side("impostor");
    trust(&alice, &bob);
    trust(&impostor, &alice);
    let alice_id = alice.identity.fingerprint();
    let bob_id = bob.identity.fingerprint();
    let message = alice.store.lock().unwrap().queue_direct(&alice_id, &bob_id, "must not reach impostor", 12).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (identity, store) = (Arc::clone(&impostor.identity), Arc::clone(&impostor.store));
    let worker = thread::spawn(move || accept_one(&identity, &store, listener.accept().unwrap().0).is_err());
    assert!(send_one(&alice.identity, &alice.store, &bob_id, address, &message.id).is_err());
    assert!(worker.join().unwrap());
    assert!(impostor.store.lock().unwrap().list_direct(&alice_id, 10, None).unwrap().is_empty());
    assert_eq!(alice.store.lock().unwrap().pending_direct(&bob_id, 10).unwrap().len(), 1);

    // Bob trusts a different identity, not Alice; local confirmation by Alice is insufficient.
    trust(&bob, &impostor);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (identity, store) = (Arc::clone(&bob.identity), Arc::clone(&bob.store));
    let worker = thread::spawn(move || accept_one(&identity, &store, listener.accept().unwrap().0).is_err());
    assert!(send_one(&alice.identity, &alice.store, &bob_id, address, &message.id).is_err());
    assert!(worker.join().unwrap());
    assert!(bob.store.lock().unwrap().list_direct(&alice_id, 10, None).unwrap().is_empty());
    assert_eq!(alice.store.lock().unwrap().pending_direct(&bob_id, 10).unwrap().len(), 1);
}

fn side(label: &str) -> Side {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("chat-tests")
        .join(format!("{label}-{}-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir.join("db.sqlite3")).unwrap();
    // Transfer tests stand in for a user who already accepts files automatically;
    // the shipping default is to ask before every file.
    store.set_file_accept_mode(crate::storage::FileAcceptMode::Auto).unwrap();
    Side { identity: Arc::new(Identity::load_or_create(&dir.join("identity.bin")).unwrap()), store: Arc::new(Mutex::new(store)) }
}

fn trust(side: &Side, peer: &Side) {
    let fingerprint = peer.identity.fingerprint();
    let store = side.store.lock().unwrap();
    store.add_candidate(&fingerprint, "peer").unwrap();
    store.confirm_candidate(&fingerprint).unwrap();
}

#[test]
fn tls_delivery_persists_before_ack_and_replay_deduplicates() {
    let alice = side("alice");
    let bob = side("bob");
    trust(&alice, &bob);
    trust(&bob, &alice);
    let alice_id = alice.identity.fingerprint();
    let bob_id = bob.identity.fingerprint();
    let first = alice.store.lock().unwrap().queue_direct(&alice_id, &bob_id, "first", 100).unwrap();
    let retry = alice.store.lock().unwrap().queue_direct(&alice_id, &bob_id, "retry", 50).unwrap();
    // Simulate a prior receive whose acknowledgement was lost.
    assert_eq!(bob.store.lock().unwrap().receive_direct(&alice_id, &retry.id, &retry.body, retry.created_at, retry.logical).unwrap(), IncomingResult::Inserted);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (identity, store) = (Arc::clone(&bob.identity), Arc::clone(&bob.store));
    let worker = thread::spawn(move || {
        for _ in 0..2 { accept_one(&identity, &store, listener.accept().unwrap().0).unwrap(); }
    });
    send_one(&alice.identity, &alice.store, &bob_id, addr, &first.id).unwrap();
    send_one(&alice.identity, &alice.store, &bob_id, addr, &retry.id).unwrap();
    worker.join().unwrap();
    let received = bob.store.lock().unwrap().list_direct(&alice_id, 20, None).unwrap();
    assert_eq!(received.len(), 2);
    assert_eq!(received.iter().filter(|m| m.id == retry.id).count(), 1);
    assert!(alice.store.lock().unwrap().pending_direct(&bob_id, 20).unwrap().is_empty());
    assert!(alice.store.lock().unwrap().list_direct(&bob_id, 20, None).unwrap().iter().all(|m| m.status == "delivered"));
}

fn file_fixture(label: &str, bytes: &[u8]) -> std::path::PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("chat-files");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{label}-{}-{}.bin", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
    fs::write(&path, bytes).unwrap();
    path
}

fn queue_file(alice: &Side, bob: &Side, path: &Path, name: &str) {
    let size = fs::metadata(path).unwrap().len();
    let hash = crate::files::sha256_file(path).unwrap();
    let (alice_id, bob_id) = (alice.identity.fingerprint(), bob.identity.fingerprint());
    alice.store.lock().unwrap().queue_direct_file(&alice_id, &bob_id, name, size, &hash, path, 9).unwrap();
}

#[test]
fn file_transfer_streams_chunks_and_persists_a_verified_copy() {
    let alice = side("alice-file");
    let bob = side("bob-file");
    trust(&alice, &bob);
    trust(&bob, &alice);
    let bob_id = bob.identity.fingerprint();
    let bytes: Vec<u8> = (0..700_000u32).map(|value| (value % 251) as u8).collect();
    let path = file_fixture("stream", &bytes);
    queue_file(&alice, &bob, &path, "photo.png");
    let message = alice.store.lock().unwrap().list_direct(&bob_id, 1, None).unwrap()[0].clone();
    assert_eq!(message.kind, "file");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (identity, store) = (Arc::clone(&bob.identity), Arc::clone(&bob.store));
    let worker = thread::spawn(move || accept_one(&identity, &store, listener.accept().unwrap().0).unwrap());
    send_one(&alice.identity, &alice.store, &bob_id, address, &message.id).unwrap();
    worker.join().unwrap();
    let (name, stored) = bob.store.lock().unwrap().attachment_path(&message.id).unwrap().unwrap();
    assert_eq!(name, "photo.png");
    assert_eq!(fs::read(&stored).unwrap(), bytes);
    assert_eq!(bob.store.lock().unwrap().file_transfer(&message.id).unwrap().unwrap().status, "completed");
    let delivered = alice.store.lock().unwrap().list_direct(&bob_id, 1, None).unwrap()[0].clone();
    assert_eq!(delivered.status, "delivered");
    assert_eq!(delivered.file_status.as_deref(), Some("completed"));
    crate::files::remove_quietly(&path);
    crate::files::remove_quietly(&stored);
}

#[test]
fn interrupted_file_transfer_resumes_from_stored_partial() {
    let alice = side("alice-resume");
    let bob = side("bob-resume");
    trust(&alice, &bob);
    trust(&bob, &alice);
    let (alice_id, bob_id) = (alice.identity.fingerprint(), bob.identity.fingerprint());
    let bytes: Vec<u8> = (0..400_000u32).map(|value| (value % 240) as u8).collect();
    let path = file_fixture("resume", &bytes);
    queue_file(&alice, &bob, &path, "archive.zip");
    let message = alice.store.lock().unwrap().list_direct(&bob_id, 1, None).unwrap()[0].clone();

    // Simulate an earlier attempt that stored the first 90,000 bytes and then dropped.
    let decision = bob.store.lock().unwrap().receive_file_offer(
        &alice_id, &message.id, "archive.zip", bytes.len() as u64, message.file_sha256.as_deref().unwrap(), message.created_at, message.logical, 10,
    ).unwrap();
    let crate::storage::FileOfferDecision::Ready { partial, .. } = decision else { panic!("expected ready") };
    fs::create_dir_all(partial.parent().unwrap()).unwrap();
    fs::write(&partial, &bytes[..90_000]).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (identity, store) = (Arc::clone(&bob.identity), Arc::clone(&bob.store));
    let worker = thread::spawn(move || accept_one(&identity, &store, listener.accept().unwrap().0).unwrap());
    send_one(&alice.identity, &alice.store, &bob_id, address, &message.id).unwrap();
    worker.join().unwrap();
    let (_, stored) = bob.store.lock().unwrap().attachment_path(&message.id).unwrap().unwrap();
    assert_eq!(fs::read(&stored).unwrap(), bytes, "resumed bytes must match the source");
    assert_eq!(bob.store.lock().unwrap().file_transfer(&message.id).unwrap().unwrap().transferred, bytes.len() as i64);
    crate::files::remove_quietly(&path);
    crate::files::remove_quietly(&stored);
}

#[test]
fn untrusted_peer_rejects_file_offer_without_storing_it() {
    let alice = side("alice-untrusted-file");
    let bob = side("bob-untrusted-file");
    trust(&alice, &bob);
    let bob_id = bob.identity.fingerprint();
    let path = file_fixture("untrusted", b"secret payload");
    queue_file(&alice, &bob, &path, "secret.txt");
    let message = alice.store.lock().unwrap().list_direct(&bob_id, 1, None).unwrap()[0].clone();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (identity, store) = (Arc::clone(&bob.identity), Arc::clone(&bob.store));
    let worker = thread::spawn(move || accept_one(&identity, &store, listener.accept().unwrap().0).is_err());
    assert!(send_one(&alice.identity, &alice.store, &bob_id, address, &message.id).is_err());
    assert!(worker.join().unwrap());
    assert!(bob.store.lock().unwrap().attachment_path(&message.id).unwrap().is_none());
    assert_ne!(alice.store.lock().unwrap().file_transfer(&message.id).unwrap().unwrap().status, "completed");
    crate::files::remove_quietly(&path);
}

#[test]
fn modified_source_fails_integrity_check_and_stays_unconfirmed() {
    let alice = side("alice-integrity");
    let bob = side("bob-integrity");
    trust(&alice, &bob);
    trust(&bob, &alice);
    let bob_id = bob.identity.fingerprint();
    let original = vec![7u8; 50_000];
    let path = file_fixture("integrity", &original);
    queue_file(&alice, &bob, &path, "report.pdf");
    let message = alice.store.lock().unwrap().list_direct(&bob_id, 1, None).unwrap()[0].clone();
    // Same size, different bytes: the receiver must reject the checksum.
    fs::write(&path, vec![9u8; 50_000]).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (identity, store) = (Arc::clone(&bob.identity), Arc::clone(&bob.store));
    let worker = thread::spawn(move || accept_one(&identity, &store, listener.accept().unwrap().0).is_err());
    assert!(send_one(&alice.identity, &alice.store, &bob_id, address, &message.id).is_err());
    assert!(worker.join().unwrap());
    assert!(bob.store.lock().unwrap().attachment_path(&message.id).unwrap().is_none());
    assert_eq!(bob.store.lock().unwrap().file_transfer(&message.id).unwrap().unwrap().status, "failed");
    assert!(alice.store.lock().unwrap().file_transfer(&message.id).unwrap().unwrap().status != "failed");
    crate::files::remove_quietly(&path);
}

#[test]
fn group_message_reaches_member_and_clears_that_members_outbox_row() {
    let alice = side("alice-group-text");
    let bob = side("bob-group-text");
    trust(&alice, &bob);
    trust(&bob, &alice);
    let alice_id = alice.identity.fingerprint();
    let bob_id = bob.identity.fingerprint();
    let group = alice.store.lock().unwrap().create_group(&alice_id, "Team", &[bob_id.clone()], 1).unwrap();
    let message = alice.store.lock().unwrap().queue_group(&alice_id, &group.id, "hello group", None, &[], 2).unwrap();
    assert_eq!(message.pending, 1);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (identity, store) = (Arc::clone(&bob.identity), Arc::clone(&bob.store));
    let worker = thread::spawn(move || accept_one(&identity, &store, listener.accept().unwrap().0).unwrap());
    send_group_one(&alice.identity, &alice.store, &bob_id, address, &message.id).unwrap();
    worker.join().unwrap();
    let received = bob.store.lock().unwrap().list_group_messages(&group.id, 10, None).unwrap();
    assert_eq!(received[0].body, "hello group");
    assert_eq!(received[0].sender, alice_id);
    assert_eq!(bob.store.lock().unwrap().list_group_members(&group.id).unwrap().iter().filter(|m| m.active).count(), 2);
    assert_eq!(alice.store.lock().unwrap().list_group_messages(&group.id, 10, None).unwrap()[0].pending, 0);
    assert!(alice.store.lock().unwrap().next_pending_group_delivery(None).unwrap().is_none());
}

#[test]
fn group_invite_creates_the_group_for_the_invited_member() {
    let alice = side("alice-group-invite");
    let bob = side("bob-group-invite");
    trust(&alice, &bob);
    trust(&bob, &alice);
    bob.store.lock().unwrap().set_auto_join_groups(true).unwrap();
    let alice_id = alice.identity.fingerprint();
    let bob_id = bob.identity.fingerprint();
    let group = alice.store.lock().unwrap().create_group(&alice_id, "Team", &[bob_id.clone()], 1).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    alice.store.lock().unwrap().set_contact_address(&bob_id, &address.to_string()).unwrap();
    let task = alice.store.lock().unwrap().next_pending_control(None).unwrap().unwrap();
    assert_eq!(task.kind, "invite");
    let (identity, store) = (Arc::clone(&bob.identity), Arc::clone(&bob.store));
    let worker = thread::spawn(move || accept_one(&identity, &store, listener.accept().unwrap().0).unwrap());
    send_control_one(&alice.identity, &alice.store, &bob_id, address, &task).unwrap();
    worker.join().unwrap();
    let groups = bob.store.lock().unwrap().list_groups(&bob_id).unwrap();
    assert!(groups.iter().any(|item| item.id == group.id && item.name == "Team"));
    assert!(alice.store.lock().unwrap().next_pending_control(None).unwrap().is_none());
}
