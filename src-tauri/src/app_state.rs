use std::{fs, io, path::Path, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}};
use crate::{discovery::Discovery, identity::Identity, probe_service::{ProbeRequest, ProbeService}, storage::Store};

type InitResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub struct AppState {
    pub identity: Arc<Identity>,
    pub store: Mutex<Store>,
    discovery: Mutex<Option<Discovery>>,
    probe_service: Mutex<Option<ProbeService>>,
    outbound_probe: Arc<AtomicBool>,
    last_outbound_fingerprint: Arc<Mutex<Option<String>>>,
}

#[derive(serde::Serialize)]
pub struct ProbeStatus {
    pub running: bool,
    pub port: Option<u16>,
    pub requests: Vec<ProbeRequest>,
}

#[tauri::command]
pub fn v1_start_probe_service(state: tauri::State<'_, AppState>, port: u16) -> Result<(), String> {
    if port == 0 { return Err("端口须为 1–65535".into()); }
    let mut slot = state.probe_service.lock().map_err(|_| "探测服务不可用".to_owned())?;
    if slot.is_some() { return Err("探测服务已启动，请先停止再更改端口".into()); }
    *slot = Some(ProbeService::start(Arc::clone(&state.identity), port).map_err(|_| "无法启动探测服务：端口可能被占用，或网络发现不可用".to_owned())?);
    Ok(())
}

#[tauri::command]
pub fn v1_stop_probe_service(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let service = state.probe_service.lock().map_err(|_| "探测服务不可用".to_owned())?.take();
    drop(service);
    Ok(())
}

#[tauri::command]
pub fn v1_probe_status(state: tauri::State<'_, AppState>) -> Result<ProbeStatus, String> {
    let slot = state.probe_service.lock().map_err(|_| "探测服务不可用".to_owned())?;
    match slot.as_ref() {
        Some(service) => Ok(ProbeStatus { running: service.is_running(), port: Some(service.port()), requests: service.requests().map_err(str::to_owned)? }),
        None => Ok(ProbeStatus { running: false, port: None, requests: vec![] }),
    }
}

struct ProbeGuard(Arc<AtomicBool>);
impl Drop for ProbeGuard { fn drop(&mut self) { self.0.store(false, Ordering::Release); } }

#[tauri::command]
pub async fn v1_probe_peer(state: tauri::State<'_, AppState>, address: String) -> Result<String, String> {
    let address = crate::address::parse_manual_address(&address).map_err(str::to_owned)?;
    if state.outbound_probe.swap(true, Ordering::AcqRel) { return Err("已有身份探测正在进行".into()); }
    let guard = ProbeGuard(Arc::clone(&state.outbound_probe));
    *state.last_outbound_fingerprint.lock().map_err(|_| "身份探测不可用".to_owned())? = None;
    let identity = Arc::clone(&state.identity);
    let last = Arc::clone(&state.last_outbound_fingerprint);
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let fingerprint = crate::probe::connect(&identity, address).map_err(|_| "探测失败：对方可能离线、端口错误或网络受阻".to_owned())?;
        *last.lock().map_err(|_| "身份探测不可用".to_owned())? = Some(fingerprint.clone());
        Ok(fingerprint)
    }).await.map_err(|_| "身份探测不可用".to_owned())?
}

impl AppState {
    fn confirm_observed(&self, fingerprint: &str, checked_fingerprint: &str, display_name: &str) -> Result<(), String> {
        if fingerprint != checked_fingerprint || fingerprint == self.identity.fingerprint() {
            return Err("指纹不一致或不能添加本机身份".into());
        }
        let name = display_name.trim();
        if name.is_empty() || name.len() > 128 { return Err("联系人名称须为 1–128 字节".into()); }
        let outbound = self.last_outbound_fingerprint.lock().map_err(|_| "身份探测不可用".to_owned())?;
        let service = self.probe_service.lock().map_err(|_| "探测服务不可用".to_owned())?;
        let inbound = match service.as_ref() {
            Some(service) => service.requests().map_err(str::to_owned)?.iter().any(|request| request.fingerprint == fingerprint),
            None => false,
        };
        if outbound.as_deref() != Some(fingerprint) && !inbound {
            return Err("探测记录不存在，请重新探测".into());
        }
        let store = self.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
        store.add_candidate(fingerprint, name).map_err(|_| "无法保存联系人".to_owned())?;
        if !store.is_verified(fingerprint).map_err(|_| "无法读取联系人".to_owned())? {
            if !store.confirm_candidate(fingerprint).map_err(|_| "无法确认联系人".to_owned())? {
                return Err("无法确认联系人".into());
            }
        }
        Ok(())
    }
}

#[tauri::command]
pub fn v1_confirm_contact(state: tauri::State<'_, AppState>, fingerprint: String, checked_fingerprint: String, display_name: String) -> Result<(), String> {
    state.confirm_observed(&fingerprint, &checked_fingerprint, &display_name)
}

#[tauri::command]
pub fn v1_discovery_snapshot(state: tauri::State<'_, AppState>) -> Result<Vec<crate::discovery::Candidate>, String> {
    let mut discovery = state.discovery.lock().map_err(|_| "局域网发现不可用".to_owned())?;
    if discovery.is_none() {
        *discovery = Some(Discovery::browse().map_err(|_| "无法启动局域网发现，请检查网络和防火墙".to_owned())?);
    }
    discovery.as_ref().expect("initialized above").candidates().map_err(str::to_owned)
}

impl AppState {
    pub fn initialize(dir: &Path) -> InitResult<Self> {
        fs::create_dir_all(dir)?;
        let identity_path = dir.join("identity.bin");
        let database_path = dir.join("qbird.sqlite3");
        // Losing an existing identity is not permission to silently create a new one.
        if database_path.exists() && !identity_path.exists() {
            return Err(io::Error::new(io::ErrorKind::NotFound, "identity file is missing while the database exists; restore the identity before starting").into());
        }
        let identity = Arc::new(Identity::load_or_create(&identity_path)?);
        let store = Store::open(&database_path)?;
        Ok(Self { identity, store: Mutex::new(store), discovery: Mutex::new(None), probe_service: Mutex::new(None), outbound_probe: Arc::new(AtomicBool::new(false)), last_outbound_fingerprint: Arc::new(Mutex::new(None)) })
    }
}

#[tauri::command]
pub fn identity_fingerprint(state: tauri::State<'_, AppState>) -> String {
    state.identity.fingerprint()
}

#[tauri::command]
pub fn v1_list_contacts(state: tauri::State<'_, AppState>) -> Result<Vec<crate::storage::Contact>, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    store.list_contacts().map_err(|_| "无法读取联系人".to_owned())
}

#[tauri::command]
pub fn v1_list_messages(state: tauri::State<'_, AppState>, peer: String, limit: u32, offset: u32) -> Result<Vec<crate::storage::Message>, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    store.list_messages(&peer, limit, offset).map_err(|_| "无法读取消息或参数无效".to_owned())
}

#[tauri::command]
pub fn v1_open_self(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?.open_self_conversation().map_err(|_| "无法打开自聊".to_owned())
}

#[tauri::command]
pub fn v1_hide_self(state: tauri::State<'_, AppState>) -> Result<bool, String> {
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?.hide_self_conversation().map_err(|_| "无法隐藏会话".to_owned())
}

#[tauri::command]
pub fn v1_list_conversations(state: tauri::State<'_, AppState>) -> Result<Vec<crate::storage::Conversation>, String> {
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?.list_conversations().map_err(|_| "无法读取会话".to_owned())
}

#[tauri::command]
pub fn v1_list_self_messages(state: tauri::State<'_, AppState>, limit: u32, offset: u32) -> Result<Vec<crate::storage::LocalMessage>, String> {
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?.list_self_messages(limit, offset).map_err(|_| "无法读取自聊记录".to_owned())
}

#[tauri::command]
pub fn v1_send_self(state: tauri::State<'_, AppState>, body: String) -> Result<crate::storage::LocalMessage, String> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_| "系统时间无效".to_owned())?.as_secs();
    let created_at = i64::try_from(now).map_err(|_| "系统时间无效".to_owned())?;
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?.send_self(&state.identity.fingerprint(), &body, created_at)
        .map_err(|_| "消息为空、过长或无法保存".to_owned())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_dir(label: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("state-tests")
            .join(format!("{label}-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()))
    }

    #[test]
    fn first_start_creates_identity_and_db_then_reloads_same_identity() {
        let dir = test_dir("first");
        let fingerprint = AppState::initialize(&dir).unwrap().identity.fingerprint();
        assert!(dir.join("identity.bin").exists());
        assert!(dir.join("qbird.sqlite3").exists());
        assert_eq!(AppState::initialize(&dir).unwrap().identity.fingerprint(), fingerprint);
    }

    #[test]
    fn missing_identity_with_existing_database_does_not_create_new_identity() {
        let dir = test_dir("missing");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("qbird.sqlite3"), b"existing database").unwrap();
        assert!(AppState::initialize(&dir).is_err());
        assert!(!dir.join("identity.bin").exists());
        assert_eq!(fs::read(dir.join("qbird.sqlite3")).unwrap(), b"existing database");
    }

    #[test]
    fn confirmation_requires_observed_matching_peer_and_survives_restart() {
        let dir = test_dir("confirm");
        let state = AppState::initialize(&dir).unwrap();
        let peer = "a".repeat(64);
        assert!(state.confirm_observed(&peer, &peer, "Alice").is_err());
        *state.last_outbound_fingerprint.lock().unwrap() = Some(peer.clone());
        assert!(state.confirm_observed(&peer, &"b".repeat(64), "Alice").is_err());
        assert!(state.confirm_observed(&peer, &peer, " ").is_err());
        let own = state.identity.fingerprint();
        *state.last_outbound_fingerprint.lock().unwrap() = Some(own.clone());
        assert!(state.confirm_observed(&own, &own, "Me").is_err());
        *state.last_outbound_fingerprint.lock().unwrap() = Some(peer.clone());
        assert!(state.confirm_observed(&peer, &peer, "Alice").is_ok());
        assert!(state.confirm_observed(&peer, &peer, "Different name").is_ok());
        drop(state);
        let reopened = AppState::initialize(&dir).unwrap();
        let store = reopened.store.lock().unwrap();
        assert!(store.is_verified(&peer).unwrap());
        assert_eq!(store.list_contacts().unwrap()[0].display_name, "Alice");
    }

    #[test]
    fn probe_requires_separate_confirmation_on_both_sides() {
        use std::{thread, time::{Duration, Instant}};
        let alice = AppState::initialize(&test_dir("alice")).unwrap();
        let bob = AppState::initialize(&test_dir("bob")).unwrap();
        let service = ProbeService::start(Arc::clone(&bob.identity), 0).unwrap();
        let port = service.port();
        *bob.probe_service.lock().unwrap() = Some(service);
        let bob_key = crate::probe::connect(&alice.identity, format!("127.0.0.1:{port}").parse().unwrap()).unwrap();
        assert_eq!(bob_key, bob.identity.fingerprint());
        *alice.last_outbound_fingerprint.lock().unwrap() = Some(bob_key.clone());
        let alice_key = alice.identity.fingerprint();
        let deadline = Instant::now() + Duration::from_secs(3);
        while bob.probe_service.lock().unwrap().as_ref().unwrap().requests().unwrap().is_empty() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        assert!(!alice.store.lock().unwrap().is_verified(&bob_key).unwrap());
        assert!(!bob.store.lock().unwrap().is_verified(&alice_key).unwrap());
        alice.confirm_observed(&bob_key, &bob_key, "Bob").unwrap();
        assert!(!bob.store.lock().unwrap().is_verified(&alice_key).unwrap());
        bob.confirm_observed(&alice_key, &alice_key, "Alice").unwrap();
        assert!(alice.store.lock().unwrap().is_verified(&bob_key).unwrap());
        assert!(bob.store.lock().unwrap().is_verified(&alice_key).unwrap());
        drop(bob.probe_service.lock().unwrap().take());
        assert!(bob.confirm_observed(&alice_key, &alice_key, "Alice").is_err());
    }
}
