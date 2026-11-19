use std::{fs, io, path::Path, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}};
use tauri::Manager;
use crate::{discovery::Discovery, identity::Identity, probe_service::{ProbeRequest, ProbeService}, storage::Store};

type InitResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub struct AppState {
    pub identity: Arc<Identity>,
    pub store: Arc<Mutex<Store>>,
    discovery: Mutex<Option<Discovery>>,
    probe_service: Mutex<Option<ProbeService>>,
    retry_queue: Mutex<Option<crate::retry_queue::RetryQueue>>,
    outbound_probe: Arc<AtomicBool>,
    outbound_chat: Arc<AtomicBool>,
    last_outbound_fingerprint: Arc<Mutex<Option<String>>>,
}

#[tauri::command]
pub fn v1_set_contact_address(state: tauri::State<'_, AppState>, peer: String, address: String) -> Result<(), String> {
    crate::address::parse_manual_address(&address).map_err(str::to_owned)?;
    let saved = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .set_contact_address(&peer, &address).map_err(|_| "无法保存联系人地址".to_owned())?;
    if saved { Ok(()) } else { Err("联系人尚未确认".into()) }
}

#[tauri::command]
pub fn v1_contact_address(state: tauri::State<'_, AppState>, peer: String) -> Result<Option<String>, String> {
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .contact_address(&peer).map_err(|_| "无法读取联系人地址".to_owned())
}

#[tauri::command]
pub async fn v1_deliver_direct(state: tauri::State<'_, AppState>, peer: String, address: String, id: String) -> Result<(), String> {
    let address = crate::address::parse_manual_address(&address).map_err(str::to_owned)?;
    if state.outbound_chat.swap(true, Ordering::AcqRel) { return Err("已有消息正在发送，请稍后重试".into()); }
    let guard = ProbeGuard(Arc::clone(&state.outbound_chat));
    let identity = Arc::clone(&state.identity);
    let store = Arc::clone(&state.store);
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        crate::chat_transport::send_one(&identity, &store, &peer, address, &id)
            .map_err(|_| "发送未确认：对方可能离线、地址错误或身份不匹配；消息仍保存在本机，可重试".to_owned())
    }).await.map_err(|_| "发送任务异常；消息仍保存在本机，可重试".to_owned())?
}

#[tauri::command]
pub fn v1_open_direct(state: tauri::State<'_, AppState>, peer: String) -> Result<(), String> {
    let opened = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?.open_direct(&peer).map_err(|_| "无法打开会话".to_owned())?;
    if opened { Ok(()) } else { Err("联系人尚未确认".into()) }
}

#[tauri::command]
pub fn v1_hide_direct(state: tauri::State<'_, AppState>, peer: String) -> Result<bool, String> {
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?.hide_direct(&peer).map_err(|_| "无法隐藏会话".to_owned())
}

#[tauri::command]
pub fn v1_list_direct(state: tauri::State<'_, AppState>, peer: String, limit: u32, before: Option<crate::storage::DirectCursor>) -> Result<Vec<crate::storage::DirectMessage>, String> {
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?.list_direct(&peer, limit, before.as_ref()).map_err(|_| "无法读取单聊记录".to_owned())
}

#[tauri::command]
pub fn v1_queue_direct(state: tauri::State<'_, AppState>, peer: String, body: String) -> Result<crate::storage::DirectMessage, String> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_| "系统时间无效".to_owned())?.as_secs();
    let created_at = i64::try_from(now).map_err(|_| "系统时间无效".to_owned())?;
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?.queue_direct(&state.identity.fingerprint(), &peer, &body, created_at)
        .map_err(|_| "联系人未确认，或消息为空、过长或无法保存".to_owned())
}

/// Queue a file picked by the user. Hashing happens off the UI thread; the
/// source file is referenced in place and re-verified on every attempt.
#[tauri::command]
pub async fn v1_queue_direct_file(state: tauri::State<'_, AppState>, peer: String, path: String) -> Result<crate::storage::DirectMessage, String> {
    let store = Arc::clone(&state.store);
    let sender = state.identity.fingerprint();
    tauri::async_runtime::spawn_blocking(move || {
        if path.len() > 4096 { return Err("文件路径无效".to_owned()); }
        let source = std::path::PathBuf::from(&path);
        let name = source.file_name().and_then(|name| name.to_str()).ok_or_else(|| "无法读取文件名".to_owned())?.to_owned();
        let size = crate::files::file_size(&source).map_err(|_| "无法读取文件：请确认它是本地普通文件".to_owned())?;
        if size == 0 { return Err("暂不支持空文件".to_owned()); }
        if size > crate::protocol::MAX_FILE_BYTES { return Err("文件超过 512 MiB 限制".to_owned()); }
        let sha256 = crate::files::sha256_file(&source).map_err(|_| "无法计算文件校验值".to_owned())?;
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_| "系统时间无效".to_owned())?.as_secs();
        let created_at = i64::try_from(now).map_err(|_| "系统时间无效".to_owned())?;
        store.lock().map_err(|_| "本地存储不可用".to_owned())?
            .queue_direct_file(&sender, &peer, &name, size, &sha256, &source, created_at)
            .map_err(|_| "联系人未确认，或文件名、大小无效".to_owned())
    }).await.map_err(|_| "文件入队任务异常".to_owned())?
}

#[tauri::command]
pub fn v1_file_progress(state: tauri::State<'_, AppState>, peer: String) -> Result<Vec<crate::storage::FileProgress>, String> {
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?.list_file_progress(&peer)
        .map_err(|_| "无法读取文件传输状态".to_owned())
}

#[derive(serde::Serialize)]
pub struct AttachmentPreview {
    pub name: String,
    pub data_url: String,
}

/// Only completed image attachments up to the preview limit are returned; the
/// bytes are bounded so the UI never receives an unbounded payload.
#[tauri::command]
pub fn v1_read_attachment(state: tauri::State<'_, AppState>, message_id: String) -> Result<Option<AttachmentPreview>, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    let Some((name, path)) = store.attachment_path(&message_id).map_err(|_| "无法读取附件".to_owned())? else { return Ok(None); };
    if !crate::files::is_image(&name) { return Ok(None); }
    let size = crate::files::file_size(&path).map_err(|_| "无法读取附件".to_owned())?;
    if size == 0 || size > crate::files::MAX_PREVIEW_BYTES { return Ok(None); }
    let data = std::fs::read(&path).map_err(|_| "无法读取附件".to_owned())?;
    let extension = std::path::Path::new(&name).extension().and_then(|ext| ext.to_str()).unwrap_or("").to_ascii_lowercase();
    let mime = match extension.as_str() {
        "png" => "image/png", "jpg" | "jpeg" => "image/jpeg", "gif" => "image/gif",
        "webp" => "image/webp", "bmp" => "image/bmp", _ => return Ok(None),
    };
    Ok(Some(AttachmentPreview { name: name.to_string(), data_url: format!("data:{mime};base64,{}", crate::files::base64_encode(&data)) }))
}

#[derive(serde::Serialize)]
pub struct ProbeStatus {
    pub running: bool,
    pub port: Option<u16>,
    pub requests: Vec<ProbeRequest>,
}

#[tauri::command]
pub async fn v1_start_probe_service(app: tauri::AppHandle, port: u16) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<AppState>().start_listener(port, false))
        .await.map_err(|_| "探测服务任务异常".to_owned())?
}

#[tauri::command]
pub async fn v1_start_chat_service(app: tauri::AppHandle, port: u16) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<AppState>().start_listener(port, true))
        .await.map_err(|_| "聊天服务任务异常".to_owned())?
}

impl AppState {
    fn start_listener(&self, port: u16, chat: bool) -> Result<(), String> {
        if port == 0 { return Err("端口须为 1–65535".into()); }
        let mut slot = self.probe_service.lock().map_err(|_| "探测服务不可用".to_owned())?;
        if slot.is_some() { return Err("探测服务已启动，请先停止再更改端口".into()); }
        let service = if chat {
            ProbeService::start_with_chat(Arc::clone(&self.identity), port, Arc::clone(&self.store))
        } else {
            ProbeService::start(Arc::clone(&self.identity), port)
        };
        let service = service.map_err(|_| "无法启动服务：端口可能被占用，或网络发现不可用".to_owned())?;
        let mut retry = self.retry_queue.lock().map_err(|_| "消息重试服务不可用".to_owned())?;
        if chat {
            *retry = Some(crate::retry_queue::RetryQueue::start(Arc::clone(&self.identity), Arc::clone(&self.store), Arc::clone(&self.outbound_chat))
                .map_err(|_| "无法启动消息重试服务".to_owned())?);
        }
        *slot = Some(service);
        Ok(())
    }

    fn stop_listener(&self) -> Result<(), String> {
        // Hold the lifecycle lock until both workers have stopped, so a concurrent
        // start cannot overlap with an old listener or retry worker.
        let mut service = self.probe_service.lock().map_err(|_| "探测服务不可用".to_owned())?;
        let mut retry = self.retry_queue.lock().map_err(|_| "消息重试服务不可用".to_owned())?;
        drop(retry.take());
        drop(service.take());
        Ok(())
    }
}

#[tauri::command]
pub async fn v1_stop_probe_service(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<AppState>().stop_listener())
        .await.map_err(|_| "停止服务任务异常".to_owned())?
}

#[tauri::command]
pub async fn v1_probe_status(app: tauri::AppHandle) -> Result<ProbeStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let slot = state.probe_service.lock().map_err(|_| "探测服务不可用".to_owned())?;
        match slot.as_ref() {
            Some(service) => Ok(ProbeStatus { running: service.is_running(), port: Some(service.port()), requests: service.requests().map_err(str::to_owned)? }),
            None => Ok(ProbeStatus { running: false, port: None, requests: vec![] }),
        }
    }).await.map_err(|_| "读取服务状态任务异常".to_owned())?
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
        Ok(Self { identity, store: Arc::new(Mutex::new(store)), discovery: Mutex::new(None), probe_service: Mutex::new(None), retry_queue: Mutex::new(None), outbound_probe: Arc::new(AtomicBool::new(false)), outbound_chat: Arc::new(AtomicBool::new(false)), last_outbound_fingerprint: Arc::new(Mutex::new(None)) })
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
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?.list_conversations(&state.identity.fingerprint()).map_err(|_| "无法读取会话".to_owned())
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

fn unix_now() -> Result<i64, String> {
    let seconds = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "系统时间无效".to_owned())?.as_secs();
    i64::try_from(seconds).map_err(|_| "系统时间无效".to_owned())
}

#[tauri::command]
pub fn v1_create_group(state: tauri::State<'_, AppState>, name: String, members: Vec<String>) -> Result<crate::storage::GroupSummary, String> {
    let owner = state.identity.fingerprint();
    let now = unix_now()?;
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .create_group(&owner, &name, &members, now)
        .map_err(|_| "群名无效、成员尚未确认或数量超出限制".to_owned())
}

#[tauri::command]
pub fn v1_list_groups(state: tauri::State<'_, AppState>) -> Result<Vec<crate::storage::GroupSummary>, String> {
    let self_id = state.identity.fingerprint();
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .list_groups(&self_id).map_err(|_| "无法读取群聊".to_owned())
}

#[tauri::command]
pub fn v1_list_group_members(state: tauri::State<'_, AppState>, group_id: String) -> Result<Vec<crate::storage::GroupMember>, String> {
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .list_group_members(&group_id).map_err(|_| "无法读取群成员".to_owned())
}

#[tauri::command]
pub fn v1_open_group(state: tauri::State<'_, AppState>, group_id: String) -> Result<(), String> {
    let self_id = state.identity.fingerprint();
    let opened = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .open_group(&group_id, &self_id).map_err(|_| "无法打开群聊".to_owned())?;
    if opened { Ok(()) } else { Err("已不在该群中".into()) }
}

#[tauri::command]
pub fn v1_hide_group(state: tauri::State<'_, AppState>, group_id: String) -> Result<bool, String> {
    let self_id = state.identity.fingerprint();
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .hide_group(&group_id, &self_id).map_err(|_| "无法隐藏会话".to_owned())
}

#[tauri::command]
pub fn v1_queue_group(state: tauri::State<'_, AppState>, group_id: String, body: String) -> Result<crate::storage::GroupMessage, String> {
    let sender = state.identity.fingerprint();
    let created_at = unix_now()?;
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .queue_group(&sender, &group_id, &body, created_at)
        .map_err(|_| "消息为空、过长或已不在该群中".to_owned())
}

#[tauri::command]
pub fn v1_list_group(state: tauri::State<'_, AppState>, group_id: String, limit: u32, before: Option<crate::storage::DirectCursor>) -> Result<Vec<crate::storage::GroupMessage>, String> {
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .list_group_messages(&group_id, limit, before.as_ref()).map_err(|_| "无法读取群聊记录".to_owned())
}

#[tauri::command]
pub fn v1_add_group_member(state: tauri::State<'_, AppState>, group_id: String, member: String) -> Result<(), String> {
    let actor = state.identity.fingerprint();
    let now = unix_now()?;
    let added = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .add_group_member(&group_id, &actor, &member, now).map_err(|_| "无法添加成员".to_owned())?;
    if added { Ok(()) } else { Err("只有群主可以添加，且该联系人须已确认、尚未在群中".into()) }
}

#[tauri::command]
pub fn v1_remove_group_member(state: tauri::State<'_, AppState>, group_id: String, member: String) -> Result<(), String> {
    let actor = state.identity.fingerprint();
    let now = unix_now()?;
    let removed = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .remove_group_member(&group_id, &actor, &member, now).map_err(|_| "无法移除成员".to_owned())?;
    if removed { Ok(()) } else { Err("只有群主可以移除群内成员".into()) }
}

#[tauri::command]
pub fn v1_leave_group(state: tauri::State<'_, AppState>, group_id: String) -> Result<(), String> {
    let self_id = state.identity.fingerprint();
    let now = unix_now()?;
    let left = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .leave_group(&group_id, &self_id, now).map_err(|_| "无法退出群聊".to_owned())?;
    if left { Ok(()) } else { Err("群主不能退出群聊".into()) }
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
    fn failed_chat_start_does_not_leave_retry_running() {
        let state = AppState::initialize(&test_dir("retry-start-failure")).unwrap();
        let reservation = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
        let port = reservation.local_addr().unwrap().port();
        assert!(state.start_listener(port, true).is_err());
        assert!(state.probe_service.lock().unwrap().is_none());
        assert!(state.retry_queue.lock().unwrap().is_none());
        drop(reservation);
        state.start_listener(port, true).unwrap();
        state.stop_listener().unwrap();
    }

    #[test]
    fn retry_follows_chat_listener_lifecycle() {
        use std::{net::TcpListener, thread, time::{Duration, Instant}};
        let alice = AppState::initialize(&test_dir("retry-lifecycle-alice")).unwrap();
        let bob = AppState::initialize(&test_dir("retry-lifecycle-bob")).unwrap();
        let alice_id = alice.identity.fingerprint();
        let bob_id = bob.identity.fingerprint();
        for (state, peer) in [(&alice, &bob_id), (&bob, &alice_id)] {
            let store = state.store.lock().unwrap();
            store.add_candidate(peer, "contact").unwrap();
            store.confirm_candidate(peer).unwrap();
        }
        let reservation_a = TcpListener::bind("127.0.0.1:0").unwrap();
        let reservation_b = TcpListener::bind("127.0.0.1:0").unwrap();
        let port_a = reservation_a.local_addr().unwrap().port();
        let port_b = reservation_b.local_addr().unwrap().port();
        drop((reservation_a, reservation_b));
        bob.start_listener(port_b, true).unwrap();
        let first = {
            let store = alice.store.lock().unwrap();
            store.set_contact_address(&bob_id, &format!("127.0.0.1:{port_b}")).unwrap();
            store.queue_direct(&alice_id, &bob_id, "first", 1).unwrap()
        };
        alice.start_listener(port_a, false).unwrap();
        thread::sleep(Duration::from_millis(700));
        assert!(alice.store.lock().unwrap().pending_direct_by_id(&bob_id, &first.id).unwrap().is_some());
        alice.stop_listener().unwrap();
        let mut first = Some(first);
        for round in 0..2 {
            let message = if round == 0 { first.take().unwrap() } else {
                // Queue only after a previously active retry worker has stopped.
                let message = alice.store.lock().unwrap().queue_direct(&alice_id, &bob_id, "second", 2).unwrap();
                thread::sleep(Duration::from_millis(700));
                assert!(alice.store.lock().unwrap().pending_direct_by_id(&bob_id, &message.id).unwrap().is_some());
                message
            };
            alice.start_listener(port_a, true).unwrap();
            let deadline = Instant::now() + Duration::from_secs(8);
            while alice.store.lock().unwrap().pending_direct_by_id(&bob_id, &message.id).unwrap().is_some() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(50));
            }
            assert!(alice.store.lock().unwrap().pending_direct_by_id(&bob_id, &message.id).unwrap().is_none());
            alice.stop_listener().unwrap();
            assert!(alice.retry_queue.lock().unwrap().is_none());
        }
        assert_eq!(bob.store.lock().unwrap().list_direct(&alice_id, 10, None).unwrap().len(), 2);
        bob.stop_listener().unwrap();
    }

    #[test]
    fn explicitly_started_chat_listener_receives_trusted_message() {
        use std::net::{TcpListener, TcpStream};
        let alice = AppState::initialize(&test_dir("chat-alice")).unwrap();
        let bob = AppState::initialize(&test_dir("chat-bob")).unwrap();
        let alice_id = alice.identity.fingerprint();
        let bob_id = bob.identity.fingerprint();
        for (state, peer) in [(&alice, &bob_id), (&bob, &alice_id)] {
            let store = state.store.lock().unwrap();
            store.add_candidate(peer, "contact").unwrap();
            store.confirm_candidate(peer).unwrap();
        }
        assert!(bob.start_listener(0, true).is_err());
        let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = reservation.local_addr().unwrap().port();
        drop(reservation);
        bob.start_listener(port, true).unwrap();
        assert!(bob.start_listener(port, true).is_err());
        let message = alice.store.lock().unwrap().queue_direct(&alice_id, &bob_id, "app listener", 1).unwrap();
        let address = format!("127.0.0.1:{port}").parse().unwrap();
        crate::chat_transport::send_one(&alice.identity, &alice.store, &bob_id, address, &message.id).unwrap();
        assert_eq!(bob.store.lock().unwrap().list_direct(&alice_id, 10, None).unwrap()[0].body, "app listener");
        bob.stop_listener().unwrap();
        assert!(TcpStream::connect_timeout(&address, std::time::Duration::from_secs(1)).is_err());
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
