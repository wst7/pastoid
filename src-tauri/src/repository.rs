use crate::models::ClipboardItem;
use crate::storage;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// `add` 的结果。
///
/// 之所以不返回 bool：调用方需要区分「新增成功」和「历史里已有同内容条目」，
/// 后者应该把老条目提到最前面（重新复制 = 重新变新），而不是静默丢弃。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddOutcome {
    /// 新条目已插入
    Added,
    /// 历史里已有同内容条目，附带其 id
    Duplicate(String),
}

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
    /// 是否有未落盘的修改
    dirty: AtomicBool,
    /// 每次修改自增；flush 用它判断写入期间是否有新修改，避免把 dirty 误清
    generation: AtomicU64,
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
            generation: AtomicU64::new(0),
            last_flush: Mutex::new(Instant::now()),
        }
    }

    /// 标记有修改：自增 generation 并置 dirty
    fn mark_dirty(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.dirty.store(true, Ordering::Relaxed);
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

    /// 按「内容 + 类型」找条目（文本去重用）
    pub fn find_by_content(&self, content: &str, item_type: &str) -> Option<ClipboardItem> {
        let items = self.items.lock().unwrap();
        items
            .iter()
            .find(|i| i.content == content && i.item_type == item_type)
            .cloned()
    }

    /// 按图片哈希找条目（图片去重用）
    pub fn find_by_image_hash(&self, hash: &str) -> Option<ClipboardItem> {
        let items = self.items.lock().unwrap();
        items
            .iter()
            .find(|i| i.image_hash.as_deref() == Some(hash))
            .cloned()
    }

    pub fn contains_text(&self, content: &str) -> bool {
        let items = self.items.lock().unwrap();
        items
            .iter()
            .any(|i| i.item_type == "text" && i.content == content)
    }

    /// 当前仍被条目引用的图片相对路径，孤儿文件回收用
    pub fn referenced_image_paths(&self) -> HashSet<String> {
        let items = self.items.lock().unwrap();
        items
            .iter()
            .filter(|i| i.item_type == "image" && !i.content.is_empty())
            .map(|i| i.content.clone())
            .collect()
    }

    pub fn len(&self) -> usize {
        self.items.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.lock().unwrap().is_empty()
    }

    // ==================== 写入（只改内存） ====================

    /// 添加条目。
    ///
    /// 去重规则：
    /// - 图片条目：按 `image_hash` 去重（同一张图重复复制不会产生新条目）
    /// - 其它：按「内容 + 类型」去重
    ///
    /// 重复时返回 [`AddOutcome::Duplicate`]，由调用方决定是否置顶。
    pub fn add(&self, item: ClipboardItem) -> AddOutcome {
        let max = *self.max_items.lock().unwrap();
        let mut items = self.items.lock().unwrap();

        let duplicate_id = if item.is_image() {
            item.image_hash.as_ref().and_then(|hash| {
                items
                    .iter()
                    .find(|i| i.image_hash.as_deref() == Some(hash.as_str()))
                    .map(|i| i.id.clone())
            })
        } else {
            items
                .iter()
                .find(|i| i.content == item.content && i.item_type == item.item_type)
                .map(|i| i.id.clone())
        };

        if let Some(id) = duplicate_id {
            return AddOutcome::Duplicate(id);
        }

        items.insert(0, item);
        storage::enforce_max_items(&mut items, max, &self.data_dir);
        self.mark_dirty();
        AddOutcome::Added
    }

    /// 把已有条目提到最前面并刷新时间戳，返回更新后的条目。
    /// 用于「重复复制同一内容」——用户期望它重新回到列表顶部。
    pub fn bump(&self, id: &str) -> Option<ClipboardItem> {
        let mut items = self.items.lock().unwrap();
        let idx = items.iter().position(|i| i.id == id)?;

        let mut item = items.remove(idx);
        let now = chrono::Utc::now().timestamp_millis();
        item.created_at = now;
        item.updated_at = now;
        items.insert(0, item.clone());
        self.mark_dirty();
        Some(item)
    }

    pub fn delete(&self, id: &str) {
        let mut items = self.items.lock().unwrap();
        items.retain(|i| i.id != id);
        self.mark_dirty();
    }

    pub fn update(&self, item: ClipboardItem) {
        let mut items = self.items.lock().unwrap();
        if let Some(idx) = items.iter().position(|i| i.id == item.id) {
            items[idx] = item;
            self.mark_dirty();
        }
    }

    pub fn clear(&self) {
        let mut items = self.items.lock().unwrap();
        items.clear();
        self.mark_dirty();
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
        storage::enforce_max_items(&mut items, max, &self.data_dir);
        self.mark_dirty();
    }

    // ==================== max_items 管理 ====================

    pub fn set_max_items(&self, new_max: u32) {
        let old_max = *self.max_items.lock().unwrap();
        if new_max == old_max {
            return;
        }

        *self.max_items.lock().unwrap() = new_max;

        if new_max < old_max {
            {
                let mut items = self.items.lock().unwrap();
                storage::enforce_max_items(&mut items, new_max, &self.data_dir);
            }
            self.mark_dirty();
            let _ = self.flush_now();
        }
    }

    pub fn max_items(&self) -> u32 {
        *self.max_items.lock().unwrap()
    }

    // ==================== 持久化 ====================

    /// 立即 flush 到文件（没有修改则不写）。
    ///
    /// 只有写入成功才清 `dirty`；写入期间若有新修改（generation 变化），
    /// 保留 dirty 让下一轮再写，避免修改被静默丢弃。
    pub fn flush_now(&self) -> Result<(), String> {
        if !self.dirty.load(Ordering::Relaxed) {
            return Ok(());
        }

        let generation = self.generation.load(Ordering::Relaxed);

        let (mut snapshot, max) = {
            let max = *self.max_items.lock().unwrap();
            let mut items = self.items.lock().unwrap();
            storage::enforce_max_items(&mut items, max, &self.data_dir);
            (items.clone(), max)
        };

        // 不持锁写盘
        let result = storage::save_clipboard_data(&self.data_dir, &mut snapshot, max);

        match &result {
            Ok(()) => {
                if self.generation.load(Ordering::Relaxed) == generation {
                    self.dirty.store(false, Ordering::Relaxed);
                }
                *self.last_flush.lock().unwrap() = Instant::now();
            }
            Err(e) => {
                // 保留 dirty，下一轮 flush 会重试
                log::error!("保存剪贴板数据失败（将重试）: {}", e);
            }
        }

        result
    }

    /// 启动后台自动 flush 线程
    pub fn start_auto_flush(self: Arc<Self>, interval_secs: u64) {
        std::thread::Builder::new()
            .name("clipboard-flush".into())
            .spawn(move || {
                let interval = Duration::from_secs(interval_secs);
                loop {
                    std::thread::sleep(interval);
                    // flush 出错不退出线程：下次继续重试
                    let _ = self.flush_now();
                }
            })
            .expect("failed to spawn flush thread");
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

    fn text_repo(max: u32) -> (TempDir, ClipboardRepository) {
        let dir = create_temp_dir();
        let repo = ClipboardRepository::new(dir.path().to_path_buf(), max);
        (dir, repo)
    }

    #[test]
    fn test_repo_add_and_get() {
        let (_dir, repo) = text_repo(100);

        let item = ClipboardItem::new("hello".to_string(), "text");
        assert_eq!(repo.add(item.clone()), AddOutcome::Added);
        assert_eq!(repo.len(), 1);

        // 重复内容不应添加，且返回已有条目 id
        assert_eq!(
            repo.add(item.clone()),
            AddOutcome::Duplicate(item.id.clone())
        );
        assert_eq!(repo.len(), 1);
    }

    #[test]
    fn test_add_image_dedupes_by_hash() {
        let (_dir, repo) = text_repo(100);

        let a = ClipboardItem::new_image("images/a.png".to_string(), "hash-a".to_string());
        let b = ClipboardItem::new_image("images/b.png".to_string(), "hash-a".to_string());

        assert_eq!(repo.add(a.clone()), AddOutcome::Added);
        assert_eq!(repo.add(b), AddOutcome::Duplicate(a.id.clone()));
        assert_eq!(repo.len(), 1);
    }

    #[test]
    fn test_bump_moves_item_to_front() {
        let (_dir, repo) = text_repo(100);

        let a = ClipboardItem::new("a".to_string(), "text");
        let b = ClipboardItem::new("b".to_string(), "text");
        repo.add(a.clone());
        repo.add(b.clone());

        let bumped = repo.bump(&a.id).expect("bump 应成功");
        assert_eq!(bumped.id, a.id);
        assert_eq!(repo.get_items()[0].id, a.id);
        assert!(bumped.created_at >= b.created_at);
    }

    #[test]
    fn test_repo_delete() {
        let (_dir, repo) = text_repo(100);

        let item = ClipboardItem::new("hello".to_string(), "text");
        repo.add(item.clone());
        repo.delete(&item.id);

        assert!(repo.is_empty());
    }

    #[test]
    fn test_repo_clear() {
        let (_dir, repo) = text_repo(100);

        repo.add(ClipboardItem::new("a".to_string(), "text"));
        repo.add(ClipboardItem::new("b".to_string(), "text"));
        repo.clear();

        assert!(repo.is_empty());
    }

    #[test]
    fn test_repo_max_items_truncation() {
        let (_dir, repo) = text_repo(100);

        repo.add(ClipboardItem::new("a".to_string(), "text"));
        repo.add(ClipboardItem::new("b".to_string(), "text"));
        repo.add(ClipboardItem::new("c".to_string(), "text"));

        repo.set_max_items(2);
        assert_eq!(repo.len(), 2);
    }

    #[test]
    fn test_repo_add_truncates_immediately() {
        let (_dir, repo) = text_repo(2);

        repo.add(ClipboardItem::new("a".to_string(), "text"));
        repo.add(ClipboardItem::new("b".to_string(), "text"));
        repo.add(ClipboardItem::new("c".to_string(), "text"));

        assert_eq!(repo.len(), 2);
        assert_eq!(repo.get_items().len(), 2);
    }

    #[test]
    fn test_repo_flush_and_reload() {
        let dir = create_temp_dir();
        let repo = ClipboardRepository::new(dir.path().to_path_buf(), 100);

        repo.add(ClipboardItem::new("persist".to_string(), "text"));
        repo.flush_now().unwrap();

        let repo2 = ClipboardRepository::new(dir.path().to_path_buf(), 100);
        assert_eq!(repo2.len(), 1);
        assert_eq!(repo2.get_items()[0].content, "persist");
    }

    #[test]
    fn test_flush_without_changes_is_noop() {
        let (_dir, repo) = text_repo(100);
        // 没有修改时 flush 应直接返回
        assert!(repo.flush_now().is_ok());
    }

    #[test]
    fn test_repo_import() {
        let (_dir, repo) = text_repo(100);

        let item1 = ClipboardItem::new("existing".to_string(), "text");
        repo.add(item1.clone());

        let item2 = ClipboardItem::new("imported".to_string(), "text");
        repo.import(vec![item1, item2]);

        assert_eq!(repo.len(), 2);
    }

    #[test]
    fn test_repo_get_history_sorted() {
        let (_dir, repo) = text_repo(100);

        let mut pinned = ClipboardItem::new("pinned".to_string(), "text");
        pinned.is_pinned = true;
        let normal = ClipboardItem::new("normal".to_string(), "text");

        repo.add(normal);
        repo.add(pinned);

        let history = repo.get_history(10);
        assert!(history[0].is_pinned);
        assert!(!history[1].is_pinned);
    }

    #[test]
    fn test_referenced_image_paths_only_lists_images() {
        let (_dir, repo) = text_repo(100);

        repo.add(ClipboardItem::new("text".to_string(), "text"));
        repo.add(ClipboardItem::new_image(
            "images/keep.png".to_string(),
            "h1".to_string(),
        ));

        let refs = repo.referenced_image_paths();
        assert_eq!(refs.len(), 1);
        assert!(refs.contains("images/keep.png"));
    }
}
