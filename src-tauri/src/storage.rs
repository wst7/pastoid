use crate::models::{ClipboardItem, Settings};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::Manager;

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

fn get_data_file_path(data_dir: &Path) -> PathBuf {
    data_dir.join("clipboard_data.json")
}

pub fn load_clipboard_data(data_dir: &Path) -> Vec<ClipboardItem> {
    let data_file = get_data_file_path(data_dir);

    if !data_file.exists() {
        return Vec::new();
    }

    match fs::read_to_string(&data_file) {
        Ok(content) => match serde_json::from_str::<Vec<ClipboardItem>>(&content) {
            Ok(items) => items,
            Err(e) => {
                eprintln!("Failed to parse clipboard data: {}", e);
                Vec::new()
            }
        },
        Err(e) => {
            eprintln!("Failed to read clipboard data file: {}", e);
            Vec::new()
        }
    }
}

pub fn save_clipboard_data(data_dir: &Path, items: &[ClipboardItem]) -> Result<(), String> {
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
                eprintln!("Failed to parse settings: {}", e);
                Settings::default()
            }
        },
        Err(e) => {
            eprintln!("Failed to read settings file: {}", e);
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
        let items = load_clipboard_data(temp_dir.path());
        assert!(items.is_empty());
    }

    #[test]
    fn test_save_and_load_clipboard_data() {
        let temp_dir = create_temp_dir();
        let items = vec![
            ClipboardItem::new("item 1".to_string(), "text"),
            ClipboardItem::new("item 2".to_string(), "text"),
        ];

        save_clipboard_data(temp_dir.path(), &items).unwrap();

        let loaded = load_clipboard_data(temp_dir.path());
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
    }

    #[test]
    fn test_save_and_load_settings() {
        let temp_dir = create_temp_dir();
        let settings = Settings {
            language: "en".to_string(),
            theme: "dark".to_string(),
            autostart: true,
            max_items: 50,
        };

        save_settings_to_file(temp_dir.path(), &settings).unwrap();

        let loaded = load_settings(temp_dir.path());
        assert_eq!(loaded.language, "en");
        assert_eq!(loaded.theme, "dark");
        assert!(loaded.autostart);
        assert_eq!(loaded.max_items, 50);
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
        let items: Vec<ClipboardItem> = vec![];

        save_clipboard_data(temp_dir.path(), &items).unwrap();

        let loaded = load_clipboard_data(temp_dir.path());
        assert!(loaded.is_empty());
    }
}
