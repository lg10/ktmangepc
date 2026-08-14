//! 网卡枚举（对应原 MainWindowViewModel.cs 的接口列表）
//!
//! Windows/Linux 的地址枚举会保留「已断开连接」网卡的过期 IP，
//! 选中它们扫描/开 DHCP 必然失败，因此按链路状态过滤；
//! 默认选中顺序做智能排序：本服务 192.168.134.x 网卡 > 默认路由出口（能通讯）> 其余

use serde::Serialize;
use std::net::IpAddr;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetInterfaceView {
    pub name: String,
    pub ip: String,
    pub mac: String,
    pub is_loopback: bool,
}

fn mac_of(name: &str) -> String {
    mac_address::mac_address_by_name(name)
        .ok()
        .flatten()
        .map(|m: mac_address::MacAddress| m.to_string().to_uppercase())
        .unwrap_or_default()
}

/// 链路已连接的网卡名；macOS 断开后不保留 IPv4，无需过滤
#[cfg(target_os = "windows")]
fn link_up_names() -> Option<Vec<String>> {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "(Get-NetAdapter | Where-Object Status -eq Up).Name",
        ])
        .creation_flags(0x0800_0000)
        .output()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
}

#[cfg(all(unix, not(target_os = "macos")))]
fn link_up_names() -> Option<Vec<String>> {
    std::fs::read_dir("/sys/class/net").ok().map(|rd| {
        rd.filter_map(|e| e.ok())
            .filter(|e| {
                // unknown 常见于隧道/虚拟接口，视为可用；仅排除明确 down 的
                std::fs::read_to_string(e.path().join("operstate"))
                    .map(|s| {
                        let s = s.trim();
                        s == "up" || s == "unknown"
                    })
                    .unwrap_or(true)
            })
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect()
    })
}

#[cfg(target_os = "macos")]
fn link_up_names() -> Option<Vec<String>> {
    None
}

/// 默认路由出口 IP（UDP connect 不真正发包）
fn default_route_ip() -> Option<IpAddr> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect(("223.5.5.5", 53)).ok()?;
    sock.local_addr().ok().map(|a| a.ip())
}

#[tauri::command]
pub async fn list_network_interfaces() -> Result<Vec<NetInterfaceView>, String> {
    let list = local_ip_address::list_afinet_netifas().map_err(|e| e.to_string())?;
    let up = link_up_names();
    let route_ip = default_route_ip();
    let mut entries: Vec<(String, std::net::Ipv4Addr)> = Vec::new();
    for (name, ip) in list {
        // 仅保留 IPv4，避免前端展示噪声
        let IpAddr::V4(v4) = ip else { continue };
        if v4.is_loopback() {
            continue;
        }
        // Windows/Linux 过滤断开连接的网卡（其过期 IP 选中后无法通讯）
        if let Some(up) = &up {
            if !up.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
                continue;
            }
        }
        entries.push((name, v4));
    }
    // 智能排序：192.168.134.x（本服务/直连网段）> 默认路由出口（当前能通讯）> 其余；stable 保原序
    let route_name = entries
        .iter()
        .find(|(_, v)| Some(IpAddr::V4(*v)) == route_ip)
        .map(|(n, _)| n.clone());
    entries.sort_by_key(|(name, v)| {
        let o = v.octets();
        let score = if o[0] == 192 && o[1] == 168 && o[2] == 134 {
            2
        } else if route_name.as_deref() == Some(name.as_str()) {
            1
        } else {
            0
        };
        std::cmp::Reverse(score)
    });
    Ok(entries
        .into_iter()
        .map(|(name, v4)| NetInterfaceView {
            mac: mac_of(&name),
            ip: v4.to_string(),
            is_loopback: false,
            name,
        })
        .collect())
}
