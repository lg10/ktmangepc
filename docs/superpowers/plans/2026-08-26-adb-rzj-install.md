# ADB 入住机一键安装 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 ADB 终端弹窗右上角加「已连接设备」按钮，滑出面板展示内置 adb 设备列表，正常连接设备可一键选择版本 → 下载 → 安装 → 拉起入住机（`com.kingint.checkin4`），全程进度条展示。

**Architecture:** 后端新增 `rzj.rs` 模块（设备列表 / 清单拉取 / 共享下载缓存 + adb install + monkey 拉起），通过 `rzj://progress` 事件推送进度；前端新增 `AdbDevicePanel.vue` 滑出面板，监听事件驱动进度条。

**Tech Stack:** Rust (tauri command, reqwest, tokio::process) + Vue 3.5 (Composition API, shadcn 风格 UI 组件)

**设计文档:** `docs/superpowers/specs/2026-08-26-adb-rzj-install-design.md`

**验证说明:** 本项目前端无测试框架，前端靠 `pnpm build`（含 vue-tsc 类型检查）验证；Rust 解析函数用 `#[cfg(test)]` 单测（TDD）。

---

### Task 1: Rust 解析函数（TDD）+ adb 路径可见性调整

**Files:**
- Create: `src-tauri/src/rzj.rs`
- Modify: `src-tauri/src/adbshell.rs:156-157`（`resolve_adb_dir` 改 `pub(crate)`）
- Modify: `src-tauri/src/lib.rs`（`pub mod rzj;`）

- [ ] **Step 1: 修改 `adbshell.rs` 可见性**

`src-tauri/src/adbshell.rs` 第 157 行：

```rust
/// 定位内置 adb 所在资源目录（打包：Resources/adb；dev：resources/adb）
pub(crate) fn resolve_adb_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
```

- [ ] **Step 2: 写失败测试**

新建 `src-tauri/src/rzj.rs`，先只放类型、解析函数和测试（模块后续 Task 逐步补全服务实现）：

```rust
//! 入住机一键安装：adb 连接设备列表 + 版本清单 + 下载/安装/拉起流水线
//!
//! - rzj_devices：执行内置 `adb devices`，解析序列号/状态/连接方式
//! - rzj_releases：拉取云端清单，解析 install 映射为可选版本列表
//! - rzj_install：共享下载（AppData/rzj-cache，下载新 URL 前清空旧 APK）
//!   → `adb -s <serial> install -r` → `monkey` 拉起固定包名；进度经
//!   `rzj://progress` 事件按设备序列号推送

use serde::Serialize;

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
```

在 `src-tauri/src/lib.rs` 的 `pub mod restore;` 行后加：

```rust
pub mod rzj;
```

- [ ] **Step 3: 运行测试确认通过**

Run: `cd src-tauri && cargo test rzj::tests`
Expected: 3 passed（纯解析函数，测试直接通过；本任务无先失败环节，若编译错误则修到通过）

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/rzj.rs src-tauri/src/adbshell.rs src-tauri/src/lib.rs
git commit -m "feat(rzj): adb 设备列表与安装进度解析函数"
```

---

### Task 2: RzjService — 设备列表与版本清单

**Files:**
- Modify: `src-tauri/src/rzj.rs`（追加服务与命令）
- Modify: `src-tauri/src/events.rs`（加 `RZJ_PROGRESS`）
- Modify: `src-tauri/src/state.rs`（挂 `rzj` 服务）
- Modify: `src-tauri/src/lib.rs`（setup 构造 + handler 注册）

- [ ] **Step 1: `events.rs` 末尾（ADB 终端段之后）加事件名**

```rust
// 入住机安装进度（按设备序列号推送）
pub const RZJ_PROGRESS: &str = "rzj://progress";
```

- [ ] **Step 2: `rzj.rs` 追加服务实现**

在解析函数与测试之间（`tests` 模块之前）追加：

```rust
use crate::events;
use crate::state::AppState;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{Mutex, OnceCell};

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
```

- [ ] **Step 3: `state.rs` 挂载服务**

import 区加 `use crate::rzj::RzjService;`，`AppState` 加字段（`adbshell` 后）：

```rust
    pub rzj: Arc<RzjService>,
```

- [ ] **Step 4: `lib.rs` setup 与注册**

setup 中 `adbshell: Arc::new(adbshell::AdbShellService::default()),` 行后加：

```rust
                rzj: Arc::new(rzj::RzjService::default()),
```

`invoke_handler` 中 ADB 终端组之后加：

```rust
            // 入住机一键安装
            rzj::rzj_devices,
            rzj::rzj_releases,
```

- [ ] **Step 5: 编译验证**

Run: `cd src-tauri && cargo check`
Expected: 无错误

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/rzj.rs src-tauri/src/events.rs src-tauri/src/state.rs src-tauri/src/lib.rs
git commit -m "feat(rzj): 设备列表与版本清单命令"
```

---

### Task 3: RzjService — 下载/安装/拉起流水线

**Files:**
- Modify: `src-tauri/src/rzj.rs`（追加流水线）
- Modify: `src-tauri/src/lib.rs`（注册 `rzj_install`）

- [ ] **Step 1: `impl RzjService` 内追加流水线方法**

```rust
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

    /// 实际下载：先清旧缓存 → .part 临时文件 → 完成后改名；进度向所有等待者广播
    async fn download(
        self: &Arc<Self>,
        app: &AppHandle,
        url: &str,
        target: &Path,
    ) -> Result<PathBuf, String> {
        let dir = target.parent().ok_or("缓存路径异常")?;
        clean_cache(dir, target);
        let resp = self
            .http
            .get(url)
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
        let mut child = tokio::process::Command::new(&adb)
            .args(["-s", serial, "install", "-r"])
            .arg(apk)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("启动 adb install 失败: {e}"))?;
        let mut stdout = child.stdout.take().ok_or("adb 输出获取失败")?;
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
        let mut err = String::new();
        if let Some(mut stderr) = child.stderr.take() {
            let _ = stderr.read_to_string(&mut err).await;
        }
        if !status.success() || !all.contains("Success") {
            let detail = err
                .trim()
                .lines()
                .last()
                .or_else(|| all.trim().lines().last())
                .unwrap_or("未知错误");
            return Err(format!("安装失败: {}", detail.trim()));
        }
        emit(app, serial, "install", 100, "安装完成".into());
        Ok(())
    }

    /// monkey 拉起入住机；失败只警告不视为任务失败（由 pipeline 决定）
    async fn launch(&self, app: &AppHandle, serial: &str) -> Result<(), String> {
        let adb = adb_binary(app)?;
        let out = tokio::process::Command::new(&adb)
            .args([
                "-s", serial, "shell", "monkey", "-p", PACKAGE,
                "-c", "android.intent.category.LAUNCHER", "1",
            ])
            .output()
            .await
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
```

- [ ] **Step 2: 追加 tauri 命令并注册**

`rzj.rs` 命令区追加：

```rust
#[tauri::command]
pub async fn rzj_install(
    app: AppHandle,
    state: State<'_, AppState>,
    serial: String,
    url: String,
) -> Result<(), String> {
    state.rzj.start_install(&app, serial, url).await
}
```

`lib.rs` handler 的入住机组内追加 `rzj::rzj_install,`。

- [ ] **Step 3: 编译与测试**

Run: `cd src-tauri && cargo check && cargo test rzj::tests`
Expected: 编译通过、3 个测试通过

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/rzj.rs src-tauri/src/lib.rs
git commit -m "feat(rzj): 下载/安装/拉起流水线与共享缓存"
```

---

### Task 4: 前端 API 与类型

**Files:**
- Modify: `src/types/index.ts`（末尾追加）
- Modify: `src/lib/api.ts`（EVENTS + api 方法）

- [ ] **Step 1: `types/index.ts` 末尾追加**

```ts
/** adb 连接设备（入住机安装面板） */
export interface RzjDevice {
  serial: string;
  status: string;
  /** "usb" | "tcp" */
  transport: string;
}

/** 可安装的入住机版本 */
export interface RzjRelease {
  key: string;
  name: string;
  version: string;
  url: string;
}
```

- [ ] **Step 2: `api.ts` EVENTS 加 `RZJ_PROGRESS: "rzj://progress",`**（`ADB_CLOSED` 行后）

- [ ] **Step 3: `api.ts` import 类型并追加方法**

import 列表加 `RzjDevice, RzjRelease,`；`api` 对象末尾（`adbShellClose` 后）加：

```ts
  /** 入住机一键安装：设备列表 / 版本清单 / 发起安装（进度见 EVENTS.RZJ_PROGRESS） */
  rzjDevices: () => invoke<RzjDevice[]>("rzj_devices"),
  rzjReleases: () => invoke<RzjRelease[]>("rzj_releases"),
  rzjInstall: (serial: string, url: string) =>
    invoke<void>("rzj_install", { serial, url }),
```

- [ ] **Step 4: 类型检查**

Run: `pnpm exec vue-tsc --noEmit`
Expected: 无错误

- [ ] **Step 5: Commit**

```bash
git add src/types/index.ts src/lib/api.ts
git commit -m "feat(rzj): 前端 API 与类型定义"
```

---

### Task 5: AdbDevicePanel.vue 滑出面板

**Files:**
- Create: `src/components/AdbDevicePanel.vue`

- [ ] **Step 1: 新建组件**

```vue
<script setup lang="ts">
import { onBeforeUnmount, onMounted, reactive, ref, watch } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api, EVENTS } from "@/lib/api";
import type { RzjDevice, RzjRelease } from "@/types";
import { useToast } from "@/components/ui/toast/use-toast";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Loader2, RefreshCw, Smartphone, X } from "lucide-vue-next";

/**
 * ADB 终端弹窗内的右侧滑出面板：已连接设备列表 + 入住机版本选择安装。
 * 安装任务由后端驱动，面板开关不影响任务；进度经 rzj://progress 事件更新。
 */
const open = defineModel<boolean>("open", { default: false });
const { toast } = useToast();

const devices = ref<RzjDevice[]>([]);
const loading = ref(false);
const listError = ref("");

/** serial -> 进行中任务进度 */
const progress = reactive<
  Record<string, { stage: string; percent: number; message: string }>
>({});

// 版本选择视图
const view = ref<"devices" | "releases">("devices");
const releases = ref<RzjRelease[]>([]);
const releasesLoading = ref(false);
const releasesError = ref("");
const selectedUrl = ref("");
const installingSerial = ref("");

let unlisten: UnlistenFn | null = null;

async function refresh() {
  loading.value = true;
  listError.value = "";
  try {
    devices.value = await api.rzjDevices();
  } catch (e) {
    listError.value = String(e);
  } finally {
    loading.value = false;
  }
}

async function onInstall(device: RzjDevice) {
  installingSerial.value = device.serial;
  view.value = "releases";
  releasesError.value = "";
  selectedUrl.value = "";
  releasesLoading.value = true;
  try {
    releases.value = await api.rzjReleases();
  } catch (e) {
    releasesError.value = String(e);
  } finally {
    releasesLoading.value = false;
  }
}

async function confirmInstall() {
  if (!selectedUrl.value) return;
  try {
    await api.rzjInstall(installingSerial.value, selectedUrl.value);
  } catch (e) {
    toast({ title: "安装发起失败", description: String(e), variant: "destructive" });
  }
  view.value = "devices";
}

function onProgress(p: { serial: string; stage: string; percent: number; message: string }) {
  if (p.stage === "done") {
    progress[p.serial] = { stage: p.stage, percent: p.percent, message: p.message };
    toast({ title: "安装完成", description: p.message, variant: "success" });
    setTimeout(() => {
      delete progress[p.serial];
    }, 2500);
  } else if (p.stage === "error") {
    delete progress[p.serial];
    toast({ title: "安装失败", description: p.message, variant: "destructive" });
  } else {
    progress[p.serial] = { stage: p.stage, percent: p.percent, message: p.message };
  }
}

function stageText(p: { stage: string; percent: number }): string {
  switch (p.stage) {
    case "download":
      return `下载中 ${p.percent}%`;
    case "install":
      return p.percent > 0 ? `安装中 ${p.percent}%` : "安装中…";
    case "launch":
      return "拉起应用…";
    case "done":
      return "已完成";
    default:
      return "处理中…";
  }
}

function statusInfo(d: RzjDevice): {
  label: string;
  variant: "success" | "warning" | "secondary";
} {
  if (d.status === "device") return { label: "已连接", variant: "success" };
  if (d.status === "unauthorized") return { label: "未授权", variant: "warning" };
  return { label: "离线", variant: "secondary" };
}

watch(open, (v) => {
  if (v) refresh();
});

onMounted(async () => {
  unlisten = await listen<{
    serial: string;
    stage: string;
    percent: number;
    message: string;
  }>(EVENTS.RZJ_PROGRESS, (e) => onProgress(e.payload));
});

onBeforeUnmount(() => {
  unlisten?.();
});
</script>

<template>
  <transition
    enter-active-class="transition-transform duration-200"
    enter-from-class="translate-x-full"
    leave-active-class="transition-transform duration-200"
    leave-to-class="translate-x-full"
  >
    <aside
      v-if="open"
      class="absolute inset-y-0 right-0 z-20 flex w-[360px] flex-col border-l bg-background shadow-xl"
    >
      <div class="flex shrink-0 items-center gap-2 border-b px-4 py-3">
        <Smartphone class="h-4 w-4 text-muted-foreground" />
        <span class="text-sm font-semibold">已连接设备</span>
        <div class="ml-auto flex items-center gap-1">
          <Button variant="ghost" size="icon" class="h-7 w-7" :disabled="loading" @click="refresh">
            <RefreshCw class="h-3.5 w-3.5" :class="{ 'animate-spin': loading }" />
          </Button>
          <Button variant="ghost" size="icon" class="h-7 w-7" @click="open = false">
            <X class="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>

      <!-- 版本选择视图 -->
      <div v-if="view === 'releases'" class="flex-1 overflow-y-auto p-4">
        <p class="mb-2 text-xs text-muted-foreground">
          为 {{ installingSerial }} 选择要安装的入住机版本
        </p>
        <p v-if="releasesLoading" class="flex items-center gap-2 py-4 text-xs text-muted-foreground">
          <Loader2 class="h-3.5 w-3.5 animate-spin" /> 正在加载版本列表…
        </p>
        <p v-else-if="releasesError" class="py-4 text-xs text-destructive">{{ releasesError }}</p>
        <div v-else class="flex flex-col gap-2">
          <label
            v-for="r in releases"
            :key="r.key"
            class="flex cursor-pointer items-center gap-2 rounded-md border p-2.5 text-sm transition-colors hover:bg-accent"
            :class="{ 'border-primary': selectedUrl === r.url }"
          >
            <input
              v-model="selectedUrl"
              type="radio"
              name="rzj-release"
              :value="r.url"
              class="accent-primary"
            />
            <span class="flex-1">{{ r.name }}</span>
            <span class="text-xs text-muted-foreground">v{{ r.version }}</span>
          </label>
        </div>
        <div class="mt-4 flex justify-end gap-2">
          <Button variant="outline" size="sm" @click="view = 'devices'">返回</Button>
          <Button
            size="sm"
            :disabled="!selectedUrl || releasesLoading || !!releasesError"
            @click="confirmInstall"
          >
            确定
          </Button>
        </div>
      </div>

      <!-- 设备列表视图 -->
      <div v-else class="flex-1 overflow-y-auto p-3">
        <p v-if="listError" class="p-2 text-xs text-destructive">{{ listError }}</p>
        <p
          v-else-if="!loading && devices.length === 0"
          class="p-6 text-center text-xs text-muted-foreground"
        >
          未检测到已连接设备
        </p>
        <ul v-else class="flex flex-col gap-2">
          <li v-for="d in devices" :key="d.serial" class="rounded-md border p-2.5">
            <div class="flex items-center gap-2">
              <span class="min-w-0 flex-1 truncate font-mono text-xs">{{ d.serial }}</span>
              <Badge :variant="statusInfo(d).variant">{{ statusInfo(d).label }}</Badge>
              <Badge variant="outline">{{ d.transport === "tcp" ? "网络" : "USB" }}</Badge>
            </div>
            <div v-if="d.status === 'device'" class="mt-2">
              <!-- 任务进行中：按钮位置替换为进度条 -->
              <div v-if="progress[d.serial]" class="flex flex-col gap-1">
                <div class="flex justify-between text-xs text-muted-foreground">
                  <span>{{ stageText(progress[d.serial]) }}</span>
                </div>
                <div class="h-1.5 overflow-hidden rounded-full bg-muted">
                  <div
                    class="h-full rounded-full transition-all"
                    :class="progress[d.serial].stage === 'done' ? 'bg-success' : 'bg-primary'"
                    :style="{
                      width:
                        progress[d.serial].stage === 'launch' || progress[d.serial].stage === 'done'
                          ? '100%'
                          : progress[d.serial].percent + '%',
                    }"
                  />
                </div>
              </div>
              <Button v-else size="sm" variant="outline" class="w-full" @click="onInstall(d)">
                安装入住机
              </Button>
            </div>
          </li>
        </ul>
      </div>
    </aside>
  </transition>
</template>
```

- [ ] **Step 2: 类型检查**

Run: `pnpm exec vue-tsc --noEmit`
Expected: 无错误

- [ ] **Step 3: Commit**

```bash
git add src/components/AdbDevicePanel.vue
git commit -m "feat(rzj): 设备列表面板组件"
```

---

### Task 6: AdbTerminalDialog.vue 集成

**Files:**
- Modify: `src/components/AdbTerminalDialog.vue`

- [ ] **Step 1: script 区改动**

import 区追加：

```ts
import { Button } from "@/components/ui/button";
import AdbDevicePanel from "@/components/AdbDevicePanel.vue";
```

`RefreshCw, PowerOff` 的 lucide import 加 `Smartphone`：

```ts
import { RefreshCw, PowerOff, Smartphone } from "lucide-vue-next";
```

`openError` 声明后加：

```ts
const panelOpen = ref(false);
```

现有 `watch(() => uiStore.adbTerminalOpen, ...)` 的 `else` 分支开头加（关终端弹窗时收起面板）：

```ts
      panelOpen.value = false;
```

- [ ] **Step 2: template 改动**

标题行按钮（`ml-2` 提示文字之后）：

```vue
        <Button
          variant="ghost"
          size="sm"
          class="ml-auto h-7 gap-1.5 text-xs"
          @click="panelOpen = !panelOpen"
        >
          <Smartphone class="h-3.5 w-3.5" />
          已连接设备
        </Button>
```

`DialogContent` 开标签改为（面板打开时隐藏自带关闭叉，避免与面板头部重叠）：

```vue
    <DialogContent
      class="max-w-6xl w-[80vw] h-[80vh] flex flex-col gap-0 p-0 overflow-hidden"
      :show-close="!panelOpen"
    >
```

`</DialogContent>` 闭合前（终端区 `</div>` 之后）加：

```vue
      <AdbDevicePanel v-model:open="panelOpen" />
```

- [ ] **Step 3: 类型检查 + 全量构建**

Run: `pnpm build`
Expected: vue-tsc 与 vite build 均通过

- [ ] **Step 4: Commit**

```bash
git add src/components/AdbTerminalDialog.vue
git commit -m "feat(rzj): ADB 终端弹窗集成设备面板入口"
```

---

### Task 7: 端到端手动验证

**Files:** 无代码改动

- [ ] **Step 1: 启动开发环境**

Run: `pnpm tauri dev`（后台运行，等编译完成）

- [ ] **Step 2: 验证清单**（需连一台安卓真机；无设备时仅验证空状态与错误分支）

1. 打开 ADB 终端弹窗 → 右上角出现「已连接设备」按钮
2. 点击 → 右侧滑出面板，列出设备（序列号 / 状态徽标 / USB 或网络徽标）；面板打开时弹窗自带关闭叉隐藏，面板头部有关闭与刷新按钮
3. 无设备时显示"未检测到已连接设备"；刷新按钮可用
4. 已连接设备点「安装入住机」→ 切换版本选择视图，列出清单 2 个版本（名称 + 版本号）；未选择时「确定」禁用
5. 选择并确定 → 切回设备列表，按钮变进度条：下载中 xx% → 安装中 → 拉起应用… → 已完成（绿色）→ 2.5s 后恢复按钮，伴随 success toast；设备上应用被拉起
6. 重复点击安装中设备的按钮不可达（进度条状态）；对另一台设备同时装同一版本 → 第二台等待共享下载
7. 断网再点安装 → 下载失败，进度条恢复按钮，destructive toast
8. 安装进行中关闭面板再打开 → 后续进度事件仍正常更新
9. `unauthorized` / `offline` 设备无安装按钮

- [ ] **Step 3: 问题修复（如有）后最终构建确认**

Run: `cd src-tauri && cargo check && cd .. && pnpm build`
Expected: 全部通过
