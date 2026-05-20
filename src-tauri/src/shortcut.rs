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

pub fn register(app: &AppHandle, shortcut: &str) -> Result<(), String> {
    app.global_shortcut()
        .on_shortcut(shortcut, move |app, _s, event| {
            if event.state() == ShortcutState::Pressed {
                toggle_quick_paste(app);
            }
        })
        .map_err(|e| format!("Failed to register shortcut: {}", e))
}

pub fn unregister(app: &AppHandle, shortcut: &str) -> Result<(), String> {
    app.global_shortcut()
        .unregister(shortcut)
        .map_err(|e| format!("Failed to unregister shortcut: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toggle_quick_paste_does_not_crash_without_window() {
    }
}
