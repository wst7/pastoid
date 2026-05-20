use crate::models::{AppState, ClipboardItem};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

pub fn start_clipboard_monitor(app_handle: AppHandle) {
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();

    std::thread::spawn(move || {
        let mut last_content: Option<String> = None;
        let mut consecutive_errors = 0u32;
        let max_errors = 5u32;

        while running_clone.load(Ordering::Relaxed) {
            // 退避机制：错误越多，休眠时间越长
            let sleep_duration = match consecutive_errors {
                0 => std::time::Duration::from_secs(1),
                1 => std::time::Duration::from_secs(2),
                2 => std::time::Duration::from_secs(5),
                _ => std::time::Duration::from_secs(10),
            };
            std::thread::sleep(sleep_duration);

            let result = check_clipboard(&app_handle, &mut last_content);

            match result {
                Ok(Some(new_item)) => {
                    consecutive_errors = 0;
                    // 发送事件但不阻塞
                    let _ = app_handle.emit("clipboard_changed", new_item);
                }
                Ok(None) => {
                    consecutive_errors = 0;
                }
                Err(_) => {
                    consecutive_errors += 1;
                    if consecutive_errors >= max_errors {
                        eprintln!("连续 {} 次读取剪贴板失败，暂停监控", max_errors);
                    }
                }
            }
        }
    });
}

/// 检查剪贴板，返回新条目（如果有）
fn check_clipboard(
    app_handle: &AppHandle,
    last_content: &mut Option<String>,
) -> Result<Option<ClipboardItem>, ()> {
    let state = app_handle.state::<AppState>();

    // 使用作用域确保 arboard 操作不持有任何锁
    let new_item = {
        match arboard::Clipboard::new() {
            Ok(mut clipboard) => match clipboard.get_text() {
                Ok(text) => {
                    if last_content.as_ref() != Some(&text) && !text.trim().is_empty() {
                        let item = ClipboardItem::new(text.clone(), "text");
                        *last_content = Some(text);
                        Some(item)
                    } else {
                        None
                    }
                }
                Err(_) => None,
            },
            Err(_) => None,
        }
    };

    if let Some(ref item) = new_item {
        // 只改内存，文件由后台 flush 线程处理
        if state.repo.add(item.clone()) {
            return Ok(Some(item.clone()));
        }
    }

    Ok(None)
}

/// 停止监控线程（供外部调用）
pub fn stop_clipboard_monitor(running: Arc<AtomicBool>) {
    running.store(false, Ordering::Relaxed);
}
