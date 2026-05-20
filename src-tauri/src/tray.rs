use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::Emitter;
use tauri::{AppHandle, Manager};

use rust_i18n::t;

pub fn setup_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let app_handle = app.handle();

    // 创建托盘菜单
    let menu = build_tray_menu(app_handle)?;

    // 创建托盘图标
    let _tray = TrayIconBuilder::with_id("main")
        .icon(Image::from_bytes(include_bytes!(
            "../icons/tray/16x16.png"
        ))?)
        .menu(&menu)
        .tooltip("ClipOn")
        .show_menu_on_left_click(true)
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::DoubleClick { .. } => {
                // 双击：显示主窗口
                if let Some(window) = tray.app_handle().get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            _ => {}
        })
        .on_menu_event(|app_handle, event| {
            let id = event.id().as_ref();
            match id {
                "open_quick_paste" => {
                    crate::shortcut::toggle_quick_paste(app_handle);
                }
                "preferences" => {
                    if let Some(window) = app_handle.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
                "clear" => {
                    clear_all_items(app_handle);
                }
                "quit" => {
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

    // 打开剪贴板面板
    let open_qp = MenuItem::with_id(app_handle, "open_quick_paste", t!("tray.open_quick_paste"), true, None::<&str>)?;
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

fn clear_all_items(app_handle: &AppHandle) {
    let state = app_handle.state::<crate::models::AppState>();

    state.repo.clear();
    // 立即 flush
    let _ = state.repo.flush_now();

    // 通知前端清空
    let _ = app_handle.emit("clipboard-cleared", ());

    send_notification(app_handle, &t!("tray.history_cleared"));
}

fn send_notification(app_handle: &AppHandle, message: &str) {
    use tauri_plugin_notification::NotificationExt;
    let _ = app_handle
        .notification()
        .builder()
        .title("ClipOn")
        .body(message)
        .show();
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
