use crate::models::{AppState, ClipboardItem, Settings};
use crate::storage;
use serde::Deserialize;
use std::io::Write;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    body: Option<String>,
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
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
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    state.repo.delete(&id);
    crate::tray::tray_menu_display(&app_handle);
    Ok(())
}

#[tauri::command]
pub fn update_clipboard_item(
    item: ClipboardItem,
    state: tauri::State<AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    state.repo.update(item);
    crate::tray::tray_menu_display(&app_handle);
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
        let _ = autostart.enable();
    } else {
        let _ = autostart.disable();
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
    }

    *current_settings = settings.clone();

    // 更新语言环境
    rust_i18n::set_locale(&settings.language);

    storage::save_settings_to_file(&state.data_dir, &settings)?;

    // 刷新托盘菜单（语言可能已更改）
    crate::tray::tray_menu_display(&app_handle);

    Ok(())
}

#[tauri::command]
pub fn get_history_items(state: tauri::State<AppState>) -> Result<Vec<ClipboardItem>, String> {
    let settings = state.settings.lock().map_err(|e| e.to_string())?;
    let max_items = settings.max_items as usize;
    drop(settings);

    Ok(state.repo.get_history(max_items))
}

#[derive(serde::Serialize)]
pub struct UpdateInfo {
    pub has_update: bool,
    pub current_version: String,
    pub latest_version: String,
    pub download_url: String,
    pub release_notes: Option<String>,
}

#[derive(serde::Serialize, Clone)]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: u64,
    pub percent: f32,
}

#[tauri::command]
pub async fn check_update() -> Result<UpdateInfo, String> {
    let current_version = env!("CARGO_PKG_VERSION").to_string();

    let client = reqwest::Client::new();
    let response = client
        .get("https://api.github.com/repos/wst7/clipon/releases/latest")
        .header("User-Agent", "ClipOn")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(UpdateInfo {
            has_update: false,
            current_version: current_version.clone(),
            latest_version: current_version,
            download_url: "https://github.com/wst7/clipon/releases".to_string(),
            release_notes: None,
        });
    }

    if !response.status().is_success() {
        return Err("Failed to fetch release info".to_string());
    }

    let release: GitHubRelease = response.json().await.map_err(|e| e.to_string())?;

    let latest_version = release.tag_name.trim_start_matches('v').to_string();
    let has_update = compare_versions(&current_version, &latest_version) == std::cmp::Ordering::Less;

    Ok(UpdateInfo {
        has_update,
        current_version,
        latest_version,
        download_url: release.html_url,
        release_notes: release.body,
    })
}

fn compare_versions(current: &str, latest: &str) -> std::cmp::Ordering {
    let current_parts: Vec<u32> = current.split('.').filter_map(|s| s.parse().ok()).collect();
    let latest_parts: Vec<u32> = latest.split('.').filter_map(|s| s.parse().ok()).collect();

    for i in 0..std::cmp::max(current_parts.len(), latest_parts.len()) {
        let cur = current_parts.get(i).unwrap_or(&0);
        let lat = latest_parts.get(i).unwrap_or(&0);
        match cur.cmp(lat) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}

fn find_asset_for_platform(release: &GitHubRelease) -> Option<&GitHubAsset> {
    let os = std::env::consts::OS;

    release.assets.iter().find(|asset| {
        match os {
            "macos" => {
                // Prefer x64 (Intel) build because unsigned aarch64 gets quarantine error on macOS
                // Universal Rosetta 2 works fine on Apple Silicon
                asset.name.ends_with("_x64.dmg") || asset.name.ends_with(".dmg")
            }
            "windows" => asset.name.ends_with(".exe") || asset.name.ends_with(".msi"),
            "linux" => {
                if asset.name.ends_with(".AppImage") {
                    return true;
                }
                asset.name.ends_with(".deb")
            }
            _ => false,
        }
    })
}

#[tauri::command]
pub async fn start_download_update(app_handle: AppHandle) -> Result<String, String> {
    let client = reqwest::Client::new();
    let response = client
        .get("https://api.github.com/repos/wst7/clipon/releases/latest")
        .header("User-Agent", "ClipOn")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err("Failed to fetch release info".to_string());
    }

    let release: GitHubRelease = response.json().await.map_err(|e| e.to_string())?;

    let asset = find_asset_for_platform(&release)
        .ok_or_else(|| "No installer found for your platform".to_string())?;

    let download_response = client
        .get(&asset.browser_download_url)
        .header("User-Agent", "ClipOn")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let total_size = download_response.content_length().unwrap_or(0);
    let mut downloaded = 0u64;
    let mut stream = download_response.bytes_stream();

    let temp_path = std::env::temp_dir().join(&asset.name);
    let mut file = std::fs::File::create(&temp_path).map_err(|e| e.to_string())?;

    use futures_util::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        file.write_all(&chunk).map_err(|e| e.to_string())?;
        downloaded += chunk.len() as u64;

        let percent = if total_size > 0 {
            (downloaded as f32 / total_size as f32) * 100.0
        } else {
            0.0
        };

        app_handle.emit("download_progress", DownloadProgress {
            downloaded,
            total: total_size,
            percent,
        }).map_err(|e| e.to_string())?;
    }

    Ok(temp_path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn open_installer(path: String) -> Result<(), String> {
    open::that(&path).map_err(|e| e.to_string())?;
    Ok(())
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
