// objc 的 sel!/msg_send! 会展开 cfg(cargo-clippy)，rustc 不认这个 cfg
#![allow(unexpected_cfgs)]

pub mod clipboard;
pub mod commands;
pub mod db;
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


const AUTOSTART_ARG: &str = "--autostart";
/// 自启拉起后的静默窗口期：macOS 勾选「重新登录时重新打开窗口」时，
/// LaunchAgent 与 loginwindow 会在登录后几秒内先后拉起两个进程；
/// 若 LaunchAgent 先启动、loginwindow 后补拉（不带 `--autostart`），
/// 在这段窗口期内直接静默吞掉，避免开机自动弹出设置窗口。
const AUTOSTART_GRACE_PERIOD: std::time::Duration = std::time::Duration::from_secs(15);

fn has_autostart_arg<S: AsRef<str>>(args: &[S]) -> bool {
    args.iter().any(|arg| arg.as_ref() == AUTOSTART_ARG)
}

fn should_show_settings_on_second_instance(
    primary_autostart: bool,
    elapsed_since_start: std::time::Duration,
    incoming_args: &[String],
) -> bool {
    if has_autostart_arg(incoming_args) {
        return false;
    }
    if primary_autostart && elapsed_since_start < AUTOSTART_GRACE_PERIOD {
        return false;
    }
    true
}

pub fn run() {
    let launched_at = std::time::Instant::now();
    let primary_args: Vec<String> = std::env::args().collect();
    let primary_autostart = has_autostart_arg(&primary_args);

    let builder = tauri::Builder::default()
        // 单实例插件必须最先注册，防止第二个进程继续初始化托盘、快捷键和剪贴板监听
        .plugin(tauri_plugin_single_instance::init(move |app, args, _cwd| {
            log::info!("Second instance detected (args={:?}), ignoring", args);
            if should_show_settings_on_second_instance(
                primary_autostart,
                launched_at.elapsed(),
                &args,
            ) {
                // 统一走这里：重新打开主窗口时必须同时恢复 Regular（Dock 图标）——
                // 隐藏 Dock 的实例被再次拉起时，只 show 窗口会让「Dock 图标随窗口出现」的约定失效
                crate::tray::show_settings_window(app);
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init());

    // NSPanel 只有 macOS 有，用 shadowing 而不是 mut，Windows 上就不会有 unused_mut
    #[cfg(target_os = "macos")]
    let builder = builder.plugin(tauri_nspanel::init());

    builder
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![AUTOSTART_ARG]),
        ))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(move |app| {
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
            #[cfg(target_os = "macos")]
            let hide_dock_when_closed = settings.hide_dock_when_closed;

            let repo = Arc::new(repository::ClipboardRepository::new(
                data_dir.clone(),
                settings.max_items,
            ));

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

            // manage 会把 data_dir 移走，首次运行判断得提前算好
            let first_run = storage::is_first_run(&data_dir);

            app.manage(AppState {
                repo,
                data_dir,
                settings: std::sync::Mutex::new(settings),
                suppressed: std::sync::Mutex::new(std::collections::HashSet::new()),
                usage_cache: std::sync::Mutex::new(None),
                pending_update: std::sync::Mutex::new(None),
            });

            // 首次运行：把设置窗口叫出来，让用户先决定数据放在哪。
            // 非首次运行：只有「登录项自启」才在启动时设为 Accessory，避免开机时 Dock 图标闪现；
            // 手动双击启动不隐藏——否则窗口没叫出来、Dock 也没有，用户会以为应用没启动。
            if first_run {
                tray::show_settings_window(app.handle());
            } else {
                #[cfg(target_os = "macos")]
                if hide_dock_when_closed && primary_autostart {
                    tray::set_dock_visible(app.handle(), false);
                }
            }

            // 数据目录相关的启动提示（logger 已就绪）
            for note in &startup_notes {
                log::info!("{}", note);
            }

            // 设置托盘
            tray::setup_tray(app)?;

            // 后台检查更新：启动 20 秒后查一次，之后每 6 小时一次。
            // 只负责「发现有新版并提示」，下载始终由用户点。
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    use tauri::Emitter;
                    use tauri_plugin_updater::UpdaterExt;

                    tokio::time::sleep(std::time::Duration::from_secs(20)).await;
                    loop {
                        if let Ok(updater) = handle.updater() {
                            match updater.check().await {
                                Ok(Some(update)) => {
                                    let version = update.version.clone();
                                    let state = handle.state::<AppState>();
                                    let changed = {
                                        let mut guard = state.pending_update.lock().unwrap();
                                        if guard.as_deref() == Some(version.as_str()) {
                                            false
                                        } else {
                                            *guard = Some(version.clone());
                                            true
                                        }
                                    };
                                    if changed {
                                        log::info!("发现新版本 {}", version);
                                        let _ = tray::update_tray_menu(&handle);
                                        let _ = handle.emit("update-available", version);
                                    }
                                }
                                Ok(None) => {}
                                Err(e) => log::warn!("后台检查更新失败: {}", e),
                            }
                        }
                        tokio::time::sleep(std::time::Duration::from_secs(6 * 3600)).await;
                    }
                });
            }

            // selftest feature：连续开合面板，验证 NSPanel 生命周期。发布包不编译这段。
            #[cfg(feature = "selftest")]
            if std::env::var("PASTOID_PANEL_SELFTEST").is_ok()
                || std::env::var("PASTOID_PANEL_OPEN").is_ok()
            {
                // PASTOID_PANEL_OPEN：只弹一次并保持打开，方便肉眼检查样式
                let keep_open = std::env::var("PASTOID_PANEL_OPEN").is_ok();
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(4));
                    let rounds = if keep_open { 1 } else { 40 };
                    for i in 0..rounds {
                        let h = handle.clone();
                        let h2 = h.clone();
                        let _ = h.run_on_main_thread(move || {
                            crate::shortcut::toggle_quick_paste(&h2);
                        });
                        std::thread::sleep(std::time::Duration::from_millis(250));
                        log::info!("selftest toggle {}", i);
                    }
                    log::info!("selftest done");
                    if std::env::var("PASTOID_PANEL_HOLD").is_ok() {
                        // 调试用：持续把面板顶到最前，方便截图检查
                        loop {
                            let h = handle.clone();
                            let h2 = h.clone();
                            let _ = h.run_on_main_thread(move || {
                                if let Some(w) = h2.get_webview_window("quick-paste") {
                                    crate::shortcut::show_panel(&h2, &w);
                                }
                            });
                            std::thread::sleep(std::time::Duration::from_millis(500));
                        }
                    }
                });
            }

            // 设置窗口关闭事件
            if let Some(main_window) = app.get_webview_window("settings") {
                tray::setup_window_close_handler(app.handle().clone(), &main_window);
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
                        // MoveToActiveSpace：面板隐藏时会留在上次的 Space，order front 时搬到当前
                        // Space（含全屏）。只用 CanJoinAllSpaces 在 macOS 26/27 上出现过
                        // 「isVisible = true 但 occluded = true」——窗口在别的 Space 上，用户看不见。
                        // CanJoinAllApplications（1 << 18，macOS 13+）允许浮窗加入其它应用的全屏
                        // Space；cocoa 0.26 的 bitflags 没有这一位，手动补上。
                        // 注意 CanJoinAllSpaces 与 MoveToActiveSpace 互斥，同时设置会抛异常。
                        // from_bits_truncate 会丢掉未声明的位，from_bits_retain 才能保留 1 << 18
                        // （macOS 13+ 的有效常量；更老的系统忽略未知位）。
                        const CAN_JOIN_ALL_APPLICATIONS: u64 = 1 << 18;
                        let behavior: u64 = (NSWindowCollectionBehavior::NSWindowCollectionBehaviorMoveToActiveSpace
                            | NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenAuxiliary)
                            .bits()
                            | CAN_JOIN_ALL_APPLICATIONS;
                        panel.set_collection_behaviour(
                            NSWindowCollectionBehavior::from_bits_retain(behavior),
                        );
                    }

                    // 关键顺序：改 style mask 会把 NSPanel 的阴影/不透明属性恢复成默认值，
                    // 所以下面这些必须放在 set_style_mask 之后，否则「直角虚影」还会回来。
                    panel.set_opaque(false);
                    panel.set_has_shadow(false);
                    {
                        use tauri_nspanel::objc::{class, msg_send, sel, sel_impl};
                        if let Ok(ptr) = window.ns_window() {
                            let ns = ptr as *mut tauri_nspanel::objc::runtime::Object;
                            unsafe {
                                let clear: *mut tauri_nspanel::objc::runtime::Object =
                                    msg_send![class!(NSColor), clearColor];
                                let _: () = msg_send![ns, setBackgroundColor: clear];
                                let opaque: bool = msg_send![ns, isOpaque];
                                let shadow: bool = msg_send![ns, hasShadow];
                                let bg: *mut tauri_nspanel::objc::runtime::Object =
                                    msg_send![ns, backgroundColor];
                                let alpha: f64 = msg_send![bg, alphaComponent];
                                log::debug!(
                                    "panel diag: opaque={} shadow={} bg_alpha={:.3}",
                                    opaque, shadow, alpha
                                );
                            }
                        }
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
            commands::list_clipboard_items,
            commands::get_clipboard_counts,
            commands::get_clipboard_tags,
            commands::preview_trim_history,
            commands::delete_items_by_tag,
            commands::preview_delete_by_type,
            commands::delete_items_by_type,
            commands::add_clipboard_item,
            commands::delete_clipboard_item,
            commands::update_clipboard_item,
            commands::clear_clipboard_items,
            commands::get_pending_update,
            commands::import_clipboard_items,
            commands::get_image_data,
        commands::get_storage_info,
        commands::mark_first_run_done,
        commands::set_data_dir,
        commands::set_global_shortcut_enabled,
        commands::open_data_dir,
        commands::restart_app,
            commands::get_settings,
            commands::save_settings,
            commands::get_history_items,
            commands::paste_item,
            commands::check_accessibility,
            commands::open_accessibility_settings,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            let _ = app_handle;
            match event {
                tauri::RunEvent::Exit => {
                    shortcut::request_shutdown();
                }
                #[cfg(target_os = "macos")]
                tauri::RunEvent::Reopen {
                    has_visible_windows: false,
                    ..
                } => {
                    tray::show_settings_window(app_handle);
                }
                _ => {}
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn autostart_second_instance_stays_silent() {
        assert!(!should_show_settings_on_second_instance(
            false,
            Duration::from_secs(600),
            &args(&["pastoid", "--autostart"]),
        ));
    }

    #[test]
    fn login_restore_shortly_after_autostart_stays_silent() {
        assert!(!should_show_settings_on_second_instance(
            true,
            Duration::from_secs(3),
            &args(&["pastoid"]),
        ));
    }

    #[test]
    fn manual_relaunch_after_autostart_grace_shows_settings() {
        assert!(should_show_settings_on_second_instance(
            true,
            AUTOSTART_GRACE_PERIOD,
            &args(&["pastoid"]),
        ));
    }

    #[test]
    fn manual_relaunch_when_primary_not_autostart_shows_settings() {
        assert!(should_show_settings_on_second_instance(
            false,
            Duration::from_secs(1),
            &args(&["pastoid"]),
        ));
    }
}
