pub mod address;
pub mod app_state;
pub mod certificate;
pub mod discovery;
pub mod identity;
pub mod probe;
pub mod probe_service;
pub mod protocol;
pub mod storage;
pub mod tls;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            let state = app_state::AppState::initialize(&dir).map_err(|error| std::io::Error::other(error.to_string()))?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_state::identity_fingerprint,
            app_state::v1_list_contacts,
            app_state::v1_confirm_contact,
            app_state::v1_list_messages,
            app_state::v1_open_self,
            app_state::v1_hide_self,
            app_state::v1_list_conversations,
            app_state::v1_list_self_messages,
            app_state::v1_send_self,
            app_state::v1_discovery_snapshot,
            app_state::v1_start_probe_service,
            app_state::v1_stop_probe_service,
            app_state::v1_probe_status,
            app_state::v1_probe_peer
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Qbird");
}
