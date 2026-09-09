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

/// 构造 adb 命令：Windows 下加 CREATE_NO_WINDOW。
/// adb.exe 是控制台程序，GUI 子系统下裸 spawn 会闪一下黑色控制台窗口
fn adb_command(adb: &Path) -> tokio::process::Command {
    // 非 Windows 平台不会调用 creation_flags，mut 未使用，压掉警告
    #[allow(unused_mut)]
    let mut cmd = tokio::process::Command::new(adb);
    // tokio 的 creation_flags 是 Windows 专属固有方法，无需引 trait
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    cmd
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

/// 清空缓存目录内其他 .apk（版本更新后旧包不堆积）
fn clean_cache(dir: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        let is_old_apk = p.extension().map(|x| x == "apk").unwrap_or(false) && p != keep;
        if is_old_apk {
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
    /// 残留 .part 是否已清理过（服务生命周期内只清一次）
    part_swept: OnceCell<()>,
    http: reqwest::Client,
}

impl Default for RzjService {
    fn default() -> Self {
        Self {
            running: Mutex::new(HashSet::new()),
            downloads: Mutex::new(HashMap::new()),
            waiters: Mutex::new(HashMap::new()),
            part_swept: OnceCell::new(),
            http: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(15))
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .expect("HTTP 客户端初始化失败"),
        }
    }
}

impl RzjService {
    /// 设备列表：执行内置 `adb devices`
    pub async fn devices(&self, app: &AppHandle) -> Result<Vec<RzjDevice>, String> {
        let adb = adb_binary(app)?;
        let out = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            adb_command(&adb)
                .arg("devices")
                .output()
                .await
        })
        .await
        .map_err(|_| "adb 响应超时".to_string())?
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
            .map_err(|e| format!("清单请求失败，请检查网络: {e}"))?
            .error_for_status()
            .map_err(|e| format!("清单请求失败: HTTP {}: {e}", e.status().map(|s| s.as_u16()).unwrap_or(0)))?
            .json()
            .await
            .map_err(|e| format!("清单解析失败: {e}"))?;
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

    /// 发起安装任务（同设备进行中时拒绝）；任务在后台运行，进度经事件推送
    pub async fn start_install(
        self: &Arc<Self>,
        app: &AppHandle,
        serial: String,
        url: String,
    ) -> Result<(), String> {
        if url.is_empty() {
            return Err("下载地址不能为空".into());
        }
        {
            let mut running = self.running.lock().await;
            if !running.insert(serial.clone()) {
                return Err("该设备正在安装中".into());
            }
        }
        let svc = self.clone();
        let app2 = app.clone();
        tokio::spawn(async move {
            let res = svc.pipeline(&app2, &serial, &url).await;
            match res {
                Ok(msg) => emit(&app2, &serial, "done", 100, msg),
                Err(e) => emit(&app2, &serial, "error", 0, e),
            }
            svc.running.lock().await.remove(&serial);
        });
        Ok(())
    }

    async fn pipeline(
        self: &Arc<Self>,
        app: &AppHandle,
        serial: &str,
        url: &str,
    ) -> Result<String, String> {
        emit(app, serial, "download", 0, "准备下载".into());
        let apk = self.ensure_downloaded(app, url, serial).await?;
        emit(app, serial, "install", 0, "开始安装".into());
        self.install_apk(app, serial, &apk).await?;
        emit(app, serial, "launch", 100, "拉起应用".into());
        match self.launch(app, serial).await {
            Ok(()) => Ok("安装成功".into()),
            Err(e) => Ok(format!("安装成功，拉起失败: {e}")),
        }
    }

    /// 共享下载：文件已存在直接复用；否则注册等待者并去重下载（先到者下载、其余等待）
    async fn ensure_downloaded(
        self: &Arc<Self>,
        app: &AppHandle,
        url: &str,
        serial: &str,
    ) -> Result<PathBuf, String> {
        let dir = cache_dir(app)?;
        let fname = url
            .rsplit('/')
            .next()
            .filter(|s| !s.is_empty())
            .ok_or("下载地址无效")?;
        let target = dir.join(fname);
        if target.exists() {
            emit(app, serial, "download", 100, "已使用本地缓存包".into());
            return Ok(target);
        }
        {
            let mut w = self.waiters.lock().await;
            w.entry(url.to_string()).or_default().push(serial.to_string());
        }
        let cell = {
            let mut d = self.downloads.lock().await;
            d.entry(url.to_string())
                .or_insert_with(|| Arc::new(OnceCell::new()))
                .clone()
        };
        let svc = self.clone();
        let app2 = app.clone();
        let url2 = url.to_string();
        let target2 = target.clone();
        let res = cell
            .get_or_try_init(move || async move { svc.download(&app2, &url2, &target2).await })
            .await;
        {
            let mut w = self.waiters.lock().await;
            if let Some(v) = w.get_mut(url) {
                v.retain(|s| s != serial);
                if v.is_empty() {
                    w.remove(url);
                }
            }
        }
        res.cloned()
    }

    /// 首次下载前清理残留 .part（整个服务生命周期只执行一次，避免误删并发下载的在途文件）
    async fn sweep_parts(&self, app: &AppHandle) {
        self.part_swept
            .get_or_init(|| async {
                if let Ok(dir) = cache_dir(app) {
                    if let Ok(entries) = std::fs::read_dir(&dir) {
                        for e in entries.flatten() {
                            let p = e.path();
                            if p.to_string_lossy().ends_with(".part") {
                                let _ = std::fs::remove_file(p);
                            }
                        }
                    }
                }
            })
            .await;
    }

    /// 实际下载：先清旧缓存 → .part 临时文件 → 完成后改名；进度向所有等待者广播
    async fn download(
        self: &Arc<Self>,
        app: &AppHandle,
        url: &str,
        target: &Path,
    ) -> Result<PathBuf, String> {
        let dir = target.parent().ok_or("缓存路径异常")?;
        self.sweep_parts(app).await;
        clean_cache(dir, target);
        let resp = self
            .http
            .get(url)
            .timeout(std::time::Duration::from_secs(1800)) // 大文件下载：覆盖客户端 15s 整体超时
            .send()
            .await
            .map_err(|e| format!("下载请求失败: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("下载失败: HTTP {}", resp.status()));
        }
        let total = resp.content_length().unwrap_or(0);
        let tmp = target.with_extension("apk.part");
        let mut file = tokio::fs::File::create(&tmp)
            .await
            .map_err(|e| format!("创建缓存文件失败: {e}"))?;
        let mut resp = resp;
        let mut done = 0u64;
        let mut last = 0u8;
        while let Some(chunk) = resp.chunk().await.map_err(|e| format!("下载中断: {e}"))? {
            file.write_all(&chunk)
                .await
                .map_err(|e| format!("写入缓存失败: {e}"))?;
            done += chunk.len() as u64;
            if total > 0 {
                let pct = ((done * 100) / total).min(100) as u8;
                if pct != last {
                    last = pct;
                    let waiters = {
                        let w = self.waiters.lock().await;
                        w.get(url).cloned().unwrap_or_default()
                    };
                    for s in waiters {
                        emit(app, &s, "download", pct, format!("下载中 {pct}%"));
                    }
                }
            }
        }
        file.flush().await.map_err(|e| format!("写入缓存失败: {e}"))?;
        drop(file);
        tokio::fs::rename(&tmp, target)
            .await
            .map_err(|e| format!("缓存写入完成失败: {e}"))?;
        Ok(target.to_path_buf())
    }

    /// `adb -s <serial> install -r <apk>`，流式解析百分比推进度
    async fn install_apk(&self, app: &AppHandle, serial: &str, apk: &Path) -> Result<(), String> {
        let adb = adb_binary(app)?;
        let mut child = adb_command(&adb)
            .args(["-s", serial, "install", "-r"])
            .arg(apk)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("启动 adb install 失败: {e}"))?;
        // stderr 已置空（防止管道写满阻塞）；整体 900s 超时兜底，超时杀掉子进程
        let res = tokio::time::timeout(std::time::Duration::from_secs(900), async {
            let mut stdout = child.stdout.take().ok_or_else(|| "adb 输出获取失败".to_string())?;
            let mut all = String::new();
            let mut buf = vec![0u8; 2048];
            loop {
                let n = stdout
                    .read(&mut buf)
                    .await
                    .map_err(|e| format!("adb 输出读取失败: {e}"))?;
                if n == 0 {
                    break;
                }
                let chunk = String::from_utf8_lossy(&buf[..n]).to_string();
                all.push_str(&chunk);
                if let Some(p) = parse_install_percent(&chunk) {
                    emit(app, serial, "install", p, format!("安装中 {p}%"));
                }
            }
            let status = child.wait().await.map_err(|e| format!("等待 adb 退出失败: {e}"))?;
            if !status.success() || !all.contains("Success") {
                let detail = all.trim().lines().last().unwrap_or("未知错误");
                return Err(format!("安装失败: {}", detail.trim()));
            }
            Ok::<(), String>(())
        })
        .await;
        match res {
            Ok(inner) => {
                inner?;
                emit(app, serial, "install", 100, "安装完成".into());
                Ok(())
            }
            Err(_) => {
                let _ = child.start_kill();
                Err("安装超时".to_string())
            }
        }
    }

    /// monkey 拉起入住机；失败只警告不视为任务失败（由 pipeline 决定）
    async fn launch(&self, app: &AppHandle, serial: &str) -> Result<(), String> {
        let adb = adb_binary(app)?;
        let out = tokio::time::timeout(std::time::Duration::from_secs(30), async {
            adb_command(&adb)
                .args([
                    "-s", serial, "shell", "monkey", "-p", PACKAGE,
                    "-c", "android.intent.category.LAUNCHER", "1",
                ])
                .output()
                .await
        })
        .await
        .map_err(|_| "拉起命令超时".to_string())?
        .map_err(|e| format!("拉起命令执行失败: {e}"))?;
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        if out.status.success() && !text.contains("Error") {
            Ok(())
        } else {
            Err(text.lines().last().unwrap_or("未知错误").trim().to_string())
        }
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

#[tauri::command]
pub async fn rzj_install(
    app: AppHandle,
    state: State<'_, AppState>,
    serial: String,
    url: String,
) -> Result<(), String> {
    state.rzj.start_install(&app, serial, url).await
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
