//! 本地 SQLite 文件库（全新建库，不迁移旧数据）
//!
//! 对应原项目 SQLite 表：UpdateFile / UpdateFilePackage / ConfigFile / ConfigFilePackage
//! 阶段一仅初始化表结构，阶段二固件升级/配置下发时写入真实数据。

use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

pub struct DbService {
    path: Mutex<Option<PathBuf>>,
}

impl Default for DbService {
    fn default() -> Self {
        Self {
            path: Mutex::new(None),
        }
    }
}

const SCHEMA: &str = r#"
-- 升级/配置文件库（uid 为云端文件 ID；与原项目 UpdateFile/ConfigFile 语义一致）
CREATE TABLE IF NOT EXISTS update_file (
    uid INTEGER PRIMARY KEY,
    name TEXT NOT NULL DEFAULT '',
    hotel_name TEXT NOT NULL DEFAULT '',
    version TEXT NOT NULL DEFAULT '',
    author TEXT NOT NULL DEFAULT '',
    custom INTEGER NOT NULL DEFAULT 0,
    size INTEGER NOT NULL,
    create_time INTEGER NOT NULL DEFAULT 0,
    file_time INTEGER NOT NULL DEFAULT 0
);
-- 分包表：data_index=0 为元信息（num=包数、package=总长度、md5=整体、version），
-- data_index=1..N 为分包（package=hex 数据、md5=分包）
CREATE TABLE IF NOT EXISTS update_file_package (
    uid INTEGER NOT NULL,
    data_index INTEGER NOT NULL,
    num INTEGER NOT NULL DEFAULT 0,
    version TEXT NOT NULL DEFAULT '',
    package TEXT NOT NULL DEFAULT '',
    md5 TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (uid, data_index)
);
CREATE TABLE IF NOT EXISTS config_file (
    uid INTEGER PRIMARY KEY,
    name TEXT NOT NULL DEFAULT '',
    hotel_name TEXT NOT NULL DEFAULT '',
    version TEXT NOT NULL DEFAULT '',
    author TEXT NOT NULL DEFAULT '',
    room_type_id INTEGER NOT NULL DEFAULT 0,
    room_type_name TEXT NOT NULL DEFAULT '',
    hotel_id INTEGER NOT NULL DEFAULT 0,
    create_time INTEGER NOT NULL DEFAULT 0,
    file_time INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS config_file_package (
    uid INTEGER NOT NULL,
    data_index INTEGER NOT NULL,
    num INTEGER NOT NULL DEFAULT 0,
    version TEXT NOT NULL DEFAULT '',
    package TEXT NOT NULL DEFAULT '',
    md5 TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (uid, data_index)
);
-- 酒店同步房间映射（equipId -> 云端房间 JSON）
CREATE TABLE IF NOT EXISTS hotel_room (
    equip_id TEXT PRIMARY KEY,
    hotel_id INTEGER NOT NULL DEFAULT 0,
    data TEXT NOT NULL
);
"#;

impl DbService {
    /// 初始化数据库文件与表结构，返回是否就绪
    pub fn init(&self, app: &AppHandle) -> bool {
        let dir = match app.path().app_data_dir() {
            Ok(d) => d,
            Err(_) => return false,
        };
        if std::fs::create_dir_all(&dir).is_err() {
            return false;
        }
        let file = dir.join("kt-local.db");
        match Connection::open(&file) {
            Ok(conn) => {
                // 阶段一旧表结构不兼容时重建（全新建库策略，无用户数据迁移需求）
                let need_rebuild = conn
                    .prepare("PRAGMA table_info(update_file)")
                    .and_then(|mut s| {
                        let cols: Vec<String> = s
                            .query_map([], |r| r.get::<_, String>(1))?
                            .filter_map(|r| r.ok().map(|v| v.to_lowercase()))
                            .collect();
                        Ok(!cols.is_empty() && !cols.iter().any(|c| c == "uid"))
                    })
                    .unwrap_or(false);
                if need_rebuild {
                    let _ = conn.execute_batch(
                        "DROP TABLE IF EXISTS update_file; DROP TABLE IF EXISTS update_file_package; DROP TABLE IF EXISTS config_file; DROP TABLE IF EXISTS config_file_package;",
                    );
                }
                match conn.execute_batch(SCHEMA) {
                    Ok(()) => {
                        *self.path.lock().unwrap() = Some(file);
                        true
                    }
                    Err(_) => false,
                }
            }
            Err(_) => false,
        }
    }

    pub fn is_ready(&self) -> bool {
        self.path.lock().unwrap().is_some()
    }

    #[allow(dead_code)]
    pub fn with_conn<T>(
        &self,
        f: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> Result<T, String> {
        let guard = self.path.lock().unwrap();
        let path = guard
            .as_ref()
            .ok_or_else(|| "数据库未初始化".to_string())?;
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        f(&conn).map_err(|e| e.to_string())
    }
}
