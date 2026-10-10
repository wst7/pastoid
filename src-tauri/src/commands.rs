use crate::models::{AppState, ClipboardItem, Settings};
use crate::storage;
use tauri::{Emitter, Manager};

// === Clipboard snapshot/restore for paste-and-keep ===

struct ClipboardSnapshot {
    text: Option<String>,
}

fn capture_clipboard() -> ClipboardSnapshot {
    let mut clip = match arboard::Clipboard::new() {
        Ok(c) => c,
        Err(e) => {
            log::warn!("capture_clipboard: failed to open clipboard: {}", e);
            return ClipboardSnapshot { text: None };
        }
    };

    let text = clip.get_text().ok();

    ClipboardSnapshot { text }
}

fn restore_clipboard(snapshot: ClipboardSnapshot) {
    let mut clip = match arboard::Clipboard::new() {
        Ok(c) => c,
        Err(e) => {
            log::warn!("restore_clipboard: failed to open clipboard: {}", e);
            return;
        }
    };

    if let Some(text) = &snapshot.text {
        if let Err(e) = clip.set_text(text) {
            log::warn!("restore_clipboard: failed to restore text: {}", e);
        }
    }
}

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
            log::error!("Failed to enable autostart: {}", e);
            #[cfg(target_os = "macos")]
            return Err(format!("开启自启动失败: {}。请确保应用已安装到 /Applications/ 目录", e));
            #[cfg(not(target_os = "macos"))]
            return Err(format!("开启自启动失败: {}", e));
        }
    } else {
        // 关闭时忽略错误（可能登录项本来就不存在）
        if let Err(e) = autostart.disable() {
            log::warn!("Failed to disable autostart (may not exist): {}", e);
        }
    }

    // 如果 max_items 变了，通知 repo 截断
    if current_settings.max_items != settings.max_items {
        state.repo.set_max_items(settings.max_items);
    }

    // 记录快捷键变更（随后在后台线程异步执行重注册，避免 Windows 阻塞 UI）
    let shortcut_changed = current_settings.shortcut != settings.shortcut;
    let old_shortcut = current_settings.shortcut.clone();
    let new_shortcut = settings.shortcut.clone();

    // 如果主题变了，广播给所有窗口
    if current_settings.theme != settings.theme {
        let _ = app_handle.emit("theme-changed", settings.theme.clone());
    }

    // 先将设置写入内存和文件（快捷键变更立即保存，注册异步执行）
    *current_settings = settings.clone();

    // 更新语言环境
    rust_i18n::set_locale(&settings.language);

    // ⚠️ 关键修复：在启动后台线程前释放锁，避免死锁
    drop(current_settings);

    storage::save_settings_to_file(&state.data_dir, &settings)?;

    if shortcut_changed {
        let app = app_handle.clone();
        let data_dir = state.data_dir.clone();

        std::thread::spawn(move || {
            log::info!("Shortcut changed: '{}' -> '{}'", old_shortcut, new_shortcut);

            if let Err(e) = crate::shortcut::unregister(&app, &old_shortcut) {
                log::error!("Failed to unregister old shortcut '{}': {}", old_shortcut, e);
            }

            match crate::shortcut::register(&app, &new_shortcut) {
                Ok(()) => {
                    if let Err(e) = crate::tray::update_tray_menu(&app) {
                        log::error!("Failed to update tray menu: {}", e);
                    }
                }
                Err(e) => {
                    log::error!("Failed to register new shortcut '{}': {}", new_shortcut, e);
                    // 回滚到旧快捷键
                    let _ = crate::shortcut::register(&app, &old_shortcut);
                    // 回滚内存及文件中的设置以保持与实际注册状态一致
                    if let Some(state) = app.try_state::<AppState>() {
                        // ✅ 使用 try_lock 避免死锁，如果锁被占用则放弃回滚
                        if let Ok(mut s) = state.settings.try_lock() {
                            s.shortcut = old_shortcut.clone();
                            let _ = storage::save_settings_to_file(&data_dir, &s);
                            // 通知前端刷新设置
                            let _ = app.emit("settings-changed", ());
                        } else {
                            log::warn!("无法回滚快捷键设置：settings 锁被占用");
                            // 通知前端设置不一致,需要刷新
                            let _ = app.emit("settings-sync-needed", ());
                        }
                    }
                    let _ = app.emit("shortcut-error",
                        format!("快捷键 '{}' 注册失败，已恢复原设置", new_shortcut));
                }
            }
        });
    }

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

    let snap = capture_clipboard();

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

    // Restore original clipboard content
    std::thread::sleep(std::time::Duration::from_millis(150));
    restore_clipboard(snap);

    Ok(())
}
