pub mod clipboard;
pub mod commands;
pub mod models;
pub mod repository;
pub mod shortcut;
pub mod storage;
pub mod tray;

#[macro_use]
extern crate rust_i18n;

i18n!("locales");

use models::AppState;
use std::sync::Arc;
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let data_dir = storage::get_data_dir(&app.handle());
            let settings = storage::load_settings(&data_dir);

            // 设置语言
            rust_i18n::set_locale(&settings.language);

            let repo = Arc::new(repository::ClipboardRepository::new(
                data_dir.clone(),
                settings.max_items,
            ));

            // 启动后台自动 flush（每 5 秒）
            let repo_flush = repo.clone();
            repo_flush.start_auto_flush(5);

            app.manage(AppState {
                repo,
                data_dir,
                settings: std::sync::Mutex::new(settings),
            });

            // 设置托盘
            tray::setup_tray(app)?;

            // 设置窗口关闭事件
            if let Some(main_window) = app.get_webview_window("main") {
                tray::setup_window_close_handler(&main_window);
            }

            // 注册全局快捷键 Cmd+Shift+V 呼出快速粘贴面板
            let handle = app.handle().clone();
            let shortcut = Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyV);
            if let Err(e) = handle.global_shortcut().on_shortcut(shortcut, move |app, _s, event| {
                if event.state() == ShortcutState::Pressed {
                    if let Some(window) = app.get_webview_window("quick-paste") {
                        if window.is_visible().unwrap_or(false) {
                            let _ = window.hide();
                        } else {
                            let _ = window.center();
                            let _ = window.show();
                            let _ = window.set_focus();
                            // 通知前端刷新数据并聚焦搜索
                            let _ = window.emit("panel-opened", ());
                        }
                    }
                }
            }) {
                eprintln!("Failed to register global shortcut: {}", e);
            }

            // 启动剪切板监听
            clipboard::start_clipboard_monitor(app.handle().clone());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_clipboard_items,
            commands::add_clipboard_item,
            commands::delete_clipboard_item,
            commands::update_clipboard_item,
            commands::clear_clipboard_items,
            commands::import_clipboard_items,
            commands::get_settings,
            commands::save_settings,
            commands::get_history_items,
            commands::check_update,
            commands::start_download_update,
            commands::open_installer,
            commands::paste_clipboard,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
