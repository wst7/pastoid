use crate::db::{self, HistoryMigration};
use crate::models::ClipboardItem;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;

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
/// - SQLite 是唯一数据源（Single Source of Truth），单条操作即时落盘（写穿）
/// - 不再有内存全量缓存 / 5 秒快照：旧的「全量序列化 + 覆写」是 O(N)，
///   条目一多 flush 就是卡顿源，而且 `fs::write` 非原子、崩溃会丢历史
/// - 所有截断逻辑内聚在此（`db::enforce_max_items`）
pub struct ClipboardRepository {
    data_dir: PathBuf,
    max_items: Mutex<u32>,
    /// 单连接：所有调用方（采集线程 / UI 命令 / 图片回收）共用这一把锁，
    /// 天然串行，不需要 busy 之外的并发控制
    db: Mutex<Connection>,
}

impl ClipboardRepository {
    pub fn new(data_dir: PathBuf, max_items: u32) -> Self {
        let conn = db::open(&data_dir);

        match db::migrate_json(&conn, &data_dir) {
            HistoryMigration::Imported(n) => log::info!("已从 clipboard_data.json 导入 {n} 条历史"),
            HistoryMigration::Fresh => log::info!("历史库初始化完成"),
            HistoryMigration::AlreadyMigrated => {}
            // 下面两种都会在日志里给出原因，这里不再重复刷屏
            HistoryMigration::JsonUnreadable | HistoryMigration::JsonCorrupt => {}
        }

        // 老数据可能超过当前上限，打开时先裁一次（和旧的 load 时裁剪等价）
        if let Err(e) = db::enforce_max_items(&conn, max_items, &data_dir) {
            log::error!("按上限裁剪历史失败：{}", e);
        }

        Self {
            data_dir,
            max_items: Mutex::new(max_items),
            db: Mutex::new(conn),
        }
    }

    /// 锁住连接并把 SQL 错误降级成「空结果」——存储出错不能让面板白屏，
    /// 具体错误都带上下文打进日志。
    fn with_conn<T>(&self, op: impl FnOnce(&Connection) -> Result<T, String>) -> Option<T> {
        let conn = self.db.lock().unwrap();
        match op(&conn) {
            Ok(value) => Some(value),
            Err(e) => {
                log::error!("历史库查询失败：{}", e);
                None
            }
        }
    }

    // ==================== 读取 ====================

    pub fn get_items(&self) -> Vec<ClipboardItem> {
        self.with_conn(|conn| {
            query_items(
                conn,
                "SELECT * FROM items ORDER BY created_at DESC, rowid DESC",
                [],
            )
        })
        .unwrap_or_default()
    }

    pub fn get_history(&self, max_items: usize) -> Vec<ClipboardItem> {
        self.with_conn(|conn| {
            query_items(
                conn,
                "SELECT * FROM items
                 ORDER BY is_pinned DESC, created_at DESC, rowid DESC
                 LIMIT ?1",
                params![max_items as i64],
            )
        })
        .unwrap_or_default()
    }

    pub fn find_by_id(&self, id: &str) -> Option<ClipboardItem> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT * FROM items WHERE id = ?1",
                params![id],
                db::row_to_item,
            )
            .optional()
            .map_err(|e| e.to_string())
        })
        .flatten()
    }

    /// 按「内容 + 类型」找条目（文本去重用）
    pub fn find_by_content(&self, content: &str, item_type: &str) -> Option<ClipboardItem> {
        let hash = db::content_hash(content);
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT * FROM items WHERE type = ?1 AND content_hash = ?2",
                params![item_type, hash],
                db::row_to_item,
            )
            .optional()
            .map_err(|e| e.to_string())
        })
        .flatten()
    }

    /// 按图片哈希找条目（图片去重用）
    pub fn find_by_image_hash(&self, hash: &str) -> Option<ClipboardItem> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT * FROM items WHERE image_hash = ?1",
                params![hash],
                db::row_to_item,
            )
            .optional()
            .map_err(|e| e.to_string())
        })
        .flatten()
    }

    pub fn contains_text(&self, content: &str) -> bool {
        let hash = db::content_hash(content);
        self.with_conn(|conn| {
            let found: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM items WHERE type = 'text' AND content_hash = ?1",
                    params![hash],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            Ok(found > 0)
        })
        .unwrap_or(false)
    }

    /// 当前仍被条目引用的图片相对路径，孤儿文件回收用
    pub fn referenced_image_paths(&self) -> HashSet<String> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare("SELECT content FROM items WHERE type = 'image' AND content != ''")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<HashSet<_>, _>>()
                .map_err(|e| e.to_string())
        })
        .unwrap_or_default()
    }

    pub fn len(&self) -> usize {
        self.with_conn(|conn| {
            let n: i64 = conn
                .query_row("SELECT COUNT(*) FROM items", [], |row| row.get(0))
                .map_err(|e| e.to_string())?;
            Ok(n as usize)
        })
        .unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    // ==================== 写入（写穿，立即落盘） ====================

    /// 添加条目。
    ///
    /// 去重规则：
    /// - 图片条目：按 `image_hash` 去重（同一张图重复复制不会产生新条目）
    /// - 其它：按「内容 + 类型」去重（唯一索引 `ux_items_dedup` 保证）
    ///
    /// 重复时返回 [`AddOutcome::Duplicate`]，由调用方决定是否置顶。
    pub fn add(&self, item: ClipboardItem) -> AddOutcome {
        let conn = self.db.lock().unwrap();
        let hash = db::content_hash(&item.content);

        let inserted = match conn.execute(
            db::INSERT_ITEM,
            params![
                item.id,
                item.content,
                item.item_type,
                hash,
                item.created_at,
                item.updated_at,
                item.is_pinned,
                tags_json(&item),
                item.source,
                item.image_hash,
            ],
        ) {
            Ok(n) => n,
            Err(e) => {
                log::error!("写入历史失败：{}", e);
                return AddOutcome::Duplicate(String::new());
            }
        };

        if inserted == 0 {
            // 撞了去重索引：把已存在的条目 id 找出来交给调用方 bump
            let existing: Option<String> = conn
                .query_row(
                    "SELECT id FROM items
                     WHERE (type = ?1 AND content_hash = ?2)
                        OR (image_hash IS NOT NULL AND image_hash = ?3)
                     LIMIT 1",
                    params![item.item_type, hash, item.image_hash],
                    |row| row.get(0),
                )
                .optional()
                .unwrap_or(None);
            return AddOutcome::Duplicate(existing.unwrap_or_default());
        }

        let max = *self.max_items.lock().unwrap();
        if let Err(e) = db::enforce_max_items(&conn, max, &self.data_dir) {
            log::error!("按上限裁剪历史失败：{}", e);
        }
        AddOutcome::Added
    }

    /// 把已有条目提到最前面并刷新时间戳，返回更新后的条目。
    /// 用于「重复复制同一内容」——用户期望它重新回到列表顶部。
    pub fn bump(&self, id: &str) -> Option<ClipboardItem> {
        self.with_conn(|conn| {
            let now = chrono::Utc::now().timestamp_millis();
            // 同一毫秒内连续操作时，新旧时间戳相等会退回按 rowid 排序，
            // 老条目反而排在前面——取 max(now, 现有最大 + 1) 保证 bump 一定回到队首
            let max_created: Option<i64> = conn
                .query_row("SELECT MAX(created_at) FROM items", [], |row| row.get(0))
                .map_err(|e| e.to_string())?;
            let stamp = max_created.map_or(now, |max| now.max(max + 1));
            let changed = conn
                .execute(
                    "UPDATE items SET created_at = ?1, updated_at = ?1 WHERE id = ?2",
                    params![stamp, id],
                )
                .map_err(|e| e.to_string())?;
            if changed == 0 {
                return Ok(None);
            }
            conn.query_row(
                "SELECT * FROM items WHERE id = ?1",
                params![id],
                db::row_to_item,
            )
            .optional()
            .map_err(|e| e.to_string())
        })
        .flatten()
    }

    pub fn delete(&self, id: &str) {
        let _ = self.with_conn(|conn| {
            conn.execute("DELETE FROM items WHERE id = ?1", params![id])
                .map(|_| ())
                .map_err(|e| e.to_string())
        });
    }

    pub fn update(&self, item: ClipboardItem) {
        let hash = db::content_hash(&item.content);
        let _ = self.with_conn(|conn| {
            conn.execute(
                "UPDATE items SET content = ?2, type = ?3, content_hash = ?4,
                        created_at = ?5, updated_at = ?6, is_pinned = ?7,
                        tags = ?8, source = ?9, image_hash = ?10
                 WHERE id = ?1",
                params![
                    item.id,
                    item.content,
                    item.item_type,
                    hash,
                    item.created_at,
                    item.updated_at,
                    item.is_pinned,
                    tags_json(&item),
                    item.source,
                    item.image_hash,
                ],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
        });
    }

    pub fn clear(&self) {
        let _ = self.with_conn(|conn| {
            conn.execute("DELETE FROM items", [])
                .map(|_| ())
                .map_err(|e| e.to_string())
        });
    }

    pub fn import(&self, new_items: Vec<ClipboardItem>) {
        let max = *self.max_items.lock().unwrap();
        let _ = self.with_conn(|conn| {
            let tx = conn
                .unchecked_transaction()
                .map_err(|e| format!("begin: {e}"))?;
            {
                let mut stmt = tx
                    .prepare(db::INSERT_ITEM)
                    .map_err(|e| format!("prepare: {e}"))?;
                for item in &new_items {
                    let hash = db::content_hash(&item.content);
                    stmt.execute(params![
                        item.id,
                        item.content,
                        item.item_type,
                        hash,
                        item.created_at,
                        item.updated_at,
                        item.is_pinned,
                        tags_json(item),
                        item.source,
                        item.image_hash,
                    ])
                    .map_err(|e| format!("insert: {e}"))?;
                }
            }
            tx.commit().map_err(|e| format!("commit: {e}"))?;
            db::enforce_max_items(conn, max, &self.data_dir)
        });
    }

    // ==================== max_items 管理 ====================

    pub fn set_max_items(&self, new_max: u32) {
        let old_max = *self.max_items.lock().unwrap();
        if new_max == old_max {
            return;
        }

        *self.max_items.lock().unwrap() = new_max;

        // 只有调小才需要立刻裁；写穿之后没有「等 flush」这回事了
        if new_max < old_max {
            let conn = self.db.lock().unwrap();
            if let Err(e) = db::enforce_max_items(&conn, new_max, &self.data_dir) {
                log::error!("按上限裁剪历史失败：{}", e);
            }
        }
    }

    pub fn max_items(&self) -> u32 {
        *self.max_items.lock().unwrap()
    }
}

fn tags_json(item: &ClipboardItem) -> Option<String> {
    item.tags
        .as_ref()
        .and_then(|t| serde_json::to_string(t).ok())
}

fn query_items(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<ClipboardItem>, String> {
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params, db::row_to_item)
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
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
    fn test_repo_persists_across_reopen() {
        // 写穿之后不再有 flush：加完即落盘，重开进程数据还在
        let dir = create_temp_dir();
        {
            let repo = ClipboardRepository::new(dir.path().to_path_buf(), 100);
            repo.add(ClipboardItem::new("persist".to_string(), "text"));
        }

        let repo2 = ClipboardRepository::new(dir.path().to_path_buf(), 100);
        assert_eq!(repo2.len(), 1);
        assert_eq!(repo2.get_items()[0].content, "persist");
    }

    #[test]
    fn test_repo_migrates_legacy_json() {
        // 老版本的 JSON 快照要在首次启动时无感导入
        let dir = create_temp_dir();
        let mut legacy = ClipboardItem::new("legacy".to_string(), "text");
        legacy.created_at = 1_000;
        std::fs::write(
            crate::storage::get_data_file_path(dir.path()),
            serde_json::to_string(&vec![legacy]).unwrap(),
        )
        .unwrap();

        let repo = ClipboardRepository::new(dir.path().to_path_buf(), 100);
        assert_eq!(repo.len(), 1);
        assert_eq!(repo.get_items()[0].content, "legacy");

        // 迁移过之后再开一次不会重复导入
        drop(repo);
        let repo2 = ClipboardRepository::new(dir.path().to_path_buf(), 100);
        assert_eq!(repo2.len(), 1);
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

    #[test]
    fn test_find_by_content_and_contains_text() {
        let (_dir, repo) = text_repo(100);

        repo.add(ClipboardItem::new("needle".to_string(), "text"));

        assert!(repo.contains_text("needle"));
        assert!(!repo.contains_text("missing"));
        let found = repo.find_by_content("needle", "text").expect("应能找到");
        assert_eq!(found.content, "needle");
        // 类型不同不算同一条
        assert!(repo.find_by_content("needle", "image").is_none());
    }
}
