pub mod address;
pub mod app_state;
pub mod avatar;
pub mod backup;
mod auto_address;
pub mod certificate;
pub mod chat_transport;
pub mod discovery;
pub mod files;
pub mod identity;
pub mod lock;
pub mod probe;
pub mod probe_service;
pub mod protocol;
pub mod retry_queue;
pub mod storage;
pub mod tls;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            // Debug-only override for isolated desktop acceptance; release builds ignore it.
            #[cfg(debug_assertions)]
            let dir = std::env::var_os("QBIRD_TEST_DATA_DIR")
                .filter(|value| !value.is_empty()).map(std::path::PathBuf::from).unwrap_or(dir);
            let state = app_state::AppState::initialize(&dir).map_err(|error| std::io::Error::other(error.to_string()))?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_state::identity_fingerprint,
            app_state::v1_profile,
            app_state::v1_set_profile,
            app_state::v1_avatar,
            app_state::v1_set_avatar,
            app_state::v1_clear_avatar,
            app_state::v1_read_image_file,
            app_state::v1_set_contact_profile,
            app_state::v1_search_contacts,
            app_state::v1_list_contacts,
            app_state::v1_confirm_contact,
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
            app_state::v1_reveal_attachment,
            app_state::v1_storage_usage,
            app_state::v1_create_backup,
            app_state::v1_inspect_backup,
            app_state::v1_restore_backup,
            app_state::v1_set_storage_quota,
            app_state::v1_deliver_direct,
            app_state::v1_open_self,
            app_state::v1_hide_self,
            app_state::v1_list_conversations,
            app_state::v1_list_self_messages,
            app_state::v1_send_self,
            app_state::v1_discovery_snapshot,
            app_state::v1_start_probe_service,
            app_state::v1_start_chat_service,
            app_state::v1_stop_probe_service,
            app_state::v1_probe_status,
            app_state::v1_create_group,
            app_state::v1_list_groups,
            app_state::v1_list_group_members,
            app_state::v1_open_group,
            app_state::v1_hide_group,
            app_state::v1_queue_group,
            app_state::v1_transfer_group_owner,
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
            app_state::v1_add_group_member,
            app_state::v1_remove_group_member,
            app_state::v1_leave_group,
            app_state::v1_probe_peer
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Qbird");
}
