//! 历史数据的 SQLite 存储（`history.db`）。
//!
//! 为什么不用 JSON 快照：旧实现每次 flush 都是「序列化全量 + 覆写全量文件」，
//! 复制一条还要克隆整个 vec 再排两次序，复杂度 O(N)——条目一多就不可用；
//! 而且 `fs::write` 非原子，崩溃截断会让历史解析失败，之后被新数据覆盖掉。
//! SQLite 单条写入是微秒级、事务保证不损坏，去重走唯一索引。
//!
//! 迁移约定（用户无感知）：
//! - `PRAGMA user_version >= 1` 表示已经迁移过，`clipboard_data.json` 只当备份，不再写入
//! - 首次启动时把 JSON 导进来（单事务，按 id / 内容去重），成功后才写 user_version
//! - JSON 读不出来或解析失败都不写 user_version，下次启动重试；**原文件永不改动**，
//!   损坏时留着手动恢复，也避免旧版「空历史 + 下次 flush 覆盖」的数据毁灭路径

use crate::models::ClipboardItem;
use crate::storage;
use rusqlite::{params, Connection};
use std::path::Path;

/// 历史库 schema 版本。表结构变更时 +1，并在 open 后按版本做升级。
pub const SCHEMA_VERSION: i32 = 1;

/// 写入一条条目；命中去重唯一索引时静默跳过（调用方用 changes 判断）。
pub(crate) const INSERT_ITEM: &str = "INSERT OR IGNORE INTO items \
     (id, content, type, content_hash, created_at, updated_at, is_pinned, tags, source, image_hash) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)";

/// JSON 快照迁移的结果
#[derive(Debug, PartialEq, Eq)]
pub enum HistoryMigration {
    /// 已经迁移过（或本来就没有旧数据要等第二次启动处理）
    AlreadyMigrated,
    /// 数据目录里没有旧 JSON，直接启用空库
    Fresh,
    /// 从 `clipboard_data.json` 导入了 n 条
    Imported(usize),
    /// 旧文件读不出来（权限 / IO），保持 user_version=0，下次启动重试
    JsonUnreadable,
    /// 旧文件存在但解析失败：原文件保持不动，本次从空历史开始
    JsonCorrupt,
}

/// 「这是一条链接」的 SQL 判定，和前端 `isLink()` 对齐：
/// 去掉首尾空白（空格/Tab/换行）后以 http:// 或 https:// 开头。
/// LIKE 默认对 ASCII 忽略大小写，等价于前端的 `/i` 标志。
pub(crate) const LINK_PREDICATE: &str = "(TRIM(content, ' ' || char(9) || char(10) || char(13)) LIKE 'http://%' \
    OR TRIM(content, ' ' || char(9) || char(10) || char(13)) LIKE 'https://%')";

/// 内容去重键：和图片 `imageHash` 同一套思路（blake3），
/// 索引只存 32 字节摘要，不拿最长 512KB 的原文当索引键。
pub fn content_hash(content: &str) -> String {
    blake3::hash(content.as_bytes()).to_hex().to_string()
}

/// 打开（必要时创建）历史库并建好 schema。
///
/// 打不开（目录没权限、磁盘满……）时退化成内存库：本次运行功能正常但不持久化，
/// 和旧实现「读失败当空、写失败重试」的降级方式对齐，不让存储问题拖垮应用。
pub fn open(data_dir: &Path) -> Connection {
    let _ = std::fs::create_dir_all(data_dir);
    let path = data_dir.join("history.db");

    let conn = match Connection::open(&path) {
        Ok(conn) => conn,
        Err(e) => {
            log::error!(
                "无法打开历史库 {}（{}），本次运行不持久化历史",
                path.display(),
                e
            );
            Connection::open_in_memory().expect("内存 SQLite 也打不开")
        }
    };

    configure(&conn);
    if let Err(e) = ensure_schema(&conn) {
        log::error!("建表失败：{}", e);
    }
    conn
}

/// 连接级配置。
///
/// 优先 WAL（读写不互斥、崩溃安全）；自定义数据目录可能在 exFAT / 网络盘上，
/// 那些文件系统不支持 WAL，PRAGMA 会退回原模式——跟着模式选 synchronous 级别。
fn configure(conn: &Connection) {
    let _ = conn.busy_timeout(std::time::Duration::from_millis(3000));

    let mode = conn
        .query_row("PRAGMA journal_mode=WAL", [], |r| r.get::<_, String>(0))
        .unwrap_or_default();
    if mode.eq_ignore_ascii_case("wal") {
        // 进程崩溃不丢数据；掉电场景 WAL 自带恢复
        let _ = conn.execute_batch("PRAGMA synchronous=NORMAL");
    } else {
        if !mode.is_empty() {
            log::warn!("当前文件系统不支持 WAL（实际模式 {}），降级为回滚日志", mode);
        }
        let _ = conn.execute_batch("PRAGMA synchronous=FULL");
    }
}

fn ensure_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS items (
            id           TEXT PRIMARY KEY,
            content      TEXT NOT NULL,
            type         TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            created_at   INTEGER NOT NULL,
            updated_at   INTEGER NOT NULL,
            is_pinned    INTEGER NOT NULL DEFAULT 0,
            tags         TEXT,
            source       TEXT,
            image_hash   TEXT
        );
        CREATE UNIQUE INDEX IF NOT EXISTS ux_items_dedup
            ON items(type, content_hash);
        CREATE UNIQUE INDEX IF NOT EXISTS ux_items_image_hash
            ON items(image_hash) WHERE image_hash IS NOT NULL;
        CREATE INDEX IF NOT EXISTS idx_items_order
            ON items(is_pinned DESC, created_at DESC);",
    )
}

/// 把 `clipboard_data.json` 导入历史库（只在 `user_version < 1` 时发生）。
pub fn migrate_json(conn: &Connection, data_dir: &Path) -> HistoryMigration {
    let version: i32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap_or(0);
    if version >= SCHEMA_VERSION {
        return HistoryMigration::AlreadyMigrated;
    }

    let json_path = storage::get_data_file_path(data_dir);
    if !json_path.exists() {
        set_version(conn, SCHEMA_VERSION);
        return HistoryMigration::Fresh;
    }

    let content = match std::fs::read_to_string(&json_path) {
        Ok(content) => content,
        Err(e) => {
            log::error!(
                "读取 {} 失败：{}。不写迁移标记，下次启动重试",
                json_path.display(),
                e
            );
            return HistoryMigration::JsonUnreadable;
        }
    };

    let items: Vec<ClipboardItem> = match serde_json::from_str(&content) {
        Ok(items) => items,
        Err(e) => {
            log::error!(
                "{} 无法解析（{}）。原文件保持不动以便手动恢复，本次从空历史开始；\
                 修复该文件后下次启动会自动重新导入",
                json_path.display(),
                e
            );
            return HistoryMigration::JsonCorrupt;
        }
    };

    let expected = items.len();
    let imported = match import_items(conn, &items) {
        Ok(n) => n,
        Err(e) => {
            log::error!("导入历史失败（事务已回滚，下次启动重试）：{}", e);
            return HistoryMigration::JsonUnreadable;
        }
    };

    // 只有真的导入完才标记迁移完成；行数不一致通常是有重复内容被去重跳过
    set_version(conn, SCHEMA_VERSION);
    if imported != expected {
        log::warn!("导入 {} 条、跳过 {} 条重复", imported, expected - imported);
    }
    HistoryMigration::Imported(imported)
}

/// 单事务批量写入，按主键 / 去重索引跳过重复，返回实际写入条数。
fn import_items(conn: &Connection, items: &[ClipboardItem]) -> Result<usize, String> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("begin: {e}"))?;

    let mut inserted = 0usize;
    {
        let mut stmt = tx
            .prepare(INSERT_ITEM)
            .map_err(|e| format!("prepare: {e}"))?;
        for item in items {
            let hash = content_hash(&item.content);
            let tags = item
                .tags
                .as_ref()
                .and_then(|t| serde_json::to_string(t).ok());
            inserted += stmt
                .execute(params![
                    item.id,
                    item.content,
                    item.item_type,
                    hash,
                    item.created_at,
                    item.updated_at,
                    item.is_pinned,
                    tags,
                    item.source,
                    item.image_hash,
                ])
                .map_err(|e| format!("insert: {e}"))?;
        }
    }

    tx.commit().map_err(|e| format!("commit: {e}"))?;
    Ok(inserted)
}

fn set_version(conn: &Connection, version: i32) {
    if let Err(e) = conn.execute_batch(&format!("PRAGMA user_version = {version}")) {
        log::error!("写入 schema 版本失败：{}", e);
    }
}

/// 把库裁剪到 `max_items` 条：置顶优先保留，非置顶保留最新的，
/// 被砍掉的图片条目同步删除磁盘文件。
///
/// `max_items == 0` 表示不限（无限保存），直接跳过。
pub fn enforce_max_items(conn: &Connection, max_items: u32, data_dir: &Path) -> Result<(), String> {
    if max_items == 0 {
        return Ok(());
    }
    let max = max_items as usize;

    let pinned_total = count(conn, "SELECT COUNT(*) FROM items WHERE is_pinned = 1")?;
    // 置顶本身可能超过上限（用户把上限调小了），先砍掉多的，否则永远裁不干净
    let pinned_kept = (pinned_total as usize).min(max);
    let keep_normal = max - pinned_kept;

    let mut overflow = Vec::new();
    if (pinned_total as usize) > pinned_kept {
        overflow.extend(overflow_rows(conn, 1, pinned_kept)?);
    }

    let normal_total = count(conn, "SELECT COUNT(*) FROM items WHERE is_pinned = 0")?;
    if (normal_total as usize) > keep_normal {
        overflow.extend(overflow_rows(conn, 0, keep_normal)?);
    }
    if overflow.is_empty() {
        return Ok(());
    }

    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("begin: {e}"))?;
    for (id, _, _) in &overflow {
        tx.execute("DELETE FROM items WHERE id = ?1", [id.as_str()])
            .map_err(|e| format!("delete: {e}"))?;
    }
    tx.commit().map_err(|e| format!("commit: {e}"))?;

    // 行已删掉，对应的图片文件也没人引用了
    for (_, content, item_type) in overflow {
        if item_type == "image" && !content.is_empty() {
            storage::delete_image_file(data_dir, &content);
        }
    }
    Ok(())
}

/// 该组里超出名额、按时间最旧的那批 (id, content, type)。
fn overflow_rows(
    conn: &Connection,
    is_pinned: i64,
    offset: usize,
) -> Result<Vec<(String, String, String)>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, content, type FROM items
             WHERE is_pinned = ?1
             ORDER BY created_at DESC, rowid DESC
             LIMIT -1 OFFSET ?2",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![is_pinned, offset as i64], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

fn count(conn: &Connection, sql: &str) -> Result<i64, String> {
    conn.query_row(sql, [], |row| row.get(0))
        .map_err(|e| e.to_string())
}

/// 数据库行 → 领域模型
pub(crate) fn row_to_item(row: &rusqlite::Row) -> rusqlite::Result<ClipboardItem> {
    let tags_json: Option<String> = row.get("tags")?;
    Ok(ClipboardItem {
        id: row.get("id")?,
        content: row.get("content")?,
        item_type: row.get("type")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        is_pinned: row.get("is_pinned")?,
        tags: tags_json.and_then(|s| serde_json::from_str(&s).ok()),
        source: row.get("source")?,
        image_hash: row.get("image_hash")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ClipboardItem;
    use std::fs;
    use tempfile::TempDir;

    fn temp() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    fn item_with(content: &str, created_at: i64) -> ClipboardItem {
        let mut item = ClipboardItem::new(content.to_string(), "text");
        item.created_at = created_at;
        item.updated_at = created_at;
        item
    }

    fn write_json(dir: &Path, items: &[ClipboardItem]) {
        fs::write(
            storage::get_data_file_path(dir),
            serde_json::to_string(items).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn test_open_creates_schema_on_fresh_dir() {
        let dir = temp();
        let conn = open(dir.path());

        let migration = migrate_json(&conn, dir.path());
        assert_eq!(migration, HistoryMigration::Fresh);

        let version: i32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);

        let tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='items'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tables, 1);
    }

    #[test]
    fn test_migrate_imports_json_with_pinned_and_image() {
        let dir = temp();
        let mut pinned = item_with("pinned text", 100);
        pinned.is_pinned = true;
        let mut image = ClipboardItem::new_image("images/a.png".to_string(), "hash-a".to_string());
        image.created_at = 200;
        image.updated_at = 200;
        let normal = item_with("normal", 300);
        write_json(dir.path(), &[pinned.clone(), image.clone(), normal.clone()]);

        let conn = open(dir.path());
        assert_eq!(
            migrate_json(&conn, dir.path()),
            HistoryMigration::Imported(3)
        );

        let imported = count(&conn, "SELECT COUNT(*) FROM items").unwrap();
        assert_eq!(imported, 3);

        let restored: ClipboardItem = conn
            .query_row("SELECT * FROM items WHERE content = 'pinned text'", [], row_to_item)
            .unwrap();
        assert!(restored.is_pinned);
        assert_eq!(restored.tags, pinned.tags);

        let restored_image: ClipboardItem = conn
            .query_row("SELECT * FROM items WHERE type = 'image'", [], row_to_item)
            .unwrap();
        assert_eq!(restored_image.image_hash.as_deref(), Some("hash-a"));
        assert_eq!(restored_image.content, "images/a.png");
    }

    #[test]
    fn test_migrate_is_idempotent() {
        let dir = temp();
        write_json(dir.path(), &[item_with("only", 1)]);

        let conn = open(dir.path());
        assert_eq!(
            migrate_json(&conn, dir.path()),
            HistoryMigration::Imported(1)
        );
        // 第二次启动：已迁移，不再读 JSON
        assert_eq!(
            migrate_json(&conn, dir.path()),
            HistoryMigration::AlreadyMigrated
        );
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM items").unwrap(), 1);
    }

    #[test]
    fn test_migrate_skips_duplicate_content() {
        let dir = temp();
        let a = item_with("same", 1);
        let mut b = item_with("same", 2);
        b.id = "different-id".to_string();
        write_json(dir.path(), &[a, b]);

        let conn = open(dir.path());
        assert_eq!(
            migrate_json(&conn, dir.path()),
            HistoryMigration::Imported(1)
        );
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM items").unwrap(), 1);
    }

    #[test]
    fn test_migrate_corrupt_json_keeps_file_and_retries() {
        let dir = temp();
        let json_path = storage::get_data_file_path(dir.path());
        fs::write(&json_path, b"[{\"id\": \"truncated").unwrap();

        let conn = open(dir.path());
        assert_eq!(migrate_json(&conn, dir.path()), HistoryMigration::JsonCorrupt);

        // 原文件一个字节都不能动，留给用户手动恢复
        assert_eq!(fs::read(&json_path).unwrap(), b"[{\"id\": \"truncated");
        // 不写迁移标记：文件修好后下次启动会重新导入
        let version: i32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 0);
        // 解析失败还不能把库标成已迁移，重复尝试结果一致
        assert_eq!(migrate_json(&conn, dir.path()), HistoryMigration::JsonCorrupt);
    }

    #[test]
    fn test_migrate_unreadable_json_retries_next_start() {
        let dir = temp();
        // 用同名目录占位：read_to_string 必然 IO 失败
        fs::create_dir(storage::get_data_file_path(dir.path())).unwrap();

        let conn = open(dir.path());
        assert_eq!(
            migrate_json(&conn, dir.path()),
            HistoryMigration::JsonUnreadable
        );
        let version: i32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 0);
    }

    fn insert(conn: &Connection, item: &ClipboardItem) {
        let hash = content_hash(&item.content);
        conn.execute(
            INSERT_ITEM,
            params![
                item.id,
                item.content,
                item.item_type,
                hash,
                item.created_at,
                item.updated_at,
                item.is_pinned,
                Option::<String>::None,
                item.source,
                item.image_hash,
            ],
        )
        .unwrap();
    }

    #[test]
    fn test_enforce_trims_oldest_and_deletes_image_file() {
        let dir = temp();
        let images_dir = dir.path().join("images");
        fs::create_dir_all(&images_dir).unwrap();
        fs::write(images_dir.join("old.png"), b"png").unwrap();

        let mut old_image =
            ClipboardItem::new_image("images/old.png".to_string(), "hash-old".to_string());
        old_image.created_at = 1_000;
        let newer_text = item_with("newer", 2_000);

        let conn = open(dir.path());
        insert(&conn, &old_image);
        insert(&conn, &newer_text);

        enforce_max_items(&conn, 1, dir.path()).unwrap();

        assert_eq!(count(&conn, "SELECT COUNT(*) FROM items").unwrap(), 1);
        let kept: ClipboardItem = conn
            .query_row("SELECT * FROM items", [], row_to_item)
            .unwrap();
        assert_eq!(kept.content, "newer");
        assert!(!images_dir.join("old.png").exists(), "被裁掉的图片文件应删除");
    }

    #[test]
    fn test_enforce_truncates_pinned_when_over_limit() {
        // 置顶条目多于上限时也要截断，否则历史会无上限增长
        let dir = temp();
        let conn = open(dir.path());
        for (content, created_at) in [("a", 1i64), ("b", 2), ("c", 3)] {
            let mut item = item_with(content, created_at);
            item.is_pinned = true;
            insert(&conn, &item);
        }

        enforce_max_items(&conn, 2, dir.path()).unwrap();

        let remaining = count(&conn, "SELECT COUNT(*) FROM items").unwrap();
        assert_eq!(remaining, 2);
        // 保留最新的两条
        let has: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM items WHERE content IN ('b', 'c')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(has, 2);
        let dropped: i64 = conn
            .query_row("SELECT COUNT(*) FROM items WHERE content = 'a'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(dropped, 0);
    }

    #[test]
    fn test_enforce_unlimited_skips() {
        // max_items = 0 表示不限（无限保存），一条都不许裁
        let dir = temp();
        let conn = open(dir.path());
        for i in 0..5 {
            insert(&conn, &item_with(&format!("item {i}"), i));
        }

        enforce_max_items(&conn, 0, dir.path()).unwrap();
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM items").unwrap(), 5);
    }
}
