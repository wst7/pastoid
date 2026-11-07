use crate::models::{AppState, ClipboardItem, Settings};
use crate::storage;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use tauri::{Emitter, Manager};

// === Clipboard snapshot/restore for paste-and-keep ===

struct ClipboardSnapshot {
    text: Option<String>,
    image: Option<arboard::ImageData<'static>>,
}

fn capture_clipboard() -> ClipboardSnapshot {
    let mut clip = match arboard::Clipboard::new() {
        Ok(c) => c,
        Err(e) => {
            log::warn!("capture_clipboard: failed to open clipboard: {}", e);
            return ClipboardSnapshot { text: None, image: None };
        }
    };

    let text = clip.get_text().ok();

    // 只在没有文本时读取图片：一是避免无谓地搬运大图，
    // 二是还原时文本优先，两者同时存在只保留文本表示。
    let image = if text.is_none() {
        clip.get_image()
            .ok()
            .map(|img| arboard::ImageData {
                width: img.width,
                height: img.height,
                bytes: std::borrow::Cow::Owned(img.bytes.into_owned()),
            })
    } else {
        None
    };

    ClipboardSnapshot { text, image }
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
        return;
    }

    if let Some(image) = snapshot.image {
        if let Err(e) = clip.set_image(image) {
            log::warn!("restore_clipboard: failed to restore image: {}", e);
        }
    }
}

// === 历史条目 ===

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
    let removed: Option<ClipboardItem> = state.repo.find_by_id(&id);
    state.repo.delete(&id);

    if let Some(item) = removed {
        // 内容可能还留在剪贴板里，记下来，避免下一轮轮询把它重新加回来
        let key = item.image_hash.clone().unwrap_or_else(|| item.content.clone());
        if let Ok(mut set) = state.suppressed.lock() {
            set.insert(key);
        }

        // 图片条目被删除时同步删掉文件，但要确认没有别的条目还在引用它
        if item.is_image() && !item.content.is_empty() {
            let still_referenced = state.repo.referenced_image_paths().contains(&item.content);
            if !still_referenced {
                storage::delete_image_file(&state.data_dir, &item.content);
            }
        }
    }

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

/// 后台检查到的新版本号（没有就返回 null）
#[tauri::command]
pub fn get_pending_update(state: tauri::State<AppState>) -> Option<String> {
    state.pending_update.lock().ok().and_then(|v| v.clone())
}

#[tauri::command]
pub fn clear_clipboard_items(
    app_handle: tauri::AppHandle,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    // 先记下所有将被清掉的内容键：剪贴板里可能还留着其中一条，
    // 不抑制的话下一轮轮询会立刻把它加回来，清空就成了摆设。
    if let Ok(mut set) = state.suppressed.lock() {
        for item in state.repo.get_items() {
            set.insert(item.image_hash.clone().unwrap_or_else(|| item.content.clone()));
        }
    }

    state.repo.clear();
    // 立即落盘：清空是用户主动操作，不能等 5 秒自动 flush 才写，
    // 否则这期间退出应用，重启后历史会「复活」。
    let _ = state.repo.flush_now();
    // 历史清空后，所有图片文件都成了孤儿
    storage::cleanup_orphan_images(&state.data_dir, &std::collections::HashSet::new());

    // 面板和设置页都靠这个事件刷新，否则清空后仍显示旧数据
    use tauri::Emitter;
    let _ = app_handle.emit("clipboard-cleared", ());
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

/// 读取历史里某张图片的内容，返回可直接塞进 <img src> 的 data URL。
#[tauri::command]
pub async fn get_image_data(
    filename: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    // 只允许读取 data_dir 内部的文件
    if filename.contains("..") || filename.starts_with('/') {
        return Err("非法的图片路径".to_string());
    }

    let data_dir = state.data_dir.clone();
    let path = data_dir.join(&filename);
    let base = data_dir
        .canonicalize()
        .map_err(|e| format!("无法解析数据目录: {}", e))?;
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("无法读取图片: {}", e))?;

    if !canonical.starts_with(&base) {
        return Err("非法的图片路径".to_string());
    }

    // 原图可能几 MB（4K 截图），直接 base64 塞给 webview 会卡。
    // 这里在阻塞线程池里缩到长边 512，只回缩略图。
    tauri::async_runtime::spawn_blocking(move || thumbnail_data_url(&canonical, 512))
        .await
        .map_err(|e| format!("缩略图任务失败: {}", e))?
}

/// 读图 → 按长边缩放 → 编码 PNG → data URL
fn thumbnail_data_url(path: &std::path::Path, max_edge: u32) -> Result<String, String> {
    let img = image::open(path).map_err(|e| format!("无法解码图片: {}", e))?;
    let (w, h) = (img.width(), img.height());
    let thumb = if w.max(h) > max_edge {
        img.resize(max_edge, max_edge, image::imageops::FilterType::Triangle)
    } else {
        img
    };

    let mut buf: Vec<u8> = Vec::new();
    thumb
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .map_err(|e| format!("无法编码缩略图: {}", e))?;

    Ok(format!(
        "data:image/png;base64,{}",
        STANDARD.encode(&buf)
    ))
}

// === 数据目录 ===

#[derive(serde::Serialize)]
pub struct StorageInfo {
    /// 当前实际使用的目录
    pub data_dir: String,
    /// 平台默认目录（Windows 为 %LOCALAPPDATA%，macOS 为 Application Support）
    pub default_dir: String,
    /// 是否用户自定义过
    pub is_custom: bool,
    /// 历史 + 图片占用字节数
    pub total_bytes: u64,
    /// 图片文件数
    pub image_count: usize,
    /// 历史条目数
    pub item_count: usize,
    /// 已选好、但还没重启生效的目标目录
    pub pending_dir: Option<String>,
    /// 首次运行（还没问过数据目录）
    pub first_run: bool,
}

#[tauri::command]
pub async fn get_storage_info(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<StorageInfo, String> {
    let data_dir = state.data_dir.clone();
    let default_dir = storage::default_data_dir(&app_handle);

    // 图片目录可能有几万个文件，全量 stat 很慢；60 秒内复用上次结果
    let cached = state.usage_cache.lock().ok().and_then(|g| *g);
    let (total_bytes, image_count) = match cached {
        Some((at, bytes, images)) if at.elapsed() < std::time::Duration::from_secs(60) => {
            (bytes, images)
        }
        _ => {
            let dir = data_dir.clone();
            let (bytes, images) =
                tauri::async_runtime::spawn_blocking(move || storage::dir_usage(&dir))
                    .await
                    .map_err(|e| format!("统计目录失败: {}", e))?;
            if let Ok(mut guard) = state.usage_cache.lock() {
                *guard = Some((std::time::Instant::now(), bytes, images));
            }
            (bytes, images)
        }
    };

    Ok(StorageInfo {
        first_run: storage::is_first_run(&data_dir),
        is_custom: data_dir != default_dir,
        data_dir: data_dir.to_string_lossy().to_string(),
        default_dir: default_dir.to_string_lossy().to_string(),
        total_bytes,
        image_count,
        item_count: state.repo.len(),
        pending_dir: storage::pending_data_dir(&default_dir)
            .filter(|p| p != &data_dir)
            .map(|p| p.to_string_lossy().to_string()),
    })
}

/// 指定新的数据目录。写入指针文件，真正搬迁发生在下次启动时
/// （仓库、图片目录、监控线程都持有旧路径，运行中切换没有意义）。
#[tauri::command]
pub fn set_data_dir(
    path: String,
    state: tauri::State<AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("数据目录不能为空".to_string());
    }

    let target = std::path::PathBuf::from(trimmed);
    if !target.is_absolute() {
        return Err("请填写绝对路径（例如 D:\\PastoidData）".to_string());
    }

    let current = state.data_dir.clone();
    if target == current {
        return Ok(());
    }

    // 互相嵌套会出事：搬进子目录相当于递归，搬到父目录会把整个 AppData 卷进来
    if target.starts_with(&current) || current.starts_with(&target) {
        return Err("不能选择当前数据目录的子目录或其上级目录".to_string());
    }

    if let Err(e) = std::fs::create_dir_all(&target) {
        return Err(format!("无法创建该目录: {}", e));
    }
    let probe = target.join(".pastoid-write-probe");
    let writable = std::fs::write(&probe, b"ok").is_ok();
    let _ = std::fs::remove_file(&probe);
    if !writable {
        return Err("该目录不可写，请换一个位置".to_string());
    }

    let default_dir = storage::default_data_dir(&app_handle);
    storage::write_data_dir_pointer(&default_dir, Some(&target), Some(&current))?;

    log::info!("数据目录已指定为 {:?}，重启后迁移", target);
    Ok(())
}

/// 用户选择「用默认位置」时调用，避免每次启动都弹引导
#[tauri::command]
pub fn mark_first_run_done(state: tauri::State<AppState>) -> Result<(), String> {
    storage::mark_first_run_done(&state.data_dir);
    Ok(())
}

/// 录制新快捷键期间把全局热键停掉：否则按下的组合会先被全局监听抢走，
/// 面板弹出来、焦点跑掉，录制就中断了（用户看到的是「设置快捷键时卡住/没反应」）。
#[tauri::command]
pub fn set_global_shortcut_enabled(
    enabled: bool,
    state: tauri::State<AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let shortcut = state
        .settings
        .lock()
        .map_err(|e| e.to_string())?
        .shortcut
        .clone();

    if enabled {
        match crate::shortcut::register(&app_handle, &shortcut) {
            Ok(()) => Ok(()),
            // 已经注册过（保存流程刚注册完）不算错误
            Err(e) if e.contains("already") => Ok(()),
            Err(e) => Err(e),
        }
    } else {
        let _ = crate::shortcut::unregister(&app_handle, &shortcut);
        Ok(())
    }
}

#[tauri::command]
pub fn open_data_dir(state: tauri::State<AppState>) -> Result<(), String> {
    open::that(&state.data_dir).map_err(|e| format!("打开目录失败: {}", e))
}

#[tauri::command]
pub fn restart_app(app_handle: tauri::AppHandle) -> Result<(), String> {
    // restart 内部会 exit 并拉起新进程，正常不会返回
    app_handle.restart();
}

// === 设置 ===

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
            if cfg!(target_os = "macos") {
                return Err(format!(
                    "开启自启动失败: {}。请确保应用已安装到 /Applications/ 目录",
                    e
                ));
            }
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

    // 记录快捷键变更（随后在后台线程异步执行重注册，避免阻塞 UI）
    let language_changed = current_settings.language != settings.language;
    let shortcut_changed = current_settings.shortcut != settings.shortcut;
    let old_shortcut = current_settings.shortcut.clone();
    let new_shortcut = settings.shortcut.clone();

    // 如果主题变了，广播给所有窗口
    if current_settings.theme != settings.theme {
        let _ = app_handle.emit("theme-changed", settings.theme.clone());
    }

    // 先落盘：写入失败就直接返回，此时内存仍是旧值，磁盘与内存保持一致
    storage::save_settings_to_file(&state.data_dir, &settings)?;

    // 落盘成功后再更新内存
    *current_settings = settings.clone();

    // 更新语言环境
    rust_i18n::set_locale(&settings.language);

    // ⚠️ 关键修复：在启动后台线程前释放锁，避免死锁
    drop(current_settings);

    // 托盘菜单的文案是在构建时用 t!() 取的，换语言后必须重建，否则菜单还是旧语言
    if language_changed {
        if let Err(e) = crate::tray::update_tray_menu(&app_handle) {
            log::error!("Failed to rebuild tray menu after language change: {}", e);
        }
    }

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
                        } else {
                            log::warn!("无法回滚快捷键设置：settings 锁被占用");
                        }
                    }
                    // 前端据此回滚 UI 上显示的快捷键
                    let _ = app.emit(
                        "shortcut-error",
                        format!("快捷键 '{}' 注册失败，已恢复原设置", new_shortcut),
                    );
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

// === 粘贴 ===

/// 把历史条目写进剪贴板并发一次 Cmd+V。
///
/// 图片条目不能再按文本粘贴——`content` 只是相对路径，按文本写会把
/// `images/xxx.png` 这串字符粘到用户窗口里。这里按类型分别写入。
#[tauri::command]
pub fn paste_item(id: String, app_handle: tauri::AppHandle) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    let item = state
        .repo
        .find_by_id(&id)
        .ok_or_else(|| "条目不存在".to_string())?;
    let data_dir = state.data_dir.clone();

    let snap = capture_clipboard();

    if item.is_image() {
        let path = data_dir.join(&item.content);
        let rgba = image::ImageReader::open(&path)
            .map_err(|e| format!("无法打开图片: {}", e))?
            .decode()
            .map_err(|e| format!("无法解码图片: {}", e))?
            .to_rgba8();
        let (width, height) = rgba.dimensions();

        let mut clipboard =
            arboard::Clipboard::new().map_err(|e| format!("打开剪贴板失败: {}", e))?;
        clipboard
            .set_image(arboard::ImageData {
                width: width as usize,
                height: height as usize,
                bytes: std::borrow::Cow::Owned(rgba.into_raw()),
            })
            .map_err(|e| format!("写入图片到剪贴板失败: {}", e))?;
    } else {
        use tauri_plugin_clipboard_manager::ClipboardExt;
        app_handle
            .clipboard()
            .write_text(item.content.clone())
            .map_err(|e| format!("clipboard write: {}", e))?;
    }

    if let Some(window) = app_handle.get_webview_window("quick-paste") {
        let _ = window.hide();
    }

    use enigo::{Direction, Enigo, Key, Keyboard, Settings};
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| format!("Failed to init enigo: {}", e))?;

    // macOS：先 Cmd+Tab 回到上一个应用，再 Cmd+V，保证粘贴落在用户原本的窗口里。
    // Windows：面板隐藏后焦点本身就会回到原窗口，直接 Ctrl+V 即可（不要用 Ctrl+Tab，
    // 那会把浏览器/编辑器的标签页切花）。
    if cfg!(target_os = "macos") {
        enigo
            .key(Key::Meta, Direction::Press)
            .map_err(|e| format!("enigo: {}", e))?;
        enigo
            .key(Key::Tab, Direction::Click)
            .map_err(|e| format!("enigo: {}", e))?;
        enigo
            .key(Key::Meta, Direction::Release)
            .map_err(|e| format!("enigo: {}", e))?;

        std::thread::sleep(std::time::Duration::from_millis(300));

        // Cmd+V 粘贴
        enigo
            .key(Key::Meta, Direction::Press)
            .map_err(|e| format!("enigo: {}", e))?;
        enigo
            .key(Key::Unicode('v'), Direction::Click)
            .map_err(|e| format!("enigo: {}", e))?;
        enigo
            .key(Key::Meta, Direction::Release)
            .map_err(|e| format!("enigo: {}", e))?;
    } else {
        // Windows：必须发虚拟键 VK_V。enigo 对 Key::Unicode 在 Windows 上只能降级成
        // 「文本输入」（它会打一条 warn 然后走 queue_char），那样 Ctrl 和 v 是两条独立事件，
        // 应用根本收不到 Ctrl+V —— 表现就是按回车没反应、或直接往编辑器里插了个 v。
        #[cfg(target_os = "windows")]
        let v_key = Key::V;
        #[cfg(not(target_os = "windows"))]
        let v_key = Key::Unicode('v');

        // 面板隐藏后要把焦点还给原窗口，Windows 上这一下比 macOS 慢一些
        std::thread::sleep(std::time::Duration::from_millis(250));

        // Ctrl+V 粘贴
        enigo
            .key(Key::Control, Direction::Press)
            .map_err(|e| format!("enigo: {}", e))?;
        enigo
            .key(v_key, Direction::Click)
            .map_err(|e| format!("enigo: {}", e))?;
        enigo
            .key(Key::Control, Direction::Release)
            .map_err(|e| format!("enigo: {}", e))?;
    }

    // Restore original clipboard content
    std::thread::sleep(std::time::Duration::from_millis(150));
    restore_clipboard(snap);

    Ok(())
}
