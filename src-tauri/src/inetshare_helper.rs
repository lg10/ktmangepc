//! 网络中继特权助手（管理员/root 权限运行，由主程序以提权方式拉起）
//!
//! 职责：调用系统原生互联网共享（Windows ICS / macOS 互联网共享），
//! 把源网卡（如 Wi-Fi）的网络中继给目标网口（含 USB 转接网口），
//! 使网线直连设备获得真实互联网；系统自带 DHCP 为设备分配地址
//! （Windows 192.168.137.x / macOS 192.168.2.x）。
//!
//! 用法：`kt-mange-pc --inet-share <app_port> <helper_port> <src_if> <dst_if>`
//! - 启动即开启共享，READY 后监听 127.0.0.1:helper_port 控制端口
//! - 目标端口 0 = 退出指令（停止共享后退出），1 = 心跳
//! - 主程序心跳丢失超过 90 秒（含崩溃/被杀）自动停止共享并退出——
//!   保证退出软件必恢复原样，无需退出时再次弹权限框

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const IDLE_LIMIT_SECS: u64 = 90;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Windows：通过 HNetCfg COM 控制 ICS（助手已管理员）。
/// 网卡名内嵌脚本后以 -EncodedCommand（UTF-16LE base64）传递，
/// 彻底规避中文/空格/引号的转义问题
#[cfg(windows)]
pub(crate) fn win_powershell(action: &str, src: &str, dst: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // 单引号字面量转义（名称中含单引号的概率极低，双写已足够）
    let q = |s: &str| s.replace('\'', "''");
    let script = match action {
        "enable" => {
            format!(
                r#"
$ErrorActionPreference = 'Stop'
regsvr32.exe /s hnetcfg.dll
$ns = New-Object -ComObject HNetCfg.HNetShare
$srcName = '{src}'
$dstName = '{dst}'
foreach ($c in $ns.EnumEveryConnection) {{
    $cfg = $ns.INetSharingConfigurationForINetConnection.Invoke($c)
    if ($cfg.SharingEnabled) {{ throw '本机已启用 Internet 连接共享，请先手动关闭后重试' }}
}}
$srcConn = $ns.EnumEveryConnection | Where-Object {{ $ns.NetConnectionProps.Invoke($_).Name -eq $srcName }}
$dstConn = $ns.EnumEveryConnection | Where-Object {{ $ns.NetConnectionProps.Invoke($_).Name -eq $dstName }}
if (-not $srcConn) {{ throw "未找到源网卡 $srcName" }}
if (-not $dstConn) {{ throw "未找到目标网口 $dstName" }}
$ns.INetSharingConfigurationForINetConnection.Invoke($srcConn).EnableSharing(0)
$ns.INetSharingConfigurationForINetConnection.Invoke($dstConn).EnableSharing(1)
"#,
                src = q(src),
                dst = q(dst)
            )
        }
        "disable" => {
            r#"
$ErrorActionPreference = 'SilentlyContinue'
regsvr32.exe /s hnetcfg.dll
$ns = New-Object -ComObject HNetCfg.HNetShare
foreach ($c in $ns.EnumEveryConnection) {
    $cfg = $ns.INetSharingConfigurationForINetConnection.Invoke($c)
    if ($cfg.SharingEnabled) { $cfg.DisableSharing() }
}
"#
            .to_string()
        }
        _ => return Err("未知操作".into()),
    };
    // UTF-16LE base64 编码
    let mut bytes: Vec<u8> = Vec::with_capacity(script.len() * 2);
    for u in script.encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-EncodedCommand", &encoded])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("拉起 PowerShell 失败: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    // 中文 Windows 的 PowerShell 错误输出为 GBK 编码，直接按 UTF-8 解读会乱码
    let (decoded, _, _) = encoding_rs::GBK.decode(&out.stderr);
    let msg = decoded.trim().to_string();
    Err(if msg.is_empty() {
        "共享操作失败（未知原因）".into()
    } else {
        msg
    })
}

/// Windows：入站防火墙规则——ICS 会把目标网口网络标记为「未识别网络」（公用配置文件），
/// 默认入站拦截可能丢弃设备回包；助手以管理员运行，启动加规则、退出移除（按名称幂等）
#[cfg(windows)]
fn firewall_rule(add: bool) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let args: Vec<String> = if add {
        [
            "advfirewall", "firewall", "add", "rule", "name", "KT Device Scan InetShare",
            "dir", "in", "action", "allow", "protocol", "UDP", "localport", "4667,4668",
            "program", exe.as_str(), "enable", "yes",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    } else {
        [
            "advfirewall", "firewall", "delete", "rule", "name", "KT Device Scan InetShare",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    };
    let _ = std::process::Command::new("netsh")
        .args(&args)
        .creation_flags(CREATE_NO_WINDOW)
        .output();
}

const MACOS_NAT_PLIST: &str = "/Library/Preferences/SystemConfiguration/com.apple.nat";

/// macOS：写 com.apple.nat 配置并引导 InternetSharing 守护进程（助手已 root）
#[cfg(target_os = "macos")]
fn mac_apply(src: &str, dst: &str) -> Result<(), String> {
    let run = |args: &[&str]| -> Result<String, String> {
        let out = std::process::Command::new(args[0])
            .args(&args[1..])
            .output()
            .map_err(|e| format!("执行 {} 失败: {e}", args[0]))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    };
    // PlistBuddy 直写（该域由 configd 直接读取，不经 cfprefsd 缓存）
    let pb = "/usr/libexec/PlistBuddy";
    let _ = run(&[pb, "-c", "Delete :NAT", MACOS_NAT_PLIST]);
    run(&[pb, "-c", "Add :NAT dict", MACOS_NAT_PLIST]).map_err(|e| format!("写共享配置失败: {e}"))?;
    run(&[pb, "-c", "Add :NAT:Enabled integer 1", MACOS_NAT_PLIST])
        .map_err(|e| format!("写共享配置失败: {e}"))?;
    run(&[pb, "-c", &format!("Add :NAT:PrimaryInterface string {src}"), MACOS_NAT_PLIST])
        .map_err(|e| format!("写共享配置失败: {e}"))?;
    run(&[pb, "-c", "Add :NAT:PrimaryUserDefined integer 1", MACOS_NAT_PLIST])
        .map_err(|e| format!("写共享配置失败: {e}"))?;
    run(&[pb, "-c", "Add :NAT:SecondaryInterfaces array", MACOS_NAT_PLIST])
        .map_err(|e| format!("写共享配置失败: {e}"))?;
    run(&[pb, "-c", &format!("Add :NAT:SecondaryInterfaces:0 string {dst}"), MACOS_NAT_PLIST])
        .map_err(|e| format!("写共享配置失败: {e}"))?;
    // 先 bootout 再 bootstrap，保证按最新配置启动（已存在则忽略 bootout 失败）
    let _ = run(&["launchctl", "bootout", "system/com.apple.InternetSharing"]);
    run(&[
        "launchctl",
        "bootstrap",
        "system",
        "/System/Library/LaunchDaemons/com.apple.InternetSharing.plist",
    ])
    .map_err(|e| format!("启动互联网共享失败: {e}"))?;
    // 等待守护进程真正拉起（bootpd/pf 规则就绪）
    std::thread::sleep(Duration::from_secs(2));
    let _ = run(&["launchctl", "print", "system/com.apple.InternetSharing"])
        .map_err(|_| "互联网共享进程未能启动，请检查系统设置中共享是否被策略禁用".to_string())?;
    Ok(())
}

/// macOS：停止互联网共享并把 Enabled 置 0（不删除配置，用户 GUI 历史配置仅置位）
#[cfg(target_os = "macos")]
pub(crate) fn mac_stop() {
    let _ = std::process::Command::new("launchctl")
        .args(["bootout", "system/com.apple.InternetSharing"])
        .output();
    let _ = std::process::Command::new("/usr/libexec/PlistBuddy")
        .args(["-c", "Set :NAT:Enabled 0", MACOS_NAT_PLIST])
        .output();
}

/// 助手中继主循环；返回进程退出码
pub fn run(app_port: u16, helper_port: u16, src: &str, dst: &str) -> i32 {
    let hs = match UdpSocket::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, helper_port))) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[inet-share] 绑定控制端口失败: {e}");
            return 1;
        }
    };
    let app_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, app_port));

    // 1. 开启系统共享
    #[cfg(windows)]
    let enabled = {
        win_powershell("enable", src, dst).and_then(|_| {
            firewall_rule(true);
            Ok(())
        })
    };
    #[cfg(target_os = "macos")]
    let enabled = mac_apply(src, dst);
    #[cfg(all(unix, not(target_os = "macos")))]
    let enabled: Result<(), String> = Err("Linux 暂不支持网络中继".into());

    if let Err(e) = enabled {
        eprintln!("[inet-share] 开启共享失败: {e}");
        // 以 ERR: 前缀把原因回传主程序，便于界面直接展示
        let mut msg = format!("ERR:{e}").into_bytes();
        msg.truncate(1500);
        let _ = hs.send_to(&msg, app_addr);
        return 1;
    }
    eprintln!("[inet-share] 共享已开启: {src} -> {dst}");
    let _ = hs.send_to(b"READY", app_addr);
    
    // 2. 控制循环：退出指令 + 心跳看门狗（主程序退出/崩溃后自动还原）
    let last_seen = AtomicU64::new(now_secs());
    let mut buf = vec![0u8; 512];
    loop {
        if now_secs().saturating_sub(last_seen.load(Ordering::Relaxed)) > IDLE_LIMIT_SECS {
            eprintln!("[inet-share] 心跳超时，自动停止共享");
            break;
        }
        hs.set_read_timeout(Some(Duration::from_secs(3))).ok();
        let Ok((len, _)) = hs.recv_from(&mut buf) else {
            continue;
        };
        last_seen.store(now_secs(), Ordering::Relaxed);
        if len < 6 {
            continue;
        }
        let port = u16::from_be_bytes([buf[4], buf[5]]);
        if port == 0 {
            break; // 退出指令
        }
    }

    // 3. 停止共享，恢复原样
    #[cfg(windows)]
    {
        firewall_rule(false);
        if let Err(e) = win_powershell("disable", src, dst) {
            eprintln!("[inet-share] 停止共享失败: {e}");
        }
    }
    #[cfg(target_os = "macos")]
    mac_stop();
    eprintln!("[inet-share] 共享已停止");
    0
}
