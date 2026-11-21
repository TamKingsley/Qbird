//! Localized tray texts and the UI language setting.
use tauri::Manager;
use crate::TrayHandle;


pub fn tray_texts(lang: &str) -> (&'static str, &'static str, &'static str) {
    match lang {
        "zh-TW" => ("Qbird 青鳥", "顯示主視窗", "結束 Qbird"),
        "en" => ("Qbird", "Show main window", "Quit Qbird"),
        _ => ("Qbird 青鸟", "显示主窗口", "退出 Qbird"),
    }
}

pub fn build_tray_menu(app: &tauri::AppHandle, lang: &str) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{Menu, MenuItem};
    let (tooltip, show_label, quit_label) = tray_texts(lang);
    let _ = tooltip;
    let show = MenuItem::with_id(app, "show", show_label, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", quit_label, true, None::<&str>)?;
    Menu::with_items(app, &[&show, &quit])
}

pub fn read_language_pub(app: &tauri::AppHandle) -> String {
    let state: tauri::State<'_, crate::app_state::AppState> = app.state();
    let Ok(store) = state.store.lock() else { return String::new() };
    store.get_setting("ui_language").unwrap_or(None).unwrap_or_default()
}

pub fn apply_tray_language(app: &tauri::AppHandle, lang: &str) {
    if let Some(handle) = app.try_state::<TrayHandle>() {
        if let Ok(menu) = build_tray_menu(app, lang) {
            let _ = handle.0.set_menu(Some(menu));
        }
    }
}

#[tauri::command]
pub fn v1_get_language(app: tauri::AppHandle) -> String {
    let lang = read_language_pub(&app);
    if lang.is_empty() { "zh-CN".into() } else { lang }
}

#[tauri::command]
pub fn v1_set_language(app: tauri::AppHandle, lang: String) -> Result<(), String> {
    if !matches!(lang.as_str(), "zh-CN" | "zh-TW" | "en") { return Err("未知的语言".into()); }
    {
        let state: tauri::State<'_, crate::app_state::AppState> = app.state();
        let store = state.store.lock().map_err(|_| "本地存储不可用".to_owned())?;
        store.set_setting("ui_language", Some(&lang)).map_err(|e| e.to_string())?;
    }
    apply_tray_language(&app, &lang);
    Ok(())
}
