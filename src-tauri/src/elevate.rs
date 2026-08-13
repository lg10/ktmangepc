//! 提权支持：检测当前进程权限 + 一键提权重启（macOS/Windows/Linux）
//!
//! DHCP 服务需要绑定 67 特权端口并给网卡配置 192.168.134.1，必须以 root/管理员运行。
//! 三个系统都不支持已运行进程自身提权，只能提权新起进程：
//! - macOS：osascript with administrator privileges（系统密码弹窗）
//! - Windows：PowerShell Start-Process -Verb RunAs（UAC 弹窗）
//! - Linux：pkexec（polkit 图形弹窗），无 pkexec 时退回 sudo
//! 提权重启通过环境变量传递原始 HOME，保证应用数据目录不漂移

use std::path::{Path, PathBuf};
use tauri::AppHandle;

/// 当前进程是否以 root/管理员权限运行
#[cfg(unix)]
pub fn process_is_elevated() -> bool {
    unsafe { libc::geteuid() == 0 }
}

#[cfg(windows)]
pub fn process_is_elevated() -> bool {
    // 非管理员令牌下 net session 访问 SAM 服务必然失败，是通用的提权探测手段
    std::process::Command::new("cmd")
        .args(["/C", "net session >nul 2>&1"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[tauri::command]
pub async fn is_elevated() -> Result<bool, String> {
    Ok(process_is_elevated())
}

/// 定位 macOS .app 包路径；dev 模式（裸二进制）返回 None
fn macos_bundle_path(exe: &Path) -> Option<PathBuf> {
    let macos_dir = exe.parent()?;
    if macos_dir.file_name()?.to_str()? != "MacOS" {
        return None;
    }
    let contents = macos_dir.parent()?;
    if contents.file_name()?.to_str()? != "Contents" {
        return None;
    }
    contents.parent().map(|p| p.to_path_buf())
}

/// 一键提权重启：以特权身份拉起新实例后退出当前实例
#[tauri::command]
pub async fn restart_elevated(app: AppHandle) -> Result<(), String> {
    if process_is_elevated() {
        return Ok(());
    }
    let exe = std::env::current_exe().map_err(|e| format!("获取程序路径失败: {e}"))?;
    let home = std::env::var("HOME").unwrap_or_default();

    #[cfg(target_os = "macos")]
    {
        // open 启动的 App 环境来自 launchd 拿不到 HOME，故按 bundle 有无分别处理：
        // bundle 内用 open --env 注入 HOME；dev 裸二进制直接后台运行并显式传 HOME
        let script = if let Some(bundle) = macos_bundle_path(&exe) {
            format!(
                r#"open --env "HOME={home}" -n "{b}""#,
                b = bundle.display()
            )
        } else {
            format!(r#""{e}" >/dev/null 2>&1 &"#, e = exe.display())
        };
        std::process::Command::new("osascript")
            .arg("-e")
            .arg(format!(
                r#"do shell script "{s}" with administrator privileges"#,
                s = script.replace('\\', "\\\\").replace('"', "\\\"")
            ))
            .spawn()
            .map_err(|e| format!("拉起提权进程失败: {e}"))?;
    }
    #[cfg(target_os = "windows")]
    {
        let _ = home;
        std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "Start-Process -FilePath '{}' -Verb RunAs",
                    exe.display()
                ),
            ])
            .spawn()
            .map_err(|e| format!("拉起提权进程失败: {e}"))?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // env 转一层保留 HOME 与图形环境，pkexec 缺失时退回 sudo（终端场景）
        let keep = ["DISPLAY", "WAYLAND_DISPLAY", "XAUTHORITY"];
        let mut args: Vec<String> = vec!["env".into(), format!("HOME={home}")];
        for k in keep {
            if let Ok(v) = std::env::var(k) {
                args.push(format!("{k}={v}"));
            }
        }
        args.push(exe.display().to_string());
        let runner = if Path::new("/usr/bin/pkexec").exists() {
            "/usr/bin/pkexec"
        } else {
            "/usr/bin/sudo"
        };
        std::process::Command::new(runner)
            .args(&args)
            .spawn()
            .map_err(|e| format!("拉起提权进程失败: {e}"))?;
    }

    // 让新实例接管，退出当前（无特权）实例
    app.exit(0);
    Ok(())
}
