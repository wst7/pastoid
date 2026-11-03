use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::Emitter;
use tauri::{AppHandle, Manager};

use rust_i18n::t;

pub fn setup_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let app_handle = app.handle();

    let menu = build_tray_menu(app_handle)?;

    // macOS 菜单栏用模板图标，跟随系统明暗自动反色
    let _tray = TrayIconBuilder::with_id("main")
        .icon(Image::from_bytes(include_bytes!(
            "../icons/tray/32x32.png"
        ))?)
        .icon_as_template(true)
        .menu(&menu)
        .tooltip("Pastoid")
        .show_menu_on_left_click(true)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::DoubleClick { .. } = event {
                // 双击：显示主窗口
                if let Some(window) = tray.app_handle().get_webview_window("settings") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .on_menu_event(|app_handle, event| {
            let id = event.id().as_ref();
            match id {
                "open_quick_paste" => {
                    crate::shortcut::toggle_quick_paste(app_handle);
                }
                "preferences" => {
                    if let Some(window) = app_handle.get_webview_window("settings") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
                "clear" => {
                    clear_all_items(app_handle);
                }
                "quit" => {
                    log::info!("Pastoid exiting");
                    std::process::exit(0);
                }
                _ => {}
            }
        })
        .build(app.handle())?;

    app.manage(_tray);
    Ok(())
}

fn build_tray_menu(app_handle: &AppHandle) -> Result<Menu<tauri::Wry>, Box<dyn std::error::Error>> {
    let menu = Menu::new(app_handle)?;

    // 获取当前绑定的快捷键
    let state = app_handle.state::<crate::models::AppState>();
    let shortcut = {
        let settings = state.settings.lock().map_err(|e| e.to_string())?;
        settings.shortcut.clone()
    };
    let accelerator = shortcut_to_accelerator(&shortcut);

    // 打开剪贴板面板（带快捷键显示）
    let open_qp = MenuItem::with_id(
        app_handle,
        "open_quick_paste",
        t!("tray.open_quick_paste"),
        true,
        Some(accelerator.as_str()),
    )?;
    menu.append(&open_qp)?;

    // 偏好设置
    let preferences_item = MenuItem::with_id(app_handle, "preferences", t!("tray.preferences"), true, None::<&str>)?;
    menu.append(&preferences_item)?;

    // 分隔线
    menu.append(&PredefinedMenuItem::separator(app_handle)?)?;

    // 清除
    let clear_item = MenuItem::with_id(app_handle, "clear", t!("tray.clear_history"), true, None::<&str>)?;
    menu.append(&clear_item)?;

    // 退出
    let quit_item = MenuItem::with_id(app_handle, "quit", t!("tray.quit"), true, None::<&str>)?;
    menu.append(&quit_item)?;

    Ok(menu)
}

/// 将 global_hotkey 格式的快捷键字符串转换为 Tauri accelerator 格式。
/// 本项目只面向 macOS，两者格式一致，直接透传。
fn shortcut_to_accelerator(shortcut: &str) -> String {
    shortcut.to_string()
}

fn clear_all_items(app_handle: &AppHandle) {
    let state = app_handle.state::<crate::models::AppState>();

    // 记下被清掉的内容键，避免剪贴板里仍在的那条下一轮被加回来
    if let Ok(mut set) = state.suppressed.lock() {
        for item in state.repo.get_items() {
            set.insert(item.image_hash.clone().unwrap_or_else(|| item.content.clone()));
        }
    }

    state.repo.clear();
    // 立即 flush
    let _ = state.repo.flush_now();
    // 历史清空后图片文件都成了孤儿
    crate::storage::cleanup_orphan_images(&state.data_dir, &std::collections::HashSet::new());

    // 通知前端清空
    let _ = app_handle.emit("clipboard-cleared", ());

    send_notification(app_handle, &t!("tray.history_cleared"));
}

fn send_notification(app_handle: &AppHandle, message: &str) {
    use tauri_plugin_notification::NotificationExt;
    let _ = app_handle
        .notification()
        .builder()
        .title("Pastoid")
        .body(message)
        .show();
}

/// 当快捷键变更时，重建并更新托盘菜单
pub fn update_tray_menu(app_handle: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(tray) = app_handle.tray_by_id("main") {
        let menu = build_tray_menu(app_handle)?;
        tray.set_menu(Some(menu))?;
    }
    Ok(())
}

pub fn setup_window_close_handler(window: &tauri::WebviewWindow) {
    let window_clone = window.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = window_clone.hide();
        }
    });
}
