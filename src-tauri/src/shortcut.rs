use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::RwLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// 健康监控配置常量
const HEALTH_CHECK_INTERVAL_SECS: u64 = 60; // 每60秒检查一次
const MAX_IDLE_TIME_SECS: u64 = 1800; // 30 分钟无触发才重注册：5 分钟太频繁，重注册瞬间按下的快捷键会丢

// 记录快捷键最后一次成功触发的时间戳（秒）
static LAST_SHORTCUT_TRIGGER: AtomicU64 = AtomicU64::new(0);

// 当前实际注册成功的快捷键（normalized）。健康监控据此检查运行期被修改过的快捷键，
// 而不是启动时的快照。
static CURRENT_SHORTCUT: RwLock<String> = RwLock::new(String::new());

// 应用退出时置位，让健康监控线程停止干预快捷键
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

/// 请求健康监控线程退出（应用退出时调用）
pub fn request_shutdown() {
    SHUTDOWN_REQUESTED.store(true, Ordering::Relaxed);
}

fn shutdown_requested() -> bool {
    SHUTDOWN_REQUESTED.load(Ordering::Relaxed)
}

/// 分片睡眠，便于在应用退出时及时收尾而不是等满一个检查周期。
/// 返回 `false` 表示期间收到了退出请求。
fn sleep_interruptible(total: Duration) -> bool {
    const SLICE: Duration = Duration::from_secs(1);
    let mut slept = Duration::ZERO;
    while slept < total {
        if shutdown_requested() {
            return false;
        }
        let step = SLICE.min(total - slept);
        std::thread::sleep(step);
        slept += step;
    }
    !shutdown_requested()
}

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

/// 取已经转换好的 NSPanel。
///
/// 必须用 `get_webview_panel`，**不要**再调 `window.to_panel()`：那个方法每次都会
/// 新建一个 RawNSPanel 包装并覆盖管理器里的旧引用，旧 panel 被 dealloc 后 AppKit
/// 还在用，会让进程随机 SIGBUS（dealloc → NSKVODeallocate）。
/// 转换只在启动时做一次，见 lib.rs。
/// 面板是否可见。
/// macOS 上窗口是 NSPanel，交给 NSPanel 自己判断；tauri 的 is_visible / show
/// 对非激活面板并不可靠（隐藏后重新 show 经常不生效）。
fn panel_is_visible(app: &AppHandle, window: &tauri::WebviewWindow) -> bool {
    #[cfg(target_os = "macos")]
    {
        use tauri_nspanel::ManagerExt;
        if let Ok(panel) = app.get_webview_panel("quick-paste") {
            return panel.is_visible();
        }
    }
    window.is_visible().unwrap_or(false)
}

pub(crate) fn show_panel(app: &AppHandle, window: &tauri::WebviewWindow) {
    #[cfg(target_os = "macos")]
    {
        use tauri_nspanel::ManagerExt;
        if let Ok(panel) = app.get_webview_panel("quick-paste") {
            // make_first_responder + order_front_regardless + make_key_window
            panel.show();
            let _ = window.set_focus();
            return;
        }
    }
    let _ = window.show();
    let _ = window.set_focus();
}

pub fn toggle_quick_paste(app: &AppHandle) {
    // 更新最后触发时间
    LAST_SHORTCUT_TRIGGER.store(unix_now(), Ordering::Relaxed);

    log::debug!("快捷键触发: toggle_quick_paste");

    if let Some(window) = app.get_webview_window("quick-paste") {
        if panel_is_visible(app, &window) {
            let _ = window.hide();
        } else {
            position_panel(app, &window);
            show_panel(app, &window);
            let _ = window.emit("panel-opened", ());
        }
    }
}

/// 面板要弹在光标所在的那块屏上。
///
/// 隐藏中的窗口 `current_monitor()` 返回的是「上次停留的位置」——
/// 在第二块屏按快捷键，面板会弹回第一块去，所以先按光标查屏，
/// 拿不到再退回窗口自己那块。
fn monitor_for_panel(app: &AppHandle, window: &tauri::WebviewWindow) -> Option<tauri::Monitor> {
    if let Ok(cursor) = app.cursor_position() {
        if let Ok(Some(monitor)) = app.monitor_from_point(cursor.x, cursor.y) {
            return Some(monitor);
        }
    }
    window.current_monitor().ok().flatten()
}

/// 把面板摆在光标所在屏的工作区下方居中，像 Paste 那样贴着底边弹出。
///
/// `work_area` 已经排除了 Dock（macOS 可以在左/右侧）和任务栏（Windows 可以在顶部），
/// 所以底部留白不再按平台写死 110 / 60。个别平台拿不到真实工作区时
/// （work_area 等于整块屏），退回原先的写死值，宁可留白大一点也不让面板压在 Dock 下面。
fn position_panel(app: &AppHandle, window: &tauri::WebviewWindow) {
    let monitor = match monitor_for_panel(app, window) {
        Some(monitor) => monitor,
        None => return,
    };

    let scale = monitor.scale_factor();
    let work = monitor.work_area();

    // work_area / outer_size 都是物理像素：全程用物理坐标，不换算成逻辑坐标。
    // 多屏不同缩放时，LogicalPosition 会按窗口当前所在屏的比例换算，跨屏就会偏。
    let screen_bottom = monitor.position().y as f64 + monitor.size().height as f64;
    let area_bottom = work.position.y as f64 + work.size.height as f64;
    // 工作区底部没有被扣掉 → 平台没给真实值，用回老的写死留白
    let has_real_work_area = (screen_bottom - area_bottom).abs() > 1.0;
    let bottom_margin = if has_real_work_area {
        24.0 * scale
    } else if cfg!(target_os = "macos") {
        110.0 * scale
    } else {
        60.0 * scale
    };

    let size = match window.outer_size() {
        Ok(size) => size,
        Err(_) => return,
    };

    let (x, y) = panel_origin(
        (work.position.x as f64, work.position.y as f64),
        (work.size.width as f64, work.size.height as f64),
        (size.width as f64, size.height as f64),
        bottom_margin,
    );

    let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
}

/// 面板左上角的物理像素坐标：水平居中，底部留 margin。
/// 单独拆出来是为了能测多屏负坐标。
fn panel_origin(
    area: (f64, f64),
    area_size: (f64, f64),
    win: (f64, f64),
    bottom_margin: f64,
) -> (f64, f64) {
    let (ax, ay) = area;
    let (aw, ah) = area_size;
    let (ww, wh) = win;
    let x = (ax + (aw - ww) / 2.0).max(ax);
    let y = (ay + ah - wh - bottom_margin).max(ay);
    (x.round(), y.round())
}

/// 获取快捷键最后触发时间（秒级时间戳）
pub fn get_last_trigger_time() -> u64 {
    LAST_SHORTCUT_TRIGGER.load(Ordering::Relaxed)
}

/// 将快捷键字符串转换为当前平台有效的格式。
///
/// 设置里统一用 macOS 的写法存（`Cmd` / `Option`），其它平台注册前映射成
/// global-hotkey 能识别的 `Ctrl` / `Alt`。
pub fn normalize_shortcut(shortcut: &str) -> String {
    if cfg!(target_os = "macos") {
        shortcut.to_string()
    } else {
        shortcut.replace("Cmd", "Ctrl").replace("Option", "Alt")
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
            if !sleep_interruptible(check_interval) {
                log::info!("检测到应用正在退出，停止快捷键健康监控");
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
        if cfg!(target_os = "macos") {
            assert_eq!(normalize_shortcut("Cmd+Shift+V"), "Cmd+Shift+V");
            assert_eq!(normalize_shortcut("Option+K"), "Option+K");
        } else {
            assert_eq!(normalize_shortcut("Cmd+Shift+V"), "Ctrl+Shift+V");
            assert_eq!(normalize_shortcut("Option+K"), "Alt+K");
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
    fn test_panel_origin_centers_and_clears_bottom_margin() {
        let (x, y) = panel_origin((0.0, 0.0), (2560.0, 1440.0), (1800.0, 680.0), 48.0);
        assert_eq!(x, 380.0);
        assert_eq!(y, 712.0);
    }

    #[test]
    fn test_panel_origin_handles_negative_monitor_origin() {
        // 左侧副屏常见负坐标：不能钳到 0
        let (x, y) = panel_origin((-1920.0, -200.0), (1920.0, 1080.0), (1800.0, 680.0), 48.0);
        assert_eq!(x, -1860.0);
        assert_eq!(y, 152.0);
    }

    #[test]
    fn test_panel_origin_clamps_when_window_larger_than_area() {
        let (x, y) = panel_origin((100.0, 100.0), (900.0, 340.0), (1200.0, 700.0), 24.0);
        assert_eq!(x, 100.0);
        assert_eq!(y, 100.0);
    }

    #[test]
    fn test_current_shortcut_starts_empty() {
        assert_eq!(current_shortcut(), "");
    }

    #[test]
    fn test_toggle_quick_paste_does_not_crash_without_window() {
    }
}
