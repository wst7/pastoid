// 学习更多关于 Tauri 命令：https://tauri.app/develop/calling-rust/

use serde::{Deserialize, Serialize};

// 剪切板条目数据结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardItem {
    pub id: String,
    pub content: String,
    #[serde(rename = "type")]
    pub item_type: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
    #[serde(rename = "isPinned")]
    pub is_pinned: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

// 应用状态
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub struct AppState {
    pub repo: Arc<crate::repository::ClipboardRepository>,
    pub data_dir: PathBuf,
    pub settings: Mutex<Settings>,
}

// 设置数据结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub language: String,
    pub theme: String,
    pub autostart: bool,
    #[serde(rename = "max_items")]
    pub max_items: u32,
    pub shortcut: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: "zh".to_string(),
            theme: "system".to_string(),
            autostart: false,
            max_items: 20,
            #[cfg(target_os = "macos")]
            shortcut: "Cmd+Shift+V".to_string(),
            #[cfg(not(target_os = "macos"))]
            shortcut: "Ctrl+Shift+V".to_string(),
        }
    }
}

impl ClipboardItem {
    pub fn new(content: String, item_type: &str) -> Self {
        let now = chrono::Utc::now().timestamp_millis();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            content,
            item_type: item_type.to_string(),
            created_at: now,
            updated_at: now,
            is_pinned: false,
            tags: None,
            source: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ClipboardItem, Settings};

    #[test]
    fn test_clipboard_item_new() {
        let item = ClipboardItem::new("test content".to_string(), "text");

        assert!(!item.id.is_empty());
        assert_eq!(item.content, "test content");
        assert_eq!(item.item_type, "text");
        assert!(!item.is_pinned);
        assert!(item.tags.is_none());
        assert!(item.source.is_none());
    }

    #[test]
    fn test_clipboard_item_created_at_and_updated_at() {
        let before = chrono::Utc::now().timestamp_millis();
        let item = ClipboardItem::new("test".to_string(), "text");
        let after = chrono::Utc::now().timestamp_millis();

        assert!(item.created_at >= before);
        assert!(item.created_at <= after);
        assert_eq!(item.created_at, item.updated_at);
    }

    #[test]
    fn test_clipboard_item_clone() {
        let item1 = ClipboardItem::new("test".to_string(), "text");
        let item2 = item1.clone();

        assert_eq!(item1.id, item2.id);
        assert_eq!(item1.content, item2.content);
    }

    #[test]
    fn test_settings_default() {
        let settings = Settings::default();

        assert_eq!(settings.language, "zh");
        assert_eq!(settings.theme, "system");
        assert!(!settings.autostart);
        assert_eq!(settings.max_items, 20);
        #[cfg(target_os = "macos")]
        assert_eq!(settings.shortcut, "Cmd+Shift+V");
        #[cfg(not(target_os = "macos"))]
        assert_eq!(settings.shortcut, "Ctrl+Shift+V");
    }

    #[test]
    fn test_settings_clone() {
        let settings1 = Settings {
            language: "en".to_string(),
            theme: "dark".to_string(),
            autostart: true,
            max_items: 50,
            shortcut: "Ctrl+Alt+K".to_string(),
        };
        let settings2 = settings1.clone();

        assert_eq!(settings1.language, settings2.language);
        assert_eq!(settings1.theme, settings2.theme);
        assert_eq!(settings1.autostart, settings2.autostart);
        assert_eq!(settings1.max_items, settings2.max_items);
        assert_eq!(settings1.shortcut, settings2.shortcut);
    }

    #[test]
    fn test_settings_serialization() {
        let settings = Settings {
            language: "en".to_string(),
            theme: "dark".to_string(),
            autostart: true,
            max_items: 100,
            shortcut: "Cmd+Shift+V".to_string(),
        };

        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains("\"language\":\"en\""));
        assert!(json.contains("\"theme\":\"dark\""));
        assert!(json.contains("\"autostart\":true"));
        assert!(json.contains("\"max_items\":100"));
        assert!(json.contains("\"shortcut\":\"Cmd+Shift+V\""));

        let decoded: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.language, "en");
        assert_eq!(decoded.theme, "dark");
        assert!(decoded.autostart);
        assert_eq!(decoded.max_items, 100);
        assert_eq!(decoded.shortcut, "Cmd+Shift+V");
    }

    #[test]
    fn test_clipboard_item_serialization() {
        let item = ClipboardItem::new("hello world".to_string(), "text");

        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("hello world"));
        assert!(json.contains("text"));

        let decoded: ClipboardItem = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.content, "hello world");
        assert_eq!(decoded.item_type, "text");
    }
}