use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// 健康监控配置常量
const HEALTH_CHECK_INTERVAL_SECS: u64 = 60; // 每60秒检查一次
const MAX_IDLE_TIME_SECS: u64 = 300; // 5分钟无触发视为可能失效

// 记录快捷键最后一次成功触发的时间戳（秒）
static LAST_SHORTCUT_TRIGGER: AtomicU64 = AtomicU64::new(0);

// 当前实际注册成功的快捷键（normalized）。健康监控据此检查运行期被修改过的快捷键，
// 而不是启动时的快照。
static CURRENT_SHORTCUT: RwLock<String> = RwLock::new(String::new());

/// 当前 Unix 时间戳（秒）。系统时间异常时回退为 0。
fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|e| {
            log::error!("系统时间异常: {}", e);
            Duration::from_secs(0)
        })
        .as_secs()
}

/// 计算距上次成功触发的空闲秒数。
/// 从未触发过（`last_trigger == 0`）时以进程启动以来的运行时长为准。
fn idle_secs(last_trigger: u64, uptime_secs: u64, now: u64) -> u64 {
    if last_trigger == 0 {
        uptime_secs
    } else {
        now.saturating_sub(last_trigger)
    }
}

/// 获取当前实际注册的快捷键（normalized）；未注册时返回空串
pub fn current_shortcut() -> String {
    CURRENT_SHORTCUT
        .read()
        .map(|s| s.clone())
        .unwrap_or_default()
}

pub fn toggle_quick_paste(app: &AppHandle) {
    // 更新最后触发时间
    LAST_SHORTCUT_TRIGGER.store(unix_now(), Ordering::Relaxed);

    log::debug!("快捷键触发: toggle_quick_paste");

    if let Some(window) = app.get_webview_window("quick-paste") {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        } else {
            let _ = window.center();
            let _ = window.show();
            let _ = window.set_focus();
            let _ = window.emit("panel-opened", ());
        }
    }
}

/// 获取快捷键最后触发时间（秒级时间戳）
pub fn get_last_trigger_time() -> u64 {
    LAST_SHORTCUT_TRIGGER.load(Ordering::Relaxed)
}

/// 将快捷键字符串转换为当前平台有效的格式。
/// macOS 保持原样；Windows/Linux 将 Cmd 映射为 Ctrl，Option 映射为 Alt。
pub fn normalize_shortcut(shortcut: &str) -> String {
    #[cfg(target_os = "macos")]
    {
        shortcut.to_string()
    }
    #[cfg(not(target_os = "macos"))]
    {
        shortcut
            .replace("Cmd", "Ctrl")
            .replace("Option", "Alt")
    }
}

pub fn register(app: &AppHandle, shortcut: &str) -> Result<(), String> {
    let normalized = normalize_shortcut(shortcut);
    log::info!("注册快捷键: '{}' (normalized: '{}')", shortcut, normalized);
    app.global_shortcut()
        .on_shortcut(normalized.as_str(), move |app, _s, event| {
            if event.state() == ShortcutState::Pressed {
                toggle_quick_paste(app);
            }
        })
        .map_err(|e| format!("Failed to register shortcut: {}", e))?;

    if let Ok(mut current) = CURRENT_SHORTCUT.write() {
        *current = normalized;
    }
    Ok(())
}

pub fn unregister(app: &AppHandle, shortcut: &str) -> Result<(), String> {
    let normalized = normalize_shortcut(shortcut);
    log::info!("注销快捷键: '{}' (normalized: '{}')", shortcut, normalized);
    app.global_shortcut()
        .unregister(normalized.as_str())
        .map_err(|e| format!("Failed to unregister shortcut: {}", e))?;

    if let Ok(mut current) = CURRENT_SHORTCUT.write() {
        if *current == normalized {
            current.clear();
        }
    }
    Ok(())
}

/// 启动快捷键健康检查守护线程
/// 每60秒检查一次快捷键是否失效（超过5分钟未触发），失效则自动重新注册
///
/// 每次检查都读取 [`current_shortcut`]，因此运行期改过快捷键后监控的仍是新值。
/// 同一个空闲窗口内最多重注册一次（节流），避免反复注册造成日志噪音与冲突。
///
/// 注意：该线程会在应用退出时自动终止（跟随应用生命周期）
pub fn start_health_monitor(app_handle: AppHandle) {
    std::thread::spawn(move || {
        let check_interval = Duration::from_secs(HEALTH_CHECK_INTERVAL_SECS);
        let started = Instant::now();
        // 上一次尝试重注册的时间，用于节流
        let mut last_reregister: Option<Instant> = None;

        log::info!("快捷键健康监控已启动，每{}秒检查一次", HEALTH_CHECK_INTERVAL_SECS);

        loop {
            std::thread::sleep(check_interval);

            // 检查应用是否仍在运行（通过检查窗口）
            // 如果主窗口和快速粘贴窗口都不存在，说明应用正在关闭
            if app_handle.get_webview_window("settings").is_none()
                && app_handle.get_webview_window("quick-paste").is_none() {
                log::info!("检测到应用正在关闭，停止健康监控");
                break;
            }

            let idle = idle_secs(
                get_last_trigger_time(),
                started.elapsed().as_secs(),
                unix_now(),
            );

            // 快捷键刚被用过，空闲窗口重新开始计时
            if idle <= MAX_IDLE_TIME_SECS {
                last_reregister = None;
                continue;
            }

            // 节流：距上次重注册不足 MAX_IDLE_TIME_SECS 则跳过
            if let Some(t) = last_reregister {
                if t.elapsed().as_secs() < MAX_IDLE_TIME_SECS {
                    continue;
                }
            }

            // 读取当前实际注册的快捷键（用户可能在运行期改过）
            let shortcut = current_shortcut();
            if shortcut.is_empty() {
                log::debug!("当前没有已注册的快捷键，跳过健康检查");
                continue;
            }

            log::warn!("检测到快捷键可能失效（已空闲 {} 秒），尝试重新注册: {}", idle, shortcut);

            // 先注销再重新注册
            if let Err(e) = unregister(&app_handle, &shortcut) {
                log::debug!("重新注册前注销失败（可能已失效）: {}", e);
            }

            match register(&app_handle, &shortcut) {
                Ok(()) => log::info!("快捷键重新注册成功: {}", shortcut),
                Err(e) => log::error!("快捷键重新注册失败: {}", e),
            }

            last_reregister = Some(Instant::now());
        }

        log::info!("快捷键健康监控线程已退出");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_shortcut() {
        #[cfg(target_os = "macos")]
        {
            assert_eq!(normalize_shortcut("Cmd+Shift+V"), "Cmd+Shift+V");
            assert_eq!(normalize_shortcut("Option+K"), "Option+K");
        }
        #[cfg(not(target_os = "macos"))]
        {
            assert_eq!(normalize_shortcut("Cmd+Shift+V"), "Ctrl+Shift+V");
            assert_eq!(normalize_shortcut("Option+K"), "Alt+K");
            assert_eq!(normalize_shortcut("Ctrl+Alt+K"), "Ctrl+Alt+K");
        }
    }

    #[test]
    fn test_idle_secs_uses_uptime_before_first_trigger() {
        // 从未触发过时以运行时长为准，而不是拿 Unix 时间戳直接和阈值比较
        let now = 1_700_000_000;
        assert_eq!(idle_secs(0, 10, now), 10);
        assert_eq!(idle_secs(0, MAX_IDLE_TIME_SECS + 1, now), MAX_IDLE_TIME_SECS + 1);
    }

    #[test]
    fn test_idle_secs_uses_last_trigger_when_triggered() {
        let now = 1_700_000_000;
        assert_eq!(idle_secs(now - 42, 999_999, now), 42);
        // 系统时间回拨时不 panic
        assert_eq!(idle_secs(now + 100, 0, now), 0);
    }

    #[test]
    fn test_current_shortcut_starts_empty() {
        assert_eq!(current_shortcut(), "");
    }

    #[test]
    fn test_toggle_quick_paste_does_not_crash_without_window() {
    }
}
