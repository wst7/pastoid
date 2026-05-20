use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
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
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } => {
                // 左键显示菜单
                tray_menu_display(tray.app_handle());
            }
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
                id if id.starts_with("clip_") => {
                    let item_id = id.strip_prefix("clip_").unwrap_or("");
                    copy_item_to_clipboard(app_handle, item_id);
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

    let state = app_handle.state::<crate::models::AppState>();

    // 与 preference 页面保持一致，用 max_items 作为显示数量
    let max_items = state.repo.max_items();
    let display_items = state.repo.get_history(max_items as usize);

    // 添加历史记录标题（禁用）
    if !display_items.is_empty() {
        let history_title = MenuItem::with_id(
            app_handle,
            "history_title",
            t!("tray.history_title"),
            false,
            None::<&str>,
        )?;
        menu.append(&history_title)?;

        // 动态添加历史记录项
        for item in &display_items {
            // 使用字符索引截断，避免中文字符被切断
            let chars: Vec<char> = item.content.chars().collect();
            let content_preview = if chars.len() > 35 {
                chars[..35].iter().collect::<String>() + "..."
            } else {
                item.content.clone()
            };

            // 替换换行符为空格
            let display_text = content_preview.replace('\n', " ").replace('\r', "");

            let menu_id = format!("clip_{}", item.id);

            let menu_text = if item.is_pinned {
                format!("📌 {}", display_text)
            } else {
                display_text
            };

            let menu_item =
                MenuItem::with_id(app_handle, &menu_id, &menu_text, true, None::<&str>)?;
            menu.append(&menu_item)?;
        }

        // 添加分隔线
        menu.append(&PredefinedMenuItem::separator(app_handle)?)?;
    } else {
        // 无历史记录
        let empty_item = MenuItem::with_id(app_handle, "empty", t!("tray.no_history"), false, None::<&str>)?;
        menu.append(&empty_item)?;
        menu.append(&PredefinedMenuItem::separator(app_handle)?)?;
    }

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

pub fn tray_menu_display(app_handle: &AppHandle) {
    // 刷新托盘菜单显示
    if let Some(tray) = app_handle.tray_by_id("main") {
        if let Ok(menu) = build_tray_menu(app_handle) {
            let _ = tray.set_menu(Some(menu));
        }
    }
}

fn copy_item_to_clipboard(app_handle: &AppHandle, item_id: &str) {
    let state = app_handle.state::<crate::models::AppState>();

    if let Some(item) = state.repo.find_by_id(item_id) {
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_text(&item.content);
        }
    }
}

fn clear_all_items(app_handle: &AppHandle) {
    let state = app_handle.state::<crate::models::AppState>();

    state.repo.clear();
    // 立即 flush，确保托盘菜单重建时数据已清空
    let _ = state.repo.flush_now();

    tray_menu_display(app_handle);

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
