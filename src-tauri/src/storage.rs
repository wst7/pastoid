use crate::models::{ClipboardItem, Settings};
use log::{LevelFilter, Log, Metadata, Record};
use simplelog::{ColorChoice, CombinedLogger, ConfigBuilder, SharedLogger, TermLogger, TerminalMode};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::Manager;

const OLD_APP_IDENTIFIER: &str = "com.pastoid.app";

/// 记录「自定义数据目录」的指针文件，放在默认目录里。
/// 位置必须固定，否则自定义目录本身就无从查起。
const DATA_DIR_POINTER: &str = "location.txt";

/// 平台默认数据目录。
///
/// Windows 用 `%LOCALAPPDATA%` 而不是 `%APPDATA%`：Roaming 会被漫游配置文件一起同步，
/// 几百 MB 的图片走网络拷贝，域环境下非常难受。
pub fn default_data_dir(app_handle: &tauri::AppHandle) -> PathBuf {
    let resolved = if cfg!(target_os = "windows") {
        app_handle.path().app_local_data_dir()
    } else {
        app_handle.path().app_data_dir()
    };
    resolved.expect("Failed to get app data dir")
}

fn data_dir_pointer_path(default_dir: &Path) -> PathBuf {
    default_dir.join(DATA_DIR_POINTER)
}

/// 指针文件内容：第一行是最终目录，可选第二行 `from=<旧目录>` 表示
/// 「下次启动时把数据从旧目录搬过来」。切换目录总是发生在重启后，
/// 所以搬迁必须延后到下次启动、在仓库和监控线程初始化之前完成。
struct DataDirPointer {
    target: PathBuf,
    move_from: Option<PathBuf>,
}

fn read_data_dir_pointer(default_dir: &Path) -> Option<DataDirPointer> {
    let raw = fs::read_to_string(data_dir_pointer_path(default_dir)).ok()?;
    let mut lines = raw.lines().map(str::trim).filter(|l| !l.is_empty());

    let target = lines.next()?;
    let move_from = lines
        .next()
        .and_then(|line| line.strip_prefix("from="))
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);

    Some(DataDirPointer {
        target: PathBuf::from(target),
        move_from,
    })
}

/// 读取「下次启动要迁移到」的目标目录（没有待处理变更时返回 None）。
/// 设置页用它把用户刚选的路径显示出来，而不是干等重启。
pub fn pending_data_dir(default_dir: &Path) -> Option<PathBuf> {
    read_data_dir_pointer(default_dir).map(|p| p.target)
}

/// 写入（或清除）自定义目录指针
pub fn write_data_dir_pointer(
    default_dir: &Path,
    target: Option<&Path>,
    move_from: Option<&Path>,
) -> Result<(), String> {
    let pointer = data_dir_pointer_path(default_dir);
    match target {
        Some(dir) => {
            let mut content = dir.to_string_lossy().to_string();
            if let Some(from) = move_from {
                content.push_str(&format!("\nfrom={}", from.to_string_lossy()));
            }
            fs::write(&pointer, content).map_err(|e| format!("写入数据目录指针失败: {}", e))
        }
        None => {
            if pointer.exists() {
                fs::remove_file(&pointer).map_err(|e| format!("清除数据目录指针失败: {}", e))?;
            }
            Ok(())
        }
    }
}

/// 目录存在、能创建、能写入
fn dir_is_usable(dir: &Path) -> bool {
    if fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".pastoid-write-probe");
    let ok = fs::write(&probe, b"ok").is_ok();
    let _ = fs::remove_file(&probe);
    ok
}

/// 移动单个文件/目录；跨卷 rename 会失败，退化成复制 + 删除
fn move_entry(src: &Path, dst: &Path) -> Result<(), String> {
    if fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    if src.is_dir() {
        copy_dir_recursive(src, dst);
        fs::remove_dir_all(src).map_err(|e| format!("删除源目录失败: {}", e))
    } else {
        fs::copy(src, dst).map_err(|e| format!("复制文件失败: {}", e))?;
        fs::remove_file(src).map_err(|e| format!("删除源文件失败: {}", e))
    }
}

/// 把 `from` 里的内容搬到 `to`（目标已有同名项则跳过，不覆盖）
fn move_children(from: &Path, to: &Path) -> Result<(), String> {
    let entries = match fs::read_dir(from) {
        Ok(entries) => entries,
        Err(_) => return Ok(()), // 源目录不存在，没什么可搬的
    };
    fs::create_dir_all(to).map_err(|e| format!("创建目标目录失败: {}", e))?;

    for entry in entries.flatten() {
        let name = entry.file_name();
        let src = entry.path();
        let dst = to.join(&name);
        if dst.exists() || name == DATA_DIR_POINTER {
            continue;
        }
        if let Err(e) = move_entry(&src, &dst) {
            log::warn!("迁移 {:?} -> {:?} 失败: {}", src, dst, e);
        }
    }
    Ok(())
}

/// 目标目录还没有任何数据时，才把源目录搬过去
fn migrate_if_empty(from: &Path, to: &Path) {
    if from == to || !from.exists() {
        return;
    }
    if to.join("settings.json").exists() || to.join("clipboard_data.json").exists() {
        return;
    }
    log::info!("迁移数据目录: {:?} -> {:?}", from, to);
    if let Err(e) = move_children(from, to) {
        log::error!("迁移数据目录失败: {}", e);
    }
}

/// 老版本 Windows 把数据放在 Roaming，升级后要搬到 Local
#[cfg(target_os = "windows")]
fn legacy_roaming_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("com.wst7.pastoid"))
}

/// 解析本次启动要用的数据目录。
///
/// 返回 `(目录, 告警)`：这个函数在 logger 初始化之前执行，直接 log 会被丢掉，
/// 所以把要提示的信息带回去，等 logger 就绪后再打。
pub fn get_data_dir(app_handle: &tauri::AppHandle) -> (PathBuf, Vec<String>) {
    let mut notes = Vec::new();
    let default_dir = default_data_dir(app_handle);
    if let Err(e) = fs::create_dir_all(&default_dir) {
        notes.push(format!("创建默认数据目录失败: {}", e));
    }

    // 1) 用户自定义目录优先
    if let Some(pointer) = read_data_dir_pointer(&default_dir) {
        if pointer.target == default_dir {
            // 指向默认目录等于没自定义，把指针清掉免得以后换默认目录时被带偏
            let _ = write_data_dir_pointer(&default_dir, None, None);
        } else if dir_is_usable(&pointer.target) {
            if let Some(from) = pointer.move_from.as_deref() {
                migrate_if_empty(from, &pointer.target);
                // 搬完了就把 from 那行去掉，避免每次启动都重试
                let _ = write_data_dir_pointer(&default_dir, Some(&pointer.target), None);
            }
            notes.push(format!("使用自定义数据目录: {}", pointer.target.display()));
            return (pointer.target, notes);
        } else {
            notes.push(format!(
                "自定义数据目录不可用（{}），本次回落到默认目录",
                pointer.target.display()
            ));
        }
    }

    // 2) Windows：把 Roaming 里的旧数据搬到 Local
    #[cfg(target_os = "windows")]
    if let Some(legacy) = legacy_roaming_dir() {
        if legacy != default_dir && legacy.exists() {
            notes.push(format!("检测到旧的 Roaming 数据目录: {}", legacy.display()));
            migrate_if_empty(&legacy, &default_dir);
        }
    }

    (default_dir, notes)
}

/// 首次运行标记：存在就说明已经问过用户「数据存哪里」了
const FIRST_RUN_MARKER: &str = ".first-run-done";

/// 是否首次运行：还没问过，且用户也没指定过目录
pub fn is_first_run(data_dir: &Path) -> bool {
    !data_dir.join(FIRST_RUN_MARKER).exists() && read_data_dir_pointer(data_dir).is_none()
}

pub fn mark_first_run_done(data_dir: &Path) {
    if let Err(e) = fs::write(data_dir.join(FIRST_RUN_MARKER), b"1") {
        log::warn!("写入首次运行标记失败: {}", e);
    }
}

/// 返回（总字节数，图片文件数）
pub fn dir_usage(dir: &Path) -> (u64, usize) {
    let mut bytes = 0u64;
    let mut images = 0usize;

    for file in ["clipboard_data.json", "settings.json"] {
        if let Ok(meta) = fs::metadata(dir.join(file)) {
            bytes += meta.len();
        }
    }

    if let Ok(entries) = fs::read_dir(dir.join("images")) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    bytes += meta.len();
                    images += 1;
                }
            }
        }
    }

    (bytes, images)
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
    // 必须和 get_data_dir 用同一个默认目录，否则 Windows 上会把数据搬进 Roaming
    let new_dir = default_data_dir(app_handle);

    // 如果新目录已有数据，说明不是首次启动，跳过迁移
    let new_settings = new_dir.join("settings.json");
    if new_settings.exists() {
        return;
    }

    // 构建旧目录路径（基于旧 identifier），按平台惯例拼。
    let old_dir = if cfg!(target_os = "macos") {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("~"))
            .join("Application Support")
            .join(OLD_APP_IDENTIFIER)
    } else if cfg!(target_os = "windows") {
        // 老版本（Roaming）和新版本（Local）都找一遍，谁有数据算谁
        let roaming = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("%APPDATA%"))
            .join(OLD_APP_IDENTIFIER);
        if roaming.exists() {
            roaming
        } else {
            dirs::data_local_dir()
                .unwrap_or_else(|| PathBuf::from("%LOCALAPPDATA%"))
                .join(OLD_APP_IDENTIFIER)
        }
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

pub(crate) fn get_images_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("images")
}

pub(crate) fn ensure_images_dir(data_dir: &Path) -> Result<PathBuf, String> {
    let dir = get_images_dir(data_dir);
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create images dir: {}", e))?;
    Ok(dir)
}

/// 删除 images 目录下所有没有被历史条目引用的文件，返回删除数量。
///
/// 这是兜底清理：正常流程里图片只在确定入库时才落盘，删条目时也会同步删文件，
/// 但历史版本的 bug 曾经留下过大量重复文件，这里负责把这类孤儿文件收干净。
pub(crate) fn cleanup_orphan_images(
    data_dir: &Path,
    referenced: &std::collections::HashSet<String>,
) -> usize {
    // 启动清理与剪贴板监控是并行的：刚写进去、还没来得及入库的新图必须放过，
    // 否则会把监控线程刚落盘的文件删掉。5 分钟足够覆盖任何入库窗口。
    cleanup_orphan_images_older_than(data_dir, referenced, std::time::Duration::from_secs(300))
}

fn cleanup_orphan_images_older_than(
    data_dir: &Path,
    referenced: &std::collections::HashSet<String>,
    min_age: std::time::Duration,
) -> usize {
    let images_dir = get_images_dir(data_dir);
    let entries = match fs::read_dir(&images_dir) {
        Ok(entries) => entries,
        Err(_) => return 0,
    };

    let mut removed = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let file_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(name) => name,
            None => continue,
        };

        let relative = format!("images/{}", file_name);
        if referenced.contains(&relative) {
            continue;
        }

        if min_age > std::time::Duration::ZERO {
            let too_new = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.elapsed().ok())
                .map(|age| age < min_age)
                .unwrap_or(false);
            if too_new {
                continue;
            }
        }

        match fs::remove_file(&path) {
            Ok(()) => removed += 1,
            Err(e) => log::warn!("清理孤儿图片失败 {:?}: {}", path, e),
        }
    }

    if removed > 0 {
        log::info!("清理孤儿图片文件 {} 个", removed);
    }

    removed
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

    let mut pinned: Vec<_> = items.iter().filter(|i| i.is_pinned).cloned().collect();
    pinned.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    // 置顶条目本身也可能多于上限（用户可以把上限调小），先按时间砍掉多的，
    // 否则 items.len() 会永远大于 max_items，历史无上限增长。
    // 被砍掉的置顶图片同样要删文件，不能只在非置顶分支里清理。
    if pinned.len() > max {
        for item in pinned.drain(max..) {
            if item.item_type == "image" && !item.content.is_empty() {
                delete_image_file(data_dir, &item.content);
            }
        }
    }

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
            Ok(mut settings) => {
                // 老数据（或从 macOS 同步过来的设置）里存的是 Cmd/Option 写法，
                // 在 Windows 上展示成 Ctrl/Alt 才对得上用户实际按的键
                if !cfg!(target_os = "macos") {
                    settings.shortcut =
                        settings.shortcut.replace("Cmd", "Ctrl").replace("Option", "Alt");
                }
                settings
            }
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
        assert_eq!(settings.shortcut, crate::models::default_shortcut());
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

    #[test]
    fn test_cleanup_orphan_images_keeps_referenced() {
        let temp_dir = create_temp_dir();
        let data_dir = temp_dir.path().join("data");
        let images_dir = data_dir.join("images");
        fs::create_dir_all(&images_dir).unwrap();

        fs::write(images_dir.join("keep.png"), b"keep").unwrap();
        fs::write(images_dir.join("orphan.png"), b"orphan").unwrap();
        fs::write(images_dir.join("orphan2.png"), b"orphan2").unwrap();

        let mut referenced = std::collections::HashSet::new();
        referenced.insert("images/keep.png".to_string());

        // 测试里文件都是刚写的，这里把年龄门槛设为 0，等价于「全部都可删」
        let removed =
            cleanup_orphan_images_older_than(&data_dir, &referenced, std::time::Duration::ZERO);

        assert_eq!(removed, 2);
        assert!(images_dir.join("keep.png").exists());
        assert!(!images_dir.join("orphan.png").exists());
        assert!(!images_dir.join("orphan2.png").exists());
    }

    #[test]
    fn test_cleanup_orphan_images_skips_fresh_files() {
        // 默认门槛是 5 分钟：刚写进来的新图不能删，否则会误删监控线程刚落盘的文件
        let temp_dir = create_temp_dir();
        let data_dir = temp_dir.path().join("data");
        let images_dir = data_dir.join("images");
        fs::create_dir_all(&images_dir).unwrap();
        fs::write(images_dir.join("fresh.png"), b"fresh").unwrap();

        let removed = cleanup_orphan_images(&data_dir, &std::collections::HashSet::new());

        assert_eq!(removed, 0);
        assert!(images_dir.join("fresh.png").exists());
    }

    #[test]
    fn test_cleanup_orphan_images_missing_dir_is_noop() {
        let temp_dir = create_temp_dir();
        let data_dir = temp_dir.path().join("no-images-here");

        let removed =
            cleanup_orphan_images_older_than(&data_dir, &std::collections::HashSet::new(), std::time::Duration::ZERO);

        assert_eq!(removed, 0);
    }

    #[test]
    fn test_data_dir_pointer_roundtrip() {
        let temp = create_temp_dir();
        let default_dir = temp.path().join("default");
        fs::create_dir_all(&default_dir).unwrap();
        let target = temp.path().join("custom");

        write_data_dir_pointer(&default_dir, Some(&target), Some(Path::new("/old/dir"))).unwrap();
        let pointer = read_data_dir_pointer(&default_dir).unwrap();
        assert_eq!(pointer.target, target);
        assert_eq!(pointer.move_from.as_deref(), Some(Path::new("/old/dir")));

        // 清除指针
        write_data_dir_pointer(&default_dir, None, None).unwrap();
        assert!(read_data_dir_pointer(&default_dir).is_none());
    }

    #[test]
    fn test_first_run_detection() {
        let temp = create_temp_dir();
        let data_dir = temp.path().join("data");
        fs::create_dir_all(&data_dir).unwrap();

        assert!(is_first_run(&data_dir));
        mark_first_run_done(&data_dir);
        assert!(!is_first_run(&data_dir));
    }

    #[test]
    fn test_first_run_is_false_when_dir_is_customized() {
        let temp = create_temp_dir();
        let data_dir = temp.path().join("data");
        fs::create_dir_all(&data_dir).unwrap();
        // 用户已经指定过目录：不该再弹引导
        write_data_dir_pointer(&data_dir, Some(&temp.path().join("custom")), None).unwrap();
        assert!(!is_first_run(&data_dir));
    }

    #[test]
    fn test_migrate_if_empty_moves_everything() {
        let temp = create_temp_dir();
        let from = temp.path().join("from");
        let to = temp.path().join("to");
        fs::create_dir_all(from.join("images")).unwrap();
        fs::write(from.join("clipboard_data.json"), "[]").unwrap();
        fs::write(from.join("images/a.png"), b"png").unwrap();

        migrate_if_empty(&from, &to);

        assert!(to.join("clipboard_data.json").exists());
        assert!(to.join("images/a.png").exists());
        // 源目录里的数据已经被搬走
        assert!(!from.join("clipboard_data.json").exists());
    }

    #[test]
    fn test_migrate_if_empty_keeps_target_data() {
        let temp = create_temp_dir();
        let from = temp.path().join("from");
        let to = temp.path().join("to");
        fs::create_dir_all(&from).unwrap();
        fs::create_dir_all(&to).unwrap();
        fs::write(from.join("clipboard_data.json"), "[1]").unwrap();
        // 目标已有数据：不能覆盖
        fs::write(to.join("clipboard_data.json"), "[2]").unwrap();

        migrate_if_empty(&from, &to);

        assert_eq!(fs::read_to_string(to.join("clipboard_data.json")).unwrap(), "[2]");
        assert!(from.join("clipboard_data.json").exists());
    }

    #[test]
    fn test_dir_usage_counts_images_and_json() {
        let temp = create_temp_dir();
        let data_dir = temp.path().join("data");
        fs::create_dir_all(data_dir.join("images")).unwrap();
        fs::write(data_dir.join("clipboard_data.json"), "[]").unwrap();
        fs::write(data_dir.join("images/a.png"), b"12345").unwrap();
        fs::write(data_dir.join("images/b.png"), b"123").unwrap();

        let (bytes, images) = dir_usage(&data_dir);
        assert_eq!(images, 2);
        assert_eq!(bytes, 2 + 5 + 3);
    }

    #[test]
    fn test_enforce_max_items_truncates_pinned_when_over_limit() {
        // 置顶条目多于上限时也要截断，否则历史会无上限增长
        let temp_dir = create_temp_dir();

        let mut a = ClipboardItem::new("a".to_string(), "text");
        a.is_pinned = true;
        a.created_at = 1;
        let mut b = ClipboardItem::new("b".to_string(), "text");
        b.is_pinned = true;
        b.created_at = 2;
        let mut c = ClipboardItem::new("c".to_string(), "text");
        c.is_pinned = true;
        c.created_at = 3;

        let mut items = vec![a, b, c];
        enforce_max_items(&mut items, 2, temp_dir.path());

        assert_eq!(items.len(), 2);
        // 保留最新的两条
        assert!(items.iter().any(|i| i.content == "b"));
        assert!(items.iter().any(|i| i.content == "c"));
        assert!(!items.iter().any(|i| i.content == "a"));
    }
}
