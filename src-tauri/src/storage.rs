use crate::models::{ClipboardItem, Settings};
use log::{LevelFilter, Log, Metadata, Record};
use simplelog::{ColorChoice, CombinedLogger, ConfigBuilder, SharedLogger, TermLogger, TerminalMode};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::Manager;

const OLD_APP_IDENTIFIER: &str = "com.pastoid.app";

pub fn get_data_dir(app_handle: &tauri::AppHandle) -> PathBuf {
    let app_dir = app_handle
        .path()
        .app_data_dir()
        .expect("Failed to get app data dir");

    if !app_dir.exists() {
        fs::create_dir_all(&app_dir).expect("Failed to create app data dir");
    }

    app_dir
}

const MAX_LOG_SIZE: u64 = 5 * 1024 * 1024; // 5 MB
const MAX_LOG_BACKUPS: usize = 3;

/// 轮转日志文件：pastoid.log → pastoid.log.1 → pastoid.log.2 → pastoid.log.3（删除最旧）
fn rotate_logs(log_dir: &Path) -> Result<(), String> {
    let log_file = log_dir.join("pastoid.log");
    if !log_file.exists() {
        return Ok(());
    }

    let size = fs::metadata(&log_file)
        .map_err(|e| format!("Failed to read log metadata: {}", e))?
        .len();

    if size < MAX_LOG_SIZE {
        return Ok(());
    }

    // 删除最旧的备份
    let oldest = log_dir.join(format!("pastoid.log.{}", MAX_LOG_BACKUPS));
    if oldest.exists() {
        let _ = fs::remove_file(&oldest);
    }

    // 依次后移：2→3, 1→2, current→1
    for i in (1..MAX_LOG_BACKUPS).rev() {
        let src = log_dir.join(format!("pastoid.log.{}", i));
        let dest = log_dir.join(format!("pastoid.log.{}", i + 1));
        if src.exists() {
            let _ = fs::rename(&src, &dest);
        }
    }

    let _ = fs::rename(&log_file, log_dir.join("pastoid.log.1"));
    Ok(())
}

// === 运行时自动轮转的文件日志器 ===

struct RotateState {
    file: Option<fs::File>,
    path: PathBuf,
    log_dir: PathBuf,
    current_size: u64,
    max_size: u64,
    max_backups: usize,
}

impl RotateState {
    fn rotate(&mut self) {
        let _old_file = self.file.take();

        let oldest = self.log_dir.join(format!("pastoid.log.{}", self.max_backups));
        if oldest.exists() {
            let _ = fs::remove_file(&oldest);
        }

        for i in (1..self.max_backups).rev() {
            let src = self.log_dir.join(format!("pastoid.log.{}", i));
            let dest = self.log_dir.join(format!("pastoid.log.{}", i + 1));
            if src.exists() {
                let _ = fs::rename(&src, &dest);
            }
        }

        let _ = fs::rename(&self.path, self.log_dir.join("pastoid.log.1"));

        match fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.path)
        {
            Ok(f) => self.file = Some(f),
            Err(e) => {
                eprintln!("RotatingFileLogger: failed to reopen after rotation: {}", e);
                if let Ok(f) = fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.path)
                {
                    self.file = Some(f);
                }
            }
        }

        self.current_size = 0;
    }
}

struct RotatingFileLogger {
    state: Mutex<RotateState>,
    level: LevelFilter,
}

impl RotatingFileLogger {
    fn new(log_dir: PathBuf, max_size: u64, max_backups: usize, level: LevelFilter) -> Result<Self, String> {
        fs::create_dir_all(&log_dir).map_err(|e| format!("Failed to create log dir: {}", e))?;

        rotate_logs(&log_dir)?;

        let path = log_dir.join("pastoid.log");
        let file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| format!("Failed to open log file: {}", e))?;

        let current_size = fs::metadata(&path)
            .ok()
            .map(|m| m.len())
            .unwrap_or(0);

        Ok(RotatingFileLogger {
            state: Mutex::new(RotateState {
                file: Some(file),
                path,
                log_dir,
                current_size,
                max_size,
                max_backups,
            }),
            level,
        })
    }
}

impl Log for RotatingFileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let now = chrono::Utc::now().to_rfc3339();
        let thread = std::thread::current();
        let thread_info = match thread.name() {
            Some(name) if !name.is_empty() => format!("[{}]", name),
            _ => format!("[{:?}]", thread.id()),
        };

        let line = format!(
            "{} {} [{}] {} - {}\n",
            now,
            thread_info,
            record.level(),
            record.target(),
            record.args()
        );

        let mut state = self.state.lock().unwrap();
        if let Some(ref mut file) = state.file {
            let _ = write!(file, "{}", line);
            let _ = file.flush();
            state.current_size += line.len() as u64;

            if state.current_size >= state.max_size {
                state.rotate();
            }
        }
    }

    fn flush(&self) {
        if let Some(ref mut file) = self.state.lock().unwrap().file {
            let _ = file.flush();
        }
    }
}

impl SharedLogger for RotatingFileLogger {
    fn level(&self) -> LevelFilter {
        self.level
    }

    fn config(&self) -> Option<&simplelog::Config> {
        None
    }

    fn as_log(self: Box<Self>) -> Box<dyn Log> {
        self
    }
}

/// 初始化日志：终端 + 文件（带自动轮转）
pub fn init_logger(data_dir: &Path) -> Result<(), String> {
    let term_config = ConfigBuilder::new()
        .set_time_level(LevelFilter::Error)
        .set_target_level(LevelFilter::Error)
        .build();

    CombinedLogger::init(vec![
        TermLogger::new(LevelFilter::Info, term_config, TerminalMode::Stderr, ColorChoice::Auto),
        Box::new(RotatingFileLogger::new(
            data_dir.join("logs"),
            MAX_LOG_SIZE,
            MAX_LOG_BACKUPS,
            LevelFilter::Debug,
        )?),
    ])
    .map_err(|e| format!("Failed to init logger: {}", e))?;

    Ok(())
}

/// 如果 identifier 变更导致 data_dir 改变，自动将旧目录数据迁移到新目录
pub fn migrate_data_if_needed(app_handle: &tauri::AppHandle) {
    let new_dir = match app_handle.path().app_data_dir() {
        Ok(d) => d,
        Err(e) => {
            log::error!("Migration: failed to get new app data dir: {}", e);
            return;
        }
    };

    // 如果新目录已有数据，说明不是首次启动，跳过迁移
    let new_settings = new_dir.join("settings.json");
    if new_settings.exists() {
        return;
    }

    // 构建旧目录路径（基于旧 identifier）
    let old_dir = if cfg!(target_os = "macos") {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("~"))
            .join("Application Support")
            .join(OLD_APP_IDENTIFIER)
    } else if cfg!(target_os = "windows") {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("%APPDATA%"))
            .join(OLD_APP_IDENTIFIER)
    } else {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
            .join(OLD_APP_IDENTIFIER)
    };

    if !old_dir.exists() {
        return;
    }

    log::info!("Migration: detected old data dir at {:?}, migrating to {:?}", old_dir, new_dir);

    if let Err(e) = fs::create_dir_all(&new_dir) {
        log::error!("Migration: failed to create new data dir: {}", e);
        return;
    }

    // 复制所有文件和目录
    copy_dir_recursive(&old_dir, &new_dir);
    log::info!("Migration: completed successfully");
}

fn copy_dir_recursive(src: &Path, dest: &Path) {
    if let Err(e) = fs::create_dir_all(dest) {
        log::error!("Migration: failed to create dir {:?}: {}", dest, e);
        return;
    }
    match fs::read_dir(src) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let src_path = entry.path();
                let dest_path = dest.join(entry.file_name());
                if src_path.is_dir() {
                    copy_dir_recursive(&src_path, &dest_path);
                } else if src_path.is_file() {
                    if let Err(e) = fs::copy(&src_path, &dest_path) {
                        log::error!("Migration: failed to copy {:?} to {:?}: {}", src_path, dest_path, e);
                    }
                }
            }
        }
        Err(e) => {
            log::error!("Migration: failed to read dir {:?}: {}", src, e);
        }
    }
}

fn get_data_file_path(data_dir: &Path) -> PathBuf {
    data_dir.join("clipboard_data.json")
}

// 图片功能的存储层地基。目前还没有代码产生 item_type == "image" 的条目，
// 因此这几个辅助函数暂时无人调用；等图片采集/渲染接上后即可去掉 allow。
#[allow(dead_code)]
pub(crate) fn get_images_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("images")
}

#[allow(dead_code)]
pub(crate) fn ensure_images_dir(data_dir: &Path) -> Result<PathBuf, String> {
    let dir = get_images_dir(data_dir);
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create images dir: {}", e))?;
    Ok(dir)
}

pub(crate) fn delete_image_file(data_dir: &Path, relative_path: &str) {
    let abs_path = data_dir.join(relative_path);

    // 两侧都要 canonicalize 后再比较：data_dir 本身也可能含符号链接
    // （macOS 上 /var -> /private/var），只解析目标会误判为越界而拒绝删除。
    let base = match data_dir.canonicalize() {
        Ok(base) => base,
        Err(e) => {
            log::warn!("Cannot resolve data_dir {:?}: {}", data_dir, e);
            return;
        }
    };

    // canonicalize 会解析 `..`，因此这里的 starts_with 就是真正的越界防护
    let canonical = match abs_path.canonicalize() {
        Ok(canonical) => canonical,
        // 文件不存在（可能已被删过）无需处理
        Err(_) => return,
    };

    if !canonical.starts_with(&base) {
        log::warn!("Refusing to delete image outside data_dir: {:?}", canonical);
        return;
    }

    if let Err(e) = fs::remove_file(&canonical) {
        log::warn!("Failed to delete image file {:?}: {}", canonical, e);
    }
}

/// 截断条目到 max_items，保留 pinned 项目，优先保留最新的非 pinned 项目
/// 被截断的图片条目会同步清理磁盘文件
pub(crate) fn enforce_max_items(items: &mut Vec<ClipboardItem>, max_items: u32, data_dir: &Path) {
    let max = max_items as usize;
    if items.len() <= max {
        return;
    }

    let pinned: Vec<_> = items.iter().filter(|i| i.is_pinned).cloned().collect();
    let mut non_pinned: Vec<_> = items.iter().filter(|i| !i.is_pinned).cloned().collect();

    non_pinned.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    // 清理被截断的图片文件
    for item in non_pinned.iter().skip(max.saturating_sub(pinned.len())) {
        if item.item_type == "image" && !item.content.is_empty() {
            delete_image_file(data_dir, &item.content);
        }
    }

    non_pinned.truncate(max.saturating_sub(pinned.len()));

    items.clear();
    items.extend(pinned);
    items.extend(non_pinned);
}

pub fn load_clipboard_data(data_dir: &Path, max_items: u32) -> Vec<ClipboardItem> {
    let data_file = get_data_file_path(data_dir);

    if !data_file.exists() {
        return Vec::new();
    }

    match fs::read_to_string(&data_file) {
        Ok(content) => match serde_json::from_str::<Vec<ClipboardItem>>(&content) {
            Ok(mut items) => {
                enforce_max_items(&mut items, max_items, data_dir);
                items
            }
            Err(e) => {
                log::error!("Failed to parse clipboard data: {}", e);
                Vec::new()
            }
        },
        Err(e) => {
            log::error!("Failed to read clipboard data file: {}", e);
            Vec::new()
        }
    }
}

pub fn save_clipboard_data(
    data_dir: &Path,
    items: &mut Vec<ClipboardItem>,
    max_items: u32,
) -> Result<(), String> {
    enforce_max_items(items, max_items, data_dir);

    let data_file = get_data_file_path(data_dir);

    let json = match serde_json::to_string(items) {
        Ok(json) => json,
        Err(e) => return Err(format!("Failed to serialize data: {}", e)),
    };

    match fs::write(&data_file, json) {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to write data file: {}", e)),
    }
}

fn get_settings_file_path(data_dir: &Path) -> PathBuf {
    data_dir.join("settings.json")
}

pub fn load_settings(data_dir: &Path) -> Settings {
    let settings_file = get_settings_file_path(data_dir);

    if !settings_file.exists() {
        return Settings::default();
    }

    match fs::read_to_string(&settings_file) {
        Ok(content) => match serde_json::from_str::<Settings>(&content) {
            Ok(settings) => settings,
            Err(e) => {
                log::error!("Failed to parse settings: {}", e);
                Settings::default()
            }
        },
        Err(e) => {
            log::error!("Failed to read settings file: {}", e);
            Settings::default()
        }
    }
}

pub fn save_settings_to_file(data_dir: &Path, settings: &Settings) -> Result<(), String> {
    let settings_file = get_settings_file_path(data_dir);

    let json = match serde_json::to_string_pretty(settings) {
        Ok(json) => json,
        Err(e) => return Err(format!("Failed to serialize settings: {}", e)),
    };

    match fs::write(&settings_file, json) {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to write settings file: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_temp_dir() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn test_get_data_file_path() {
        let temp_dir = create_temp_dir();
        let path = get_data_file_path(temp_dir.path());
        assert!(path.ends_with("clipboard_data.json"));
    }

    #[test]
    fn test_get_settings_file_path() {
        let temp_dir = create_temp_dir();
        let path = get_settings_file_path(temp_dir.path());
        assert!(path.ends_with("settings.json"));
    }

    #[test]
    fn test_load_clipboard_data_empty_dir() {
        let temp_dir = create_temp_dir();
        let items = load_clipboard_data(temp_dir.path(), 100);
        assert!(items.is_empty());
    }

    #[test]
    fn test_save_and_load_clipboard_data() {
        let temp_dir = create_temp_dir();
        let mut items = vec![
            ClipboardItem::new("item 1".to_string(), "text"),
            ClipboardItem::new("item 2".to_string(), "text"),
        ];

        save_clipboard_data(temp_dir.path(), &mut items, 100).unwrap();

        let loaded = load_clipboard_data(temp_dir.path(), 100);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].content, "item 1");
        assert_eq!(loaded[1].content, "item 2");
    }

    #[test]
    fn test_load_settings_default() {
        let temp_dir = create_temp_dir();
        let settings = load_settings(temp_dir.path());
        assert_eq!(settings.language, "zh");
        assert_eq!(settings.theme, "system");
        #[cfg(target_os = "macos")]
        assert_eq!(settings.shortcut, "Cmd+Shift+V");
        #[cfg(not(target_os = "macos"))]
        assert_eq!(settings.shortcut, "Ctrl+Shift+V");
    }

    #[test]
    fn test_save_and_load_settings() {
        let temp_dir = create_temp_dir();
        let settings = Settings {
            language: "en".to_string(),
            theme: "dark".to_string(),
            autostart: true,
            max_items: 50,
            shortcut: "Ctrl+Alt+K".to_string(),
        };

        save_settings_to_file(temp_dir.path(), &settings).unwrap();

        let loaded = load_settings(temp_dir.path());
        assert_eq!(loaded.language, "en");
        assert_eq!(loaded.theme, "dark");
        assert!(loaded.autostart);
        assert_eq!(loaded.max_items, 50);
        assert_eq!(loaded.shortcut, "Ctrl+Alt+K");
    }

    #[test]
    fn test_load_settings_invalid_file() {
        let temp_dir = create_temp_dir();
        // Write invalid JSON
        fs::write(temp_dir.path().join("settings.json"), "invalid json").unwrap();

        let settings = load_settings(temp_dir.path());
        // Should return default settings
        assert_eq!(settings.language, "zh");
    }

    #[test]
    fn test_save_clipboard_data_empty() {
        let temp_dir = create_temp_dir();
        let mut items: Vec<ClipboardItem> = vec![];

        save_clipboard_data(temp_dir.path(), &mut items, 100).unwrap();

        let loaded = load_clipboard_data(temp_dir.path(), 100);
        assert!(loaded.is_empty());
    }

    #[test]
    fn test_save_clipboard_data_trims_to_max() {
        let temp_dir = create_temp_dir();
        let mut items = vec![
            ClipboardItem::new("item 1".to_string(), "text"),
            ClipboardItem::new("item 2".to_string(), "text"),
            ClipboardItem::new("item 3".to_string(), "text"),
        ];

        save_clipboard_data(temp_dir.path(), &mut items, 2).unwrap();

        let loaded = load_clipboard_data(temp_dir.path(), 2);
        assert_eq!(loaded.len(), 2);
    }

    #[test]
    fn test_load_clipboard_data_trims_excess() {
        let temp_dir = create_temp_dir();
        let mut items = vec![
            ClipboardItem::new("item 1".to_string(), "text"),
            ClipboardItem::new("item 2".to_string(), "text"),
            ClipboardItem::new("item 3".to_string(), "text"),
        ];

        save_clipboard_data(temp_dir.path(), &mut items, 100).unwrap();

        // 加载时使用更小的 max_items，应该被截断
        let loaded = load_clipboard_data(temp_dir.path(), 2);
        assert_eq!(loaded.len(), 2);
    }

    #[test]
    fn test_save_clipboard_data_keeps_pinned() {
        let temp_dir = create_temp_dir();
        let mut item1 = ClipboardItem::new("item 1".to_string(), "text");
        item1.is_pinned = true;
        let item2 = ClipboardItem::new("item 2".to_string(), "text");
        let item3 = ClipboardItem::new("item 3".to_string(), "text");
        let mut items = vec![item3.clone(), item2.clone(), item1.clone()];

        save_clipboard_data(temp_dir.path(), &mut items, 2).unwrap();

        let loaded = load_clipboard_data(temp_dir.path(), 2);
        assert_eq!(loaded.len(), 2);
        // pinned 项目应该保留
        assert!(loaded.iter().any(|i| i.content == "item 1"));
    }

    #[test]
    fn test_rotate_logs_under_threshold_noop() {
        let temp_dir = create_temp_dir();
        let log_dir = temp_dir.path().join("logs");
        fs::create_dir_all(&log_dir).unwrap();

        // 写入 1KB 日志（小于 5MB 阈值）
        fs::write(log_dir.join("pastoid.log"), "x".repeat(1024)).unwrap();

        rotate_logs(&log_dir).unwrap();

        // 不应产生任何备份文件
        assert!(log_dir.join("pastoid.log").exists());
        assert!(!log_dir.join("pastoid.log.1").exists());
    }

    #[test]
    fn test_rotate_logs_over_threshold_creates_backup() {
        let temp_dir = create_temp_dir();
        let log_dir = temp_dir.path().join("logs");
        fs::create_dir_all(&log_dir).unwrap();

        // 写入刚好超过 5MB 的内容
        let over = (MAX_LOG_SIZE + 1024) as usize;
        fs::write(log_dir.join("pastoid.log"), "x".repeat(over)).unwrap();

        rotate_logs(&log_dir).unwrap();

        // 当前日志被清空（新进程会重新创建），旧日志移到 .1
        assert!(!log_dir.join("pastoid.log").exists());
        assert!(log_dir.join("pastoid.log.1").exists());
        assert!(!log_dir.join("pastoid.log.2").exists());
    }

    #[test]
    fn test_rotate_logs_shifts_backups() {
        let temp_dir = create_temp_dir();
        let log_dir = temp_dir.path().join("logs");
        fs::create_dir_all(&log_dir).unwrap();

        // 预置 .1 和 .2
        fs::write(log_dir.join("pastoid.log.1"), "backup1").unwrap();
        fs::write(log_dir.join("pastoid.log.2"), "backup2").unwrap();

        // 当前日志超过阈值
        let over = (MAX_LOG_SIZE + 1024) as usize;
        fs::write(log_dir.join("pastoid.log"), "x".repeat(over)).unwrap();

        rotate_logs(&log_dir).unwrap();

        // .1 → .2, .2 → .3, current → .1
        assert!(!log_dir.join("pastoid.log").exists());
        assert!(log_dir.join("pastoid.log.1").exists());
        assert!(log_dir.join("pastoid.log.2").exists());
        assert!(log_dir.join("pastoid.log.3").exists());
        assert!(!log_dir.join("pastoid.log.4").exists());

        // 验证内容
        assert_eq!(fs::read_to_string(log_dir.join("pastoid.log.2")).unwrap(), "backup1");
        assert_eq!(fs::read_to_string(log_dir.join("pastoid.log.3")).unwrap(), "backup2");
    }

    #[test]
    fn test_rotate_logs_drops_oldest_backup() {
        let temp_dir = create_temp_dir();
        let log_dir = temp_dir.path().join("logs");
        fs::create_dir_all(&log_dir).unwrap();

        // 预置 .1 .2 .3（达到上限）
        fs::write(log_dir.join("pastoid.log.1"), "b1").unwrap();
        fs::write(log_dir.join("pastoid.log.2"), "b2").unwrap();
        fs::write(log_dir.join("pastoid.log.3"), "b3").unwrap();

        // 当前日志超过阈值
        let over = (MAX_LOG_SIZE + 1024) as usize;
        fs::write(log_dir.join("pastoid.log"), "x".repeat(over)).unwrap();

        rotate_logs(&log_dir).unwrap();

        // 最旧的 .3 被删除，但 .2 移到 .3，.1 移到 .2，current → .1
        assert!(!log_dir.join("pastoid.log.4").exists());
        assert!(log_dir.join("pastoid.log.3").exists());
        assert!(log_dir.join("pastoid.log.2").exists());
        assert!(log_dir.join("pastoid.log.1").exists());

        // 验证内容链式移动
        assert_eq!(fs::read_to_string(log_dir.join("pastoid.log.3")).unwrap(), "b2");
        assert_eq!(fs::read_to_string(log_dir.join("pastoid.log.2")).unwrap(), "b1");
    }

    #[test]
    fn test_rotate_state_moves_files() {
        let temp_dir = create_temp_dir();
        let log_dir = temp_dir.path().join("logs");
        fs::create_dir_all(&log_dir).unwrap();

        // Preseed log files
        fs::write(log_dir.join("pastoid.log"), b"current content").unwrap();
        fs::write(log_dir.join("pastoid.log.1"), b"backup1").unwrap();
        fs::write(log_dir.join("pastoid.log.2"), b"backup2").unwrap();

        let path = log_dir.join("pastoid.log");
        let file = fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        let mut state = RotateState {
            file: Some(file),
            path,
            log_dir: log_dir.clone(),
            current_size: 999_999_999,
            max_size: 100,
            max_backups: 2,
        };

        state.rotate();

        assert!(log_dir.join("pastoid.log").exists());
        assert!(log_dir.join("pastoid.log.1").exists());
        assert!(log_dir.join("pastoid.log.2").exists());
        assert!(!log_dir.join("pastoid.log.3").exists());

        assert_eq!(
            fs::read_to_string(log_dir.join("pastoid.log.1")).unwrap(),
            "current content"
        );
        assert_eq!(
            fs::read_to_string(log_dir.join("pastoid.log.2")).unwrap(),
            "backup1"
        );
    }

    #[test]
    fn test_delete_image_file_removes_file_inside_data_dir() {
        let temp_dir = create_temp_dir();
        let data_dir = temp_dir.path().join("data");
        let images_dir = data_dir.join("images");
        fs::create_dir_all(&images_dir).unwrap();
        let image = images_dir.join("shot.png");
        fs::write(&image, b"png").unwrap();

        delete_image_file(&data_dir, "images/shot.png");

        assert!(!image.exists());
    }

    #[test]
    fn test_delete_image_file_refuses_path_outside_data_dir() {
        let temp_dir = create_temp_dir();
        let data_dir = temp_dir.path().join("data");
        fs::create_dir_all(&data_dir).unwrap();
        let outside = temp_dir.path().join("outside.txt");
        fs::write(&outside, b"keep me").unwrap();

        delete_image_file(&data_dir, "../outside.txt");

        assert!(outside.exists());
    }

    #[test]
    fn test_delete_image_file_missing_file_is_noop() {
        let temp_dir = create_temp_dir();
        let data_dir = temp_dir.path().join("data");
        fs::create_dir_all(&data_dir).unwrap();

        delete_image_file(&data_dir, "images/nope.png");
    }

    #[test]
    fn test_enforce_max_items_deletes_trimmed_image_file() {
        let temp_dir = create_temp_dir();
        let data_dir = temp_dir.path().join("data");
        let images_dir = data_dir.join("images");
        fs::create_dir_all(&images_dir).unwrap();
        let image = images_dir.join("old.png");
        fs::write(&image, b"png").unwrap();

        let mut old_image = ClipboardItem::new("images/old.png".to_string(), "image");
        old_image.created_at = 1_000;
        let mut newer_text = ClipboardItem::new("newer".to_string(), "text");
        newer_text.created_at = 2_000;
        let mut items = vec![old_image, newer_text];

        enforce_max_items(&mut items, 1, &data_dir);

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].content, "newer");
        assert!(!image.exists(), "trimmed image file should be removed");
    }
}
