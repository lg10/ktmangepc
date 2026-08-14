//! 网卡枚举（对应原 MainWindowViewModel.cs 的接口列表）
//!
//! Windows 地址枚举只覆盖有 IPv4 的网卡：设备直连的网口在拿到地址前（APIPA 前/无 DHCP）
//! 会从列表消失，导致用户只能选到 Wi-Fi，把 134.1 加错网卡、扫描广播走错广播域。
//! 因此 Windows 按适配器状态全量枚举：Up 但无 IPv4 的显示「未配置 IPv4」，
//! Disconnected 的置底灰显（前端禁选）；
//! 默认选中顺序：192.168.134.x > 默认路由出口 > 其余 Up 有 IP > Up 无 IP > Disconnected

use serde::Serialize;
use std::net::IpAddr;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetInterfaceView {
    pub name: String,
    pub ip: String,
    pub mac: String,
    pub is_loopback: bool,
    /// 是否持有 IPv4（false 时 ip 为空，开 DHCP 会自动配置 134.1）
    pub has_ipv4: bool,
    /// 链路是否已连接（false = 已断开，前端灰显禁选）
    pub up: bool,
}

fn mac_of(name: &str) -> String {
    mac_address::mac_address_by_name(name)
        .ok()
        .flatten()
        .map(|m: mac_address::MacAddress| m.to_string().to_uppercase())
        .unwrap_or_default()
}

/// Windows 全部物理适配器 (名称, 是否 Up)；macOS/Linux 返回 None 走地址枚举
#[cfg(target_os = "windows")]
fn adapter_states() -> Option<Vec<(String, bool)>> {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Get-NetAdapter | ForEach-Object { \"$($_.Name)|$($_.Status -eq 'Up')\" }",
        ])
        .creation_flags(0x0800_0000)
        .output()
        .ok()
        .map(|o| {
            // 中文 Windows 的 PowerShell 输出为 GBK 编码，按 UTF-8 解读会使「以太网」等网卡名乱码
            let (text, _, _) = encoding_rs::GBK.decode(&o.stdout);
            text.lines()
                .filter_map(|l| {
                    let (name, up) = l.trim().split_once('|')?;
                    if name.is_empty() {
                        return None;
                    }
                    Some((name.to_string(), up == "True"))
                })
                .collect()
        })
}

/// 默认路由出口 IP（UDP connect 不真正发包）
fn default_route_ip() -> Option<IpAddr> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect(("223.5.5.5", 53)).ok()?;
    sock.local_addr().ok().map(|a| a.ip())
}

/// 指定网卡链路是否已连接（非 Windows 无法可靠判定，恒 true）
pub fn adapter_is_up(name: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        adapter_states()
            .map(|v| {
                v.iter()
                    .any(|(n, up)| *up && n.eq_ignore_ascii_case(name))
            })
            .unwrap_or(true)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = name;
        true
    }
}

#[tauri::command]
pub async fn list_network_interfaces() -> Result<Vec<NetInterfaceView>, String> {
    let list = local_ip_address::list_afinet_netifas().map_err(|e| e.to_string())?;
    let route_ip = default_route_ip();
    let mut entries: Vec<NetInterfaceView> = Vec::new();

    #[cfg(target_os = "windows")]
    let adapters = adapter_states();

    #[cfg(target_os = "windows")]
    if let Some(adapters) = adapters {
        for (name, up) in adapters {
            let ips: Vec<std::net::Ipv4Addr> = list
                .iter()
                .filter_map(|(n, ip)| {
                    if !n.eq_ignore_ascii_case(&name) {
                        return None;
                    }
                    match ip {
                        IpAddr::V4(v4) if !v4.is_loopback() => Some(*v4),
                        _ => None,
                    }
                })
                .collect();
            if ips.is_empty() {
                // Up 但无 IPv4（如直连设备网口未拿到地址）与 Disconnected 都进列表，
                // 由前端区分样式（未配置 IPv4 / 已断开灰显）
                let mac = mac_of(&name);
                entries.push(NetInterfaceView {
                    name,
                    ip: String::new(),
                    mac,
                    is_loopback: false,
                    has_ipv4: false,
                    up,
                });
            } else {
                for v4 in ips {
                    entries.push(NetInterfaceView {
                        name: name.clone(),
                        ip: v4.to_string(),
                        mac: mac_of(&name),
                        is_loopback: false,
                        has_ipv4: true,
                        up,
                    });
                }
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        #[cfg(all(unix, not(target_os = "macos")))]
        let up_names: Option<Vec<String>> =
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
            });
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        let up_names: Option<Vec<String>> = None;

        for (name, ip) in list {
            let IpAddr::V4(v4) = ip else { continue };
            if v4.is_loopback() {
                continue;
            }
            // Linux 过滤断开连接的网卡（其过期 IP 选中后无法通讯）
            if let Some(up) = &up_names {
                if !up.iter().any(|n| n == &name) {
                    continue;
                }
            }
            let mac = mac_of(&name);
            entries.push(NetInterfaceView {
                name,
                ip: v4.to_string(),
                mac,
                is_loopback: false,
                has_ipv4: true,
                up: true,
            });
        }
    }

    // 智能排序：134 网段 > 默认路由出口 > 其余 Up 有 IP > Up 无 IP > Disconnected
    let route_name = entries
        .iter()
        .find(|e| e.has_ipv4 && Some(e.ip.as_str()) == route_ip.map(|r| r.to_string()).as_deref())
        .map(|e| e.name.clone());
    entries.sort_by_key(|e| {
        let score = if e.has_ipv4 && e.ip.starts_with("192.168.134.") {
            4
        } else if e.has_ipv4 && route_name.as_deref() == Some(e.name.as_str()) {
            3
        } else if e.has_ipv4 && e.up {
            2
        } else if e.up {
            1
        } else {
            0
        };
        std::cmp::Reverse(score)
    });
    Ok(entries)
}

/// 全局扫描可用地址：所有非环回、非 APIPA 的 IPv4，按名称过滤隧道/虚拟网卡，
/// 避免向 VPN 网段白发广播。
/// 过滤表按平台区分：Linux 上 br0 常是物理网桥不能排除，但需排除 Docker 的 br-*
/// 与 libvirt 的 virbr*；macOS 的虚拟桥接口叫 bridge100 等，直接按 bridge 前缀排除
pub fn usable_scan_addrs() -> Vec<(String, std::net::Ipv4Addr)> {
    #[cfg(target_os = "linux")]
    const TUNNEL_PREFIXES: [&str; 10] = [
        "tun", "tap", "ppp", "wg", "vpn", "veth", "br-", "virbr", "docker", "lo",
    ];
    #[cfg(not(target_os = "linux"))]
    const TUNNEL_PREFIXES: [&str; 8] = [
        "tun", "tap", "ppp", "wg", "utun", "vpn", "veth", "bridge",
    ];
    let Ok(list) = local_ip_address::list_afinet_netifas() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (name, ip) in list {
        let IpAddr::V4(v4) = ip else { continue };
        if v4.is_loopback() {
            continue;
        }
        let o = v4.octets();
        if o[0] == 169 && o[1] == 254 {
            continue;
        }
        let lower = name.to_ascii_lowercase();
        if TUNNEL_PREFIXES.iter().any(|p| lower.starts_with(p)) {
            continue;
        }
        out.push((name, v4));
    }
    out
}

/// 按网卡名解析其 IPv4（扫描/DHCP 启动用）：
/// 名字为空时智能兜底（唯一 134 段 > 唯一 Up 有 IP），无 IPv4 时报错引导先开 DHCP
pub fn resolve_nic_ipv4(name: &str) -> Result<std::net::Ipv4Addr, String> {
    let list = local_ip_address::list_afinet_netifas().map_err(|e| e.to_string())?;
    let all: Vec<(String, std::net::Ipv4Addr)> = list
        .into_iter()
        .filter_map(|(n, ip)| match ip {
            IpAddr::V4(v4) if !v4.is_loopback() => Some((n, v4)),
            _ => None,
        })
        .collect();

    if !name.is_empty() {
        // 同一网卡可持有多个 IPv4（如原有主地址 + DHCP 添加的 134.1 副地址）：
        // 优先 192.168.134 段（设备通讯段，DHCP 专为直连设备配置），其次任意非 APIPA，
        // 避免系统枚举顺序不确定导致绑定错网段、扫描广播发不到设备
        let mut aipa = None;
        let mut other = None;
        for (n, v4) in &all {
            if !n.eq_ignore_ascii_case(name) {
                continue;
            }
            let o = v4.octets();
            if o[0] == 169 && o[1] == 254 {
                aipa = Some(*v4);
            } else if o[..3] == [192, 168, 134] {
                return Ok(*v4);
            } else if other.is_none() {
                other = Some(*v4);
            }
        }
        if let Some(v) = other {
            return Ok(v);
        }
        if aipa.is_some() {
            return Err(format!(
                "网卡 {name} 当前只有 169.254 自分配地址（网段内无 DHCP 服务器）。请先对该网卡开启 DHCP 配置 192.168.134.1 后再启动扫描"
            ));
        }
        return Err(format!(
            "网卡 {name} 未配置 IPv4，请先对该网卡开启 DHCP（自动配置 192.168.134.1）后再启动扫描"
        ));
    }
    // 未指定：优先唯一 134 段网卡，其次唯一有 IPv4 的网卡
    let subnet: Vec<std::net::Ipv4Addr> = all
        .iter()
        .filter(|(_, v)| v.octets()[..3] == [192, 168, 134])
        .map(|(_, v)| *v)
        .collect();
    if subnet.len() == 1 {
        return Ok(subnet[0]);
    }
    if all.len() == 1 {
        return Ok(all[0].1);
    }
    Err("请先在顶部网卡选择中选中直连设备的那块网卡".to_string())
}
