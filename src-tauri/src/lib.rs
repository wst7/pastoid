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
use tauri::Manager;
#[cfg(target_os = "macos")]
use tauri_nspanel::WebviewWindowExt;


pub fn run() {
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build());

    #[cfg(target_os = "macos")]
    {
        builder = builder.plugin(tauri_nspanel::init());
    }

    builder
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::AppleScript,
            Some(vec!["--autostart"]),
        ))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let data_dir = storage::get_data_dir(&app.handle());
            let settings = storage::load_settings(&data_dir);

            // 设置语言
            rust_i18n::set_locale(&settings.language);

            let shortcut_str = settings.shortcut.clone();

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
            if let Some(main_window) = app.get_webview_window("settings") {
                tray::setup_window_close_handler(&main_window);
            }

            // quick-paste 窗口转为 NSPanel 以在全屏应用上方显示
            #[cfg(target_os = "macos")]
            if let Some(window) = app.get_webview_window("quick-paste") {
                if let Ok(panel) = window.to_panel() {
                    #[allow(deprecated)]
                    {
                        use tauri_nspanel::cocoa::appkit::{
                            NSMainMenuWindowLevel, NSWindowCollectionBehavior,
                        };
                        // 不抢占其它窗口焦点
                        panel.set_style_mask(1 << 7); // NSWindowStyleMaskNonActivatingPanel
                        // 层级高于菜单栏，确保覆盖全屏应用
                        panel.set_level(NSMainMenuWindowLevel + 1);
                        // 在所有 Space（含全屏）中共享
                        panel.set_collection_behaviour(
                            NSWindowCollectionBehavior::NSWindowCollectionBehaviorCanJoinAllSpaces
                                | NSWindowCollectionBehavior::NSWindowCollectionBehaviorStationary
                                | NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenAuxiliary,
                        );
                    }
                }
            }

            #[cfg(not(target_os = "macos"))]
            if let Some(window) = app.get_webview_window("quick-paste") {
                let _ = window.set_visible_on_all_workspaces(true);
            }

            // 注册全局快捷键
            if let Err(e) = shortcut::register(
                app.handle(),
                &shortcut_str,
            ) {
                eprintln!(
                    "Failed to register shortcut '{}': {}. Falling back to '{}'.",
                    shortcut_str, e, models::Settings::default().shortcut
                );
                let default_shortcut = models::Settings::default().shortcut;
                if let Err(e2) = shortcut::register(app.handle(), &default_shortcut) {
                    eprintln!("Failed to register fallback shortcut '{}': {}", default_shortcut, e2);
                }
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
            commands::paste_clipboard,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
