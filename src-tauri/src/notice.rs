//! Incoming-message notices. The backend never decides whether to show a popup;
//! it only forwards the event so the frontend can apply the user's setting.
use std::sync::{Mutex, OnceLock};
use tauri::{Emitter, Manager};

static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
/// Tray handle shared with the notice flasher.
static TRAY: Mutex<Option<tauri::tray::TrayIcon<tauri::Wry>>> = Mutex::new(None);
/// Alternate icon with a red badge, built once from the application icon.
static ALERT_ICON: OnceLock<tauri::image::Image<'static>> = OnceLock::new();

pub fn attach(app: tauri::AppHandle) {
    let _ = APP.set(app);
}

pub fn set_tray(tray: tauri::tray::TrayIcon<tauri::Wry>) {
    if let Ok(mut slot) = TRAY.lock() { *slot = Some(tray); }
}

pub fn set_alert_icon(base: &tauri::image::Image<'_>) {
    let (width, height) = (base.width(), base.height());
    let mut rgba = base.rgba().to_vec();
    if width > 4 && height > 4 {
        let radius = (width.min(height) / 3).max(2) as i32;
        let cx = width as i32 - radius - 1;
        let cy = radius + 1;
        for y in 0..height as i32 {
            for x in 0..width as i32 {
                let (dx, dy) = (x - cx, y - cy);
                if dx * dx + dy * dy <= radius * radius {
                    let index = ((y as u32 * width + x as u32) * 4) as usize;
                    rgba[index] = 226;
                    rgba[index + 1] = 74;
                    rgba[index + 2] = 58;
                    rgba[index + 3] = 255;
                }
            }
        }
    }
    let _ = ALERT_ICON.set(tauri::image::Image::new_owned(rgba, width, height));
}

/// Bounded preview text; the UI shows at most one line.
pub fn preview(body: &str) -> String {
    let mut text = String::new();
    for ch in body.chars() {
        if text.chars().count() >= 80 { text.push('…'); break; }
        if ch.is_control() { continue; }
        text.push(ch);
    }
    text
}

pub fn incoming(kind: &str, id: &str, preview: String) {
    let Some(app) = APP.get() else { return };
    let _ = app.emit("qbird://notice", serde_json::json!({
        "kind": kind,
        "id": id,
        "preview": preview,
        "ts": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_secs()).unwrap_or(0),
    }));
}

/// Flashes the tray icon with the alert badge for a few seconds.
#[tauri::command]
pub fn v1_flash_tray(label: String) {
    let Some(tray) = TRAY.lock().ok().and_then(|slot| slot.clone()) else { return };
    let Some(alert) = ALERT_ICON.get().cloned() else { return };
    let app = tray.app_handle().clone();
    let base = app.default_window_icon().map(|icon| tauri::image::Image::new_owned(
        icon.rgba().to_vec(), icon.width(), icon.height()));
    let text = if label.trim().is_empty() { "Qbird 青鸟".to_string() } else { format!("Qbird · {} 发来消息", label.trim()) };
    let _ = tray.set_tooltip(Some(text.as_str()));
    std::thread::spawn(move || {
        for _ in 0..6 {
            let _ = tray.set_icon(Some(alert.clone()));
            std::thread::sleep(std::time::Duration::from_millis(350));
            if let Some(base) = base.as_ref() { let _ = tray.set_icon(Some(base.clone())); }
            std::thread::sleep(std::time::Duration::from_millis(350));
        }
        let _ = tray.set_tooltip(Some("Qbird 青鸟"));
    });
}

#[tauri::command]
pub fn v1_show_notice(app: tauri::AppHandle, payload: serde_json::Value) -> Result<(), String> {
    let Some(window) = app.get_webview_window("notice") else { return Ok(()) };
    if let (Ok(Some(monitor)), Ok(size)) = (window.current_monitor(), window.outer_size()) {
        let scale = monitor.scale_factor();
        let monitor_size = monitor.size().to_logical::<f64>(scale);
        let x = monitor.position().x as f64 / scale + monitor_size.width - size.width as f64 / scale - 24.0;
        let y = monitor.position().y as f64 / scale + monitor_size.height - size.height as f64 / scale - 24.0;
        let _ = window.set_position(tauri::LogicalPosition::new(x, y));
    }
    let _ = window.emit("qbird://notice", payload);
    let _ = window.show();
    Ok(())
}

#[tauri::command]
pub fn v1_hide_notice(app: tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("notice") { let _ = window.hide(); }
}

/// Opens the conversation behind a notice and brings the main window forward.
#[tauri::command]
pub fn v1_open_notice(app: tauri::AppHandle, payload: serde_json::Value) {
    if let Some(window) = app.get_webview_window("notice") { let _ = window.hide(); }
    crate::show_main_window(&app);
    let _ = app.emit("qbird://open-chat", payload);
}
