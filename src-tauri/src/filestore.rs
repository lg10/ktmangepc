//! 文件库：云端拉取固件/配置文件 → 分包 MD5 → 本地 SQLite
//!
//! 对应原 UpdateFileDialog.axaml.cs / ConfigFileDialog.axaml.cs：
//! - 输入云端 download 链接，将 download 替换为 getInfo 获取文件元信息
//! - 下载文件后按 size（固件 512/1024、配置固定 512）分包
//! - 索引 0 存元信息（包数/总长度/整体 MD5/版本），1..N 存分包 hex 与分包 MD5
//! - 进度通过事件 file://fetch-progress 推送

use crate::db::DbService;
use crate::events;
use rusqlite::OptionalExtension;
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// 文件类型：upgrade 固件 / config 配置
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Upgrade,
    Config,
}

impl FileKind {
    pub fn table(&self) -> &'static str {
        match self {
            FileKind::Upgrade => "update_file",
            FileKind::Config => "config_file",
        }
    }
    pub fn package_table(&self) -> &'static str {
        match self {
            FileKind::Upgrade => "update_file_package",
            FileKind::Config => "config_file_package",
        }
    }
}

/// 文件库条目（列表展示）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub uid: i64,
    pub name: String,
    pub hotel_name: String,
    pub version: String,
    pub author: String,
    pub custom: i64,
    pub size: i64,
    pub room_type_name: String,
    pub hotel_id: i64,
    pub create_time: i64,
    pub file_time: i64,
}

/// 拉取进度事件载荷
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchProgress {
    pub stage: String,  // info / download / split / done / error
    pub percent: u8,    // 0-100
    pub message: String,
}

fn emit_progress(app: &AppHandle, stage: &str, percent: u8, message: String) {
    let _ = app.emit(
        events::FILE_FETCH_PROGRESS,
        FetchProgress {
            stage: stage.to_string(),
            percent,
            message,
        },
    );
}

fn md5_hex(data: &[u8]) -> String {
    format!("{:x}", md5::compute(data))
}

pub struct FileStore {
    db: Arc<DbService>,
    http: reqwest::Client,
}

impl Default for FileStore {
    fn default() -> Self {
        Self::new(Arc::new(DbService::default()))
    }
}

impl FileStore {
    pub fn new(db: Arc<DbService>) -> Self {
        Self {
            db,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(300))
                .build()
                .expect("HTTP 客户端初始化失败"),
        }
    }

    /// 从云端拉取文件并入库（阻塞式任务，前端在后台调用）
    pub async fn fetch(&self, app: &AppHandle, kind: FileKind, url: &str) -> Result<FileEntry, String> {
        let url = url.trim();
        if url.is_empty() {
            return Err("文件地址不能为空".into());
        }
        // download/downloadFor -> getInfo
        let info_url = if url.contains("downloadFor") {
            url.replace("downloadFor", "getInfo")
        } else {
            url.replace("download", "getInfo")
        };

        emit_progress(app, "info", 5, "正在尝试获取文件数据".into());
        let resp: serde_json::Value = self
            .http
            .get(&info_url)
            .send()
            .await
            .map_err(|_| "数据获取失败，请检查网络或文件地址".to_string())?
            .json()
            .await
            .map_err(|_| "文件信息解析失败".to_string())?;

        let data = resp
            .get("data")
            .filter(|d| d.is_object())
            .ok_or("文件已失效无法下载")?;
        let uid = data.get("id").and_then(|v| v.as_i64()).ok_or("文件信息缺少 id")?;
        let name = data.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let hotel_name = data.get("hotel_name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let version = data.get("version").and_then(|v| v.as_str()).ok_or("文件信息缺少版本号")?.to_string();
        let author = data.get("author").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let custom = data.get("custom").and_then(|v| v.as_i64()).unwrap_or(0);
        let room_type_id = data.get("room_type_id").and_then(|v| v.as_i64()).unwrap_or(0);
        let room_type_name = data.get("room_type_name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let hotel_id = data.get("hotel_id").and_then(|v| v.as_i64()).unwrap_or(0);
        let create_time_ms = data.get("create_time").and_then(|v| v.as_i64()).unwrap_or(0);
        let file_time_s = data.get("file_time").and_then(|v| v.as_i64()).unwrap_or(0);
        let file_url = data
            .get("file_url")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or("文件不存在")?
            .to_string();

        // 分包大小：固件 1024（新版）/512（旧版）由链接区分交给前端传入，这里固定规则：
        // 配置文件恒 512；升级文件按元信息 size 字段（无则 1024）
        let size: i64 = match kind {
            FileKind::Config => 512,
            FileKind::Upgrade => data.get("size").and_then(|v| v.as_i64()).unwrap_or(1024),
        };

        // 已存在检查（与原逻辑一致：同 id 不同 size 禁止混存）
        let exists = self
            .db
            .with_conn(|conn| {
                conn.query_row(
                    &format!("SELECT size FROM {} WHERE uid = ?", kind.table()),
                    [uid],
                    |r| r.get::<_, i64>(0),
                )
                .optional()
            })
            .map_err(|e| e.to_string())?;
        if let Some(old_size) = exists {
            if old_size != size {
                return Err("新老版本升级文件不能重复，请先删除另一个版本文件库中文件".into());
            }
            return Err("文件已存在，请直接选中升级".into());
        }

        // 下载
        emit_progress(app, "download", 15, "文件开始下载".into());
        let resp = self
            .http
            .get(&file_url)
            .send()
            .await
            .map_err(|e| format!("文件下载失败: {e}"))?;
        let file_data = resp
            .bytes()
            .await
            .map_err(|e| format!("文件读取失败: {e}"))?
            .to_vec();
        if file_data.is_empty() {
            return Err("下载的文件为空".into());
        }

        emit_progress(app, "split", 55, "文件下载完成，开始载入本地库".into());
        let total = file_data.len();
        let num = ((total as f64) / (size as f64)).ceil() as i64;
        let total_md5 = md5_hex(&file_data);

        // 分包入库（单事务）；返回值用副本，避免被 move 闭包消耗
        let ret = (
            name.clone(),
            hotel_name.clone(),
            version.clone(),
            author.clone(),
            room_type_name.clone(),
        );
        let table = kind.table().to_string();
        let pkg_table = kind.package_table().to_string();
        self.db.with_conn(move |conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute(&format!("DELETE FROM {pkg_table} WHERE uid = ?1"), [uid])?;
            match kind {
                FileKind::Upgrade => {
                    tx.execute(
                        &format!(
                            "INSERT OR REPLACE INTO {table} (uid,name,hotel_name,version,author,custom,size,create_time,file_time) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)"
                        ),
                        rusqlite::params![
                            uid, name, hotel_name, version, author, custom, size,
                            create_time_ms / 1000, file_time_s
                        ],
                    )?;
                }
                FileKind::Config => {
                    tx.execute(
                        &format!(
                            "INSERT OR REPLACE INTO {table} (uid,name,hotel_name,version,author,room_type_id,room_type_name,hotel_id,create_time,file_time) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)"
                        ),
                        rusqlite::params![
                            uid, name, hotel_name, version, author, room_type_id,
                            room_type_name, hotel_id, create_time_ms / 1000, file_time_s
                        ],
                    )?;
                }
            }
            // 索引 0：元信息
            tx.execute(
                &format!(
                    "INSERT INTO {pkg_table} (uid,data_index,num,version,package,md5) VALUES (?1,0,?2,?3,?4,?5)"
                ),
                rusqlite::params![uid, num, version, total.to_string(), total_md5],
            )?;
            // 分包 1..N
            let mut stmt = tx.prepare(&format!(
                "INSERT INTO {pkg_table} (uid,data_index,num,version,package,md5) VALUES (?1,?2,0,'',?3,?4)"
            ))?;
            for i in 0..num {
                let from = (i * size) as usize;
                let to = ((i + 1) * size as i64).min(total as i64) as usize;
                let chunk = &file_data[from..to];
                stmt.execute(rusqlite::params![
                    uid,
                    i + 1,
                    hex::encode(chunk),
                    md5_hex(chunk)
                ])?;
            }
            drop(stmt);
            tx.commit()?;
            Ok(())
        })?;

        emit_progress(app, "done", 100, "文件已载入本地文件库".into());
        Ok(FileEntry {
            uid,
            name: ret.0,
            hotel_name: ret.1,
            version: ret.2,
            author: ret.3,
            custom,
            size,
            room_type_name: ret.4,
            hotel_id,
            create_time: create_time_ms / 1000,
            file_time: file_time_s,
        })
    }

    /// 文件列表
    pub fn list(&self, kind: FileKind, size: Option<i64>) -> Result<Vec<FileEntry>, String> {
        self.db.with_conn(move |conn| {
            let table = kind.table();
            let mut sql = match kind {
                FileKind::Upgrade => format!(
                    "SELECT uid,name,hotel_name,version,author,custom,size,'' AS room_type_name,0 AS hotel_id,create_time,file_time FROM {table}"
                ),
                FileKind::Config => format!(
                    "SELECT uid,name,hotel_name,version,author,0 AS custom,512 AS size,room_type_name,hotel_id,create_time,file_time FROM {table}"
                ),
            };
            if let Some(s) = size {
                sql.push_str(&format!(" WHERE size = {s}"));
            }
            sql.push_str(" ORDER BY create_time DESC");
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map([], |r| {
                Ok(FileEntry {
                    uid: r.get(0)?,
                    name: r.get(1)?,
                    hotel_name: r.get(2)?,
                    version: r.get(3)?,
                    author: r.get(4)?,
                    custom: r.get(5)?,
                    size: r.get(6)?,
                    room_type_name: r.get(7)?,
                    hotel_id: r.get(8)?,
                    create_time: r.get(9)?,
                    file_time: r.get(10)?,
                })
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })
    }

    /// 删除文件及其分包
    pub fn delete(&self, kind: FileKind, uid: i64) -> Result<(), String> {
        self.db.with_conn(move |conn| {
            conn.execute(&format!("DELETE FROM {} WHERE uid = ?", kind.table()), [uid])?;
            conn.execute(
                &format!("DELETE FROM {} WHERE uid = ?", kind.package_table()),
                [uid],
            )?;
            Ok(())
        })
    }

    /// 取分包元信息（索引 0）：(num, total_len, md5, version)
    pub fn get_meta(&self, kind: FileKind, uid: i64) -> Result<Option<(i64, i64, String, String)>, String> {
        self.db
            .with_conn(move |conn| {
                conn.query_row(
                    &format!(
                        "SELECT num, package, md5, version FROM {} WHERE uid = ? AND data_index = 0",
                        kind.package_table()
                    ),
                    [uid],
                    |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, String>(1)?.parse::<i64>().unwrap_or(0),
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                        ))
                    },
                )
                .optional()
            })
            .map_err(|e| e.to_string())
    }

    /// 取指定分包 (hex, md5)，供升级应答使用
    pub fn get_package(&self, kind: FileKind, uid: i64, index: i64) -> Result<Option<(String, String)>, String> {
        self.db
            .with_conn(move |conn| {
                conn.query_row(
                    &format!(
                        "SELECT package, md5 FROM {} WHERE uid = ? AND data_index = ?",
                        kind.package_table()
                    ),
                    rusqlite::params![uid, index],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
                )
                .optional()
            })
            .map_err(|e| e.to_string())
    }
}

fn parse_kind(kind: &str) -> Result<FileKind, String> {
    match kind {
        "firmware" | "upgrade" => Ok(FileKind::Upgrade),
        "config" => Ok(FileKind::Config),
        _ => Err("文件类型非法（firmware/config）".into()),
    }
}

#[tauri::command]
pub async fn fetch_file(
    app: AppHandle,
    state: tauri::State<'_, crate::state::AppState>,
    kind: String,
    url: String,
) -> Result<FileEntry, String> {
    let kind = parse_kind(&kind)?;
    state.filestore.fetch(&app, kind, &url).await
}

#[tauri::command]
pub fn list_files(
    state: tauri::State<'_, crate::state::AppState>,
    kind: String,
    size: Option<i64>,
) -> Result<Vec<FileEntry>, String> {
    let kind = parse_kind(&kind)?;
    state.filestore.list(kind, size)
}

#[tauri::command]
pub fn delete_file(
    state: tauri::State<'_, crate::state::AppState>,
    kind: String,
    uid: i64,
) -> Result<(), String> {
    let kind = parse_kind(&kind)?;
    state.filestore.delete(kind, uid)
}
