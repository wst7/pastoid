use crate::models::{AppState, ClipboardItem};
use crate::repository::AddOutcome;
use crate::storage;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

use std::fs;

/// 轮询间隔
const POLL_INTERVAL: Duration = Duration::from_secs(1);
/// 连续失败时的最大退避
const MAX_BACKOFF: Duration = Duration::from_secs(30);
/// 单条文本上限，避免把巨型文本塞进历史
const MAX_TEXT_BYTES: usize = 512 * 1024;
/// 单张图片像素上限（宽 × 高）
const MAX_IMAGE_PIXELS: u64 = 40_000_000;
/// 单张图片字节上限
const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;

/// 监控线程的跨轮次状态。
///
/// 作用只有一个：**判断剪贴板内容是否真的变了**，避免每轮轮询都重复处理同一份内容。
#[derive(Default)]
struct MonitorState {
    /// 上一次处理过的文本
    last_text: Option<String>,
    /// 上一次处理过的图片哈希
    last_image_hash: Option<String>,
    /// 上一次处理过的图片文件：路径、大小、修改时间、哈希。
    /// 有了它，同一份文件在被覆盖之前不需要再解码一次。
    last_image_file: Option<ImageFileKey>,
}

#[derive(Clone, PartialEq, Eq)]
struct ImageFileKey {
    path: PathBuf,
    len: u64,
    mtime_ms: u128,
    hash: String,
}

pub fn start_clipboard_monitor(app_handle: AppHandle) {
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();

    std::thread::Builder::new()
        .name("clipboard-monitor".into())
        .spawn(move || {
            let mut state = MonitorState::default();
            let mut failures: u32 = 0;

            while running_clone.load(Ordering::Relaxed) {
                std::thread::sleep(backoff(failures));

                // 监控线程一旦 panic 就永久停摆，外部完全看不出来。
                // 这里兜住 panic，让线程至少活下去并留下日志。
                let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    poll(&app_handle, &mut state)
                }));

                match outcome {
                    Ok(Ok(Some(item))) => {
                        failures = 0;
                        let _ = app_handle.emit("clipboard_changed", item);
                    }
                    Ok(Ok(None)) => failures = 0,
                    Ok(Err(e)) => {
                        failures = failures.saturating_add(1);
                        log::warn!("读取剪贴板失败（连续 {} 次）: {}", failures, e);
                    }
                    Err(_) => {
                        failures = failures.saturating_add(1);
                        log::error!("剪贴板监控线程发生 panic，已捕获并继续运行");
                    }
                }
            }
        })
        .expect("failed to spawn clipboard monitor thread");
}

/// 失败次数越多，退避越久；成功一次立刻恢复 1 秒。
fn backoff(failures: u32) -> Duration {
    match failures {
        0 => POLL_INTERVAL,
        1..=3 => Duration::from_secs(2),
        4..=9 => Duration::from_secs(5),
        _ => MAX_BACKOFF,
    }
}

/// 读取一次剪贴板。返回 `Some(item)` 表示内容有变化且已写入仓库，需要通知前端。
fn poll(app: &AppHandle, state: &mut MonitorState) -> Result<Option<ClipboardItem>, String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| format!("打开剪贴板失败: {}", e))?;

    let text = clipboard
        .get_text()
        .ok()
        .filter(|t| !t.trim().is_empty())
        .filter(|t| t.len() <= MAX_TEXT_BYTES);

    // 1) 图片文件路径（截图工具、Finder 复制文件等场景）
    if let Some(t) = text.as_deref() {
        if is_image_path(t) && Path::new(t).is_file() {
            return image_from_file(app, state, Path::new(t));
        }
    }

    // 2) 剪贴板里的位图（截图直接进剪贴板）
    if let Ok(image) = clipboard.get_image() {
        return image_from_rgba(
            app,
            state,
            image.width as u32,
            image.height as u32,
            &image.bytes,
        );
    }

    // 3) 普通文本
    match text {
        Some(t) => text_item(app, state, t),
        None => Ok(None),
    }
}

/// 文本条目：历史里已有就提到顶部，没有就新增。
///
/// 注意这里不再用「和上一轮文本比较」来直接跳过——那样一旦条目被删除或被挤出历史，
/// 同一段文本就永远收不回来了。现在以仓库为准，`last_text` 只用来判断是否真的变了。
fn text_item(
    app: &AppHandle,
    state: &mut MonitorState,
    text: String,
) -> Result<Option<ClipboardItem>, String> {
    let app_state = app.state::<AppState>();
    let repo = app_state.repo.clone();

    let unchanged = state.last_text.as_deref() == Some(text.as_str());

    if unchanged {
        if repo.contains_text(&text) {
            return Ok(None);
        }
        // 用户刚把它删掉，别下一轮又加回来
        if is_suppressed(&app_state, &text) {
            return Ok(None);
        }
    } else {
        state.last_text = Some(text.clone());
        state.last_image_hash = None;
        state.last_image_file = None;
        // 剪贴板换成新内容了，之前对这条的抑制解除
        unsuppress(&app_state, &text);
    }

    if let Some(existing) = repo.find_by_content(&text, "text") {
        // 重新复制了历史里已有的内容：提到顶部，让用户看到「它回来了」
        return Ok(repo.bump(&existing.id));
    }

    let item = ClipboardItem::new(text, "text");
    match repo.add(item.clone()) {
        AddOutcome::Added => Ok(Some(item)),
        AddOutcome::Duplicate(id) => Ok(repo.bump(&id)),
    }
}

fn is_suppressed(state: &tauri::State<'_, AppState>, key: &str) -> bool {
    state
        .suppressed
        .lock()
        .map(|s| s.contains(key))
        .unwrap_or(false)
}

fn unsuppress(state: &tauri::State<'_, AppState>, key: &str) {
    if let Ok(mut set) = state.suppressed.lock() {
        set.remove(key);
    }
}

/// 从图片文件读取图片。
///
/// 关键点：**先判断文件是否处理过，再决定要不要解码**。
/// 之前这里是每次都重新解码并另存一张新 PNG，于是同一个文件路径停在剪贴板里时，
/// 每秒产生一个新条目 + 一张 500KB 的新文件（实测 21 小时写了 58992 个文件、29GB）。
fn image_from_file(
    app: &AppHandle,
    state: &mut MonitorState,
    path: &Path,
) -> Result<Option<ClipboardItem>, String> {
    let meta = fs::metadata(path).map_err(|e| format!("读取图片信息失败: {}", e))?;

    if meta.len() as usize > MAX_IMAGE_BYTES {
        log::warn!("跳过过大的图片文件: {} ({} 字节)", path.display(), meta.len());
        return Ok(None);
    }

    let mtime_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis())
        .unwrap_or(0);

    let repo = app.state::<AppState>().repo.clone();

    // 同一个文件且未修改过：不做任何解码工作
    if let Some(ref cached) = state.last_image_file {
        if cached.path == path && cached.len == meta.len() && cached.mtime_ms == mtime_ms {
            if repo.find_by_image_hash(&cached.hash).is_some() {
                return Ok(None);
            }
            // 用户删掉了这张图：保持不变，别再解码一次
            if is_suppressed(&app.state::<AppState>(), &cached.hash) {
                return Ok(None);
            }
            // 文件没变但条目被挤出了历史：往下走一次完整流程把它收回来
        }
    }

    let rgba = decode_file(path)?;
    let hash = blake3::hash(&rgba.bytes).to_hex().to_string();

    // 换了一张新图：解除对它之前可能存在的抑制
    if state.last_image_file.as_ref().map(|k| k.hash.as_str()) != Some(hash.as_str()) {
        unsuppress(&app.state::<AppState>(), &hash);
    }

    state.last_image_file = Some(ImageFileKey {
        path: path.to_path_buf(),
        len: meta.len(),
        mtime_ms,
        hash: hash.clone(),
    });

    store_image(app, state, rgba, hash)
}

/// 剪贴板位图（截图等）
fn image_from_rgba(
    app: &AppHandle,
    state: &mut MonitorState,
    width: u32,
    height: u32,
    bytes: &[u8],
) -> Result<Option<ClipboardItem>, String> {
    if width == 0 || height == 0 {
        return Ok(None);
    }
    if width as u64 * height as u64 > MAX_IMAGE_PIXELS {
        log::warn!("跳过过大的剪贴板图片: {}x{}", width, height);
        return Ok(None);
    }

    let hash = blake3::hash(bytes).to_hex().to_string();

    // 同一张图还在剪贴板里：直接跳过，不解码不落盘
    if state.last_image_hash.as_deref() == Some(hash.as_str())
        && app
            .state::<AppState>()
            .repo
            .find_by_image_hash(&hash)
            .is_some()
    {
        return Ok(None);
    }

    let rgba = DecodedImage {
        width,
        height,
        bytes: bytes.to_vec(),
    };

    // 换了一张新图：解除对它之前可能存在的抑制
    if state.last_image_hash.as_deref() != Some(hash.as_str()) {
        unsuppress(&app.state::<AppState>(), &hash);
    }

    store_image(app, state, rgba, hash)
}

struct DecodedImage {
    width: u32,
    height: u32,
    bytes: Vec<u8>,
}

fn decode_file(path: &Path) -> Result<DecodedImage, String> {
    let img = image::ImageReader::open(path)
        .map_err(|e| format!("无法打开图片: {}", e))?
        .decode()
        .map_err(|e| format!("无法解码图片: {}", e))?
        .to_rgba8();

    let (width, height) = img.dimensions();
    if width as u64 * height as u64 > MAX_IMAGE_PIXELS {
        return Err(format!("图片尺寸过大: {}x{}", width, height));
    }

    Ok(DecodedImage {
        width,
        height,
        bytes: img.into_raw(),
    })
}

/// 落盘 + 入库。
///
/// 顺序很重要：先按哈希查历史，命中就只做置顶；只有确定要新增时才写 PNG。
/// 文件名直接用哈希，同一张图不可能在磁盘上出现第二份。
fn store_image(
    app: &AppHandle,
    state: &mut MonitorState,
    rgba: DecodedImage,
    hash: String,
) -> Result<Option<ClipboardItem>, String> {
    let state_ref = app.state::<AppState>();
    let repo = state_ref.repo.clone();
    let data_dir = state_ref.data_dir.clone();

    if let Some(existing) = repo.find_by_image_hash(&hash) {
        state.last_image_hash = Some(hash);
        state.last_text = None;
        return Ok(repo.bump(&existing.id));
    }

    // 用户刚删掉这张图，别下一轮又加回来
    if is_suppressed(&state_ref, &hash) {
        return Ok(None);
    }

    let relative = write_png(&data_dir, &rgba, &hash)?;
    let item = ClipboardItem::new_image(relative.clone(), hash.clone());

    match repo.add(item.clone()) {
        AddOutcome::Added => {
            state.last_image_hash = Some(hash);
            state.last_text = None;
            Ok(Some(item))
        }
        AddOutcome::Duplicate(id) => {
            // 竞态：别的路径先入库了。文件名按哈希命名，文件可复用，只做置顶。
            state.last_image_hash = Some(hash);
            Ok(repo.bump(&id))
        }
    }
}

fn write_png(data_dir: &Path, image: &DecodedImage, hash: &str) -> Result<String, String> {
    let images_dir = storage::ensure_images_dir(data_dir)?;
    let file_name = format!("{}.png", &hash[..hash.len().min(40)]);
    let path = images_dir.join(&file_name);

    if path.exists() {
        return Ok(format!("images/{}", file_name));
    }

    let buffer = image::RgbaImage::from_raw(image.width, image.height, image.bytes.clone())
        .ok_or_else(|| "RGBA 数据与尺寸不匹配".to_string())?;

    buffer
        .save_with_format(&path, image::ImageFormat::Png)
        .map_err(|e| format!("保存 PNG 失败: {}", e))?;

    Ok(format!("images/{}", file_name))
}

/// 判断路径是否为常见图片扩展名。
///
/// 显式排除 URL 与 data URI：它们同样以 `.png` 之类结尾，但不是本地文件路径，
/// 交给 `Path::is_file()` 判断虽然也能挡住，但函数本身的语义应该是「本地图片路径」。
fn is_image_path(path: &str) -> bool {
    if path.contains("://") || path.starts_with("data:") {
        return false;
    }

    let lower = path.to_lowercase();
    [
        ".png", ".jpg", ".jpeg", ".gif", ".bmp", ".webp", ".tiff", ".tif", ".ico", ".heic",
        ".heif",
    ]
    .iter()
    .any(|ext| lower.ends_with(ext))
}

/// 停止监控线程（供外部调用）
pub fn stop_clipboard_monitor(running: Arc<AtomicBool>) {
    running.store(false, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_image_path_accepts_common_extensions() {
        assert!(is_image_path("/tmp/a.png"));
        assert!(is_image_path("/tmp/a.JPEG"));
        assert!(is_image_path("/tmp/a.heic"));
        assert!(is_image_path("C:/tmp/a.webp"));
    }

    #[test]
    fn test_is_image_path_rejects_others() {
        assert!(!is_image_path("https://example.com/a.png"));
        assert!(!is_image_path("/tmp/a.txt"));
        // 只是以扩展名结尾但不是路径，仍然只是文本
        assert!(!is_image_path("png"));
    }

    #[test]
    fn test_backoff_grows_with_failures() {
        assert_eq!(backoff(0), POLL_INTERVAL);
        assert!(backoff(1) > backoff(0));
        assert!(backoff(5) > backoff(1));
        assert_eq!(backoff(100), MAX_BACKOFF);
    }

    #[test]
    fn test_write_png_is_content_addressed() {
        let dir = tempfile::tempdir().unwrap();
        let image = DecodedImage {
            width: 1,
            height: 1,
            bytes: vec![255, 0, 0, 255],
        };

        let first = write_png(dir.path(), &image, "abcdef0123").unwrap();
        let second = write_png(dir.path(), &image, "abcdef0123").unwrap();

        // 同一张图两次写入命中同一个文件，不会产生第二份
        assert_eq!(first, second);
        assert_eq!(first, "images/abcdef0123.png");

        let count = std::fs::read_dir(dir.path().join("images")).unwrap().count();
        assert_eq!(count, 1);
    }
}
