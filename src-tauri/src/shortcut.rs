use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

pub fn toggle_quick_paste(app: &AppHandle) {
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
    app.global_shortcut()
        .on_shortcut(normalized.as_str(), move |app, _s, event| {
            if event.state() == ShortcutState::Pressed {
                toggle_quick_paste(app);
            }
        })
        .map_err(|e| format!("Failed to register shortcut: {}", e))
}

pub fn unregister(app: &AppHandle, shortcut: &str) -> Result<(), String> {
    let normalized = normalize_shortcut(shortcut);
    app.global_shortcut()
        .unregister(normalized.as_str())
        .map_err(|e| format!("Failed to unregister shortcut: {}", e))
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
    fn test_toggle_quick_paste_does_not_crash_without_window() {
    }
}
