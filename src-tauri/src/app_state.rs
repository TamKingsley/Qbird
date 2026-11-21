use std::{fs, io, net::SocketAddr, path::{Path, PathBuf}, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, time::Duration};
use tauri::{Emitter, Manager};
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
    /// Application lock state; set at startup when a lock password exists.
    locked: AtomicBool,
    /// Data directory for local-only assets such as the profile avatar.
    data_dir: Arc<PathBuf>,
    /// Pending full-screen capture shown by the capture overlay window.
    capture: Mutex<Option<crate::capture::ScreenCapture>>,
    /// Whether the running listener also serves chat, not probes only. Kept beside
    /// `probe_service` because `ProbeService` itself does not expose the mode.
    chat_listener: AtomicBool,
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

/// Attempts the saved address right away for every message still pending, so the
/// user gets feedback on the spot instead of waiting for the retry worker.
#[tauri::command]
pub async fn v1_flush_direct(state: tauri::State<'_, AppState>, peer: String) -> Result<u32, String> {
    let (address, pending) = {
        let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
        let address = store.contact_address(&peer).map_err(|_| "无法读取联系人地址".to_owned())?
            .ok_or_else(|| "尚未保存对方地址无法发送".to_owned())?;
        let pending = store.pending_direct(&peer, 20).map_err(|_| "无法读取待发送消息".to_owned())?;
        (address, pending)
    };
    if pending.is_empty() { return Ok(0); }
    let address = crate::address::parse_manual_address(&address).map_err(str::to_owned)?;
    if state.outbound_chat.swap(true, Ordering::AcqRel) { return Err("已有消息正在发送，请稍后重试".into()); }
    let guard = ProbeGuard(Arc::clone(&state.outbound_chat));
    let identity = Arc::clone(&state.identity);
    let store = Arc::clone(&state.store);
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let mut sent = 0u32;
        for message in pending {
            if crate::chat_transport::send_one(&identity, &store, &peer, address, &message.id).is_err() { break; }
            sent += 1;
        }
        if sent == 0 {
            return Err("发送未确认：对方可能离线、地址错误或未启动聊天服务；消息仍保存在本机，可稍后重试".to_owned());
        }
        Ok(sent)
    }).await.map_err(|_| "发送任务异常；消息仍保存在本机，可重试".to_owned())?
}

/// Tells a contact we added it, so a single add can become mutual. Best effort: the peer
/// may be offline, may have automatic acceptance off, or may already know us.
fn notify_contact_async(identity: Arc<Identity>, store: Arc<Mutex<Store>>, peer: &str, address: Option<String>) {
    let address = address.or_else(|| store.lock().ok().and_then(|store| store.contact_address(peer).ok().flatten()));
    let Some(parsed) = address.and_then(|value| crate::address::parse_manual_address(&value).ok()) else { return };
    let announce = store.lock().ok().and_then(|store| store.get_profile().ok())
        .map(|profile| crate::probe::ContactAnnounce {
            nickname: profile.nickname,
            username: Some(profile.username).filter(|value| !value.is_empty()),
            kind: None,
            address: None,
            reason: None,
        });
    if let Some(announce) = announce {
        std::thread::spawn(move || { let _ = crate::probe::announce_contact(&identity, parsed, &announce); });
    }
}

#[tauri::command]
pub fn v1_open_direct(state: tauri::State<'_, AppState>, peer: String) -> Result<(), String> {
    let opened = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?.open_direct(&peer).map_err(|_| "无法打开会话".to_owned())?;
    if !opened { return Err("联系人尚未确认".into()); }
    // Retry the mutual-add notice when the chat is opened; it is idempotent.
    notify_contact_async(Arc::clone(&state.identity), Arc::clone(&state.store), &peer, None);
    Ok(())
}

#[tauri::command]
pub fn v1_announce_contact(state: tauri::State<'_, AppState>, peer: String) -> Result<(), String> {
    notify_contact_async(Arc::clone(&state.identity), Arc::clone(&state.store), &peer, None);
    Ok(())
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
pub struct TextPreview {
    pub name: String,
    pub kind: String,
    pub text: String,
}

/// Bounded text preview for TXT / Markdown / HTML attachments. Images use the
/// data-URL command; other types return `None` so the UI can say preview is unsupported.
#[tauri::command]
pub fn v1_read_text_attachment(state: tauri::State<'_, AppState>, message_id: String) -> Result<Option<TextPreview>, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    let Some((name, path)) = store.attachment_path(&message_id).map_err(|_| "无法读取附件".to_owned())? else { return Ok(None); };
    let kind = crate::files::preview_kind(&name);
    if !matches!(kind, "text" | "markdown" | "html") { return Ok(None); }
    let Some(text) = crate::files::read_text_preview(&path).map_err(|_| "无法读取附件".to_owned())? else { return Ok(None); };
    Ok(Some(TextPreview { name, kind: kind.to_owned(), text }))
}

/// Group text preview for active members only; mirrors the direct chat command so
/// restored workspace tabs can reload text without the original message object.
#[tauri::command]
pub fn v1_read_group_text_attachment(state: tauri::State<'_, AppState>, group_id: String, message_id: String)
    -> Result<Option<TextPreview>, String> {
    let self_id = state.identity.fingerprint();
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    let Some((name, path)) = store.group_attachment_path(&group_id, &self_id, &message_id)
        .map_err(|_| "无法读取群文件".to_owned())? else { return Ok(None); };
    let kind = crate::files::preview_kind(&name);
    if !matches!(kind, "text" | "markdown" | "html") { return Ok(None); }
    let Some(text) = crate::files::read_text_preview(&path).map_err(|_| "无法读取群文件".to_owned())? else { return Ok(None); };
    Ok(Some(TextPreview { name, kind: kind.to_owned(), text }))
}

/// Upper bound for the serialized workspace (12 tabs, long URLs). Kept out of the
/// settings table because that table caps values at 4 KiB.
const MAX_WORKSPACE_STATE_BYTES: usize = 64 * 1024;

/// Returns the persisted tab layout as opaque JSON; the frontend owns the schema.
#[tauri::command]
pub fn v1_workspace_state(state: tauri::State<'_, AppState>) -> Result<Option<String>, String> {
    let path = state.data_dir.join("workspace.json");
    match std::fs::read(&path) {
        Ok(bytes) if !bytes.is_empty() && bytes.len() <= MAX_WORKSPACE_STATE_BYTES => {
            Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
        }
        Ok(_) => Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("无法读取工作区状态".into()),
    }
}

/// Writes the tab layout with a temp file + rename so a crash cannot leave a
/// half-written layout behind. An empty payload removes the file.
#[tauri::command]
pub fn v1_set_workspace_state(state: tauri::State<'_, AppState>, payload: String) -> Result<(), String> {
    if payload.len() > MAX_WORKSPACE_STATE_BYTES { return Err("工作区状态过大".into()); }
    let dir = state.data_dir.as_ref();
    std::fs::create_dir_all(dir).map_err(|_| "无法保存工作区状态".to_owned())?;
    let path = dir.join("workspace.json");
    if payload.trim().is_empty() {
        match std::fs::remove_file(&path) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err("无法保存工作区状态".into()),
        }
    }
    let temp = dir.join("workspace.json.tmp");
    std::fs::write(&temp, payload.as_bytes()).map_err(|_| "无法保存工作区状态".to_owned())?;
    std::fs::rename(&temp, &path).map_err(|_| "无法保存工作区状态".to_owned())
}

/// Completed group image preview for active members only.
#[tauri::command]
pub fn v1_read_group_attachment(state: tauri::State<'_, AppState>, group_id: String, message_id: String)
    -> Result<Option<AttachmentPreview>, String> {
    let self_id = state.identity.fingerprint();
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    let Some((name, path)) = store.group_attachment_path(&group_id, &self_id, &message_id)
        .map_err(|_| "无法读取群文件".to_owned())? else { return Ok(None); };
    if !crate::files::is_image(&name) { return Ok(None); }
    let size = crate::files::file_size(&path).map_err(|_| "无法读取群文件".to_owned())?;
    if size == 0 || size > crate::files::MAX_PREVIEW_BYTES { return Ok(None); }
    let data = std::fs::read(&path).map_err(|_| "无法读取群文件".to_owned())?;
    let extension = std::path::Path::new(&name).extension().and_then(|ext| ext.to_str()).unwrap_or("").to_ascii_lowercase();
    let mime = match extension.as_str() {
        "png" => "image/png", "jpg" | "jpeg" => "image/jpeg", "gif" => "image/gif",
        "webp" => "image/webp", "bmp" => "image/bmp", _ => return Ok(None),
    };
    Ok(Some(AttachmentPreview { name: name.to_string(), data_url: format!("data:{mime};base64,{}", crate::files::base64_encode(&data)) }))
}

#[tauri::command]
pub fn v1_reveal_group_attachment(state: tauri::State<'_, AppState>, group_id: String, message_id: String) -> Result<(), String> {
    let self_id = state.identity.fingerprint();
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    let Some((_, path)) = store.group_attachment_path(&group_id, &self_id, &message_id)
        .map_err(|_| "无法读取群文件".to_owned())? else {
        return Err("文件尚未接收完成或已被移动".into());
    };
    reveal_path(&path)
}

/// Opens the system file manager with the received/sent attachment selected.
/// The path always comes from the local database, never from UI input.
#[tauri::command]
pub fn v1_reveal_attachment(state: tauri::State<'_, AppState>, message_id: String) -> Result<(), String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    let Some((_, path)) = store.attachment_path(&message_id).map_err(|_| "无法读取附件".to_owned())? else {
        return Err("附件尚未完成传输或已被移动".into());
    };
    reveal_path(&path)
}

fn reveal_path(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", path.to_string_lossy()))
            .spawn()
            .map_err(|_| "无法打开文件所在目录".to_owned())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg("-R").arg(&path).spawn().map_err(|_| "无法打开文件所在目录".to_owned())?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let parent = path.parent().ok_or_else(|| "无法打开文件所在目录".to_owned())?;
        std::process::Command::new("xdg-open").arg(parent).spawn().map_err(|_| "无法打开文件所在目录".to_owned())?;
    }
    Ok(())
}

/// Opens a completed attachment with the system default program. The path always
/// comes from the local database, never from UI input, but it still derives from a
/// peer-supplied file name, so it must never pass through the command interpreter:
/// `cmd /C start` would treat `&`, `%` or `^` inside the name as shell syntax.
fn open_path(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let verb: Vec<u16> = "open\0".encode_utf16().collect();
        let file: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        // ShellExecuteW returns a value greater than 32 on success and never runs a shell.
        let result = unsafe {
            ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL)
        };
        if (result as isize) <= 32 {
            return Err("无法用默认程序打开文件".to_owned());
        }
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(path).spawn().map_err(|_| "无法用默认程序打开文件".to_owned())?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open").arg(path).spawn().map_err(|_| "无法用默认程序打开文件".to_owned())?;
    }
    Ok(())
}

#[tauri::command]
pub fn v1_open_attachment(state: tauri::State<'_, AppState>, message_id: String) -> Result<(), String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    let Some((_, path)) = store.attachment_path(&message_id).map_err(|_| "无法读取附件".to_owned())? else {
        return Err("文件尚未接收完成或已被移动".into());
    };
    drop(store);
    open_path(&path)
}

#[tauri::command]
pub fn v1_open_group_attachment(state: tauri::State<'_, AppState>, group_id: String, message_id: String) -> Result<(), String> {
    let self_id = state.identity.fingerprint();
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    let Some((_, path)) = store.group_attachment_path(&group_id, &self_id, &message_id)
        .map_err(|_| "无法读取群文件".to_owned())? else {
        return Err("文件尚未接收完成或已被移动".into());
    };
    drop(store);
    open_path(&path)
}

#[tauri::command]
pub fn v1_storage_usage(state: tauri::State<'_, AppState>) -> Result<crate::storage::StorageUsage, String> {
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .storage_status().map_err(|_| "无法读取存储占用".to_owned())
}

#[tauri::command]
pub fn v1_set_storage_quota(state: tauri::State<'_, AppState>, bytes: u64) -> Result<crate::storage::StorageUsage, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    if !store.set_storage_quota(bytes).map_err(|_| "无法保存存储配额".to_owned())? {
        return Err(format!("配额需在 {} MiB 到 {} GiB 之间", crate::storage::MIN_STORAGE_QUOTA / (1024 * 1024), crate::storage::MAX_STORAGE_QUOTA / (1024 * 1024 * 1024)));
    }
    store.storage_status().map_err(|_| "无法读取存储占用".to_owned())
}

#[tauri::command]
pub async fn v1_create_backup(state: tauri::State<'_, AppState>, password: String, path: String, include_chat: bool)
    -> Result<crate::backup::BackupInfo, String> {
    if path.chars().count() > 4096 { return Err("备份路径无效".into()); }
    let store = Arc::clone(&state.store);
    tauri::async_runtime::spawn_blocking(move || {
        let store = store.lock().map_err(|_| "本地存储不可用".to_owned())?;
        crate::backup::create(&password, &store, std::path::Path::new(&path), include_chat)
    }).await.map_err(|_| "备份任务异常".to_owned())?
}

#[tauri::command]
pub async fn v1_inspect_backup(password: String, path: String) -> Result<crate::backup::BackupInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::backup::inspect(&password, std::path::Path::new(&path))
    }).await.map_err(|_| "读取备份任务异常".to_owned())?
}

#[tauri::command]
pub async fn v1_restore_backup(state: tauri::State<'_, AppState>, password: String, path: String, overwrite: bool)
    -> Result<crate::storage::RestoreReport, String> {
    let store = Arc::clone(&state.store);
    tauri::async_runtime::spawn_blocking(move || {
        let mut store = store.lock().map_err(|_| "本地存储不可用".to_owned())?;
        crate::backup::restore(&password, std::path::Path::new(&path), &mut store, overwrite)
    }).await.map_err(|_| "恢复任务异常".to_owned())?
}

#[derive(serde::Serialize)]
pub struct ProbeStatus {
    pub running: bool,
    pub port: Option<u16>,
    pub requests: Vec<ProbeRequest>,
}

#[tauri::command]
pub async fn v1_start_probe_service(app: tauri::AppHandle, port: u16) -> Result<(), String> {
    if port == 0 { return Err("端口须为 1–65535".into()); }
    tauri::async_runtime::spawn_blocking(move || app.state::<AppState>().start_listener(port, false))
        .await.map_err(|_| "探测服务任务异常".to_owned())?
}

#[tauri::command]
pub async fn v1_start_chat_service(app: tauri::AppHandle, port: u16) -> Result<(), String> {
    if port == 0 { return Err("端口须为 1–65535".into()); }
    tauri::async_runtime::spawn_blocking(move || app.state::<AppState>().start_listener(port, true))
        .await.map_err(|_| "聊天服务任务异常".to_owned())?
}

impl AppState {
    fn start_listener(&self, port: u16, chat: bool) -> Result<(), String> {
        // Port 0 is internal only: bind any free port when the preferred one is taken.
        let mut slot = self.probe_service.lock().map_err(|_| "探测服务不可用".to_owned())?;
        if slot.is_some() {
            // Launch-time auto listening makes repeated "start" calls normal: treat the
            // same mode as success, but never silently switch chat/probe modes.
            return if self.chat_listener.load(Ordering::Acquire) == chat { Ok(()) }
                else { Err("已有监听在运行，请先停止再切换模式".into()) };
        }
        let service = if chat {
            ProbeService::start_with_chat(Arc::clone(&self.identity), port, Arc::clone(&self.store))
        } else {
            let announce = {
                let store = self.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
                let profile = store.get_profile().map_err(|_| "无法读取个人资料".to_owned())?;
                crate::probe_service::LanAnnounce { nickname: profile.nickname, username: profile.username }
            };
            ProbeService::start_announcing(Arc::clone(&self.identity), port, announce, Arc::clone(&self.store))
        };
        let service = service.map_err(|_| "无法启动服务：端口可能被占用，或网络发现不可用".to_owned())?;
        let mut retry = self.retry_queue.lock().map_err(|_| "消息重试服务不可用".to_owned())?;
        if chat {
            *retry = Some(crate::retry_queue::RetryQueue::start(Arc::clone(&self.identity), Arc::clone(&self.store), Arc::clone(&self.outbound_chat))
                .map_err(|_| "无法启动消息重试服务".to_owned())?);
        }
        *slot = Some(service);
        self.chat_listener.store(chat, Ordering::Release);
        Ok(())
    }

    /// One long-lived mDNS browser shared by the LAN list, diagnostics and presence.
    fn ensure_discovery(&self) -> Result<(), String> {
        let mut discovery = self.discovery.lock().map_err(|_| "局域网发现不可用".to_owned())?;
        if discovery.is_none() {
            *discovery = Some(Discovery::browse().map_err(|_| "无法启动局域网发现，请检查网络和防火墙".to_owned())?);
        }
        Ok(())
    }

    /// Restarts a listener whose worker stopped unexpectedly. A listener the user
    /// stopped manually has no service slot, so it is never resurrected silently.
    fn restart_dead_listener(&self) -> bool {
        let (port, chat) = {
            let Ok(mut slot) = self.probe_service.lock() else { return false };
            match slot.as_ref() {
                Some(service) if !service.is_running() => {
                    let port = service.port();
                    let chat = self.chat_listener.load(Ordering::Acquire);
                    slot.take();
                    (port, chat)
                }
                _ => return false,
            }
        };
        self.start_listener(port, chat).is_ok()
    }

    fn stop_listener(&self) -> Result<(), String> {
        // Hold the lifecycle lock until both workers have stopped, so a concurrent
        // start cannot overlap with an old listener or retry worker.
        let mut service = self.probe_service.lock().map_err(|_| "探测服务不可用".to_owned())?;
        let mut retry = self.retry_queue.lock().map_err(|_| "消息重试服务不可用".to_owned())?;
        drop(retry.take());
        drop(service.take());
        self.chat_listener.store(false, Ordering::Release);
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

/// Read-only snapshot for the debug panel: listener, discovery, addresses and
/// counts. It never probes peers or changes any state.
#[tauri::command]
pub async fn v1_network_diagnostics(app: tauri::AppHandle) -> Result<crate::diagnostics::NetworkReport, String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<AppState>().network_report())
        .await.map_err(|_| "网络检测任务异常".to_owned())?
}

impl AppState {
    fn network_report(&self) -> Result<crate::diagnostics::NetworkReport, String> {
        use crate::diagnostics::{local_ipv4_addresses, port_available, NetworkReport};
        let (service_running, service_port) = {
            let slot = self.probe_service.lock().map_err(|_| "探测服务不可用".to_owned())?;
            match slot.as_ref() {
                Some(service) => (service.is_running(), Some(service.port())),
                None => (false, None),
            }
        };
        let chat_enabled = service_running && self.chat_listener.load(Ordering::Acquire);
        let (discovery_started, lan_candidates) = {
            let discovery = self.discovery.lock().map_err(|_| "局域网发现不可用".to_owned())?;
            let started = discovery.is_some();
            let count = discovery.as_ref().and_then(|discovery| discovery.candidates().ok()).map(|items| items.len()).unwrap_or(0);
            (started, count)
        };
        let (default_port, contacts) = {
            let store = self.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
            let port = read_network(&store)?.default_port;
            let contacts = store.list_contacts().map(|items| items.len()).unwrap_or(0);
            (port, contacts)
        };
        let default_port_available = port_available(default_port);
        let local_ipv4 = local_ipv4_addresses();
        let mut notes = Vec::new();
        if !service_running {
            notes.push("聊天服务未启动：局域网用户暂时无法向你发起连接。".to_owned());
        } else if !chat_enabled {
            notes.push("当前监听只接受身份探测，未启用聊天接收。".to_owned());
        }
        if !default_port_available && !(service_running && service_port == Some(default_port)) {
            notes.push(format!("默认端口 {default_port} 已被其他程序占用，启动监听会失败。"));
        }
        if local_ipv4.is_empty() {
            notes.push("未能读取本机 IPv4 地址，请检查网络连接或适配器状态。".to_owned());
        }
        if !discovery_started {
            notes.push("局域网发现尚未启动，可打开「局域网用户」页或使用重建发现。".to_owned());
        } else if lan_candidates == 0 {
            notes.push("已启动发现，但暂未收到同网段广播；可能是无人在线或组播被防火墙拦截。".to_owned());
        }
        Ok(NetworkReport {
            platform: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
            hostname: crate::diagnostics::hostname(),
            local_ipv4,
            service_running,
            chat_enabled,
            service_port,
            default_port,
            default_port_available,
            discovery_started,
            lan_candidates,
            contacts,
            notes,
        })
    }

    /// Restarts the local chat listener on the port it was using, falling back to
    /// the saved default if the old port cannot be bound again.
    fn restart_chat_listener(&self) -> crate::diagnostics::RepairStep {
        use crate::diagnostics::RepairStep;
        let (running_port, saved_port) = {
            let port = self.probe_service.lock().ok().and_then(|slot| slot.as_ref().map(|service| service.port()));
            let saved = self.store.lock().ok().and_then(|store| read_network(&store).ok()).map(|settings| settings.default_port)
                .unwrap_or(DEFAULT_LISTEN_PORT);
            (port, saved)
        };
        let mut target = running_port.unwrap_or(saved_port);
        if let Err(error) = self.stop_listener() {
            return RepairStep { step: "重启聊天服务".into(), ok: false, detail: format!("无法停止现有监听：{error}") };
        }
        match self.start_listener(target, true) {
            Ok(()) => RepairStep { step: "重启聊天服务".into(), ok: true, detail: format!("已在端口 {target} 重新监听") },
            Err(first) if target != saved_port => {
                target = saved_port;
                match self.start_listener(target, true) {
                    Ok(()) => RepairStep { step: "重启聊天服务".into(), ok: true, detail: format!("原端口不可用（{first}），已改用默认端口 {target}") },
                    Err(second) => RepairStep { step: "重启聊天服务".into(), ok: false, detail: format!("重启失败，服务已停止；原端口：{first}；默认端口：{second}") },
                }
            }
            Err(error) => RepairStep { step: "重启聊天服务".into(), ok: false, detail: format!("重启失败，服务已停止：{error}") },
        }
    }

    /// Drops the current browse daemon and starts a fresh mDNS browse. Advertised
    /// service records owned by the chat listener are left untouched.
    fn rebuild_discovery(&self) -> crate::diagnostics::RepairStep {
        use crate::diagnostics::RepairStep;
        let mut slot = match self.discovery.lock() {
            Ok(slot) => slot,
            Err(_) => return RepairStep { step: "重建局域网发现".into(), ok: false, detail: "局域网发现不可用".into() },
        };
        *slot = None;
        match Discovery::browse() {
            Ok(discovery) => {
                let count = discovery.candidates().map(|items| items.len()).unwrap_or(0);
                *slot = Some(discovery);
                RepairStep { step: "重建局域网发现".into(), ok: true, detail: format!("已重新浏览 mDNS，当前候选 {count} 个") }
            }
            Err(_) => RepairStep { step: "重建局域网发现".into(), ok: false, detail: "无法启动局域网发现，请检查网络和防火墙".into() },
        }
    }
}

/// Bounded repair: only restarts the local listener and/or rebuilds mDNS discovery.
/// Identity, contacts, messages and the saved default port are never changed.
#[tauri::command]
pub async fn v1_network_repair(app: tauri::AppHandle, action: String) -> Result<crate::diagnostics::RepairReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let mut report = crate::diagnostics::RepairReport::new();
        let restart = action == "restart" || action == "all";
        let rediscover = action == "rediscover" || action == "all";
        if !restart && !rediscover { return Err("未知的修复操作".into()); }
        if restart { report.steps.push(state.restart_chat_listener()); }
        if rediscover { report.steps.push(state.rebuild_discovery()); }
        Ok(report)
    }).await.map_err(|_| "网络修复任务异常".to_owned())?
}

/// Read-only storage snapshot: data directory, database, attachment quota and the
/// free space of every receive directory.
#[tauri::command]
pub async fn v1_space_diagnostics(app: tauri::AppHandle) -> Result<crate::diagnostics::SpaceReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
        Ok(crate::diagnostics::space_report(&state.data_dir, &store))
    }).await.map_err(|_| "空间检测任务异常".to_owned())?
}

/// Where this install migrated legacy data from, if any; consumed once so the
/// notice shows exactly one time after a fresh install.
#[tauri::command]
pub fn v1_migration_notice(state: tauri::State<'_, AppState>) -> Result<Option<String>, String> {
    let marker = state.data_dir.join("migration-source.txt");
    if !marker.is_file() { return Ok(None); }
    match std::fs::read_to_string(&marker) {
        Ok(text) => { let _ = std::fs::remove_file(&marker); Ok(Some(text.trim().to_owned())) }
        Err(_) => Ok(None),
    }
}

/// Recent application log text for the debug panel (at most 256 KiB).
#[tauri::command]
pub fn v1_read_logs() -> Result<String, String> {
    crate::logging::read_recent(256 * 1024).map_err(|error| format!("无法读取日志: {error}"))
}

/// Opens the folder that holds qbird.log in the system file manager.
#[tauri::command]
pub fn v1_open_logs_dir() -> Result<(), String> {
    let path = crate::logging::log_path()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .ok_or_else(|| "日志目录不可用".to_owned())?;
    open_url(&path.to_string_lossy())
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
        if name.is_empty() || name.chars().count() > 64 { return Err("联系人名称须为 1–64 个字符".into()); }
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

/// Rebuilds the mDNS announcement after the local profile changed, so peers see the
/// new nickname instead of what was broadcast at launch.
#[tauri::command]
pub async fn v1_refresh_listener(app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let (port, chat) = {
            let slot = state.probe_service.lock().map_err(|_| "探测服务不可用".to_owned())?;
            match slot.as_ref() {
                Some(service) => (service.port(), state.chat_listener.load(Ordering::Acquire)),
                None => return Err("监听未启动".to_owned()),
            }
        };
        state.stop_listener()?;
        for attempt in 0..3u64 {
            if attempt > 0 { std::thread::sleep(std::time::Duration::from_millis(250 * attempt)); }
            if state.start_listener(port, chat).is_ok() { return Ok(()); }
        }
        Err("刷新监听失败，请在局域网用户页重新开始监听".to_owned())
    }).await.map_err(|_| "刷新监听任务异常".to_owned())?
}

#[tauri::command]
pub fn v1_discovery_snapshot(state: tauri::State<'_, AppState>) -> Result<Vec<crate::discovery::Candidate>, String> {
    state.ensure_discovery()?;
    let discovery = state.discovery.lock().map_err(|_| "局域网发现不可用".to_owned())?;
    discovery.as_ref().expect("initialized above").candidates().map_err(str::to_owned)
}

/// Launch-time listener: Qbird listens as soon as it starts, so peers can reach it
/// without opening the discover page first.
pub fn start_listener_on_launch(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let port = {
            let Ok(store) = state.store.lock() else { return };
            read_network(&store).map(|settings| settings.default_port).unwrap_or(DEFAULT_LISTEN_PORT)
        };
        // The previous instance may still be releasing the port.
        for attempt in 0..4u64 {
            if attempt > 0 { std::thread::sleep(std::time::Duration::from_millis(300 * attempt)); }
            if state.start_listener(port, true).is_ok() {
                let _ = state.ensure_discovery();
                return;
            }
        }
        // Last resort: bind any free port; mDNS advertises the actual one.
        if state.start_listener(0, true).is_ok() { let _ = state.ensure_discovery(); }
    });
}

/// Supervises the listener: a worker that died without a manual stop is restarted.
pub fn watch_listener(app: tauri::AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(15));
        let state = app.state::<AppState>();
        if state.restart_dead_listener() { let _ = state.ensure_discovery(); }
    });
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
        // Bug fix: create the default receive folders eagerly so the settings page
        // no longer reports "directory missing" on a fresh install.
        for kind in [crate::storage::RecvKind::Direct, crate::storage::RecvKind::Group] {
            if let Err(error) = std::fs::create_dir_all(store.recv_dir(kind)) {
                crate::log_warn!("创建接收目录失败({kind:?}): {error}");
            }
        }
        // A configured lock always starts locked, even after a crash.
        let locked = crate::lock::is_configured(&store).unwrap_or(false);
        Ok(Self { identity, store: Arc::new(Mutex::new(store)), discovery: Mutex::new(None), probe_service: Mutex::new(None), retry_queue: Mutex::new(None), outbound_probe: Arc::new(AtomicBool::new(false)), outbound_chat: Arc::new(AtomicBool::new(false)), last_outbound_fingerprint: Arc::new(Mutex::new(None)), locked: AtomicBool::new(locked), data_dir: Arc::new(dir.to_path_buf()), capture: Mutex::new(None), chat_listener: AtomicBool::new(false) })
    }
}

#[tauri::command]
pub fn identity_fingerprint(state: tauri::State<'_, AppState>) -> String {
    state.identity.fingerprint()
}

#[tauri::command]
pub fn v1_profile(state: tauri::State<'_, AppState>) -> Result<crate::storage::Profile, String> {
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .get_profile().map_err(|_| "无法读取个人资料".to_owned())
}

/// Stable short identifier derived from the identity fingerprint. Display only.
fn qbird_id(fingerprint: &str) -> String {
    let upper = fingerprint.to_ascii_uppercase();
    if upper.len() < 8 { return "QB-0000-0000".to_owned(); }
    format!("QB-{}-{}", &upper[0..4], &upper[4..8])
}

#[tauri::command]
pub fn v1_identity_id(state: tauri::State<'_, AppState>) -> String {
    qbird_id(&state.identity.fingerprint())
}

#[tauri::command]
pub fn v1_set_username(state: tauri::State<'_, AppState>, username: String) -> Result<crate::storage::Profile, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    if !store.set_username(&username).map_err(|_| "无法保存用户名".to_owned())? {
        return Err("用户名须为 2–20 位小写字母、数字、- 或 _".into());
    }
    store.get_profile().map_err(|_| "无法读取个人资料".to_owned())
}

#[derive(serde::Serialize)]
pub struct OnboardingState {
    pub done: bool,
    pub profile: crate::storage::Profile,
    pub qbird_id: String,
}

#[tauri::command]
pub fn v1_onboarding_state(state: tauri::State<'_, AppState>) -> Result<OnboardingState, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    let profile = store.get_profile().map_err(|_| "无法读取个人资料".to_owned())?;
    let done = store.onboarding_done().map_err(|_| "无法读取引导状态".to_owned())?;
    Ok(OnboardingState { done, profile, qbird_id: qbird_id(&state.identity.fingerprint()) })
}

/// Saves the first-run answers in one step. `recv_dir` empty means keep the default.
#[tauri::command]
pub fn v1_complete_onboarding(
    state: tauri::State<'_, AppState>, nickname: String, username: String, email: String, recv_dir: Option<String>,
) -> Result<crate::storage::Profile, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    if !store.set_profile(&nickname, &email).map_err(|_| "无法保存个人资料".to_owned())? {
        return Err("昵称不超过 64 字符；邮箱需包含一个 @ 或留空".into());
    }
    if !store.set_username(&username).map_err(|_| "无法保存用户名".to_owned())? {
        return Err("用户名须为 2–20 位小写字母、数字、- 或 _".into());
    }
    if let Some(dir) = recv_dir.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        let exe_dir = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf));
        let data_dir = store.data_dir().to_path_buf();
        let cleaned = crate::files::validate_recv_dir(dir, exe_dir.as_deref(), Some(&data_dir))?;
        store.set_recv_dir(crate::storage::RecvKind::Direct, Some(&cleaned)).map_err(|_| "无法保存接收目录".to_owned())?;
    }
    store.set_onboarding_done().map_err(|_| "无法保存引导状态".to_owned())?;
    store.get_profile().map_err(|_| "无法读取个人资料".to_owned())
}

/// One LAN row: advertised display hints plus the identity proven by probing its address.
#[derive(serde::Serialize)]
pub struct LanPerson {
    pub service_name: String,
    pub address: String,
    pub fingerprint: String,
    pub qbird_id: String,
    pub nickname: Option<String>,
    pub username: Option<String>,
    pub known: bool,
    pub verified: bool,
}

#[tauri::command]
pub async fn v1_lan_people(state: tauri::State<'_, AppState>) -> Result<Vec<LanPerson>, String> {
    let candidates = {
        let mut discovery = state.discovery.lock().map_err(|_| "局域网发现不可用".to_owned())?;
        if discovery.is_none() {
            *discovery = Some(Discovery::browse().map_err(|_| "无法启动局域网发现，请检查网络和防火墙".to_owned())?);
        }
        discovery.as_ref().expect("initialized above").candidates().map_err(str::to_owned)?
    };
    let identity = Arc::clone(&state.identity);
    // Probing is cheap but bounded: at most 24 candidates, 600 ms each, sequential so a
    // crowded network cannot stall the UI thread or flood peers.
    let probed = tauri::async_runtime::spawn_blocking(move || {
        let mut rows = Vec::new();
        for candidate in candidates.into_iter().take(24) {
            let Some(address) = candidate.addresses.first().cloned() else { continue };
            let Ok(addr) = address.parse::<SocketAddr>() else { continue };
            if let Ok(fingerprint) = crate::probe::connect_with_timeout(&identity, addr, Duration::from_millis(600)) {
                if fingerprint != identity.fingerprint() { rows.push((candidate, address, fingerprint)); }
            }
        }
        rows
    }).await.map_err(|_| "局域网探测任务异常".to_owned())?;
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    let contacts = store.list_contacts().map_err(|_| "无法读取联系人".to_owned())?;
    Ok(probed.into_iter().map(|(candidate, address, fingerprint)| {
        let existing = contacts.iter().find(|contact| contact.fingerprint == fingerprint);
        LanPerson {
            service_name: candidate.service_name,
            address,
            qbird_id: qbird_id(&fingerprint),
            nickname: existing.map(|contact| contact.display_name.clone()).or(candidate.nickname),
            username: existing.and_then(|contact| contact.username.clone()).or(candidate.username),
            known: existing.is_some(),
            verified: existing.is_some_and(|contact| contact.trust_state == "verified"),
            fingerprint,
        }
    }).collect())
}

/// Adds a discovered peer without asking the user to compare fingerprints by hand.
/// The address is probed again so the stored fingerprint always matches the live peer.
#[tauri::command]
pub async fn v1_add_contact(
    state: tauri::State<'_, AppState>, address: String, fingerprint: String, display_name: String, username: Option<String>,
) -> Result<(), String> {
    let address = crate::address::parse_manual_address(&address).map_err(str::to_owned)?;
    if !crate::storage::valid_fingerprint_public(&fingerprint) { return Err("身份标识格式不正确".into()); }
    if fingerprint == state.identity.fingerprint() { return Err("不能添加本机身份".into()); }
    let name = display_name.trim();
    if name.is_empty() || name.chars().count() > 64 { return Err("联系人名称须为 1–64 个字符".into()); }
    let username = username.map(|value| value.trim().to_owned()).filter(|value| !value.is_empty());
    if let Some(value) = username.as_deref() {
        if !crate::storage::valid_username_public(value) { return Err("对方用户名格式不正确".into()); }
    }
    if state.outbound_probe.swap(true, Ordering::AcqRel) { return Err("已有身份探测正在进行，请稍后重试".into()); }
    let guard = ProbeGuard(Arc::clone(&state.outbound_probe));
    let identity = Arc::clone(&state.identity);
    let expected = fingerprint.clone();
    let probed = tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        crate::probe::connect(&identity, address)
    }).await.map_err(|_| "身份探测不可用".to_owned())?
      .map_err(|_| "无法连接对方：对方可能离线、端口错误或网络受阻".to_owned())?;
    if probed != expected { return Err("该地址当前的用户与列表不一致，请刷新后重试".into()); }
    {
        let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
        store.add_candidate_with_username(&fingerprint, name, username.as_deref()).map_err(|_| "无法保存联系人".to_owned())?;
        if !store.is_verified(&fingerprint).map_err(|_| "无法读取联系人".to_owned())? {
            if !store.confirm_candidate(&fingerprint).map_err(|_| "无法确认联系人".to_owned())? {
                return Err("无法确认联系人".into());
            }
        }
    }
    // Keep the probed address: without it a cross-subnet contact can never be
    // reached again and queued messages would stay pending forever.
    if let Ok(store) = state.store.lock() {
        let _ = store.set_contact_address(&fingerprint, &address.to_string());
    }
    // Tell the peer to add us back. Failure is expected when it is offline or has
    // automatic acceptance disabled; the local contact stays saved either way.
    notify_contact_async(Arc::clone(&state.identity), Arc::clone(&state.store), &fingerprint, Some(address.to_string()));
    Ok(())
}

#[derive(serde::Serialize)]
pub struct ContactSyncSettings { pub auto_accept: bool }

#[tauri::command]
pub fn v1_contact_sync_settings(state: tauri::State<'_, AppState>) -> Result<ContactSyncSettings, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    Ok(ContactSyncSettings { auto_accept: store.auto_accept_contacts().map_err(|_| "无法读取联系人设置".to_owned())? })
}

#[tauri::command]
pub fn v1_set_contact_sync(state: tauri::State<'_, AppState>, auto_accept: bool) -> Result<ContactSyncSettings, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    store.set_auto_accept_contacts(auto_accept).map_err(|_| "无法保存联系人设置".to_owned())?;
    Ok(ContactSyncSettings { auto_accept })
}

#[tauri::command]
pub fn v1_set_profile(state: tauri::State<'_, AppState>, nickname: String, email: String) -> Result<crate::storage::Profile, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    if !store.set_profile(&nickname, &email).map_err(|_| "无法保存个人资料".to_owned())? {
        return Err("昵称不超过 64 字符；邮箱需包含一个 @ 或留空".into());
    }
    store.get_profile().map_err(|_| "无法读取个人资料".to_owned())
}

/// Receive policy plus the sources the user chose not to be asked about again.
#[derive(serde::Serialize)]
pub struct FileAcceptSettings {
    pub mode: String,
    pub remembered: Vec<String>,
}

fn accept_settings(store: &Store) -> Result<FileAcceptSettings, String> {
    let mode = store.file_accept_mode().map_err(|_| "无法读取接收设置".to_owned())?;
    let remembered = store.remembered_file_sources().map_err(|_| "无法读取接收设置".to_owned())?;
    Ok(FileAcceptSettings { mode: mode.as_str().to_owned(), remembered })
}

#[tauri::command]
pub fn v1_file_accept_settings(state: tauri::State<'_, AppState>) -> Result<FileAcceptSettings, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    accept_settings(&store)
}

/// `mode` is one of `always`, `remember` or `auto`.
#[tauri::command]
pub fn v1_set_file_accept_policy(state: tauri::State<'_, AppState>, mode: String) -> Result<FileAcceptSettings, String> {
    let parsed = crate::storage::FileAcceptMode::parse(&mode).ok_or_else(|| "未知的接收策略".to_owned())?;
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    store.set_file_accept_mode(parsed).map_err(|_| "无法保存接收设置".to_owned())?;
    accept_settings(&store)
}

#[tauri::command]
pub fn v1_forget_file_sources(state: tauri::State<'_, AppState>) -> Result<FileAcceptSettings, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    store.forget_file_sources().map_err(|_| "无法清除记住的来源".to_owned())?;
    accept_settings(&store)
}

/// Effective receive directories plus whether each one is a user choice.
#[derive(serde::Serialize)]
pub struct FileRecvSettings {
    pub direct: String,
    pub group: String,
    pub direct_custom: bool,
    pub group_custom: bool,
}

fn recv_settings(store: &Store) -> FileRecvSettings {
    use crate::storage::RecvKind;
    FileRecvSettings {
        direct: store.recv_dir(RecvKind::Direct).to_string_lossy().to_string(),
        group: store.recv_dir(RecvKind::Group).to_string_lossy().to_string(),
        direct_custom: store.recv_dir_custom(RecvKind::Direct),
        group_custom: store.recv_dir_custom(RecvKind::Group),
    }
}

#[tauri::command]
pub fn v1_file_recv_settings(state: tauri::State<'_, AppState>) -> Result<FileRecvSettings, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    Ok(recv_settings(&store))
}

/// `path` empty or missing restores the built-in default for that kind.
#[tauri::command]
pub fn v1_set_file_recv_dir(state: tauri::State<'_, AppState>, kind: String, path: Option<String>) -> Result<FileRecvSettings, String> {
    let recv_kind = match kind.as_str() {
        "direct" => crate::storage::RecvKind::Direct,
        "group" => crate::storage::RecvKind::Group,
        _ => return Err("未知的会话类型".to_owned()),
    };
    let cleaned = match path.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => {
            let exe_dir = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf));
            // A receive folder inside the installed data directory is allowed; the rest
            // of the install directory is not, because uninstall removes it.
            let data_dir = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?.data_dir().to_path_buf();
            Some(crate::files::validate_recv_dir(value, exe_dir.as_deref(), Some(&data_dir))?)
        }
        None => None,
    };
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    store.set_recv_dir(recv_kind, cleaned.as_deref()).map_err(|_| "无法保存接收目录".to_owned())?;
    Ok(recv_settings(&store))
}

#[tauri::command]
pub fn v1_file_offer_requests(state: tauri::State<'_, AppState>, peer: String)
    -> Result<Vec<crate::storage::FileOfferRequest>, String> {
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .list_file_offer_requests(&peer).map_err(|_| "无法读取待确认文件".to_owned())
}

#[tauri::command]
pub fn v1_group_file_offer_requests(state: tauri::State<'_, AppState>, group_id: String)
    -> Result<Vec<crate::storage::FileOfferRequest>, String> {
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .list_group_file_offer_requests(&group_id).map_err(|_| "无法读取待确认文件".to_owned())
}

/// Accepting a deferred offer lets the sender's next retry proceed automatically.
/// `dir` is an optional one-shot "save as" destination for this file.
#[tauri::command]
pub fn v1_resolve_file_offer(state: tauri::State<'_, AppState>, id: String, accept: bool, dir: Option<String>) -> Result<bool, String> {
    let chosen = match dir.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => {
            let exe_dir = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf));
            let data_dir = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?.data_dir().to_path_buf();
            Some(crate::files::validate_recv_dir(value, exe_dir.as_deref(), Some(&data_dir))?)
        }
        None => None,
    };
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .resolve_file_offer(&id, accept, chosen.as_deref()).map_err(|_| "无法更新文件确认状态".to_owned())
}

/// Remember a contact or group so later offers from it skip the prompt.
#[tauri::command]
pub fn v1_remember_file_source(state: tauri::State<'_, AppState>, source: String) -> Result<FileAcceptSettings, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    store.remember_file_source(&source).map_err(|_| "无法记住该来源".to_owned())?;
    accept_settings(&store)
}

#[derive(serde::Serialize)]
pub struct ScreenCaptureResult {
    pub data_url: String,
    pub width: u32,
    pub height: u32,
}

/// Captures the primary screen; the UI crops a region and sends it back as an image.
#[tauri::command]
pub async fn v1_capture_screen() -> Result<ScreenCaptureResult, String> {
    tauri::async_runtime::spawn_blocking(|| {
        crate::capture::capture().map(|capture| ScreenCaptureResult {
            data_url: capture.data_url, width: capture.width, height: capture.height,
        })
    }).await.map_err(|_| "截图任务异常".to_owned())?
}

/// Saves a cropped PNG/BMP into the app temp folder and returns its path.
#[tauri::command]
pub fn v1_save_temp_image(state: tauri::State<'_, AppState>, bytes: Vec<u8>, extension: String) -> Result<String, String> {
    if bytes.is_empty() || bytes.len() > 16 * 1024 * 1024 { return Err("图片大小无效".into()); }
    let is_png = bytes.len() > 8 && bytes[..8] == [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let is_bmp = bytes.len() > 2 && &bytes[..2] == b"BM";
    let extension = match extension.as_str() {
        "png" if is_png => "png",
        "bmp" if is_bmp => "bmp",
        _ => return Err("图片内容与格式不符".into()),
    };
    let dir = state.data_dir.join("temp");
    std::fs::create_dir_all(&dir).map_err(|_| "无法创建临时目录".to_owned())?;
    let now = unix_now()?;
    let name = format!("shot-{}-{}-{}.{}", std::process::id(), now,
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0), extension);
    let path = dir.join(name);
    std::fs::write(&path, &bytes).map_err(|_| "无法保存截图".to_owned())?;
    path.to_str().map(str::to_owned).ok_or_else(|| "截图路径无效".to_owned())
}

/// Hides the main window, captures the screen and opens the fullscreen overlay where
/// the user picks a region. The capture is stored so the overlay can fetch it after
/// its page has loaded.
#[tauri::command]
pub async fn v1_begin_capture(app: tauri::AppHandle) -> Result<(), String> {
    if app.get_webview_window("capture").is_some() { return Ok(()); }
    if let Some(window) = app.get_webview_window("main") { let _ = window.hide(); }
    let shot = tauri::async_runtime::spawn_blocking(|| {
        // Give the compositor a moment so the hidden window is not part of the shot.
        std::thread::sleep(std::time::Duration::from_millis(180));
        crate::capture::capture()
    }).await.map_err(|_| "截图任务异常".to_owned())??;
    {
        let state = app.state::<AppState>();
        let mut slot = state.capture.lock().map_err(|_| "截图状态不可用".to_owned())?;
        *slot = Some(shot);
    }
    let window = tauri::WebviewWindowBuilder::new(&app, "capture", tauri::WebviewUrl::App("capture.html".into()))
        .title("Qbird 截图")
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .fullscreen(true)
        .build();
    let window = match window {
        Ok(window) => window,
        Err(_) => {
            // Never leave the app invisible when the overlay cannot open.
            if let Some(main) = app.get_webview_window("main") { let _ = main.show(); let _ = main.set_focus(); }
            let state = app.state::<AppState>();
            if let Ok(mut slot) = state.capture.lock() { *slot = None; }
            return Err("无法打开截图窗口".to_owned());
        }
    };
    let handle = app.clone();
    window.on_window_event(move |event| {
        // Safety net: the app must never stay invisible if the overlay goes away.
        if let tauri::WindowEvent::Destroyed = event {
            if let Some(main) = handle.get_webview_window("main") { let _ = main.show(); let _ = main.set_focus(); }
            if let Some(state) = handle.try_state::<AppState>() {
                if let Ok(mut slot) = state.capture.lock() { *slot = None; }
            }
        }
    });
    let _ = window.set_focus();
    Ok(())
}

#[derive(serde::Serialize)]
pub struct CaptureImage {
    pub data_url: String,
    pub width: u32,
    pub height: u32,
}

/// Screen image for the overlay window; only valid while a capture is in progress.
#[tauri::command]
pub fn v1_capture_current(state: tauri::State<'_, AppState>) -> Result<CaptureImage, String> {
    let slot = state.capture.lock().map_err(|_| "截图状态不可用".to_owned())?;
    let shot = slot.as_ref().ok_or_else(|| "没有待处理的截图".to_owned())?;
    Ok(CaptureImage { data_url: shot.data_url.clone(), width: shot.width, height: shot.height })
}

/// Closes the overlay, restores the main window and hands the cropped temp file
/// (if any) back to the chat window. `None` means the user cancelled.
#[tauri::command]
pub fn v1_end_capture(app: tauri::AppHandle, path: Option<String>) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("capture") { let _ = window.close(); }
    {
        let state = app.state::<AppState>();
        if let Ok(mut slot) = state.capture.lock() { *slot = None; };
    }
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.unminimize();
        let _ = main.show();
        let _ = main.set_focus();
    }
    if let Some(path) = path { let _ = app.emit("qbird://capture-done", path); }
    Ok(())
}

/// Experimental switches; each defaults to off and is stored as "0"/"1".
#[derive(serde::Serialize)]
pub struct ExperimentSettings {
    pub group_file_quote: bool,
}

fn read_experiments(store: &crate::storage::Store) -> Result<ExperimentSettings, String> {
    let enabled = store.get_setting("experiment_group_file_quote").map_err(|_| "无法读取实验设置".to_owned())?
        .map(|value| value == "1").unwrap_or(false);
    Ok(ExperimentSettings { group_file_quote: enabled })
}

#[tauri::command]
pub fn v1_experiment_settings(state: tauri::State<'_, AppState>) -> Result<ExperimentSettings, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    read_experiments(&store)
}

#[tauri::command]
pub fn v1_set_experiment_settings(state: tauri::State<'_, AppState>, group_file_quote: bool)
    -> Result<ExperimentSettings, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    store.set_setting("experiment_group_file_quote", Some(if group_file_quote { "1" } else { "0" }))
        .map_err(|_| "无法保存实验设置".to_owned())?;
    read_experiments(&store)
}

fn read_scan_blacklist(store: &crate::storage::Store) -> Result<Vec<String>, String> {
    let raw = store.get_setting("scan_blacklist").map_err(|_| "无法读取扫描黑名单".to_owned())?.unwrap_or_default();
    let entries: Vec<String> = serde_json::from_str(&raw).unwrap_or_default();
    Ok(entries)
}

#[tauri::command]
pub fn v1_scan_blacklist(state: tauri::State<'_, AppState>) -> Result<Vec<String>, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    read_scan_blacklist(&store)
}

#[tauri::command]
pub fn v1_set_scan_blacklist(state: tauri::State<'_, AppState>, entries: Vec<String>) -> Result<Vec<String>, String> {
    let entries: Vec<String> = entries.into_iter().map(|entry| entry.trim().to_owned()).filter(|entry| !entry.is_empty()).collect();
    crate::scan::validate_blacklist(&entries)?;
    let encoded = serde_json::to_string(&entries).map_err(|_| "无法保存扫描黑名单".to_owned())?;
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    store.set_setting("scan_blacklist", Some(&encoded)).map_err(|_| "无法保存扫描黑名单".to_owned())?;
    Ok(entries)
}

/// Scans at most a /24 with a bounded worker pool; results never grant trust.
#[tauri::command]
pub async fn v1_scan_range(state: tauri::State<'_, AppState>, cidr: String, port: u16)
    -> Result<Vec<crate::scan::ScanHit>, String> {
    let identity = Arc::clone(&state.identity);
    let store = Arc::clone(&state.store);
    tauri::async_runtime::spawn_blocking(move || {
        let blacklist = {
            let store = store.lock().map_err(|_| "本地存储不可用".to_owned())?;
            read_scan_blacklist(&store)?
        };
        crate::scan::scan(&identity, &cidr, port, &blacklist)
    }).await.map_err(|_| "扫描任务异常".to_owned())?
}

/// Default TCP port for the shared probe/chat listener.
#[derive(serde::Serialize)]
pub struct NetworkSettings {
    pub default_port: u16,
}

pub const DEFAULT_LISTEN_PORT: u16 = 42853;
pub const MIN_LISTEN_PORT: u16 = 1024;

fn read_network(store: &crate::storage::Store) -> Result<NetworkSettings, String> {
    let port = store.get_setting("listen_port").map_err(|_| "无法读取网络设置".to_owned())?
        .and_then(|text| text.parse::<u16>().ok())
        .filter(|port| *port >= MIN_LISTEN_PORT)
        .unwrap_or(DEFAULT_LISTEN_PORT);
    Ok(NetworkSettings { default_port: port })
}

#[tauri::command]
pub fn v1_network_settings(state: tauri::State<'_, AppState>) -> Result<NetworkSettings, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    read_network(&store)
}

/// Saves the default port used the next time a listener starts; a running
/// listener keeps its port.
#[tauri::command]
pub fn v1_set_network_settings(state: tauri::State<'_, AppState>, port: u16) -> Result<NetworkSettings, String> {
    if port < MIN_LISTEN_PORT {
        return Err(format!("监听端口需为 {MIN_LISTEN_PORT}–65535"));
    }
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    store.set_setting("listen_port", Some(&port.to_string())).map_err(|_| "无法保存网络设置".to_owned())?;
    read_network(&store)
}

#[tauri::command]
pub fn v1_list_stickers(state: tauri::State<'_, AppState>) -> Result<Vec<String>, String> {
    crate::stickers::list(&state.data_dir)
}

/// Copies a user-picked image into the sticker collection.
#[tauri::command]
pub fn v1_add_sticker(state: tauri::State<'_, AppState>, path: String) -> Result<String, String> {
    crate::stickers::add(&state.data_dir, Path::new(&path))
}

#[tauri::command]
pub fn v1_sticker_bytes(state: tauri::State<'_, AppState>, name: String) -> Result<Vec<u8>, String> {
    crate::stickers::read(&state.data_dir, &name)
}

/// Returns the absolute path so the sticker can be sent as a normal image file.
#[tauri::command]
pub fn v1_sticker_path(state: tauri::State<'_, AppState>, name: String) -> Result<String, String> {
    let path = crate::stickers::path(&state.data_dir, &name)?;
    path.to_str().map(str::to_owned).ok_or_else(|| "表情路径无效".to_owned())
}

#[tauri::command]
pub fn v1_remove_sticker(state: tauri::State<'_, AppState>, name: String) -> Result<(), String> {
    crate::stickers::remove(&state.data_dir, &name)
}

/// Composer preferences shared by direct, group and self chats.
#[derive(serde::Serialize)]
pub struct ComposerSettings {
    /// `enter` or `ctrl+enter`.
    pub send_shortcut: String,
    /// Textarea height in rows.
    pub rows: u32,
}

pub const MIN_COMPOSER_ROWS: u32 = 2;
pub const MAX_COMPOSER_ROWS: u32 = 12;

fn read_composer(store: &crate::storage::Store) -> Result<ComposerSettings, String> {
    let shortcut = store.get_setting("send_shortcut").map_err(|_| "无法读取输入设置".to_owned())?
        .filter(|value| value == "ctrl+enter").unwrap_or_else(|| "enter".to_owned());
    let rows = store.get_setting("composer_rows").map_err(|_| "无法读取输入设置".to_owned())?
        .and_then(|text| text.parse::<u32>().ok())
        .unwrap_or(3)
        .clamp(MIN_COMPOSER_ROWS, MAX_COMPOSER_ROWS);
    Ok(ComposerSettings { send_shortcut: shortcut, rows })
}

#[tauri::command]
pub fn v1_composer_settings(state: tauri::State<'_, AppState>) -> Result<ComposerSettings, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    read_composer(&store)
}

/// Validates before writing so a bad request never leaves half-applied settings.
#[tauri::command]
pub fn v1_set_composer_settings(state: tauri::State<'_, AppState>, send_shortcut: String, rows: u32)
    -> Result<ComposerSettings, String> {
    if send_shortcut != "enter" && send_shortcut != "ctrl+enter" {
        return Err("发送快捷键无效".into());
    }
    if !(MIN_COMPOSER_ROWS..=MAX_COMPOSER_ROWS).contains(&rows) {
        return Err(format!("输入区高度需为 {MIN_COMPOSER_ROWS}–{MAX_COMPOSER_ROWS} 行"));
    }
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    store.set_setting("send_shortcut", Some(&send_shortcut)).map_err(|_| "无法保存输入设置".to_owned())?;
    store.set_setting("composer_rows", Some(&rows.to_string())).map_err(|_| "无法保存输入设置".to_owned())?;
    read_composer(&store)
}

/// Installed preview plugins; entries are re-validated from disk on every listing.
#[tauri::command]
pub fn v1_list_plugins(state: tauri::State<'_, AppState>) -> Result<Vec<crate::plugins::PluginView>, String> {
    let rows = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?.list_plugins()
        .map_err(|_| "无法读取插件列表".to_owned())?;
    let root = crate::plugins::plugins_dir(&state.data_dir);
    Ok(rows.into_iter().filter_map(|row| crate::plugins::load_view(&root.join(&row.id), row.enabled)).collect())
}

#[tauri::command]
pub async fn v1_install_plugin(state: tauri::State<'_, AppState>, path: String)
    -> Result<crate::plugins::PluginView, String> {
    let store = Arc::clone(&state.store);
    let data_dir = Arc::clone(&state.data_dir);
    tauri::async_runtime::spawn_blocking(move || {
        if path.len() > 4096 { return Err("插件路径无效".to_owned()); }
        let now = unix_now()?;
        let (view, _) = crate::plugins::install(&data_dir, std::path::Path::new(&path))?;
        store.lock().map_err(|_| "本地存储不可用".to_owned())?
            .register_plugin(&view.id, &view.name, &view.version, now)
            .map_err(|_| "无法登记插件".to_owned())?;
        Ok(view)
    }).await.map_err(|_| "插件安装任务异常".to_owned())?
}

#[tauri::command]
pub fn v1_set_plugin_enabled(state: tauri::State<'_, AppState>, id: String, enabled: bool) -> Result<(), String> {
    let changed = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .set_plugin_enabled(&id, enabled).map_err(|_| "无法更新插件状态".to_owned())?;
    if changed { Ok(()) } else { Err("插件不存在".into()) }
}

/// User-initiated removal: drops the database row and the copied plugin folder only.
#[tauri::command]
pub fn v1_remove_plugin(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    crate::plugins::remove(&state.data_dir, &id)?;
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .remove_plugin_row(&id).map_err(|_| "无法移除插件记录".to_owned())?;
    Ok(())
}

#[derive(serde::Serialize)]
pub struct PluginPreview {
    pub plugin_id: String,
    pub html: String,
}

/// The entry HTML for one file name, if an enabled plugin claims the extension.
#[tauri::command]
pub fn v1_plugin_preview(state: tauri::State<'_, AppState>, file_name: String) -> Result<Option<PluginPreview>, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    let Some((plugin_id, html)) = crate::plugins::entry_html(&state.data_dir, &file_name, |id| {
        store.is_plugin_enabled(id).unwrap_or(false)
    }) else { return Ok(None); };
    Ok(Some(PluginPreview { plugin_id, html }))
}

/// Base64 bytes for a completed attachment, bounded so the IPC payload stays sane.
/// Bounded data URL for a local image the user just picked, so the composer can show
/// a thumbnail before anything is queued. Read-only and extension-whitelisted.
#[tauri::command]
pub fn v1_local_preview(path: String) -> Result<String, String> {
    let source = std::path::PathBuf::from(&path);
    let extension = source.extension().and_then(|value| value.to_str()).unwrap_or("").to_ascii_lowercase();
    let mime = match extension.as_str() {
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "jpg" | "jpeg" => "image/jpeg",
        _ => return Err("仅支持图片预览".into()),
    };
    let size = crate::files::file_size(&source).map_err(|_| "无法读取图片".to_owned())?;
    if size == 0 { return Err("暂不支持空文件".into()); }
    if size > 8 * 1024 * 1024 { return Err("图片超过 8 MiB，暂不能预览".into()); }
    let data = std::fs::read(&source).map_err(|_| "无法读取图片".to_owned())?;
    Ok(format!("data:{mime};base64,{}", crate::files::base64_encode(&data)))
}

#[tauri::command]
pub fn v1_attachment_base64(state: tauri::State<'_, AppState>, message_id: String) -> Result<String, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    let Some((_, path)) = store.attachment_path(&message_id).map_err(|_| "无法读取附件".to_owned())? else {
        return Err("文件尚未接收完成或已被移动".into());
    };
    let size = crate::files::file_size(&path).map_err(|_| "无法读取附件".to_owned())?;
    if size == 0 || size > crate::plugins::MAX_ENTRY_BYTES { return Err("文件过大，暂不能交给插件预览".into()); }
    let data = std::fs::read(&path).map_err(|_| "无法读取附件".to_owned())?;
    Ok(crate::files::base64_encode(&data))
}

#[tauri::command]
pub fn v1_group_attachment_base64(state: tauri::State<'_, AppState>, group_id: String, message_id: String) -> Result<String, String> {
    let self_id = state.identity.fingerprint();
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    let Some((_, path)) = store.group_attachment_path(&group_id, &self_id, &message_id)
        .map_err(|_| "无法读取群文件".to_owned())? else {
        return Err("文件尚未接收完成或已被移动".into());
    };
    let size = crate::files::file_size(&path).map_err(|_| "无法读取群文件".to_owned())?;
    if size == 0 || size > crate::plugins::MAX_ENTRY_BYTES { return Err("文件过大，暂不能交给插件预览".into()); }
    let data = std::fs::read(&path).map_err(|_| "无法读取群文件".to_owned())?;
    Ok(crate::files::base64_encode(&data))
}

/// Raw bytes for the workspace PDF viewer. The path is resolved from the local
/// message id, the file must be a completed attachment and must actually start with
/// `%PDF-`; oversized or renamed files are refused instead of guessed at.
#[tauri::command]
pub fn v1_attachment_bytes(state: tauri::State<'_, AppState>, message_id: String)
    -> Result<tauri::ipc::Response, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    let Some((name, path)) = store.attachment_path(&message_id).map_err(|_| "无法读取附件".to_owned())? else {
        return Err("文件尚未接收完成或已被移动".into());
    };
    if !crate::files::is_pdf(&name) { return Err("该类型暂不支持字节预览".into()); }
    let size = crate::files::file_size(&path).map_err(|_| "无法读取附件".to_owned())?;
    if size > crate::files::MAX_WORKSPACE_BYTES {
        return Err(format!("文件超过 {} MiB，暂不能预览", crate::files::MAX_WORKSPACE_BYTES / (1024 * 1024)));
    }
    let data = crate::files::read_pdf_preview(&path).map_err(|_| "无法读取附件".to_owned())?;
    let Some(bytes) = data else { return Err("文件为空、不是有效的 PDF 或已被移动".into()); };
    Ok(tauri::ipc::Response::new(bytes))
}

/// Same as `v1_attachment_bytes` for group files; membership and message ownership
/// are still checked by the storage layer.
#[tauri::command]
pub fn v1_group_attachment_bytes(state: tauri::State<'_, AppState>, group_id: String, message_id: String)
    -> Result<tauri::ipc::Response, String> {
    let self_id = state.identity.fingerprint();
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    let Some((name, path)) = store.group_attachment_path(&group_id, &self_id, &message_id)
        .map_err(|_| "无法读取群文件".to_owned())? else {
        return Err("文件尚未接收完成或已被移动".into());
    };
    if !crate::files::is_pdf(&name) { return Err("该类型暂不支持字节预览".into()); }
    let size = crate::files::file_size(&path).map_err(|_| "无法读取群文件".to_owned())?;
    if size > crate::files::MAX_WORKSPACE_BYTES {
        return Err(format!("文件超过 {} MiB，暂不能预览", crate::files::MAX_WORKSPACE_BYTES / (1024 * 1024)));
    }
    let data = crate::files::read_pdf_preview(&path).map_err(|_| "无法读取群文件".to_owned())?;
    let Some(bytes) = data else { return Err("文件为空、不是有效的 PDF 或已被移动".into()); };
    Ok(tauri::ipc::Response::new(bytes))
}

/// Opens an http(s) URL in the system browser. Only web schemes are accepted so the
/// workspace cannot be used to launch local executables or file:// paths.
#[tauri::command]
pub fn v1_open_external_url(url: String) -> Result<(), String> {
    let trimmed = url.trim();
    if trimmed.is_empty() || trimmed.len() > 2048 { return Err("链接无效".into()); }
    if trimmed.chars().any(|c| c.is_control() || c.is_whitespace()) { return Err("链接无效".into()); }
    let lower = trimmed.to_ascii_lowercase();
    if !lower.starts_with("http://") && !lower.starts_with("https://") {
        return Err("只允许打开 http 或 https 链接".into());
    }
    open_url(trimmed)
}

fn open_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let verb: Vec<u16> = "open\0".encode_utf16().collect();
        let target: Vec<u16> = std::ffi::OsStr::new(url).encode_wide().chain(std::iter::once(0)).collect();
        let result = unsafe {
            ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), target.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL)
        };
        if (result as isize) <= 32 { return Err("无法用系统浏览器打开链接".to_owned()); }
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(url).spawn().map_err(|_| "无法用系统浏览器打开链接".to_owned())?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open").arg(url).spawn().map_err(|_| "无法用系统浏览器打开链接".to_owned())?;
    }
    Ok(())
}

#[derive(serde::Serialize)]
pub struct HoverSettings {
    pub show_ip: bool,
    pub show_mac: bool,
}

fn read_hover(store: &crate::storage::Store) -> Result<HoverSettings, String> {
    let flag = |key: &str, default: bool| -> Result<bool, String> {
        Ok(store.get_setting(key).map_err(|_| "无法读取悬浮信息设置".to_owned())?
            .map(|value| value == "1").unwrap_or(default))
    };
    Ok(HoverSettings { show_ip: flag("hover_show_ip", true)?, show_mac: flag("hover_show_mac", false)? })
}

/// Contact hover card fields; MAC is opt-in and never fabricated when unavailable.
#[tauri::command]
pub fn v1_hover_settings(state: tauri::State<'_, AppState>) -> Result<HoverSettings, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    read_hover(&store)
}

#[tauri::command]
pub fn v1_set_hover_settings(state: tauri::State<'_, AppState>, show_ip: bool, show_mac: bool) -> Result<HoverSettings, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    store.set_setting("hover_show_ip", Some(if show_ip { "1" } else { "0" })).map_err(|_| "无法保存悬浮信息设置".to_owned())?;
    store.set_setting("hover_show_mac", Some(if show_mac { "1" } else { "0" })).map_err(|_| "无法保存悬浮信息设置".to_owned())?;
    read_hover(&store)
}

#[derive(serde::Serialize)]
pub struct ContactNetwork {
    pub address: Option<String>,
    pub ip: Option<String>,
    pub mac: Option<String>,
}

/// Address comes from the local database; the MAC is a best-effort ARP lookup.
#[tauri::command]
pub fn v1_contact_network(state: tauri::State<'_, AppState>, peer: String) -> Result<ContactNetwork, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    let address = store.contact_address(&peer).map_err(|_| "无法读取联系人地址".to_owned())?;
    drop(store);
    let ip = address.as_deref().and_then(crate::network_info::ip_of);
    let mac = ip.as_deref().and_then(crate::network_info::mac_for);
    Ok(ContactNetwork { address, ip, mac })
}

/// Where clicking a chat image leads: the in-app lightbox or the right workspace panel.
#[tauri::command]
pub fn v1_image_open_mode(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    Ok(store.get_setting("image_open_mode").map_err(|_| "无法读取工作区设置".to_owned())?
        .filter(|value| value == "workspace").unwrap_or_else(|| "lightbox".to_owned()))
}

#[tauri::command]
pub fn v1_set_image_open_mode(state: tauri::State<'_, AppState>, mode: String) -> Result<String, String> {
    if mode != "lightbox" && mode != "workspace" { return Err("图片打开方式无效".into()); }
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    store.set_setting("image_open_mode", Some(&mode)).map_err(|_| "无法保存工作区设置".to_owned())?;
    Ok(mode)
}

/// What clicking a chat file bubble does: preview inside the workspace or hand the
/// file to the system default program. Images keep using `image_open_mode`.
#[tauri::command]
pub fn v1_file_open_mode(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    Ok(store.get_setting("file_open_mode").map_err(|_| "无法读取工作区设置".to_owned())?
        .filter(|value| value == "system")
        .unwrap_or_else(|| "workspace".to_owned()))
}

#[tauri::command]
pub fn v1_set_file_open_mode(state: tauri::State<'_, AppState>, mode: String) -> Result<String, String> {
    if mode != "workspace" && mode != "system" { return Err("文件打开方式无效".into()); }
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    store.set_setting("file_open_mode", Some(&mode)).map_err(|_| "无法保存工作区设置".to_owned())?;
    Ok(mode)
}

/// Settings key for what the window close button does. Values: `ask`, `tray`, `quit`.
pub const CLOSE_ACTION_KEY: &str = "window_close_action";
/// Emitted when the close prompt has to be shown by the frontend.
pub const CLOSE_REQUESTED_EVENT: &str = "qbird://close-requested";

#[derive(serde::Serialize)]
pub struct WindowSettings {
    pub close_action: String,
}

fn read_window_settings(store: &crate::storage::Store) -> WindowSettings {
    let close_action = store.get_setting(CLOSE_ACTION_KEY).ok().flatten()
        .filter(|value| value == "tray" || value == "quit")
        .unwrap_or_else(|| "ask".to_owned());
    WindowSettings { close_action }
}

/// Current close behavior; `ask` means the in-app prompt is shown every time.
#[tauri::command]
pub fn v1_window_settings(state: tauri::State<'_, AppState>) -> Result<WindowSettings, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    Ok(read_window_settings(&store))
}

#[tauri::command]
pub fn v1_set_window_settings(state: tauri::State<'_, AppState>, close_action: String)
    -> Result<WindowSettings, String> {
    if !matches!(close_action.as_str(), "ask" | "tray" | "quit") { return Err("关闭行为无效".into()); }
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    store.set_setting(CLOSE_ACTION_KEY, Some(&close_action)).map_err(|_| "无法保存关闭行为".to_owned())?;
    Ok(read_window_settings(&store))
}

/// Applies the choice made in the close prompt. The window stays open until the
/// user decides, so hiding and quitting happen here and only here.
#[tauri::command]
pub fn v1_resolve_window_close(app: tauri::AppHandle, state: tauri::State<'_, AppState>,
    action: String, remember: bool) -> Result<(), String> {
    match action.as_str() {
        "cancel" => Ok(()),
        "tray" | "quit" => {
            if remember {
                let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
                store.set_setting(CLOSE_ACTION_KEY, Some(&action)).map_err(|_| "无法保存关闭行为".to_owned())?;
            }
            if action == "quit" {
                app.exit(0);
                return Ok(());
            }
            let window = app.get_webview_window("main").ok_or_else(|| "主窗口不可用".to_owned())?;
            window.hide().map_err(|_| "无法隐藏主窗口".to_owned())
        }
        _ => Err("关闭行为无效".into()),
    }
}

/// Returns the local avatar as raw PNG bytes, or `null` when none is saved.
#[tauri::command]
pub fn v1_avatar(state: tauri::State<'_, AppState>) -> Result<Option<Vec<u8>>, String> {
    crate::avatar::load(&state.data_dir)
}

/// Replaces the avatar with a cropped square PNG produced by the front end.
/// Encrypted per-conversation archive; attachments are not included.
#[tauri::command]
pub async fn v1_export_chat_archive(state: tauri::State<'_, AppState>, kind: String, target: String, password: String, path: String)
    -> Result<crate::chatfile::ChatArchiveInfo, String> {
    let store = Arc::clone(&state.store);
    let self_id = state.identity.fingerprint();
    tauri::async_runtime::spawn_blocking(move || {
        if path.len() > 4096 { return Err("导出路径无效".to_owned()); }
        let guard = store.lock().map_err(|_| "本地存储不可用".to_owned())?;
        crate::chatfile::export(&kind, &target, &password, &guard, &self_id, std::path::Path::new(&path))
    }).await.map_err(|_| "聊天记录导出任务异常".to_owned())?
}

#[tauri::command]
pub async fn v1_import_chat_archive(state: tauri::State<'_, AppState>, password: String, path: String)
    -> Result<crate::storage::ArchiveImportReport, String> {
    let store = Arc::clone(&state.store);
    let self_id = state.identity.fingerprint();
    tauri::async_runtime::spawn_blocking(move || {
        if path.len() > 4096 { return Err("聊天记录路径无效".to_owned()); }
        let guard = store.lock().map_err(|_| "本地存储不可用".to_owned())?;
        crate::chatfile::import(&password, std::path::Path::new(&path), &guard, &self_id)
    }).await.map_err(|_| "聊天记录导入任务异常".to_owned())?
}

const TEAM_NAME_KEY: &str = "team_name";
const TEAM_LOGO_FILE: &str = "team-logo.png";

#[derive(serde::Serialize)]
pub struct TeamProfile {
    pub name: String,
    pub has_logo: bool,
}

/// Local branding only: the team name and logo never leave this device.
#[tauri::command]
pub fn v1_team_profile(state: tauri::State<'_, AppState>) -> Result<TeamProfile, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    let name = store.get_setting(TEAM_NAME_KEY).map_err(|_| "无法读取团队名称".to_owned())?.unwrap_or_default();
    drop(store);
    Ok(TeamProfile { name, has_logo: crate::avatar::load_named(&state.data_dir, TEAM_LOGO_FILE)
        .map_err(|_| "无法读取团队 Logo".to_owned())?.is_some() })
}

/// Empty input clears the name and restores the product name.
#[tauri::command]
pub fn v1_set_team_name(state: tauri::State<'_, AppState>, name: String) -> Result<TeamProfile, String> {
    let trimmed = name.trim();
    if trimmed.chars().count() > 64 || trimmed.chars().any(char::is_control) {
        return Err("团队名称需为 1–64 个字符".into());
    }
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    store.set_setting(TEAM_NAME_KEY, if trimmed.is_empty() { None } else { Some(trimmed) })
        .map_err(|_| "无法保存团队名称".to_owned())?;
    let saved = store.get_setting(TEAM_NAME_KEY).map_err(|_| "无法读取团队名称".to_owned())?.unwrap_or_default();
    drop(store);
    Ok(TeamProfile { name: saved, has_logo: crate::avatar::load_named(&state.data_dir, TEAM_LOGO_FILE)
        .map_err(|_| "无法读取团队 Logo".to_owned())?.is_some() })
}

#[tauri::command]
pub fn v1_team_logo(state: tauri::State<'_, AppState>) -> Result<Option<Vec<u8>>, String> {
    crate::avatar::load_named(&state.data_dir, TEAM_LOGO_FILE)
}

#[tauri::command]
pub fn v1_set_team_logo(state: tauri::State<'_, AppState>, bytes: Vec<u8>) -> Result<(), String> {
    crate::avatar::save_named(&state.data_dir, TEAM_LOGO_FILE, &bytes)
}

#[tauri::command]
pub fn v1_clear_team_logo(state: tauri::State<'_, AppState>) -> Result<(), String> {
    crate::avatar::remove_named(&state.data_dir, TEAM_LOGO_FILE)
}

#[tauri::command]
pub fn v1_set_avatar(state: tauri::State<'_, AppState>, bytes: Vec<u8>) -> Result<(), String> {
    crate::avatar::save(&state.data_dir, &bytes)
}

#[tauri::command]
pub fn v1_clear_avatar(state: tauri::State<'_, AppState>) -> Result<(), String> {
    crate::avatar::remove(&state.data_dir)
}

/// Reads a user-picked image for cropping; only PNG/JPEG and at most 8 MiB.
/// The path comes from the system file picker, so no extra permission is granted.
#[tauri::command]
pub fn v1_read_image_file(path: String) -> Result<Vec<u8>, String> {
    crate::avatar::read_source(Path::new(&path))
}

#[tauri::command]
pub fn v1_set_contact_profile(state: tauri::State<'_, AppState>, peer: String, remark: Option<String>, category: Option<String>)
    -> Result<(), String> {
    let saved = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .set_contact_profile(&peer, remark.as_deref(), category.as_deref())
        .map_err(|_| "备注或分组过长".to_owned())?;
    if saved { Ok(()) } else { Err("联系人或分组不存在".into()) }
}

#[tauri::command]
pub fn v1_search_contacts(state: tauri::State<'_, AppState>, query: String, limit: u32) -> Result<Vec<crate::storage::Contact>, String> {
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .search_contacts(&query, limit).map_err(|_| "搜索条件无效".to_owned())
}

#[tauri::command]
pub fn v1_accept_contact(state: tauri::State<'_, AppState>, fingerprint: String) -> Result<(), String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    if store.confirm_candidate(&fingerprint).map_err(|_| "无法确认联系人".to_owned())? { Ok(()) }
    else { Err("该联系人已确认或不存在".into()) }
}

#[tauri::command]
pub fn v1_decline_contact(state: tauri::State<'_, AppState>, fingerprint: String) -> Result<(), String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    if store.decline_candidate(&fingerprint).map_err(|_| "无法移除联系人".to_owned())? { Ok(()) }
    else { Err("只能移除待确认的联系人".into()) }
}

#[derive(serde::Serialize)]
pub struct ContactPresence {
    pub fingerprint: String,
    pub online: bool,
}

fn address_is_reachable(address: &str) -> bool {
    let Ok(target) = address.parse::<std::net::SocketAddr>() else { return false };
    std::net::TcpStream::connect_timeout(&target, std::time::Duration::from_millis(350)).is_ok()
}

/// Best-effort presence: either the peer is visible over mDNS on this LAN, or its saved
/// address accepts a TCP connection. Reachability is not proof of identity, and an online
/// peer may still be idle; the UI must not claim more than that.
#[tauri::command]
pub async fn v1_contact_presence(state: tauri::State<'_, AppState>) -> Result<Vec<ContactPresence>, String> {
    let mut targets: Vec<(String, Option<String>)> = Vec::new();
    {
        let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
        for contact in store.list_contacts().map_err(|_| "无法读取联系人".to_owned())? {
            if contact.trust_state != "verified" { continue; }
            let address = store.contact_address(&contact.fingerprint).ok().flatten();
            targets.push((contact.fingerprint, address));
        }
    }
    let lan_addresses: Vec<String> = {
        let discovery = state.discovery.lock().map_err(|_| "局域网发现不可用".to_owned())?;
        match discovery.as_ref() {
            Some(discovery) => discovery.candidates().unwrap_or_default().into_iter()
                .flat_map(|candidate| candidate.addresses).collect(),
            None => Vec::new(),
        }
    };
    tauri::async_runtime::spawn_blocking(move || {
        std::thread::scope(|scope| {
            let handles: Vec<_> = targets.into_iter().map(|(fingerprint, address)| {
                let lan = &lan_addresses;
                scope.spawn(move || {
                    let host = |value: &str| value.rsplit_once(':').map(|(host, _)| host.to_owned()).unwrap_or_else(|| value.to_owned());
                    let visible_on_lan = address.as_deref().is_some_and(|saved| {
                        let saved_host = host(saved);
                        lan.iter().any(|entry| host(entry) == saved_host)
                    });
                    let reachable = address.as_deref().is_some_and(address_is_reachable);
                    ContactPresence { fingerprint, online: visible_on_lan || reachable }
                })
            }).collect();
            handles.into_iter().filter_map(|handle| handle.join().ok()).collect()
        })
    }).await.map_err(|_| "在线状态查询异常".to_owned())
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

/// Opens (or focuses) the standalone chat-records window for one conversation.
#[tauri::command]
pub fn v1_open_records_window(app: tauri::AppHandle, kind: String, id: String) -> Result<(), String> {
    use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
    let label = match kind.as_str() {
        "self" => "records-self".to_string(),
        "direct" => format!("records-direct-{}", id),
        "group" => format!("records-group-{}", id),
        _ => return Err("会话类型无效".to_owned()),
    };
    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.unminimize();
        let _ = window.set_focus();
        return Ok(());
    }
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/32x32.png"))
        .map_err(|error| error.to_string())?;
    WebviewWindowBuilder::new(&app, &label, WebviewUrl::App(format!("records.html?kind={kind}&id={id}").into()))
        .title("聊天记录").inner_size(880.0, 620.0).min_inner_size(600.0, 420.0)
        .icon(icon).map_err(|error| error.to_string())?
        .build().map_err(|error| {
            crate::log_error!("创建聊天记录窗口失败({label}): {error}");
            error.to_string()
    })?;
    crate::log_info!("已创建聊天记录窗口: {label}");
    Ok(())
}

/// Closes the webview window that asked for it (records windows).
#[tauri::command]
pub fn v1_close_current_window(window: tauri::WebviewWindow) -> Result<(), String> {
    window.close().map_err(|error| {
        crate::log_error!("关闭聊天记录窗口失败({}): {error}", window.label());
        error.to_string()
    })
}

/// Deletes selected local messages of one conversation by id.
#[tauri::command]
pub fn v1_delete_messages(state: tauri::State<'_, AppState>, kind: String, id: String, ids: Vec<String>) -> Result<u64, String> {
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .delete_messages(&kind, &id, &ids).map(|count| count as u64).map_err(|_| "无法删除所选消息".to_owned())
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

/// One row of the cross-conversation search.
#[tauri::command]
pub fn v1_search_all(state: tauri::State<'_, AppState>, query: String, category: String, limit: u32)
    -> Result<Vec<crate::storage::SearchHit>, String> {
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .search_all(&state.identity.fingerprint(), &query, &category, limit)
        .map_err(|_| "搜索条件无效".to_owned())
}

#[derive(serde::Serialize)]
pub struct ExportResult {
    pub conversation_name: String,
    pub message_count: u64,
    pub path: String,
}

/// Writes a plain-text transcript chosen by the user. The text is unencrypted,
/// so the UI must ask before exporting and mention that in its confirmation.
#[tauri::command]
pub fn v1_export_chat(state: tauri::State<'_, AppState>, kind: String, id: String, path: String)
    -> Result<ExportResult, String> {
    if path.is_empty() || path.len() > 4096 || path.contains('\0') {
        return Err("导出路径无效".into());
    }
    let export = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .export_conversation(&state.identity.fingerprint(), &kind, &id)
        .map_err(|_| "会话不可导出：可能不存在、已退出该群或消息数量过多".to_owned())?;
    if export.text.len() > 64 * 1024 * 1024 {
        return Err("聊天记录过大，暂不支持一次导出".into());
    }
    std::fs::write(&path, export.text.as_bytes()).map_err(|_| "无法写入导出文件".to_owned())?;
    Ok(ExportResult { conversation_name: export.conversation_name, message_count: export.message_count, path })
}

#[derive(serde::Serialize)]
pub struct LockStatus {
    pub has_password: bool,
    pub locked: bool,
    pub idle_minutes: u32,
}

fn lock_status(state: &tauri::State<'_, AppState>) -> Result<LockStatus, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    Ok(LockStatus {
        has_password: crate::lock::is_configured(&store)?,
        locked: state.locked.load(Ordering::Acquire),
        idle_minutes: crate::lock::idle_minutes(&store)?,
    })
}

/// Raw OS idle time in seconds; the UI uses it so automatic locking also works
/// when Qbird is unfocused or minimized.
#[tauri::command]
pub fn v1_system_idle_seconds() -> Option<u64> {
    crate::lock::system_idle_seconds()
}
#[tauri::command]
pub fn v1_lock_status(state: tauri::State<'_, AppState>) -> Result<LockStatus, String> {
    lock_status(&state)
}

#[tauri::command]
pub fn v1_lock_now(state: tauri::State<'_, AppState>) -> Result<LockStatus, String> {
    state.locked.store(true, Ordering::Release);
    lock_status(&state)
}

/// Requires the current password when one exists. An empty `next` removes the lock.
#[tauri::command]
pub fn v1_set_lock_password(state: tauri::State<'_, AppState>, current: Option<String>, next: String)
    -> Result<LockStatus, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    if crate::lock::is_configured(&store)? {
        let Some(current) = current.filter(|value| !value.is_empty()) else {
            return Err("请输入当前应用锁密码".into());
        };
        if !crate::lock::verify(&store, &current)? { return Err("当前应用锁密码错误".into()); }
    }
    if next.is_empty() {
        crate::lock::remove_password(&store)?;
        state.locked.store(false, Ordering::Release);
    } else {
        crate::lock::set_password(&store, &next)?;
    }
    drop(store);
    lock_status(&state)
}

/// Unlocking without a configured password is allowed; the lock then only hides the UI.
#[tauri::command]
pub fn v1_unlock(state: tauri::State<'_, AppState>, password: Option<String>) -> Result<LockStatus, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    if crate::lock::is_configured(&store)? {
        let password = password.unwrap_or_default();
        if !crate::lock::verify(&store, &password)? { return Err("应用锁密码错误".into()); }
    }
    drop(store);
    state.locked.store(false, Ordering::Release);
    lock_status(&state)
}

#[tauri::command]
pub fn v1_set_lock_idle(state: tauri::State<'_, AppState>, minutes: u32) -> Result<LockStatus, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    if !crate::lock::set_idle_minutes(&store, minutes)? {
        return Err(format!("空闲锁定时间需为 0–{} 分钟（0 表示关闭）", crate::lock::MAX_IDLE_MINUTES));
    }
    drop(store);
    lock_status(&state)
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

fn unix_seconds() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64).unwrap_or(0)
}

#[tauri::command]
pub fn v1_list_left_groups(state: tauri::State<'_, AppState>) -> Result<Vec<crate::storage::GroupSummary>, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    store.list_left_groups(&state.identity.fingerprint()).map_err(|_| "无法读取已退出的群聊".to_owned())
}

/// Opens read-only history for a group the user left. Returns false when there is no record.
#[tauri::command]
pub fn v1_open_group_history(state: tauri::State<'_, AppState>, group_id: String) -> Result<bool, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    Ok(store.group_membership(&group_id, &state.identity.fingerprint())
        .map_err(|_| "无法读取群聊记录".to_owned())?.is_some())
}

#[tauri::command]
pub fn v1_group_invitations(state: tauri::State<'_, AppState>) -> Result<Vec<crate::storage::GroupInvitation>, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    store.pending_group_invitations(&state.identity.fingerprint()).map_err(|_| "无法读取群邀请".to_owned())
}

#[tauri::command]
pub fn v1_accept_group_invitation(state: tauri::State<'_, AppState>, id: String) -> Result<Option<String>, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    store.accept_group_invitation(&id, &state.identity.fingerprint(), unix_seconds()).map_err(|_| "无法加入群聊".to_owned())
}

#[tauri::command]
pub fn v1_decline_group_invitation(state: tauri::State<'_, AppState>, id: String) -> Result<bool, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    store.decline_group_invitation(&id, &state.identity.fingerprint(), unix_seconds()).map_err(|_| "无法拒绝群邀请".to_owned())
}

#[derive(serde::Serialize)]
pub struct GroupSettings { pub auto_join: bool }

#[tauri::command]
pub fn v1_group_settings(state: tauri::State<'_, AppState>) -> Result<GroupSettings, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    Ok(GroupSettings { auto_join: store.auto_join_groups().map_err(|_| "无法读取群设置".to_owned())? })
}

#[tauri::command]
pub fn v1_set_group_auto_join(state: tauri::State<'_, AppState>, auto_join: bool) -> Result<GroupSettings, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    store.set_auto_join_groups(auto_join).map_err(|_| "无法保存群设置".to_owned())?;
    Ok(GroupSettings { auto_join })
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

/// One entry point for the conversation list's right-click menu: `kind` is
/// "self", "direct" or "group" and `id` is the peer fingerprint or group id.
#[tauri::command]
pub fn v1_set_conversation_muted(state: tauri::State<'_, AppState>, kind: String, id: String, muted: bool) -> Result<bool, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    match kind.as_str() {
        "self" => store.set_self_muted(muted).map_err(|_| "无法更新免打扰".to_owned()),
        "direct" => store.set_direct_muted(&id, muted).map_err(|_| "无法更新免打扰".to_owned()),
        "group" => store.set_group_muted(&id, &state.identity.fingerprint(), muted).map_err(|_| "无法更新免打扰".to_owned()),
        _ => Err("未知的会话类型".to_owned()),
    }
}

#[tauri::command]
pub fn v1_mark_conversation_unread(state: tauri::State<'_, AppState>, kind: String, id: String) -> Result<bool, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    match kind.as_str() {
        "direct" => store.mark_direct_unread(&id).map_err(|_| "无法标记未读".to_owned()),
        "group" => store.mark_group_unread(&id, &state.identity.fingerprint()).map_err(|_| "无法标记未读".to_owned()),
        _ => Err("自聊不能标记未读".to_owned()),
    }
}

#[tauri::command]
pub fn v1_queue_group(state: tauri::State<'_, AppState>, group_id: String, body: String,
    reply_to: Option<String>, mentions: Option<Vec<String>>) -> Result<crate::storage::GroupMessage, String> {
    let sender = state.identity.fingerprint();
    let created_at = unix_now()?;
    let mentions = mentions.unwrap_or_default();
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .queue_group(&sender, &group_id, &body, reply_to.as_deref(), &mentions, created_at)
        .map_err(|_| "消息为空、过长、引用的消息不存在或已不在该群中".to_owned())
}

/// Queue a file into a group; hashing and size checks run off the UI thread.
/// One bubble of images with an optional caption; at most nine images per bubble.
pub const MAX_BATCH_IMAGES: usize = 9;

fn image_paths(paths: &[String]) -> Result<Vec<std::path::PathBuf>, String> {
    if paths.is_empty() || paths.len() > MAX_BATCH_IMAGES { return Err(format!("每次可发送 1–{MAX_BATCH_IMAGES} 张图片")); }
    let mut out = Vec::with_capacity(paths.len());
    for path in paths {
        if path.len() > 4096 { return Err("图片路径无效".to_owned()); }
        let source = std::path::PathBuf::from(path);
        let name = source.file_name().and_then(|name| name.to_str()).ok_or_else(|| "无法读取文件名".to_owned())?.to_owned();
        if !crate::files::is_image(&name) { return Err(format!("不是支持的图片格式：{name}")); }
        out.push(source);
    }
    Ok(out)
}

fn batch_caption(caption: &str) -> Result<String, String> {
    let caption = caption.trim_matches(['\r', '\n']).to_owned();
    if caption.chars().count() > crate::protocol::MAX_BATCH_CAPTION { return Err("图片说明过长".to_owned()); }
    Ok(caption)
}

#[tauri::command]
pub async fn v1_queue_direct_images(state: tauri::State<'_, AppState>, peer: String, paths: Vec<String>, caption: String)
    -> Result<Vec<crate::storage::DirectMessage>, String> {
    let store = Arc::clone(&state.store);
    let sender = state.identity.fingerprint();
    tauri::async_runtime::spawn_blocking(move || {
        let sources = image_paths(&paths)?;
        let caption = batch_caption(&caption)?;
        let created_at = unix_now()?;
        let batch = store.lock().map_err(|_| "本地存储不可用".to_owned())?.new_batch_id().map_err(|_| "无法创建图片批次".to_owned())?;
        let mut messages = Vec::with_capacity(sources.len());
        for source in sources {
            let name = source.file_name().and_then(|name| name.to_str()).ok_or_else(|| "无法读取文件名".to_owned())?.to_owned();
            let size = crate::files::file_size(&source).map_err(|_| "无法读取图片：请确认它是本地普通文件".to_owned())?;
            if size == 0 { return Err("暂不支持空文件".to_owned()); }
            if size > crate::protocol::MAX_FILE_BYTES { return Err("图片超过 512 MiB 限制".to_owned()); }
            let sha256 = crate::files::sha256_file(&source).map_err(|_| "无法计算校验值".to_owned())?;
            let message = store.lock().map_err(|_| "本地存储不可用".to_owned())?
                .queue_direct_file(&sender, &peer, &name, size, &sha256, &source, created_at)
                .map_err(|_| "对方须为已确认联系人".to_owned())?;
            store.lock().map_err(|_| "本地存储不可用".to_owned())?
                .attach_image_batch(&message.id, &batch, &caption, created_at)
                .map_err(|_| "无法保存图片气泡".to_owned())?;
            messages.push(message);
        }
        Ok(messages)
    }).await.map_err(|_| "图片入队任务异常".to_owned())?
}

#[tauri::command]
pub async fn v1_queue_group_images(state: tauri::State<'_, AppState>, group_id: String, paths: Vec<String>, caption: String)
    -> Result<Vec<crate::storage::GroupMessage>, String> {
    let store = Arc::clone(&state.store);
    let sender = state.identity.fingerprint();
    tauri::async_runtime::spawn_blocking(move || {
        let sources = image_paths(&paths)?;
        let caption = batch_caption(&caption)?;
        let created_at = unix_now()?;
        let batch = store.lock().map_err(|_| "本地存储不可用".to_owned())?.new_batch_id().map_err(|_| "无法创建图片批次".to_owned())?;
        let mut messages = Vec::with_capacity(sources.len());
        for source in sources {
            let name = source.file_name().and_then(|name| name.to_str()).ok_or_else(|| "无法读取文件名".to_owned())?.to_owned();
            let size = crate::files::file_size(&source).map_err(|_| "无法读取图片：请确认它是本地普通文件".to_owned())?;
            if size == 0 { return Err("暂不支持空文件".to_owned()); }
            if size > crate::protocol::MAX_FILE_BYTES { return Err("图片超过 512 MiB 限制".to_owned()); }
            let sha256 = crate::files::sha256_file(&source).map_err(|_| "无法计算校验值".to_owned())?;
            let message = store.lock().map_err(|_| "本地存储不可用".to_owned())?
                .queue_group_file(&sender, &group_id, &name, size, &sha256, &source, created_at)
                .map_err(|_| "群不存在或你不是活跃成员".to_owned())?;
            store.lock().map_err(|_| "本地存储不可用".to_owned())?
                .attach_image_batch(&message.id, &batch, &caption, created_at)
                .map_err(|_| "无法保存图片气泡".to_owned())?;
            messages.push(message);
        }
        Ok(messages)
    }).await.map_err(|_| "图片入队任务异常".to_owned())?
}

/// Batches of the conversation so the UI can render one bubble per batch.
#[tauri::command]
pub fn v1_image_batches(state: tauri::State<'_, AppState>, kind: String, target: String)
    -> Result<Vec<crate::storage::ImageBatch>, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    store.list_image_batches(&kind, &target).map_err(|_| "无法读取图片气泡信息".to_owned())
}

#[tauri::command]
pub async fn v1_queue_group_file(state: tauri::State<'_, AppState>, group_id: String, path: String)
    -> Result<crate::storage::GroupMessage, String> {
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
        let created_at = unix_now()?;
        store.lock().map_err(|_| "本地存储不可用".to_owned())?
            .queue_group_file(&sender, &group_id, &name, size, &sha256, &source, created_at)
            .map_err(|_| "群不存在、你不是活跃成员，或文件名、大小无效".to_owned())
    }).await.map_err(|_| "文件入队任务异常".to_owned())?
}

#[tauri::command]
pub fn v1_group_file_progress(state: tauri::State<'_, AppState>, group_id: String)
    -> Result<Vec<crate::storage::GroupFileProgress>, String> {
    let self_id = state.identity.fingerprint();
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .group_file_progress(&group_id, &self_id).map_err(|_| "无法读取群文件状态".to_owned())
}

/// Opens one completed group attachment in the file panel or file manager.
#[tauri::command]
pub fn v1_group_attachment_path(state: tauri::State<'_, AppState>, group_id: String, message_id: String)
    -> Result<String, String> {
    let self_id = state.identity.fingerprint();
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    let progress = store.group_file_transfer(&message_id).map_err(|_| "无法读取文件记录".to_owned())?
        .ok_or_else(|| "文件记录不存在".to_owned())?;
    if progress.status != "completed" { return Err("文件尚未接收完成".into()); }
    let member = store.group_file_progress(&group_id, &self_id).map_err(|_| "无法读取群文件状态".to_owned())?;
    if !member.iter().any(|item| item.message_id == message_id) { return Err("无权访问该群文件".into()); }
    progress.target_path.ok_or_else(|| "本地附件路径缺失".to_owned())
}

/// Reads the current announcement of one group.
#[tauri::command]
pub fn v1_group_notice(state: tauri::State<'_, AppState>, group_id: String) -> Result<Option<String>, String> {
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .group_notice(&group_id).map_err(|_| "无法读取群公告".to_owned())
}

/// Publishes or clears (empty string) the group announcement; owner only.
#[tauri::command]
pub fn v1_set_group_notice(state: tauri::State<'_, AppState>, group_id: String, notice: String) -> Result<(), String> {
    let actor = state.identity.fingerprint();
    let now = unix_now()?;
    let updated = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .set_group_notice(&group_id, &actor, &notice, now).map_err(|_| "无法保存群公告".to_owned())?;
    if updated { Ok(()) } else { Err("只有群主可以设置群公告".into()) }
}

#[tauri::command]
pub fn v1_transfer_group_owner(state: tauri::State<'_, AppState>, group_id: String, member: String) -> Result<(), String> {
    let actor = state.identity.fingerprint();
    let now = unix_now()?;
    let transferred = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .transfer_group_owner(&group_id, &actor, &member, now).map_err(|_| "无法转让群主".to_owned())?;
    if transferred { Ok(()) } else { Err("只有群主可以把群主转让给已确认的群内成员".into()) }
}

#[tauri::command]
pub fn v1_dissolve_group(state: tauri::State<'_, AppState>, group_id: String) -> Result<(), String> {
    let actor = state.identity.fingerprint();
    let now = unix_now()?;
    let dissolved = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .dissolve_group(&group_id, &actor, now).map_err(|_| "无法解散群聊".to_owned())?;
    if dissolved { Ok(()) } else { Err("只有群主可以解散该群".into()) }
}

#[tauri::command]
pub fn v1_search_direct(state: tauri::State<'_, AppState>, peer: String, query: String,
    category: String, limit: u32, offset: u32) -> Result<Vec<crate::storage::DirectMessage>, String> {
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .search_direct(&peer, &query, &category, limit, offset).map_err(|_| "搜索条件无效".to_owned())
}

#[tauri::command]
pub fn v1_search_group(state: tauri::State<'_, AppState>, group_id: String, query: String,
    category: String, member: Option<String>, limit: u32, offset: u32) -> Result<Vec<crate::storage::GroupMessage>, String> {
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .search_group_messages(&group_id, &query, &category, member.as_deref(), limit, offset)
        .map_err(|_| "搜索条件无效".to_owned())
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

/// Avatar bytes for one group; only readable by active members.
#[tauri::command]
pub fn v1_group_avatar(state: tauri::State<'_, AppState>, group_id: String) -> Result<Option<Vec<u8>>, String> {
    let self_id = state.identity.fingerprint();
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    let member = store.list_group_members(&group_id).map_err(|_| "无法读取群成员".to_owned())?
        .into_iter().any(|item| item.member == self_id && item.active);
    if !member { return Err("已不在该群中".into()); }
    store.group_avatar(&group_id).map_err(|_| "无法读取群头像".to_owned())
}

/// Owner-only. An empty payload removes the avatar; the bytes are validated as a square PNG.
#[tauri::command]
pub fn v1_set_group_avatar(state: tauri::State<'_, AppState>, group_id: String, bytes: Vec<u8>) -> Result<(), String> {
    let self_id = state.identity.fingerprint();
    let now = unix_now()?;
    let stored = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .set_group_avatar(&self_id, &group_id, if bytes.is_empty() { None } else { Some(&bytes) }, now)
        .map_err(|_| "群头像需为不超过 96 KiB 的正方形 PNG".to_owned())?;
    if stored { Ok(()) } else { Err("只有群主可以设置群头像".into()) }
}

/// Pulls the avatar from the owner when another member opens the group.
#[tauri::command]
pub async fn v1_fetch_group_avatar(state: tauri::State<'_, AppState>, group_id: String) -> Result<bool, String> {
    let self_id = state.identity.fingerprint();
    let (owner, address) = {
        let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
        let members = store.list_group_members(&group_id).map_err(|_| "无法读取群成员".to_owned())?;
        let Some(owner) = members.iter().find(|item| item.role == "owner").map(|item| item.member.clone()) else {
            return Err("群不存在".into());
        };
        if owner == self_id { return Ok(false); }
        if !members.iter().any(|item| item.member == self_id && item.active) { return Err("已不在该群中".into()); }
        let address = store.contact_address(&owner).ok().flatten();
        (owner, address)
    };
    let Some(address) = address.and_then(|value| crate::address::parse_manual_address(&value).ok()) else {
        return Ok(false);
    };
    let store = Arc::clone(&state.store);
    let identity = Arc::clone(&state.identity);
    tauri::async_runtime::spawn_blocking(move || {
        match crate::chat_transport::fetch_group_avatar(&identity, &owner, address, &group_id) {
            Ok(bytes) => {
                let now = unix_now()?;
                store.lock().map_err(|_| "本地存储不可用".to_owned())?
                    .receive_group_avatar(&self_id, &owner, &group_id, bytes.as_deref(), now)
                    .map_err(|_| "无法保存群头像".to_owned())?;
                Ok(true)
            }
            Err(_) => Ok(false),
        }
    }).await.map_err(|_| "群头像任务异常".to_owned())?
}

#[derive(serde::Serialize)]
pub struct GroupSearchHit {
    pub group_id: String,
    pub name: String,
    pub owner: String,
    pub member_count: u32,
    pub reported_by: String,
    pub owner_known: bool,
}

/// LAN search asks confirmed contacts that already have a saved address. Calls are
/// best effort and bounded: offline peers simply contribute no results.
#[tauri::command]
pub async fn v1_search_groups(state: tauri::State<'_, AppState>, query: String)
    -> Result<Vec<GroupSearchHit>, String> {
    let query = query.trim().to_owned();
    if query.is_empty() || query.len() > crate::protocol::MAX_SEARCH_QUERY {
        return Err(format!("请输入 1–{} 个字符的群名关键词", crate::protocol::MAX_SEARCH_QUERY));
    }
    let store = Arc::clone(&state.store);
    let identity = Arc::clone(&state.identity);
    tauri::async_runtime::spawn_blocking(move || {
        let self_id = identity.fingerprint();
        let mut joined = std::collections::BTreeSet::new();
        let mut known = std::collections::BTreeSet::new();
        let mut targets = Vec::new();
        {
            let store = store.lock().map_err(|_| "本地存储不可用".to_owned())?;
            for group in store.list_groups(&self_id).map_err(|_| "无法读取群聊".to_owned())? {
                joined.insert(group.id);
            }
            for contact in store.list_contacts().map_err(|_| "无法读取联系人".to_owned())? {
                if contact.trust_state != "verified" { continue; }
                known.insert(contact.fingerprint.clone());
                if targets.len() >= 16 { continue; }
                if let Ok(Some(address)) = store.contact_address(&contact.fingerprint) {
                    if let Ok(parsed) = crate::address::parse_manual_address(&address) {
                        targets.push((contact.fingerprint, parsed));
                    }
                }
            }
        }
        let hits = crate::chat_transport::search_groups(&identity, &targets, &query);
        Ok(hits.into_iter().filter(|(_, group)| !joined.contains(&group.id)).map(|(reported_by, group)| GroupSearchHit {
            owner_known: known.contains(&group.owner),
            reported_by,
            group_id: group.id,
            name: group.name,
            owner: group.owner,
            member_count: group.member_count,
        }).collect())
    }).await.map_err(|_| "群名搜索任务异常".to_owned())?
}

#[tauri::command]
pub fn v1_request_join_group(state: tauri::State<'_, AppState>, group_id: String, name: String, owner: String)
    -> Result<crate::storage::JoinRequest, String> {
    let self_id = state.identity.fingerprint();
    let now = unix_now()?;
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .queue_join_request(&self_id, &group_id, &name, &owner, now)
        .map_err(|_| "群主须为本机已确认联系人，且你尚未在该群中".to_owned())
}

#[derive(serde::Serialize)]
pub struct JoinRequests {
    pub incoming: Vec<crate::storage::JoinRequest>,
    pub outgoing: Vec<crate::storage::JoinRequest>,
}

#[tauri::command]
pub fn v1_join_requests(state: tauri::State<'_, AppState>, group_id: Option<String>) -> Result<JoinRequests, String> {
    let self_id = state.identity.fingerprint();
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    Ok(JoinRequests {
        incoming: store.list_join_requests(&self_id, group_id.as_deref()).map_err(|_| "无法读取入群申请".to_owned())?,
        outgoing: store.list_out_join_requests(&self_id).map_err(|_| "无法读取已提交申请".to_owned())?,
    })
}

#[tauri::command]
pub fn v1_resolve_join_request(state: tauri::State<'_, AppState>, id: String, accept: bool) -> Result<(), String> {
    let self_id = state.identity.fingerprint();
    let now = unix_now()?;
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    let Some((group_id, applicant)) = store.resolve_join_request(&self_id, &id, accept)
        .map_err(|_| "无法更新入群申请".to_owned())? else {
        return Err("申请不存在或已处理".into());
    };
    if accept {
        if !store.add_group_member(&group_id, &self_id, &applicant, now).map_err(|_| "无法加入成员".to_owned())? {
            return Err("对方须仍是已确认联系人且不在群中".into());
        }
    } else {
        store.queue_join_reject(&self_id, &group_id, &applicant, now).map_err(|_| "无法通知申请人".to_owned())?;
    }
    Ok(())
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

fn local_contact_address(state: &AppState) -> Option<String> {
    let port = state.probe_service.lock().ok().and_then(|guard| guard.as_ref().map(|service| service.port()))?;
    let ip = crate::diagnostics::local_ipv4_addresses().into_iter().next()?;
    Some(format!("{ip}:{port}"))
}

fn announce_status(identity: Arc<Identity>, store: Arc<Mutex<Store>>, peer: &str, address: String,
    kind: &'static str, reason: Option<String>) {
    let _ = peer;
    let Some(parsed) = crate::address::parse_manual_address(&address).ok() else { return };
    let announce = store.lock().ok().and_then(|store| store.get_profile().ok())
        .map(|profile| crate::probe::ContactAnnounce {
            nickname: profile.nickname,
            username: Some(profile.username).filter(|value| !value.is_empty()),
            kind: Some(kind.to_owned()),
            address: None,
            reason,
        });
    if let Some(announce) = announce {
        std::thread::spawn(move || { let _ = crate::probe::announce_contact(&identity, parsed, &announce); });
    }
}

/// Files a friend request and sends it; `accepted` means the peer auto-accepted.
#[tauri::command]
pub fn v1_request_contact(state: tauri::State<'_, AppState>, peer: String) -> Result<String, String> {
    let store = Arc::clone(&state.store);
    let profile = store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .get_profile().map_err(|_| "无法读取资料".to_owned())?;
    let now = unix_now()?;
    let address = store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .contact_address(&peer).map_err(|_| "无法读取地址".to_owned())?;
    let Some(address) = address else { return Err("还没有对方地址，请先探测该用户".into()); };
    {
        let store = store.lock().map_err(|_| "本地存储不可用".to_owned())?;
        if store.is_verified(&peer).map_err(|_| "本地存储不可用".to_owned())? {
            return Ok("verified".into());
        }
        store.create_contact_request("out", &peer, &profile.nickname,
            Some(profile.username.as_str()).filter(|value| !value.is_empty()),
            Some(address.as_str()), now).map_err(|_| "无法保存申请".to_owned())?;
    }
    let local = local_contact_address(&state);
    let identity = Arc::clone(&state.identity);
    let target = address.clone();
    let nickname = profile.nickname.clone();
    let username = profile.username.clone();
    let accepted = std::thread::spawn(move || {
        let announce = crate::probe::ContactAnnounce {
            nickname,
            username: Some(username).filter(|value| !value.is_empty()),
            kind: Some("request".into()),
            address: local,
            reason: None,
        };
        crate::address::parse_manual_address(&target).ok()
            .and_then(|parsed| crate::probe::announce_contact(&identity, parsed, &announce).ok())
            .unwrap_or(false)
    }).join().unwrap_or(false);
    if accepted {
        store.lock().map_err(|_| "本地存储不可用".to_owned())?
            .resolve_contact_request_by_peer(&peer, "out", true, now).map_err(|_| "无法更新申请".to_owned())?;
        return Ok("accepted".into());
    }
    Ok("pending".into())
}

#[tauri::command]
pub fn v1_contact_requests(state: tauri::State<'_, AppState>, direction: String)
    -> Result<Vec<crate::storage::ContactRequest>, String> {
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .list_contact_requests(&direction).map_err(|_| "无法读取好友申请".to_owned())
}

#[tauri::command]
pub fn v1_respond_contact_request(state: tauri::State<'_, AppState>, id: String, accept: bool) -> Result<(), String> {
    let now = unix_now()?;
    let request = {
        let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
        store.resolve_contact_request(&id, accept, now).map_err(|_| "无法处理申请".to_owned())?
    };
    if let Some(request) = request {
        if let Some(address) = request.address.clone() {
            announce_status(Arc::clone(&state.identity), Arc::clone(&state.store), &request.peer, address,
                if accept { "accept" } else { "reject" },
                if accept { None } else { Some("rejected".into()) });
        }
    }
    Ok(())
}

#[tauri::command]
pub fn v1_withdraw_contact_request(state: tauri::State<'_, AppState>, peer: String) -> Result<bool, String> {
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .withdraw_contact_request(&peer).map_err(|_| "无法撤回申请".to_owned())
}

#[tauri::command]
pub fn v1_lan_visibility(state: tauri::State<'_, AppState>) -> Result<bool, String> {
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .lan_visible().map_err(|_| "无法读取可见性".to_owned())
}

#[tauri::command]
pub fn v1_set_lan_visibility(state: tauri::State<'_, AppState>, visible: bool) -> Result<bool, String> {
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .set_lan_visible(visible).map_err(|_| "无法保存可见性".to_owned())?;
    if let Ok(mut guard) = state.probe_service.lock() {
        if let Some(service) = guard.as_mut() { service.set_visible(visible); }
    }
    Ok(visible)
}

/// Withdraws one message inside the two-minute window.
#[tauri::command]
pub fn v1_recall_message(state: tauri::State<'_, AppState>, kind: String, id: String, group_id: Option<String>) -> Result<String, String> {
    let me = state.identity.fingerprint();
    let now = unix_now()?;
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    let result = match kind.as_str() {
        "direct" => store.recall_direct(&me, &id, now),
        "group" => {
            let Some(group_id) = group_id else { return Err("缺少群标识".into()); };
            store.recall_group(&me, &group_id, &id, now)
        }
        _ => return Err("未知的消息类型".into()),
    }.map_err(|_| "无法撤回消息".to_owned())?;
    Ok(match result {
        crate::storage::RecallResult::Recalled { .. } => "recalled".into(),
        crate::storage::RecallResult::TooLate => "too_late".into(),
        crate::storage::RecallResult::NotYours => "not_yours".into(),
        crate::storage::RecallResult::Missing => "missing".into(),
        crate::storage::RecallResult::Already => "already".into(),
        crate::storage::RecallResult::NotMember => "not_member".into(),
    })
}

#[tauri::command]
pub fn v1_cancel_file_transfer(state: tauri::State<'_, AppState>, message_id: String) -> Result<bool, String> {
    let now = unix_now()?;
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .cancel_file(&message_id, now).map_err(|_| "无法取消文件".to_owned())
}

#[tauri::command]
pub fn v1_list_dissolved_groups(state: tauri::State<'_, AppState>)
    -> Result<Vec<crate::storage::DissolvedGroup>, String> {
    let me = state.identity.fingerprint();
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .list_dissolved_groups(&me).map_err(|_| "无法读取无效群".to_owned())
}

#[tauri::command]
pub fn v1_delete_group_history(state: tauri::State<'_, AppState>, group_id: String) -> Result<bool, String> {
    state.store.lock().map_err(|_| "本地存储不可用".to_owned())?
        .delete_group_history(&group_id).map_err(|_| "无法删除群记录".to_owned())
}

#[derive(serde::Serialize)]
pub struct NoticeSettings {
    pub mode: String,
    pub raise_window: bool,
    pub preview: bool,
}

fn read_notice_settings(store: &crate::storage::Store) -> Result<NoticeSettings, String> {
    let mode = store.get_setting("notice_mode").map_err(|_| "无法读取提醒设置".to_owned())?
        .unwrap_or_else(|| "popup".into());
    let raise_window = store.get_setting("notice_raise_window").map_err(|_| "无法读取提醒设置".to_owned())?
        .as_deref() == Some("1");
    let preview = store.get_setting("notice_preview").map_err(|_| "无法读取提醒设置".to_owned())?
        .as_deref() != Some("0");
    Ok(NoticeSettings { mode, raise_window, preview })
}

#[tauri::command]
pub fn v1_notice_settings(state: tauri::State<'_, AppState>) -> Result<NoticeSettings, String> {
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    read_notice_settings(&store)
}

#[tauri::command]
pub fn v1_set_notice_settings(state: tauri::State<'_, AppState>, mode: String, raise_window: bool, preview: bool)
    -> Result<NoticeSettings, String> {
    if !matches!(mode.as_str(), "popup" | "tray" | "off") { return Err("未知的提醒方式".into()); }
    let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
    store.set_setting("notice_mode", Some(&mode)).map_err(|_| "无法保存提醒设置".to_owned())?;
    store.set_setting("notice_raise_window", Some(if raise_window { "1" } else { "0" })).map_err(|_| "无法保存提醒设置".to_owned())?;
    store.set_setting("notice_preview", Some(if preview { "1" } else { "0" })).map_err(|_| "无法保存提醒设置".to_owned())?;
    read_notice_settings(&store)
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
        use std::net::TcpStream;
        let alice = AppState::initialize(&test_dir("chat-alice")).unwrap();
        let bob = AppState::initialize(&test_dir("chat-bob")).unwrap();
        let alice_id = alice.identity.fingerprint();
        let bob_id = bob.identity.fingerprint();
        for (state, peer) in [(&alice, &bob_id), (&bob, &alice_id)] {
            let store = state.store.lock().unwrap();
            store.add_candidate(peer, "contact").unwrap();
            store.confirm_candidate(peer).unwrap();
        }
        bob.start_listener(0, true).unwrap();
        let port = bob.probe_service.lock().unwrap().as_ref().unwrap().port();
        bob.start_listener(port, true).unwrap();
        assert!(bob.start_listener(port, false).is_err());
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
