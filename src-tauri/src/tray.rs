use std::sync::atomic::{AtomicBool, Ordering};

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use rust_i18n::t;

/// 记录当前 macOS 是否处于 Accessory（隐藏 Dock 图标）模式。
/// `paste_item` 据此判断能否用 Cmd+Tab 切回上一个应用：Accessory 应用不在 Cmd+Tab 列表里。
static DOCK_HIDDEN: AtomicBool = AtomicBool::new(false);

pub fn is_dock_hidden() -> bool {
    DOCK_HIDDEN.load(Ordering::Relaxed)
}

#[cfg(target_os = "macos")]
pub fn mark_dock_hidden(hidden: bool) {
    DOCK_HIDDEN.store(hidden, Ordering::Relaxed);
}

fn should_show_dock(hide_dock_when_closed: bool, settings_visible: bool) -> bool {
    !hide_dock_when_closed || settings_visible
}

/// 在 macOS 上切换 Dock 图标显示（Regular 显示 / Accessory 隐藏）；其它平台空操作。
pub fn set_dock_visible(app_handle: &AppHandle, visible: bool) {
    #[cfg(target_os = "macos")]
    {
        let policy = if visible {
            tauri::ActivationPolicy::Regular
        } else {
            tauri::ActivationPolicy::Accessory
        };
        if let Err(e) = app_handle.set_activation_policy(policy) {
            log::warn!("切换 Dock 图标可见性({})失败: {}", visible, e);
            return;
        }
        DOCK_HIDDEN.store(!visible, Ordering::Relaxed);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app_handle, visible);
    }
}

/// 显示并聚焦设置主窗口；若之前隐藏了 Dock 图标，先恢复为 Regular 模式。
pub fn show_settings_window(app_handle: &AppHandle) {
    set_dock_visible(app_handle, true);
    if let Some(window) = app_handle.get_webview_window("settings") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// 根据「关闭窗口后隐藏 Dock 图标」设置与当前主窗口可见状态同步 Dock 图标。
pub fn sync_dock_visibility(app_handle: &AppHandle, hide_dock_when_closed: bool) {
    let settings_visible = app_handle
        .get_webview_window("settings")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);
    set_dock_visible(app_handle, should_show_dock(hide_dock_when_closed, settings_visible));
}

pub fn setup_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let app_handle = app.handle();

    let menu = build_tray_menu(app_handle)?;

    // 两个平台按各自的惯例来：
    // - macOS 菜单栏用模板图（纯黑 + alpha），系统按菜单栏明暗自动染色，
    //   深色菜单栏下就是白色，浅色下是黑色，和系统图标一致。
    // - Windows 托盘保留彩色品牌图标（模板图在 Windows 上不会反色）。
    #[cfg(target_os = "macos")]
    let icon_bytes: &[u8] = include_bytes!("../icons/tray-template/32x32.png");
    #[cfg(not(target_os = "macos"))]
    let icon_bytes: &[u8] = include_bytes!("../icons/tray/32x32.png");

    let _tray = TrayIconBuilder::with_id("main")
        .icon(Image::from_bytes(icon_bytes)?)
        .icon_as_template(cfg!(target_os = "macos"))
        .menu(&menu)
        .tooltip("Pastoid")
        .show_menu_on_left_click(true)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::DoubleClick { .. } = event {
                // 双击：显示主窗口
                show_settings_window(tray.app_handle());
            }
        })
        .on_menu_event(|app_handle, event| {
            let id = event.id().as_ref();
            match id {
                "open_quick_paste" => {
                    crate::shortcut::toggle_quick_paste(app_handle);
                }
                "update_available" => {
                    show_settings_window(app_handle);
                    use tauri::Emitter;
                    let _ = app_handle.emit("update-available", ());
                }
                "preferences" => {
                    show_settings_window(app_handle);
                }
                "quit" => {
                    log::info!("Pastoid exiting");
                    // std::process::exit 会跳过 cleanup_before_exit，先主动释放单实例锁/socket
                    tauri_plugin_single_instance::destroy(app_handle);
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

    // 有未处理的新版本时，菜单最上面提示一行，点它打开设置页去下载
    let pending = app_handle
        .state::<crate::models::AppState>()
        .pending_update
        .lock()
        .ok()
        .and_then(|v| v.clone());
    if let Some(version) = pending {
        let item = MenuItem::with_id(
            app_handle,
            "update_available",
            t!("tray.update_available", version = version.as_str()),
            true,
            None::<&str>,
        )?;
        menu.append(&item)?;
        menu.append(&PredefinedMenuItem::separator(app_handle)?)?;
    }

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

    // 退出
    let quit_item = MenuItem::with_id(app_handle, "quit", t!("tray.quit"), true, None::<&str>)?;
    menu.append(&quit_item)?;

    Ok(menu)
}

/// 将 global_hotkey 格式的快捷键字符串转换为 Tauri accelerator 格式。
/// 设置里统一用 macOS 写法，其它平台映射成 Ctrl/Alt。
fn shortcut_to_accelerator(shortcut: &str) -> String {
    if cfg!(target_os = "macos") {
        shortcut.to_string()
    } else {
        shortcut.replace("Cmd", "Ctrl").replace("Option", "Alt")
    }
}


/// 当快捷键变更时，重建并更新托盘菜单
pub fn update_tray_menu(app_handle: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(tray) = app_handle.tray_by_id("main") {
        let menu = build_tray_menu(app_handle)?;
        tray.set_menu(Some(menu))?;
    }
    Ok(())
}

pub fn setup_window_close_handler(app_handle: AppHandle, window: &tauri::WebviewWindow) {
    let window_clone = window.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = window_clone.hide();
            let hide_dock = app_handle
                .try_state::<crate::models::AppState>()
                .and_then(|s| s.settings.lock().ok().map(|g| g.hide_dock_when_closed))
                .unwrap_or(false);
            if hide_dock {
                set_dock_visible(&app_handle, false);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::should_show_dock;

    #[test]
    fn dock_stays_visible_when_feature_disabled() {
        assert!(should_show_dock(false, false));
        assert!(should_show_dock(false, true));
    }

    #[test]
    fn dock_follows_settings_window_when_feature_enabled() {
        assert!(!should_show_dock(true, false));
        assert!(should_show_dock(true, true));
    }
}
