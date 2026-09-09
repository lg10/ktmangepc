//! 通用命令：启动环境检查（Splash 实时日志）、检查更新、打开外链

use crate::state::AppState;
use serde::Serialize;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, State};

/// 不等待网络恢复直接退出（残留由下次启动的检测/恢复功能或助手看门狗处理）
#[tauri::command]
pub fn quit_now(app: AppHandle) {
    app.exit(0);
}

/// 更新清单地址：发版后上传 JSON（{ version, notes, url }）即可生效
const UPDATE_MANIFEST_URL: &str = "https://mange.kingint.com/app/kt-mange-pc/latest.json";

/// 检查结果：当前版本 / 最新版本 / 是否有更新 / 更新说明 / 下载地址
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub has_update: bool,
    pub notes: String,
    pub url: String,
}

/// 版本号逐段数值比较（3.0.0 vs 3.10.2 等任意段数）
fn cmp_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |v: &str| -> Vec<u64> {
        v.trim()
            .trim_start_matches('v')
            .split('.')
            .map(|s| s.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let (mut va, mut vb) = (parse(a), parse(b));
    let len = va.len().max(vb.len());
    va.resize(len, 0);
    vb.resize(len, 0);
    va.cmp(&vb)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvReport {
    pub platform: String,
    pub app_version: String,
    pub elevated: bool,
    pub db_ready: bool,
    pub internet_reachable: bool,
    pub login_valid: bool,
}

/// Splash 屏增量更新：task 当前阶段 / log 真实日志行 / progress 0-100
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SplashUpdate {
    pub task: Option<String>,
    pub log: Option<String>,
    pub progress: Option<u8>,
}

fn splash(app: &AppHandle, task: Option<&str>, log: Option<String>, progress: Option<u8>) {
    let _ = app.emit(
        crate::events::SPLASH_LOG,
        SplashUpdate {
            task: task.map(|s| s.to_string()),
            log,
            progress,
        },
    );
}

/// 是否以特权身份运行（DHCP 67 端口需要）
fn is_elevated() -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(windows)]
    {
        // 简化判定：Windows 下以管理员运行时 67 端口绑定会成功，
        // 这里通过 ProgramFiles 可写性无法准确判断，默认 false 由 Splash 提示
        false
    }
    #[cfg(not(any(unix, windows)))]
    {
        false
    }
}

/// 云端可达性：UDP connect 公共 DNS（不真正发包），有默认路由即视为可达
async fn internet_reachable() -> bool {
    let probe = tokio::time::timeout(
        Duration::from_millis(1500),
        async {
            let sock = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
            sock.connect("223.5.5.5:53").await
        },
    )
    .await;
    matches!(probe, Ok(Ok(())))
}

#[tauri::command]
pub async fn check_environment(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<EnvReport, String> {
    let platform = std::env::consts::OS.to_string();
    let app_version = app
        .config()
        .version
        .clone()
        .unwrap_or_else(|| "0.0.0".into());
    splash(
        &app,
        Some("检查系统权限"),
        Some(format!("运行环境：{platform} · v{app_version}")),
        Some(45),
    );

    let elevated = is_elevated();
    splash(
        &app,
        None,
        Some(if elevated {
            "系统权限：特权（root/管理员），DHCP 可用".to_string()
        } else {
            "系统权限：普通用户，DHCP 端口受限".to_string()
        }),
        Some(55),
    );

    let t = Instant::now();
    let db_ready = state.db.init(&app);
    splash(
        &app,
        Some("初始化本地文件库"),
        Some(format!(
            "本地文件库：{}（{}ms）",
            if db_ready { "SQLite 就绪" } else { "初始化失败" },
            t.elapsed().as_millis()
        )),
        Some(70),
    );

    let t = Instant::now();
    let internet = internet_reachable().await;
    splash(
        &app,
        Some("检测网络环境"),
        Some(format!(
            "外网探针 223.5.5.5:53：{}（{}ms）",
            if internet { "可达" } else { "不可达，局域网模式" },
            t.elapsed().as_millis()
        )),
        Some(82),
    );

    let login_valid = state.auth.is_logged_in();
    splash(
        &app,
        Some("校验登录状态"),
        Some(if login_valid {
            "登录态：本地凭证有效，恢复会话".to_string()
        } else {
            "登录态：无有效凭证".to_string()
        }),
        Some(92),
    );

    Ok(EnvReport {
        platform,
        app_version,
        elevated,
        db_ready,
        internet_reachable: internet,
        login_valid,
    })
}

/// 检查更新：拉取云端清单 latest.json 与当前版本比较
#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<UpdateInfo, String> {
    let current = app
        .config()
        .version
        .clone()
        .unwrap_or_else(|| "0.0.0".into());
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|e| format!("HTTP 客户端初始化失败: {e}"))?;
    let resp = client
        .get(UPDATE_MANIFEST_URL)
        .send()
        .await
        .map_err(|e| format!("连接更新服务失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("更新服务响应异常: HTTP {}", resp.status()));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|_| "更新信息解析失败".to_string())?;
    let latest = body
        .get("version")
        .and_then(|v| v.as_str())
        .ok_or("更新信息缺少版本号")?
        .to_string();
    let notes = body
        .get("notes")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let url = body
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    Ok(UpdateInfo {
        has_update: cmp_versions(&latest, &current) == std::cmp::Ordering::Greater,
        current,
        latest,
        notes,
        url,
    })
}

/// 用系统默认浏览器打开外链（更新下载页等）
#[tauri::command]
pub async fn open_url(url: String) -> Result<(), String> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("不支持的链接".into());
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW：cmd 是控制台程序，裸 spawn 会闪一下黑色控制台窗口
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &url])
            .creation_flags(0x0800_0000)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
