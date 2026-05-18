use crate::models::{AppState, ClipboardItem, Settings};
use crate::storage;
use serde::Deserialize;
use std::io::Write;
use tauri::AppHandle;
use tauri::Emitter;

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
    let items = state.clipboard_items.lock().map_err(|e| e.to_string())?;
    Ok(items.clone())
}

#[tauri::command]
pub fn add_clipboard_item(
    item: ClipboardItem,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    let settings = state.settings.lock().map_err(|e| e.to_string())?;
    let max_items = settings.max_items;
    drop(settings);

    let mut items = state.clipboard_items.lock().map_err(|e| e.to_string())?;

    let is_duplicate = items
        .iter()
        .any(|existing| existing.content == item.content && existing.item_type == item.item_type);

    if !is_duplicate {
        items.insert(0, item.clone());

        let max_items = max_items as usize;
        if items.len() > max_items {
            let pinned: Vec<_> = items.iter().filter(|i| i.is_pinned).cloned().collect();
            let mut non_pinned: Vec<_> = items.iter().filter(|i| !i.is_pinned).cloned().collect();

            non_pinned.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            non_pinned.truncate(max_items.saturating_sub(pinned.len()));

            items.clear();
            items.extend(pinned);
            items.extend(non_pinned);
        }

        let data_dir = state.data_dir.lock().map_err(|e| e.to_string())?;
        storage::save_clipboard_data(&data_dir, &items)?;
    }

    Ok(())
}

#[tauri::command]
pub fn delete_clipboard_item(
    id: String,
    state: tauri::State<AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let mut items = state.clipboard_items.lock().map_err(|e| e.to_string())?;
    items.retain(|item| item.id != id);

    let data_dir = state.data_dir.lock().map_err(|e| e.to_string())?;
    storage::save_clipboard_data(&data_dir, &items)?;
    drop(items);
    drop(data_dir);
    crate::tray::tray_menu_display(&app_handle);

    Ok(())
}

#[tauri::command]
pub fn update_clipboard_item(
    item: ClipboardItem,
    state: tauri::State<AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let mut items = state.clipboard_items.lock().map_err(|e| e.to_string())?;

    if let Some(index) = items.iter().position(|i| i.id == item.id) {
        items[index] = item.clone();

        let data_dir = state.data_dir.lock().map_err(|e| e.to_string())?;
        storage::save_clipboard_data(&data_dir, &items)?;
        drop(items);
        drop(data_dir);
        crate::tray::tray_menu_display(&app_handle);
    }

    Ok(())
}

#[tauri::command]
pub fn clear_clipboard_items(state: tauri::State<AppState>) -> Result<(), String> {
    let mut items = state.clipboard_items.lock().map_err(|e| e.to_string())?;
    items.clear();

    let data_dir = state.data_dir.lock().map_err(|e| e.to_string())?;
    storage::save_clipboard_data(&data_dir, &items)?;

    Ok(())
}

#[tauri::command]
pub fn import_clipboard_items(
    items: Vec<ClipboardItem>,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    let settings = state.settings.lock().map_err(|e| e.to_string())?;
    let max_items = settings.max_items as usize;
    drop(settings);

    let mut existing_items = state.clipboard_items.lock().map_err(|e| e.to_string())?;

    for item in items {
        if !existing_items.iter().any(|i| i.id == item.id) {
            existing_items.push(item);
        }
    }

    existing_items.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    let pinned: Vec<_> = existing_items.iter().filter(|i| i.is_pinned).cloned().collect();
    let mut non_pinned: Vec<_> = existing_items.iter().filter(|i| !i.is_pinned).cloned().collect();

    non_pinned.truncate(max_items.saturating_sub(pinned.len()));

    existing_items.clear();
    existing_items.extend(pinned);
    existing_items.extend(non_pinned);

    let data_dir = state.data_dir.lock().map_err(|e| e.to_string())?;
    storage::save_clipboard_data(&data_dir, &existing_items)?;

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

    *current_settings = settings.clone();

    // 更新语言环境
    rust_i18n::set_locale(&settings.language);

    let data_dir = state.data_dir.lock().map_err(|e| e.to_string())?;
    storage::save_settings_to_file(&data_dir, &settings)?;

    // 刷新托盘菜单（语言可能已更改）
    crate::tray::tray_menu_display(&app_handle);

    Ok(())
}

#[tauri::command]
pub fn get_history_items(state: tauri::State<AppState>) -> Result<Vec<ClipboardItem>, String> {
    let settings = state.settings.lock().map_err(|e| e.to_string())?;
    let max_items = settings.max_items as usize;
    drop(settings);

    let items = state.clipboard_items.lock().map_err(|e| e.to_string())?;
    let mut sorted_items = items.clone();
    sorted_items.sort_by(|a, b| {
        if a.is_pinned != b.is_pinned {
            b.is_pinned.cmp(&a.is_pinned)
        } else {
            b.created_at.cmp(&a.created_at)
        }
    });
    Ok(sorted_items.into_iter().take(max_items).collect())
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
