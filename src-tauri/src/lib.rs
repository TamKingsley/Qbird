pub mod address;
pub mod app_state;
pub mod avatar;
pub mod base64;
pub mod network_info;
pub mod notice;
mod tray_lang;
pub mod plugins;
pub mod backup;
mod auto_address;
pub mod certificate;
pub mod capture;
pub mod chat_transport;
pub mod chatfile;
pub mod diagnostics;
pub mod discovery;
pub mod files;
pub mod identity;
pub mod lock;
pub mod logging;
pub mod probe;
pub mod probe_service;
pub mod protocol;
pub mod retry_queue;
pub mod stickers;
pub mod scan;
pub mod storage;
pub mod tls;

use std::path::{Path, PathBuf};
use tauri::{Emitter, Manager, WindowEvent};

/// Keeps the tray icon alive: the icon is removed as soon as the last handle drops.
pub struct TrayHandle(#[allow(dead_code)] tauri::tray::TrayIcon<tauri::Wry>);

pub(crate) fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Reads the saved close behavior. A missing or unreadable value falls back to
/// asking, so a corrupt setting can never trap the user inside the app.
fn saved_close_action(app: &tauri::AppHandle) -> String {
    let state = app.state::<app_state::AppState>();
    let Ok(store) = state.store.lock() else { return "ask".to_owned() };
    store.get_setting(app_state::CLOSE_ACTION_KEY).ok().flatten().unwrap_or_else(|| "ask".to_owned())
}

/// Tray icon with a menu to reopen the window or quit while it is hidden.
fn setup_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let lang = tray_lang::read_language_pub(app);
    let (tooltip, _, _) = tray_lang::tray_texts(&lang);
    let menu = tray_lang::build_tray_menu(app, &lang)?;
    let mut builder = TrayIconBuilder::with_id("qbird-tray")
        .tooltip(tooltip)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
        notice::set_alert_icon(icon);
    }
    let tray = builder.build(app)?;
    notice::set_tray(tray.clone());
    let _ = app.manage(TrayHandle(tray));
    Ok(())
}

/// The window is never closed by the default handler: either the saved behavior is
/// applied directly, or the frontend is asked to show the tray/quit prompt.
fn install_close_behavior(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("main") else { return };
    let handle = app.clone();
    window.on_window_event(move |event| {
        let WindowEvent::CloseRequested { api, .. } = event else { return };
        api.prevent_close();
        match saved_close_action(&handle).as_str() {
            "quit" => handle.exit(0),
            "tray" => {
                if let Some(window) = handle.get_webview_window("main") { let _ = window.hide(); }
            }
            _ => {
                show_main_window(&handle);
                let _ = handle.emit(app_state::CLOSE_REQUESTED_EVENT, ());
            }
        }
    });
}

/// Marker written into the install directory by the NSIS installer. Its presence
/// makes the executable keep its data in `<install dir>\data` instead of the
/// per-user app data directory, so the chosen install path is also the data path.
pub const PORTABLE_MARKER: &str = "qbird.portable";
const PORTABLE_DATA_DIR: &str = "data";

/// Resolve the data directory for release builds.
/// - A bare `qbird.exe` without the marker keeps using the per-user app data dir.
/// - An installed build (marker present) uses `<exe dir>\data`.
/// First start of an installed build copies identity and database from the old
/// per-user location once, without deleting the original files.
fn resolve_data_dir(exe_dir: &Path, default_dir: PathBuf) -> std::io::Result<PathBuf> {
    if !exe_dir.join(PORTABLE_MARKER).is_file() {
        return Ok(default_dir);
    }
    let data_dir = exe_dir.join(PORTABLE_DATA_DIR);
    std::fs::create_dir_all(&data_dir)?;
    // The install directory is the data directory, so an unwritable install must
    // fail loudly instead of silently losing every write later.
    let probe = data_dir.join(".qbird-write-test");
    std::fs::write(&probe, b"probe").map_err(|error| std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        format!("安装目录不可写：{}。请把 Qbird 安装到当前用户可写的目录后重试（{error}）", data_dir.display()),
    ))?;
    let _ = std::fs::remove_file(&probe);
    migrate_legacy_data(&default_dir, &data_dir);
    Ok(data_dir)
}

/// Copy a previous per-user identity and database into a fresh install data dir.
/// Attachments keep their absolute paths, so they stay readable from the old
/// location; they are deliberately not copied to keep first start fast.
fn migrate_legacy_data(legacy: &Path, target: &Path) {
    let migrated = target.join("qbird.sqlite3");
    if migrated.exists() { return; }
    let legacy_db = legacy.join("qbird.sqlite3");
    let legacy_identity = legacy.join("identity.bin");
    // A database without its identity would start in the "identity missing" error
    // state, so migrate only complete pairs.
    if !legacy_db.is_file() || !legacy_identity.is_file() { return; }
    let copy_stickers = || -> std::io::Result<()> {
        let source = legacy.join("stickers");
        let dest = target.join("stickers");
        if !source.is_dir() { return Ok(()); }
        std::fs::create_dir_all(&dest)?;
        for entry in std::fs::read_dir(&source)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                std::fs::copy(entry.path(), dest.join(entry.file_name()))?;
            }
        }
        Ok(())
    };
    crate::log_info!("从旧位置迁移数据: {} -> {}", legacy.display(), target.display());
    let result = std::fs::copy(&legacy_identity, target.join("identity.bin"))
        .and_then(|_| std::fs::copy(&legacy_db, &migrated))
        .and_then(|_| copy_stickers());
    match result {
        Ok(()) => {
            // Remember where the data came from so the UI can explain why a fresh
            // install already shows old records.
            let _ = std::fs::write(target.join("migration-source.txt"), legacy.to_string_lossy().as_bytes());
        }
        Err(_) => {
            // Never leave a half migration: a copied database without a usable identity
            // would block startup.
            let _ = std::fs::remove_file(target.join("identity.bin"));
            let _ = std::fs::remove_file(&migrated);
            crate::log_error!("迁移旧数据失败，已回滚");
        }
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let default_dir = app.path().app_data_dir()?;
            let resolved = match std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf)) {
                Some(exe_dir) => resolve_data_dir(&exe_dir, default_dir.clone())?,
                None => default_dir.clone(),
            };
            // Debug-only override for isolated desktop acceptance; release builds ignore it.
            #[cfg(debug_assertions)]
            let dir = std::env::var_os("QBIRD_TEST_DATA_DIR")
                .filter(|value| !value.is_empty()).map(std::path::PathBuf::from).unwrap_or(resolved);
            #[cfg(not(debug_assertions))]
            let dir = resolved;
            logging::init(&dir.join("logs"));
            crate::log_info!("数据目录: {}", dir.display());
            let state = match app_state::AppState::initialize(&dir) {
                Ok(state) => state,
                Err(error) => {
                    crate::log_error!("AppState 初始化失败: {error}");
                    return Err(Box::new(std::io::Error::other(error.to_string())));
                }
            };
            app.manage(state);
            crate::log_info!("启动监听与守护线程已安排");
            app_state::start_listener_on_launch(app.handle().clone());
            app_state::watch_listener(app.handle().clone());
            setup_tray(app.handle())?;
            notice::attach(app.handle().clone());
            install_close_behavior(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_state::identity_fingerprint,
            app_state::v1_identity_id,
            app_state::v1_profile,
            app_state::v1_set_profile,
            app_state::v1_set_username,
            app_state::v1_onboarding_state,
            app_state::v1_complete_onboarding,
            app_state::v1_network_settings,
            app_state::v1_file_accept_settings,
            app_state::v1_set_file_accept_policy,
            app_state::v1_remember_file_source,
            app_state::v1_forget_file_sources,
            app_state::v1_file_recv_settings,
            app_state::v1_set_file_recv_dir,
            app_state::v1_file_offer_requests,
            app_state::v1_group_file_offer_requests,
            app_state::v1_resolve_file_offer,
            app_state::v1_capture_screen,
            app_state::v1_begin_capture,
            app_state::v1_capture_current,
            app_state::v1_end_capture,
            app_state::v1_save_temp_image,
            app_state::v1_experiment_settings,
            app_state::v1_set_experiment_settings,
            app_state::v1_scan_blacklist,
            app_state::v1_set_scan_blacklist,
            app_state::v1_scan_range,
            app_state::v1_set_network_settings,
            app_state::v1_list_plugins,
            app_state::v1_install_plugin,
            app_state::v1_set_plugin_enabled,
            app_state::v1_remove_plugin,
            app_state::v1_plugin_preview,
            app_state::v1_local_preview,
            app_state::v1_attachment_base64,
            app_state::v1_group_attachment_base64,
            app_state::v1_attachment_bytes,
            app_state::v1_group_attachment_bytes,
            app_state::v1_open_external_url,
            app_state::v1_hover_settings,
            app_state::v1_set_hover_settings,
            app_state::v1_contact_network,
            app_state::v1_image_open_mode,
            app_state::v1_set_image_open_mode,
            app_state::v1_file_open_mode,
            app_state::v1_set_file_open_mode,
            app_state::v1_window_settings,
            app_state::v1_set_window_settings,
            app_state::v1_resolve_window_close,
            app_state::v1_composer_settings,
            app_state::v1_list_stickers,
            app_state::v1_add_sticker,
            app_state::v1_sticker_bytes,
            app_state::v1_sticker_path,
            app_state::v1_remove_sticker,
            app_state::v1_set_composer_settings,
            app_state::v1_export_chat_archive,
            app_state::v1_import_chat_archive,
            app_state::v1_team_profile,
            app_state::v1_set_team_name,
            app_state::v1_team_logo,
            app_state::v1_set_team_logo,
            app_state::v1_clear_team_logo,
            app_state::v1_avatar,
            app_state::v1_set_avatar,
            app_state::v1_clear_avatar,
            app_state::v1_read_image_file,
            app_state::v1_set_contact_profile,
            app_state::v1_search_contacts,
            app_state::v1_list_contacts,
            app_state::v1_accept_contact,
            app_state::v1_decline_contact,
            app_state::v1_contact_presence,
            app_state::v1_confirm_contact,
            app_state::v1_add_contact,
            app_state::v1_contact_sync_settings,
            app_state::v1_announce_contact,
            app_state::v1_set_contact_sync,
            app_state::v1_set_contact_address,
            app_state::v1_contact_address,
            app_state::v1_list_messages,
            app_state::v1_open_direct,
            app_state::v1_hide_direct,
            app_state::v1_list_direct,
            app_state::v1_queue_direct,
            app_state::v1_queue_direct_file,
            app_state::v1_file_progress,
            app_state::v1_read_attachment,
            app_state::v1_read_text_attachment,
            app_state::v1_read_group_text_attachment,
            app_state::v1_workspace_state,
            app_state::v1_set_workspace_state,
            app_state::v1_open_attachment,
            app_state::v1_open_group_attachment,
            app_state::v1_reveal_attachment,
            app_state::v1_storage_usage,
            app_state::v1_create_backup,
            app_state::v1_inspect_backup,
            app_state::v1_restore_backup,
            app_state::v1_set_storage_quota,
            app_state::v1_deliver_direct,
            app_state::v1_flush_direct,
            app_state::v1_open_self,
            app_state::v1_hide_self,
            app_state::v1_list_conversations,
            app_state::v1_delete_messages,
            app_state::v1_close_current_window,
            app_state::v1_open_records_window,
            app_state::v1_list_self_messages,
            app_state::v1_send_self,
            app_state::v1_discovery_snapshot,
            app_state::v1_refresh_listener,
            app_state::v1_lan_people,
            app_state::v1_start_probe_service,
            app_state::v1_start_chat_service,
            app_state::v1_stop_probe_service,
            app_state::v1_probe_status,
            app_state::v1_network_diagnostics,
            app_state::v1_network_repair,
            app_state::v1_space_diagnostics,
            app_state::v1_read_logs,
            app_state::v1_chat_display_settings,
            app_state::v1_list_export_targets,
            app_state::v1_remove_contact,
            app_state::v1_export_chat_archives,
            app_state::v1_set_chat_display,
            app_state::v1_migration_notice,
            app_state::v1_open_logs_dir,
            app_state::v1_create_group,
            app_state::v1_list_groups,
            app_state::v1_list_left_groups,
            app_state::v1_open_group_history,
            app_state::v1_group_invitations,
            app_state::v1_accept_group_invitation,
            app_state::v1_decline_group_invitation,
            app_state::v1_group_settings,
            app_state::v1_set_group_auto_join,
            app_state::v1_list_group_members,
            app_state::v1_open_group,
            app_state::v1_hide_group,
            app_state::v1_set_conversation_muted,
            app_state::v1_mark_conversation_unread,
            app_state::v1_queue_group,
            app_state::v1_queue_direct_images,
            app_state::v1_queue_group_images,
            app_state::v1_image_batches,
            app_state::v1_queue_group_file,
            app_state::v1_group_file_progress,
            app_state::v1_group_attachment_path,
            app_state::v1_read_group_attachment,
            app_state::v1_reveal_group_attachment,
            app_state::v1_transfer_group_owner,
            app_state::v1_group_notice,
            app_state::v1_set_group_notice,
            app_state::v1_dissolve_group,
            app_state::v1_search_direct,
            app_state::v1_search_group,
            app_state::v1_search_all,
            app_state::v1_export_chat,
            app_state::v1_system_idle_seconds,
            app_state::v1_lock_status,
            app_state::v1_lock_now,
            app_state::v1_set_lock_password,
            app_state::v1_set_lock_idle,
            app_state::v1_unlock,
            app_state::v1_list_group,
            app_state::v1_group_avatar,
            app_state::v1_set_group_avatar,
            app_state::v1_fetch_group_avatar,
            app_state::v1_search_groups,
            app_state::v1_request_join_group,
            app_state::v1_join_requests,
            app_state::v1_resolve_join_request,
            app_state::v1_add_group_member,
            app_state::v1_remove_group_member,
            app_state::v1_leave_group,
            app_state::v1_recall_message,
            app_state::v1_cancel_file_transfer,
            app_state::v1_list_dissolved_groups,
            app_state::v1_delete_group_history,
            app_state::v1_request_contact,
            app_state::v1_contact_requests,
            app_state::v1_respond_contact_request,
            app_state::v1_withdraw_contact_request,
            app_state::v1_lan_visibility,
            app_state::v1_set_lan_visibility,
            app_state::v1_notice_settings,
            tray_lang::v1_get_language,
            tray_lang::v1_set_language,
            app_state::v1_set_notice_settings,
            notice::v1_flash_tray,
            notice::v1_show_notice,
            notice::v1_hide_notice,
            notice::v1_open_notice,
            app_state::v1_probe_peer
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Qbird");
}

#[cfg(test)]
mod data_dir_tests {
    use super::*;

    fn scratch(label: &str) -> PathBuf {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("data-dir-tests").join(format!(
            "{label}-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    #[test]
    fn marker_moves_data_next_to_the_executable_and_migrates_once() {
        let base = scratch("portable");
        let exe = base.join("app");
        let legacy = base.join("legacy");
        std::fs::create_dir_all(&exe).unwrap();
        std::fs::create_dir_all(legacy.join("stickers")).unwrap();
        std::fs::write(legacy.join("identity.bin"), b"key").unwrap();
        std::fs::write(legacy.join("qbird.sqlite3"), b"v1").unwrap();
        std::fs::write(legacy.join("stickers").join("a.png"), b"png").unwrap();

        // Without the marker a bare exe keeps using the per-user directory.
        assert_eq!(resolve_data_dir(&exe, legacy.clone()).unwrap(), legacy);

        std::fs::write(exe.join(PORTABLE_MARKER), b"portable").unwrap();
        let data = resolve_data_dir(&exe, legacy.clone()).unwrap();
        assert_eq!(data, exe.join(PORTABLE_DATA_DIR));
        assert_eq!(std::fs::read(data.join("identity.bin")).unwrap(), b"key");
        assert_eq!(std::fs::read(data.join("qbird.sqlite3")).unwrap(), b"v1");
        assert_eq!(std::fs::read(data.join("stickers").join("a.png")).unwrap(), b"png");

        // A second start must never overwrite the live database.
        std::fs::write(data.join("qbird.sqlite3"), b"v2").unwrap();
        resolve_data_dir(&exe, legacy.clone()).unwrap();
        assert_eq!(std::fs::read(data.join("qbird.sqlite3")).unwrap(), b"v2");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn migration_is_skipped_without_a_complete_identity_and_database_pair() {
        let base = scratch("incomplete");
        let exe = base.join("app");
        let legacy = base.join("legacy");
        std::fs::create_dir_all(&exe).unwrap();
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("qbird.sqlite3"), b"v1").unwrap();
        std::fs::write(exe.join(PORTABLE_MARKER), b"portable").unwrap();

        let data = resolve_data_dir(&exe, legacy.clone()).unwrap();
        assert!(!data.join("qbird.sqlite3").exists());
        assert!(!data.join("identity.bin").exists());

        let _ = std::fs::remove_dir_all(&base);
    }
}
