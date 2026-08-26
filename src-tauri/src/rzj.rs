//! 入住机一键安装：adb 连接设备列表 + 版本清单 + 下载/安装/拉起流水线
//!
//! - rzj_devices：执行内置 `adb devices`，解析序列号/状态/连接方式
//! - rzj_releases：拉取云端清单，解析 install 映射为可选版本列表
//! - rzj_install：共享下载（AppData/rzj-cache，下载新 URL 前清空旧 APK）
//!   → `adb -s <serial> install -r` → `monkey` 拉起固定包名；进度经
//!   `rzj://progress` 事件按设备序列号推送

use serde::Serialize;

use crate::events;
use crate::state::AppState;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{Mutex, OnceCell};

/// 清单地址（注意：服务端文件名为 lastest.json，非拼写错误）
pub const MANIFEST_URL: &str = "https://d.kingint.com/app/rzj/lastest.json";
/// 入住机包名（安装后拉起用）
pub const PACKAGE: &str = "com.kingint.checkin4";

/// adb 连接设备（序列号即展示名）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RzjDevice {
    pub serial: String,
    pub status: String,
    /// "usb" | "tcp"（序列号含 ':' 判为网络连接）
    pub transport: String,
}

/// 可安装的入住机版本
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RzjRelease {
    pub key: String,
    pub name: String,
    pub version: String,
    pub url: String,
}

/// 解析 `adb devices` 输出：跳过 banner / server 提示，仅取「序列号 + 状态」两字段行
fn parse_devices(stdout: &str) -> Vec<RzjDevice> {
    stdout
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let serial = parts.next()?;
            let status = parts.next()?;
            if parts.next().is_some() {
                return None; // 三字段以上为干扰行（如 List of devices attached）
            }
            Some(RzjDevice {
                serial: serial.to_string(),
                status: status.to_string(),
                transport: if serial.contains(':') { "tcp".into() } else { "usb".into() },
            })
        })
        .collect()
}

/// 解析 `adb install` 流式输出中的末尾百分比（进度以 \r 分隔输出，如 "37%"）
fn parse_install_percent(chunk: &str) -> Option<u8> {
    let last = chunk
        .split('\r')
        .rev()
        .map(str::trim)
        .find(|s| !s.is_empty())?;
    let digits = last.strip_suffix('%')?;
    let tail: String = digits.chars().rev().take_while(|c| c.is_ascii_digit()).collect();
    if tail.is_empty() {
        return None;
    }
    tail.chars().rev().collect::<String>().parse().ok()
}

/// 进度事件载荷
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RzjProgressPayload {
    serial: String,
    stage: String, // download | install | launch | done | error
    percent: u8,
    message: String,
}

fn emit(app: &AppHandle, serial: &str, stage: &str, percent: u8, message: String) {
    let _ = app.emit(
        events::RZJ_PROGRESS,
        RzjProgressPayload {
            serial: serial.to_string(),
            stage: stage.to_string(),
            percent,
            message,
        },
    );
}

/// 内置 adb 可执行文件路径
fn adb_binary(app: &AppHandle) -> Result<PathBuf, String> {
    let name = if cfg!(windows) { "adb.exe" } else { "adb" };
    Ok(crate::adbshell::resolve_adb_dir(app)?.join(name))
}

/// 下载缓存目录：AppData/rzj-cache
fn cache_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取数据目录失败: {e}"))?
        .join("rzj-cache");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建缓存目录失败: {e}"))?;
    Ok(dir)
}

/// 清空缓存目录内其他 .apk 与残留 .part（版本更新后旧包不堆积）
fn clean_cache(dir: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        let is_old_apk = p.extension().map(|x| x == "apk").unwrap_or(false) && p != keep;
        let is_part = p.to_string_lossy().ends_with(".part");
        if is_old_apk || is_part {
            let _ = std::fs::remove_file(p);
        }
    }
}

pub struct RzjService {
    /// 进行中的安装任务（防同设备重复发起）
    running: Mutex<HashSet<String>>,
    /// URL -> 共享下载结果（多设备同 URL 只下一次）
    downloads: Mutex<HashMap<String, Arc<OnceCell<PathBuf>>>>,
    /// URL -> 等待者序列号列表（下载进度广播用）
    waiters: Mutex<HashMap<String, Vec<String>>>,
    http: reqwest::Client,
}

impl Default for RzjService {
    fn default() -> Self {
        Self {
            running: Mutex::new(HashSet::new()),
            downloads: Mutex::new(HashMap::new()),
            waiters: Mutex::new(HashMap::new()),
            http: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(15))
                .build()
                .expect("HTTP 客户端初始化失败"),
        }
    }
}

impl RzjService {
    /// 设备列表：执行内置 `adb devices`
    pub async fn devices(&self, app: &AppHandle) -> Result<Vec<RzjDevice>, String> {
        let adb = adb_binary(app)?;
        let out = tokio::process::Command::new(&adb)
            .arg("devices")
            .output()
            .await
            .map_err(|e| format!("adb 执行失败: {e}"))?;
        Ok(parse_devices(&String::from_utf8_lossy(&out.stdout)))
    }

    /// 版本清单：拉取 lastest.json 的 install 映射
    pub async fn releases(&self) -> Result<Vec<RzjRelease>, String> {
        let v: serde_json::Value = self
            .http
            .get(MANIFEST_URL)
            .send()
            .await
            .map_err(|_| "清单请求失败，请检查网络".to_string())?
            .json()
            .await
            .map_err(|_| "清单解析失败".to_string())?;
        let install = v
            .get("install")
            .and_then(|i| i.as_object())
            .ok_or("清单缺少 install 字段")?;
        let mut list = Vec::new();
        for (key, item) in install {
            let url = item.get("url").and_then(|x| x.as_str()).unwrap_or_default();
            if url.is_empty() {
                continue;
            }
            list.push(RzjRelease {
                key: key.clone(),
                name: item
                    .get("name")
                    .and_then(|x| x.as_str())
                    .unwrap_or(key)
                    .to_string(),
                version: item
                    .get("version")
                    .and_then(|x| x.as_str())
                    .unwrap_or_default()
                    .to_string(),
                url: url.to_string(),
            });
        }
        list.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(list)
    }
}

#[tauri::command]
pub async fn rzj_devices(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<RzjDevice>, String> {
    state.rzj.devices(&app).await
}

#[tauri::command]
pub async fn rzj_releases(state: State<'_, AppState>) -> Result<Vec<RzjRelease>, String> {
    state.rzj.releases().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_devices_basic() {
        let out = "List of devices attached\nABC123\tdevice\n192.168.1.5:5555\tunauthorized\n\n";
        let list = parse_devices(out);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].serial, "ABC123");
        assert_eq!(list[0].status, "device");
        assert_eq!(list[0].transport, "usb");
        assert_eq!(list[1].serial, "192.168.1.5:5555");
        assert_eq!(list[1].status, "unauthorized");
        assert_eq!(list[1].transport, "tcp");
    }

    #[test]
    fn parse_devices_skips_banner() {
        let out = "* daemon not running; starting now at tcp:5037\n* daemon started successfully\nList of devices attached\n";
        assert!(parse_devices(out).is_empty());
    }

    #[test]
    fn parse_install_percent_variants() {
        assert_eq!(parse_install_percent("37%"), Some(37));
        assert_eq!(parse_install_percent("\r12%\r40%"), Some(40));
        assert_eq!(parse_install_percent("Performing Streamed Install\r55%"), Some(55));
        assert_eq!(parse_install_percent("Success"), None);
    }
}
