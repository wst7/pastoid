use crate::models::ClipboardItem;
use crate::storage;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 剪贴板数据仓库：统一的数据管理层
///
/// 设计原则：
/// - 内存 items 是唯一操作对象（Single Source of Truth）
/// - 文件只是定期快照（Write-Back Cache）
/// - 所有截断逻辑内聚在此
pub struct ClipboardRepository {
    data_dir: PathBuf,
    max_items: Mutex<u32>,
    items: Mutex<Vec<ClipboardItem>>,
    dirty: AtomicBool,
    last_flush: Mutex<Instant>,
}

impl ClipboardRepository {
    pub fn new(data_dir: PathBuf, max_items: u32) -> Self {
        let items = storage::load_clipboard_data(&data_dir, max_items);
        Self {
            data_dir,
            max_items: Mutex::new(max_items),
            items: Mutex::new(items),
            dirty: AtomicBool::new(false),
            last_flush: Mutex::new(Instant::now()),
        }
    }

    // ==================== 读取 ====================

    pub fn get_items(&self) -> Vec<ClipboardItem> {
        self.items.lock().unwrap().clone()
    }

    pub fn get_history(&self, max_items: usize) -> Vec<ClipboardItem> {
        let items = self.items.lock().unwrap();
        let mut sorted = items.clone();
        sorted.sort_by(|a, b| {
            if a.is_pinned != b.is_pinned {
                b.is_pinned.cmp(&a.is_pinned)
            } else {
                b.created_at.cmp(&a.created_at)
            }
        });
        sorted.into_iter().take(max_items).collect()
    }

    pub fn find_by_id(&self, id: &str) -> Option<ClipboardItem> {
        let items = self.items.lock().unwrap();
        items.iter().find(|i| i.id == id).cloned()
    }

    pub fn len(&self) -> usize {
        self.items.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.lock().unwrap().is_empty()
    }

    // ==================== 写入（只改内存） ====================

    /// 添加条目，返回 true 表示成功添加（非重复）
    pub fn add(&self, item: ClipboardItem) -> bool {
        let max = *self.max_items.lock().unwrap();
        let mut items = self.items.lock().unwrap();
        let is_duplicate = items
            .iter()
            .any(|i| i.content == item.content && i.item_type == item.item_type);
        if !is_duplicate {
            items.insert(0, item);
            storage::enforce_max_items(&mut items, max);
            self.dirty.store(true, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    pub fn delete(&self, id: &str) {
        let mut items = self.items.lock().unwrap();
        items.retain(|i| i.id != id);
        self.dirty.store(true, Ordering::Relaxed);
    }

    pub fn update(&self, item: ClipboardItem) {
        let mut items = self.items.lock().unwrap();
        if let Some(idx) = items.iter().position(|i| i.id == item.id) {
            items[idx] = item;
            self.dirty.store(true, Ordering::Relaxed);
        }
    }

    pub fn clear(&self) {
        let mut items = self.items.lock().unwrap();
        items.clear();
        self.dirty.store(true, Ordering::Relaxed);
    }

    pub fn import(&self, new_items: Vec<ClipboardItem>) {
        let max = *self.max_items.lock().unwrap();
        let mut items = self.items.lock().unwrap();
        for item in new_items {
            if !items.iter().any(|i| i.id == item.id) {
                items.push(item);
            }
        }
        items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        storage::enforce_max_items(&mut items, max);
        self.dirty.store(true, Ordering::Relaxed);
    }

    // ==================== max_items 管理 ====================

    pub fn set_max_items(&self, new_max: u32) {
        let old_max = *self.max_items.lock().unwrap();
        if new_max == old_max {
            return;
        }

        *self.max_items.lock().unwrap() = new_max;

        if new_max < old_max {
            let mut items = self.items.lock().unwrap();
            storage::enforce_max_items(&mut items, new_max);
            drop(items);
            let _ = self.flush_now();
        }
    }

    pub fn max_items(&self) -> u32 {
        *self.max_items.lock().unwrap()
    }

    // ==================== 持久化 ====================

    /// 立即 flush 到文件（如果没有修改则不写）
    pub fn flush_now(&self) -> Result<(), String> {
        if !self.dirty.swap(false, Ordering::Relaxed) {
            return Ok(());
        }

        let max = *self.max_items.lock().unwrap();
        let mut items = self.items.lock().unwrap();
        storage::enforce_max_items(&mut items, max);

        let result = storage::save_clipboard_data(&self.data_dir, &mut items, max);
        *self.last_flush.lock().unwrap() = Instant::now();
        result
    }

    /// 启动后台自动 flush 线程
    pub fn start_auto_flush(self: Arc<Self>, interval_secs: u64) {
        std::thread::spawn(move || {
            let interval = Duration::from_secs(interval_secs);
            loop {
                std::thread::sleep(interval);
                let _ = self.flush_now();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ClipboardItem;
    use tempfile::TempDir;

    fn create_temp_dir() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn test_repo_add_and_get() {
        let temp_dir = create_temp_dir();
        let repo = ClipboardRepository::new(temp_dir.path().to_path_buf(), 100);

        let item = ClipboardItem::new("hello".to_string(), "text");
        assert!(repo.add(item.clone()));
        assert_eq!(repo.len(), 1);

        // 重复内容不应添加
        assert!(!repo.add(item.clone()));
        assert_eq!(repo.len(), 1);
    }

    #[test]
    fn test_repo_delete() {
        let temp_dir = create_temp_dir();
        let repo = ClipboardRepository::new(temp_dir.path().to_path_buf(), 100);

        let item = ClipboardItem::new("hello".to_string(), "text");
        repo.add(item.clone());
        repo.delete(&item.id);

        assert!(repo.is_empty());
    }

    #[test]
    fn test_repo_clear() {
        let temp_dir = create_temp_dir();
        let repo = ClipboardRepository::new(temp_dir.path().to_path_buf(), 100);

        repo.add(ClipboardItem::new("a".to_string(), "text"));
        repo.add(ClipboardItem::new("b".to_string(), "text"));
        repo.clear();

        assert!(repo.is_empty());
    }

    #[test]
    fn test_repo_max_items_truncation() {
        let temp_dir = create_temp_dir();
        let repo = ClipboardRepository::new(temp_dir.path().to_path_buf(), 100);

        repo.add(ClipboardItem::new("a".to_string(), "text"));
        repo.add(ClipboardItem::new("b".to_string(), "text"));
        repo.add(ClipboardItem::new("c".to_string(), "text"));

        repo.set_max_items(2);
        assert_eq!(repo.len(), 2);
    }

    #[test]
    fn test_repo_add_truncates_immediately() {
        let temp_dir = create_temp_dir();
        let repo = ClipboardRepository::new(temp_dir.path().to_path_buf(), 2);

        repo.add(ClipboardItem::new("a".to_string(), "text"));
        repo.add(ClipboardItem::new("b".to_string(), "text"));
        repo.add(ClipboardItem::new("c".to_string(), "text"));

        // 应该被截断到 2 条
        assert_eq!(repo.len(), 2);
        assert_eq!(repo.get_items().len(), 2);
    }

    #[test]
    fn test_repo_flush_and_reload() {
        let temp_dir = create_temp_dir();
        let repo = ClipboardRepository::new(temp_dir.path().to_path_buf(), 100);

        let item = ClipboardItem::new("persist".to_string(), "text");
        repo.add(item);
        repo.flush_now().unwrap();

        // 重新创建 repo（模拟重启），数据应存在
        let repo2 = ClipboardRepository::new(temp_dir.path().to_path_buf(), 100);
        assert_eq!(repo2.len(), 1);
        assert_eq!(repo2.get_items()[0].content, "persist");
    }

    #[test]
    fn test_repo_import() {
        let temp_dir = create_temp_dir();
        let repo = ClipboardRepository::new(temp_dir.path().to_path_buf(), 100);

        let item1 = ClipboardItem::new("existing".to_string(), "text");
        repo.add(item1.clone());

        let item2 = ClipboardItem::new("imported".to_string(), "text");
        repo.import(vec![item1, item2]);

        // item1 重复不导入，item2 导入，共 2 条
        assert_eq!(repo.len(), 2);
    }

    #[test]
    fn test_repo_get_history_sorted() {
        let temp_dir = create_temp_dir();
        let repo = ClipboardRepository::new(temp_dir.path().to_path_buf(), 100);

        let mut pinned = ClipboardItem::new("pinned".to_string(), "text");
        pinned.is_pinned = true;
        let normal = ClipboardItem::new("normal".to_string(), "text");

        repo.add(normal);
        repo.add(pinned);

        let history = repo.get_history(10);
        assert!(history[0].is_pinned);
        assert!(!history[1].is_pinned);
    }
}
