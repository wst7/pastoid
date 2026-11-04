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
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build());

    // NSPanel 只有 macOS 有，用 shadowing 而不是 mut，Windows 上就不会有 unused_mut
    #[cfg(target_os = "macos")]
    let builder = builder.plugin(tauri_nspanel::init());

    builder
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::AppleScript,
            Some(vec!["--autostart"]),
        ))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            // 数据目录要在 logger 之前确定，所以它把告警带回来，logger 就绪后再打
            let (data_dir, startup_notes) = storage::get_data_dir(&app.handle());

            // 初始化日志
            if let Err(e) = storage::init_logger(&data_dir) {
                eprintln!("Failed to initialize file logger: {}. Falling back to stderr-only.", e);
                let _ = simplelog::TermLogger::init(
                    log::LevelFilter::Info,
                    simplelog::ConfigBuilder::new().build(),
                    simplelog::TerminalMode::Stderr,
                    simplelog::ColorChoice::Auto,
                );
            }
            // identifier 变更后自动迁移旧数据
            storage::migrate_data_if_needed(&app.handle());

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

            // 后台清理没有被任何条目引用的图片文件。
            // 旧版本的采集 bug 会留下大量重复文件，这里在启动时顺手收干净。
            let repo_gc = repo.clone();
            let data_dir_gc = data_dir.clone();
            let _ = std::thread::Builder::new()
                .name("image-gc".into())
                .spawn(move || {
                    let referenced = repo_gc.referenced_image_paths();
                    storage::cleanup_orphan_images(&data_dir_gc, &referenced);
                });

            app.manage(AppState {
                repo,
                data_dir,
                settings: std::sync::Mutex::new(settings),
                suppressed: std::sync::Mutex::new(std::collections::HashSet::new()),
            });

            // 数据目录相关的启动提示（logger 已就绪）
            for note in &startup_notes {
                log::info!("{}", note);
            }

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

            // Windows / Linux：普通窗口即可，但要让面板在所有虚拟桌面可见，
            // 否则切了桌面按快捷键呼出来会「看不见」
            #[cfg(not(target_os = "macos"))]
            if let Some(window) = app.get_webview_window("quick-paste") {
                let _ = window.set_visible_on_all_workspaces(true);
            }

            // 注册全局快捷键
            if let Err(e) = shortcut::register(
                app.handle(),
                &shortcut_str,
            ) {
                log::error!(
                    "Failed to register shortcut '{}': {}. Falling back to '{}'.",
                    shortcut_str, e, models::Settings::default().shortcut
                );
                let default_shortcut = models::Settings::default().shortcut;
                if let Err(e2) = shortcut::register(app.handle(), &default_shortcut) {
                    log::error!("Failed to register fallback shortcut '{}': {}", default_shortcut, e2);
                }
            }

            // 启动快捷键健康监控（每60秒检查一次，5分钟无触发时自动重新注册）
            shortcut::start_health_monitor(app.handle().clone());

            // 启动剪切板监听
            clipboard::start_clipboard_monitor(app.handle().clone());

            log::info!("Pastoid started");

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_clipboard_items,
            commands::add_clipboard_item,
            commands::delete_clipboard_item,
            commands::update_clipboard_item,
            commands::clear_clipboard_items,
            commands::import_clipboard_items,
            commands::get_image_data,
        commands::get_storage_info,
        commands::set_data_dir,
        commands::open_data_dir,
        commands::restart_app,
            commands::get_settings,
            commands::save_settings,
            commands::get_history_items,
            commands::paste_item,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                shortcut::request_shutdown();
            }
        });
}
