use crate::models::{AppState, ClipboardItem, Settings};
use crate::storage;
use tauri::{Emitter, Manager};

#[tauri::command]
pub fn get_clipboard_items(state: tauri::State<AppState>) -> Result<Vec<ClipboardItem>, String> {
    Ok(state.repo.get_items())
}

#[tauri::command]
pub fn add_clipboard_item(
    item: ClipboardItem,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    state.repo.add(item);
    Ok(())
}

#[tauri::command]
pub fn delete_clipboard_item(
    id: String,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    state.repo.delete(&id);
    Ok(())
}

#[tauri::command]
pub fn update_clipboard_item(
    item: ClipboardItem,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    state.repo.update(item);
    Ok(())
}

#[tauri::command]
pub fn clear_clipboard_items(state: tauri::State<AppState>) -> Result<(), String> {
    state.repo.clear();
    Ok(())
}

#[tauri::command]
pub fn import_clipboard_items(
    items: Vec<ClipboardItem>,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    state.repo.import(items);
    Ok(())
}

#[tauri::command]
pub fn get_settings(state: tauri::State<AppState>) -> Result<Settings, String> {
    let settings = state.settings.lock().map_err(|e| e.to_string())?;
    Ok(settings.clone())
}

#[tauri::command]
pub fn save_settings(
    settings: Settings,
    state: tauri::State<AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let mut current_settings = state.settings.lock().map_err(|e| e.to_string())?;

    // 处理自启动
    use tauri_plugin_autostart::ManagerExt;
    let autostart = app_handle.autolaunch();
    if settings.autostart {
        if let Err(e) = autostart.enable() {
            eprintln!("Failed to enable autostart: {}", e);
            #[cfg(target_os = "macos")]
            return Err(format!("开启自启动失败: {}。请确保应用已安装到 /Applications/ 目录", e));
            #[cfg(not(target_os = "macos"))]
            return Err(format!("开启自启动失败: {}", e));
        }
    } else {
        // 关闭时忽略错误（可能登录项本来就不存在）
        if let Err(e) = autostart.disable() {
            eprintln!("Warn: Failed to disable autostart (may not exist): {}", e);
        }
    }

    // 如果 max_items 变了，通知 repo 截断
    if current_settings.max_items != settings.max_items {
        state.repo.set_max_items(settings.max_items);
    }

    // shortcut 变更检测
    if current_settings.shortcut != settings.shortcut {
        let old_shortcut = current_settings.shortcut.clone();
        let new_shortcut = settings.shortcut.clone();

        // 先注销旧快捷键
        if let Err(e) = crate::shortcut::unregister(&app_handle, &old_shortcut) {
            eprintln!("Failed to unregister old shortcut '{}': {}", old_shortcut, e);
        }

        // 注册新快捷键
        if let Err(e) = crate::shortcut::register(&app_handle, &new_shortcut) {
            // 注册失败时回滚到旧快捷键
            eprintln!("Failed to register new shortcut '{}': {}", new_shortcut, e);
            let _ = crate::shortcut::register(&app_handle, &old_shortcut);
            return Err(format!("快捷键 '{}' 注册失败，已恢复原设置", new_shortcut));
        }

        // 更新托盘菜单上的快捷键显示
        if let Err(e) = crate::tray::update_tray_menu(&app_handle) {
            eprintln!("Failed to update tray menu: {}", e);
        }
    }

    // 如果主题变了，广播给所有窗口
    if current_settings.theme != settings.theme {
        let _ = app_handle.emit("theme-changed", settings.theme.clone());
    }

    *current_settings = settings.clone();

    // 更新语言环境
    rust_i18n::set_locale(&settings.language);

    storage::save_settings_to_file(&state.data_dir, &settings)?;

    Ok(())
}

#[tauri::command]
pub fn get_history_items(state: tauri::State<AppState>) -> Result<Vec<ClipboardItem>, String> {
    let settings = state.settings.lock().map_err(|e| e.to_string())?;
    let max_items = settings.max_items as usize;
    drop(settings);

    Ok(state.repo.get_history(max_items))
}

#[tauri::command]
pub fn paste_clipboard(content: String, app_handle: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_clipboard_manager::ClipboardExt;

    app_handle
        .clipboard()
        .write_text(content)
        .map_err(|e| format!("clipboard write: {}", e))?;

    if let Some(window) = app_handle.get_webview_window("quick-paste") {
        let _ = window.hide();
    }

    #[cfg(target_os = "macos")]
    {
        use enigo::{Direction, Enigo, Key, Keyboard, Settings};
        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| format!("Failed to init enigo: {}", e))?;

        // Cmd+Tab 切换到上一个应用
        enigo.key(Key::Meta, Direction::Press)
            .map_err(|e| format!("enigo: {}", e))?;
        enigo.key(Key::Tab, Direction::Click)
            .map_err(|e| format!("enigo: {}", e))?;
        enigo.key(Key::Meta, Direction::Release)
            .map_err(|e| format!("enigo: {}", e))?;

        std::thread::sleep(std::time::Duration::from_millis(300));

        // Cmd+V 粘贴
        enigo.key(Key::Meta, Direction::Press)
            .map_err(|e| format!("enigo: {}", e))?;
        enigo.key(Key::Unicode('v'), Direction::Click)
            .map_err(|e| format!("enigo: {}", e))?;
        enigo.key(Key::Meta, Direction::Release)
            .map_err(|e| format!("enigo: {}", e))?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::thread::sleep(std::time::Duration::from_millis(150));

        use enigo::{Direction, Enigo, Key, Keyboard, Settings};
        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| format!("Failed to init enigo: {}", e))?;
        enigo.key(Key::Control, Direction::Press)
            .map_err(|e| format!("enigo: {}", e))?;
        enigo.key(Key::Unicode('v'), Direction::Click)
            .map_err(|e| format!("enigo: {}", e))?;
        enigo.key(Key::Control, Direction::Release)
            .map_err(|e| format!("enigo: {}", e))?;
    }

    Ok(())
}
