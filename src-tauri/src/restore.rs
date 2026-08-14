//! 网卡残留检测与恢复：应用被 kill、崩溃、强制关机后，
//! 系统可能残留 ICS/互联网共享开关与 192.168.134.1 副地址（且网卡可能被切成静态模式）。
//! - 检测：只读探测（无需特权）——标记文件 + 系统共享状态 + 134.1 残留地址扫描
//! - 恢复：提权拉起 `--nic-restore` 助手逐项清理（幂等，可重复执行）

use serde::Serialize;
use tauri::{AppHandle, State};

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ResidueReport {
    /// 上次运行留下了未清理的状态标记（说明非正常退出）
    pub marker_found: bool,
    /// 系统互联网共享仍处于开启状态（Windows ICS / macOS 互联网共享）
    pub sharing_enabled: bool,
    /// 仍持有 192.168.134.1 副地址的网卡名列表
    pub residual_nics: Vec<String>,
}

impl ResidueReport {
    pub fn has_residue(&self) -> bool {
        self.marker_found || self.sharing_enabled || !self.residual_nics.is_empty()
    }
}

/// 扫描本机网卡中仍持有 134.1 副地址的网卡名
fn scan_residual_nics() -> Vec<String> {
    let target = &*crate::dhcp::SUBNET_IP;
    local_ip_address::list_afinet_netifas()
        .map(|list| {
            let mut seen = std::collections::HashSet::new();
            let mut out = Vec::new();
            for (name, ip) in list {
                if let std::net::IpAddr::V4(v4) = ip {
                    if v4 == *target && seen.insert(name.clone()) {
                        out.push(name);
                    }
                }
            }
            out
        })
        .unwrap_or_default()
}

/// Windows：只读查询 ICS 是否处于开启状态（无需管理员）
#[cfg(windows)]
fn ics_enabled() -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let script = r#"
$ErrorActionPreference = 'SilentlyContinue'
regsvr32.exe /s hnetcfg.dll
$ns = New-Object -ComObject HNetCfg.HNetShare
foreach ($c in $ns.EnumEveryConnection) {
    if ($ns.INetSharingConfigurationForINetConnection.Invoke($c).SharingEnabled) { 'SHARED'; exit 0 }
}
'NONE'
"#;
    let mut bytes: Vec<u8> = Vec::with_capacity(script.len() * 2);
    for u in script.encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    std::process::Command::new("powershell")
        .args(["-NoProfile", "-EncodedCommand", &encoded])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("SHARED"))
        .unwrap_or(false)
}

/// macOS：读 com.apple.nat 的 Enabled 位判断互联网共享是否开启
#[cfg(target_os = "macos")]
fn ics_enabled() -> bool {
    std::process::Command::new("/usr/libexec/PlistBuddy")
        .args([
            "-c",
            "Print :NAT:Enabled",
            "/Library/Preferences/SystemConfiguration/com.apple.nat",
        ])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "1")
        .unwrap_or(false)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn ics_enabled() -> bool {
    false
}

fn detect(app: &AppHandle) -> ResidueReport {
    let marker = crate::nicstate::load(app);
    ResidueReport {
        marker_found: !marker.is_empty(),
        sharing_enabled: ics_enabled(),
        residual_nics: scan_residual_nics(),
    }
}

#[tauri::command]
pub async fn check_nic_residue(app: AppHandle) -> Result<ResidueReport, String> {
    tokio::task::spawn_blocking(move || detect(&app))
        .await
        .map_err(|e| e.to_string())
}

/// 一键恢复：提权助手停止系统共享、移除 134.1 残留、还原网卡 DHCP 模式（幂等）
#[tauri::command]
pub async fn restore_network(
    app: AppHandle,
    state: State<'_, crate::state::AppState>,
) -> Result<String, String> {
    if state.dhcp.is_running().await || state.inetshare.is_running().await {
        return Err("内置 DHCP 或网络中继正在运行，请先停止后再恢复".into());
    }
    let report = tokio::task::spawn_blocking({
        let app = app.clone();
        move || detect(&app)
    })
    .await
    .map_err(|e| e.to_string())?;
    if !report.has_residue() {
        return Ok("未检测到残留，网络配置正常".into());
    }
    let marker = crate::nicstate::load(&app);
    let exe = std::env::current_exe()
        .map_err(|e| format!("获取程序路径失败: {e}"))?
        .display()
        .to_string();
    let args = [
        "--nic-restore".to_string(),
        report.residual_nics.join(","),
        marker.dhcp_iface.clone(),
        if marker.dhcp_was_dhcp { "1" } else { "0" }.to_string(),
    ];
    let mut child = crate::elevate::spawn_elevated(&exe, &args)?
        .ok_or_else(|| "拉起提权进程失败".to_string())?;

    // 等待恢复完成（含用户输入密码的时间；助手为一次性任务，完成即退出）
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    let finished = loop {
        if std::time::Instant::now() >= deadline {
            break false;
        }
        match child.try_wait() {
            Ok(Some(code)) => break code.success(),
            Ok(None) => tokio::time::sleep(std::time::Duration::from_millis(200)).await,
            Err(_) => break false,
        }
    };
    if !finished {
        let _ = child.kill();
        return Err("恢复超时或被取消授权，请重试".into());
    }

    // 恢复成功：清除标记文件
    crate::nicstate::update(&app, |s| *s = Default::default());
    let _ = tauri::Emitter::emit(
        &app,
        crate::events::UDP_LOG,
        "网络配置已恢复原状（系统共享已停止，残留地址已移除）".to_string(),
    );
    Ok("网络配置已恢复原状".into())
}

/// 特权恢复助手入口（--nic-restore，管理员/root 运行，一次性任务）
///
/// 1. 停止系统互联网共享（ICS / InternetSharing，幂等）
/// 2. 移除残留的 192.168.134.1 副地址
/// 3. 把标记记录的网卡还原为 DHCP 自动获取（防止 netsh 操作遗留静态模式）
pub fn run_restore(residual_csv: &str, dhcp_iface: &str, was_dhcp: bool) -> i32 {
    #[cfg(windows)]
    if let Err(e) = crate::inetshare_helper::win_powershell("disable", "", "") {
        eprintln!("[nic-restore] 停止 ICS 失败: {e}");
    }
    #[cfg(target_os = "macos")]
    crate::inetshare_helper::mac_stop();

    let mut nics: Vec<String> = residual_csv
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    // 标记网卡即使无残留地址也可能被遗留在静态模式，一并处理
    if !dhcp_iface.is_empty() && !nics.iter().any(|n| n == dhcp_iface) {
        nics.push(dhcp_iface.to_string());
    }
    for nic in &nics {
        crate::dhcp::remove_subnet_ip(nic, was_dhcp);
        eprintln!("[nic-restore] 已清理网卡: {nic}");
    }
    0
}
