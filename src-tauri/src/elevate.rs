//! 提权支持：以特权身份拉起指定命令（macOS/Windows/Linux）
//!
//! DHCP 服务需要绑定 67 特权端口并给网卡配置 192.168.134.1，必须以 root/管理员运行。
//! macOS 上 WKWebView 无法在 root 进程运行，因此不能提权重启整个 GUI，
//! 而是只提权拉起本程序的 --dhcp-relay 助手进程（特权部分与 GUI 分离）：
//! - macOS：osascript with administrator privileges（系统密码弹窗）
//! - Windows：PowerShell Start-Process -Verb RunAs（UAC 弹窗）
//! - Linux：pkexec（polkit 图形弹窗），无 pkexec 时退回 sudo

/// shell 单引号转义
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// AppleScript 字符串字面量转义（双引号包裹）
fn applescript_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// 以特权身份拉起命令（非阻塞，密码弹窗由各平台机制呈现），返回拉起的子进程句柄
pub fn spawn_elevated(
    program: &str,
    args: &[String],
) -> Result<Option<std::process::Child>, String> {
    #[cfg(target_os = "macos")]
    {
        let mut cmd = shell_quote(program);
        for a in args {
            cmd.push(' ');
            cmd.push_str(&shell_quote(a));
        }
        return std::process::Command::new("osascript")
            .arg("-e")
            .arg(format!(
                "do shell script {q} with administrator privileges",
                q = applescript_quote(&cmd)
            ))
            .spawn()
            .map(Some)
            .map_err(|e| format!("拉起提权进程失败: {e}"));
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        // ArgumentList 内嵌双引号：防止含空格的参数（如网卡名 "Ethernet 2"）被 Start-Process 拼接时拆散；
        // -Wait 使 powershell 驻留到助手退出，主程序据此可用 try_wait 及时察觉 UAC 取消/助手崩溃；
        // CREATE_NO_WINDOW 避免控制台窗口闪现
        let arg_list: Vec<String> = args.iter().map(|a| format!("'\"{a}\"'")).collect();
        return std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "Start-Process -FilePath '{}' -ArgumentList {} -Verb RunAs -WindowStyle Hidden -Wait",
                    program.replace('\'', "''"),
                    arg_list.join(",")
                ),
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map(Some)
            .map_err(|e| format!("拉起提权进程失败: {e}"));
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // env 转一层保留图形环境；pkexec 缺失时退回 sudo（终端场景）
        let keep = ["DISPLAY", "WAYLAND_DISPLAY", "XAUTHORITY"];
        let mut full: Vec<String> = vec!["env".into()];
        for k in keep {
            if let Ok(v) = std::env::var(k) {
                full.push(format!("{k}={v}"));
            }
        }
        full.push(program.to_string());
        full.extend(args.iter().cloned());
        let runner = if std::path::Path::new("/usr/bin/pkexec").exists() {
            "/usr/bin/pkexec"
        } else {
            "/usr/bin/sudo"
        };
        return std::process::Command::new(runner)
            .args(&full)
            .spawn()
            .map(Some)
            .map_err(|e| format!("拉起提权进程失败: {e}"));
    }
    #[allow(unreachable_code)]
    {
        let _ = (program, args);
        Err("当前平台不支持提权".into())
    }
}

/// 当前进程是否具备管理员/root 权限。
/// 不能用「能否绑定 67 端口」判断：新版 macOS 与 Windows 均不限制特权端口，
/// 真正需要特权的操作是配置网卡 IP。
pub fn is_elevated() -> bool {
    #[cfg(unix)]
    {
        return unsafe { libc::geteuid() == 0 };
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        // net session 仅管理员身份可成功执行
        return std::process::Command::new("net")
            .arg("session")
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
    }
}
