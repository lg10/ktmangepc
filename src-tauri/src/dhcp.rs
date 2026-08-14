//! DHCP 服务器：网线直连场景为设备自动分配 192.168.134.x 地址
//!
//! 对应原 MainWindow.axaml.cs 中的 DhcpDotNet 实现：
//! - 绑定 67 端口（需要管理员/root 权限），响应 DISCOVER/REQUEST，分配 192.168.134.100~200
//! - 智能开关：无论自动/手动模式启动都检测真实网络环境（排除本机 192.168.134.x 接口）；
//!   auto 模式直接拒绝，手动模式返回 REAL_NETWORK: 前缀错误由前端确认后 force 重试
//! - 启动时自动给所选网卡配置 192.168.134.1（需 root/管理员），停止时移除还原
//! - 事件：dhcp://status、dhcp://lease

use crate::events;
use serde::{Deserialize, Serialize};
use socket2::{Domain, Protocol, Socket, Type};
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

const DHCP_SERVER_PORT: u16 = 67;
const DHCP_CLIENT_PORT: u16 = 68;
const SUBNET: [u8; 3] = [192, 168, 134];
const LEASE_START: u8 = 100;
const LEASE_END: u8 = 200;
/// 中继模式：主程序回环接收端口 / 助手控制端口（均 >1024 无需特权）
pub const RELAY_APP_PORT: u16 = 46367;
pub const RELAY_HELPER_PORT: u16 = 46368;
/// 助手心跳间隔（助手 90 秒未收到心跳自动退出并还原网卡配置）
const RELAY_PING_SECS: u64 = 20;

/// 手动启动时检测到真实网络的错误前缀，前端据此弹风险确认框后带 force 重试
pub const REAL_NETWORK_MARK: &str = "REAL_NETWORK:";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DhcpStatusView {
    pub running: bool,
    pub interface_name: String,
    pub server_ip: String,
    pub lease_count: usize,
    pub auto_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DhcpLeaseView {
    pub mac: String,
    pub ip: String,
    pub ts: u64,
}

/// 租约有效期（与 option 51 的 24h 一致）；超过的持久记录在加载时移除
const LEASE_TTL_MS: u64 = 24 * 3600 * 1000;

struct DhcpInner {
    running: bool,
    interface_name: String,
    auto_mode: bool,
    leases: HashMap<String, DhcpLeaseView>,
    /// 是否已从磁盘加载持久化租约（进程内只加载一次）
    loaded: bool,
    cancel: Option<CancellationToken>,
    /// 启动时由程序自动添加的 192.168.134.1 网卡地址（直连模式下停止时移除还原；
    /// 中继模式由助手进程自行还原）
    nic_ip_added: Option<String>,
    /// 添加 134.1 前该网卡是否为 DHCP 自动获取（停止时据此恢复，防 Windows 切静态）
    nic_was_dhcp: bool,
    /// 中继模式：主程序回环 socket（停止时向助手发退出指令）
    relay_socket: Option<Arc<UdpSocket>>,
    /// 伪互联网服务（DNS/HTTP 模拟，直连模式随服务启停；中继模式由助手进程自带）
    fake: Option<crate::fakeinet::FakeInternet>,
}

impl Default for DhcpInner {
    fn default() -> Self {
        Self {
            running: false,
            interface_name: String::new(),
            auto_mode: false,
            leases: HashMap::new(),
            loaded: false,
            cancel: None,
            nic_ip_added: None,
            nic_was_dhcp: false,
            relay_socket: None,
            fake: None,
        }
    }
}

#[derive(Default)]
pub struct DhcpService {
    inner: Mutex<DhcpInner>,
}

/// 虚拟隧道类接口（VPN/代理 TUN 等）：不会接触物理局域网，不构成 rogue DHCP 风险，
/// 且断网后仍可能保留假地址/默认路由，需排除以免误判
fn is_virtual_iface(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    // macOS/Linux/Windows 常见虚拟接口前缀：隧道/VPN/容器网桥等不接触物理局域网
    [
        "utun", "tun", "tap", "ppp", "wg", "wintun", "ipsec", "vpn", "vethernet",
        "docker", "veth", "virbr", "br-", "lxc", "cni", "flannel", "tailscale",
    ]
    .iter()
    .any(|p| lower.starts_with(p))
}

/// 检测是否存在真实网络环境（外网/内网）：排除回环、本服务的 192.168.134.0/24、
/// 链路本地 169.254.0/16（自分配地址即无 DHCP 环境）与虚拟隧道接口
pub async fn has_real_network() -> bool {
    // 1. 接口检查：存在非回环、非排除网段、非隧道的 IPv4 接口
    if let Ok(list) = local_ip_address::list_afinet_netifas() {
        for (name, ip) in &list {
            if let std::net::IpAddr::V4(v4) = ip {
                if v4.is_loopback() || is_virtual_iface(name) {
                    continue;
                }
                let o = v4.octets();
                if (o[0] == SUBNET[0] && o[1] == SUBNET[1] && o[2] == SUBNET[2])
                    || (o[0] == 169 && o[1] == 254)
                {
                    continue;
                }
                return true;
            }
        }
    }
    // 2. 路由检查：UDP connect 到公共 DNS（不真正发包），有默认路由则视为有网；
    //    但若出口地址落在隧道/排除网段接口上（如代理 TUN 保留的默认路由）不算
    let probe = tokio::time::timeout(
        Duration::from_millis(600),
        async {
            let sock = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
            sock.connect("223.5.5.5:53").await?;
            sock.local_addr()
        },
    )
    .await;
    match probe {
        Ok(Ok(addr)) => {
            let std::net::IpAddr::V4(v4) = addr.ip() else {
                return false;
            };
            let o = v4.octets();
            if v4.is_loopback() || (o[0] == 169 && o[1] == 254) {
                return false;
            }
            // 出口 IP 属于隧道接口则不算真实网络
            if let Ok(list) = local_ip_address::list_afinet_netifas() {
                for (name, ip) in &list {
                    if *ip == addr.ip() && is_virtual_iface(name) {
                        return false;
                    }
                }
            }
            true
        }
        _ => false,
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 解析 BOOTP 请求，返回 (客户端MAC, 消息类型, xid)
fn parse_request(data: &[u8]) -> Option<([u8; 6], u8, [u8; 4])> {
    if data.len() < 240 || data[0] != 1 || data[2] != 6 {
        return None;
    }
    let mut mac = [0u8; 6];
    mac.copy_from_slice(&data[28..34]);
    let mut xid = [0u8; 4];
    xid.copy_from_slice(&data[4..8]);
    // 定位 magic cookie 后的选项区
    let opts = &data[240..];
    let mut i = 0;
    while i < opts.len() {
        let kind = opts[i];
        if kind == 255 {
            break;
        }
        if kind == 0 {
            i += 1;
            continue;
        }
        if i + 1 >= opts.len() {
            break;
        }
        let len = opts[i + 1] as usize;
        if kind == 53 && len >= 1 && i + 2 < opts.len() {
            return Some((mac, opts[i + 2], xid));
        }
        i += 2 + len;
    }
    None
}

/// 构造 DHCP OFFER/ACK 响应包
fn build_reply(xid: [u8; 4], mac: [u8; 6], offer_ip: Ipv4Addr, msg_type: u8) -> Vec<u8> {
    let mut p = vec![0u8; 236];
    p[0] = 2; // BOOTREPLY
    p[1] = 1; // Ethernet
    p[2] = 6; // hw len
    p[4..8].copy_from_slice(&xid);
    p[8..10].copy_from_slice(&0u16.to_be_bytes()); // secs
    p[10] = 0x80; // broadcast flag
    p[16..20].copy_from_slice(&offer_ip.octets()); // yiaddr
    p[20..24].copy_from_slice(&SUBNET_IP.octets()); // siaddr
    p[28..34].copy_from_slice(&mac);
    // magic cookie
    p.extend_from_slice(&[99, 130, 83, 99]);
    // option 53: message type
    p.extend_from_slice(&[53, 1, msg_type]);
    // option 1: subnet mask
    p.extend_from_slice(&[1, 4, 255, 255, 255, 0]);
    // option 3: router
    p.extend_from_slice(&[3, 4]);
    p.extend_from_slice(&SUBNET_IP.octets());
    // option 6: DNS
    p.extend_from_slice(&[6, 4]);
    p.extend_from_slice(&SUBNET_IP.octets());
    // option 51: lease time 24h
    p.extend_from_slice(&[51, 4]);
    p.extend_from_slice(&86400u32.to_be_bytes());
    // option 54: server identifier
    p.extend_from_slice(&[54, 4]);
    p.extend_from_slice(&SUBNET_IP.octets());
    // end
    p.push(255);
    p
}

/// 服务端自身地址 192.168.134.1
pub static SUBNET_IP: std::sync::LazyLock<Ipv4Addr> =
    std::sync::LazyLock::new(|| Ipv4Addr::new(SUBNET[0], SUBNET[1], SUBNET[2], 1));

/// 枚举所有网卡名（含无 IPv4 地址的，如断网后的直连网卡）
fn list_all_iface_names() -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("ifconfig")
            .arg("-l")
            .output()
            .ok()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .split_whitespace()
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default()
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::fs::read_dir("/sys/class/net")
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default()
    }
    #[cfg(windows)]
    {
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
                // 中文 Windows 的 PowerShell 输出为 GBK 编码，按 UTF-8 解读会乱码
                let (text, _, _) = encoding_rs::GBK.decode(&o.stdout);
                text.lines()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// 确定目标网卡：前端选中的名字直接信任（断网后该网卡可能无 IPv4 地址）；
/// 未传入时依次回退：唯一非回环 IPv4 接口 → 唯一物理网卡（排除隧道/回环）
pub fn resolve_interface(requested: &str) -> Result<String, String> {
    if !requested.is_empty() {
        return Ok(requested.to_string());
    }
    let list = local_ip_address::list_afinet_netifas()
        .map_err(|e| format!("枚举网卡失败: {e}"))?;
    let v4: Vec<String> = list
        .into_iter()
        .filter_map(|(n, ip)| match ip {
            std::net::IpAddr::V4(v4) if !v4.is_loopback() => Some(n),
            _ => None,
        })
        .collect();
    let uniq_v4: Vec<&String> = {
        let mut u: Vec<&String> = Vec::new();
        for n in &v4 {
            if !u.contains(&n) {
                u.push(n);
            }
        }
        u
    };
    if uniq_v4.len() == 1 {
        return Ok(uniq_v4[0].clone());
    }
    // 无任何 IPv4 接口时：从全量网卡中选唯一物理网卡（断网直连场景）
    let phys: Vec<String> = list_all_iface_names()
        .into_iter()
        .filter(|n| {
            n != "lo"
                && n != "lo0"
                && !is_virtual_iface(n)
                && !n.starts_with("awdl")
                && !n.starts_with("llw")
                && !n.starts_with("bridge")
        })
        .collect();
    if phys.len() == 1 {
        return Ok(phys.into_iter().next().unwrap());
    }
    Err("请先在顶部网卡选择中选中直连设备的那块网卡".into())
}

/// 确保目标网卡持有 192.168.134.x 地址（设备回包目标）；无则自动添加。
/// 返回是否由本次调用新增（新增的在停止时移除还原）。需要 root/管理员权限，失败返回提示文案
pub fn ensure_subnet_ip(requested: &str) -> Result<bool, String> {
    let name = resolve_interface(requested)?;
    let has = local_ip_address::list_afinet_netifas()
        .map(|l| {
            l.iter().any(|(n, ip)| {
                n == &name
                    && matches!(ip,
                        std::net::IpAddr::V4(v)
                        if v.octets()[..3] == SUBNET)
            })
        })
        .unwrap_or(false);
    if has {
        return Ok(false);
    }
    let ip = SUBNET_IP.to_string();
    match run_ip_cmd(&name, true) {
        Some(out) if out.status.success() => Ok(true),
        Some(out) => Err(format!(
            "自动为网卡 {name} 配置 {ip} 失败（需要管理员/root 权限）：{}",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
        None => Err(format!("自动为网卡 {name} 配置 {ip} 失败：无法执行系统命令")),
    }
}

/// 移除启动时自动添加的 192.168.134.1（停止服务时还原现场）；
/// was_dhcp = 添加前该网卡是否为 DHCP 自动获取：Windows 上增删辅助地址可能把
/// DHCP 网卡切成静态模式（表现为 169.254 且无法 renew），移除后校验并恢复
pub fn remove_subnet_ip(name: &str, was_dhcp: bool) {
    let _ = run_ip_cmd(name, false);
    if was_dhcp && !nic_dhcp_enabled(name) {
        restore_nic_dhcp(name);
    }
}

/// Windows：查询网卡 IPv4 是否为 DHCP 自动获取；无法判定时保守返回 false（不做恢复）
#[cfg(windows)]
pub fn nic_dhcp_enabled(name: &str) -> bool {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("netsh")
        .args(["interface", "ip", "show", "config", &format!("name={name}")])
        .creation_flags(0x0800_0000)
        .output();
    let Ok(o) = out else { return false };
    // netsh 在中文 Windows 输出 GBK 编码，必须按 GBK 解码，否则「已启用/是」匹配永远失败
    let (decoded, _, _) = encoding_rs::GBK.decode(&o.stdout);
    let text = decoded.to_ascii_lowercase();
    // 逐行找「已启用 DHCP」行：中文「是/否」、英文 "Yes/No"
    for line in text.lines() {
        if line.contains("dhcp") && (line.contains("已启用") || line.contains("enabled")) {
            return line.contains('是') || line.contains("yes");
        }
    }
    false
}

#[cfg(not(windows))]
pub fn nic_dhcp_enabled(_name: &str) -> bool {
    false
}

/// Windows：把网卡恢复为 DHCP 自动获取（IP + DNS），失败忽略
#[cfg(windows)]
fn restore_nic_dhcp(name: &str) {
    use std::os::windows::process::CommandExt;
    for args in [
        ["interface", "ip", "set", "address", name, "dhcp"],
        ["interface", "ip", "set", "dnsservers", name, "dhcp"],
    ] {
        let _ = std::process::Command::new("netsh")
            .args(args)
            .creation_flags(0x0800_0000)
            .output();
    }
}

#[cfg(not(windows))]
fn restore_nic_dhcp(_name: &str) {}

/// 平台特定的网卡副地址增删：macOS ifconfig alias、Linux ip addr、Windows netsh
fn run_ip_cmd(name: &str, add: bool) -> Option<std::process::Output> {
    let ip = SUBNET_IP.to_string();
    #[cfg(target_os = "macos")]
    let (prog, args): (&str, Vec<String>) = if add {
        (
            "ifconfig",
            vec![
                name.into(),
                "alias".into(),
                ip,
                "netmask".into(),
                "255.255.255.0".into(),
            ],
        )
    } else {
        ("ifconfig", vec![name.into(), "-alias".into(), ip])
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let (prog, args): (&str, Vec<String>) = if add {
        ("ip", vec!["addr".into(), "add".into(), format!("{ip}/24"), "dev".into(), name.into()])
    } else {
        ("ip", vec!["addr".into(), "del".into(), format!("{ip}/24"), "dev".into(), name.into()])
    };
    #[cfg(windows)]
    let (prog, args): (&str, Vec<String>) = if add {
        (
            "netsh",
            vec![
                "interface".into(),
                "ip".into(),
                "add".into(),
                "address".into(),
                name.into(),
                ip,
                "255.255.255.0".into(),
            ],
        )
    } else {
        (
            "netsh",
            vec![
                "interface".into(),
                "ip".into(),
                "delete".into(),
                "address".into(),
                name.into(),
                ip,
            ],
        )
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new(prog)
            .args(&args)
            .creation_flags(0x0800_0000)
            .output()
            .ok()
    }
    #[cfg(not(windows))]
    std::process::Command::new(prog).args(&args).output().ok()
}

impl DhcpService {
    pub async fn start(
        &self,
        app: AppHandle,
        interface_name: String,
        auto_mode: bool,
        force: bool,
    ) -> Result<(), String> {
        // 所选网卡链路已断开：仅警告不阻断——直连设备场景常需先开 DHCP 再插网线/上电设备
        if !interface_name.is_empty() && !crate::netif::adapter_is_up(&interface_name) {
            let _ = app.emit(
                events::UDP_LOG,
                format!("警告：网卡 {interface_name} 当前为断开状态，已按您的选择启用 DHCP，连接设备网线后即生效"),
            );
        }
        // 真实网络检测：auto 模式直接拒绝；手动模式给出可确认重试的标记错误（防 rogue DHCP）
        if has_real_network().await {
            if auto_mode {
                return Err("检测到真实网络环境，自动 DHCP 已跳过（仅网线直连离线场景需要）".into());
            }
            if !force {
                return Err(format!(
                    "{REAL_NETWORK_MARK}检测到真实网络环境，此时启动 DHCP 可能导致同网段其他设备被分配 192.168.134.x 地址而断网，请确认仅在网线直连离线场景使用"
                ));
            }
        }

        self.stop_inner(&app).await;
        // 恢复持久化租约：重启后同一设备仍拿回同一 IP，列表展示真实分配状态
        self.ensure_loaded(&app).await;

        // 按进程权限选模式：新版 macOS/Windows 不限制特权端口，
        // 未提权时一律走中继模式（GUI 不提权，特权助手处理绑 67 与配网卡）
        if !crate::elevate::is_elevated() {
            return self.start_relay(&app, interface_name, auto_mode).await;
        }
        match try_bind_67() {
            Ok(socket) => {
                // 为所选网卡自动配置 192.168.134.1（设备回包目标），失败不阻断但给出警告
                let was_dhcp = nic_dhcp_enabled(&interface_name);
                let nic_warning = match ensure_subnet_ip(&interface_name) {
                    Ok(added) => {
                        if added {
                            let mut inner = self.inner.lock().await;
                            inner.nic_ip_added = Some(interface_name.clone());
                            inner.nic_was_dhcp = was_dhcp;
                        }
                        String::new()
                    }
                    Err(e) => e,
                };

                let cancel = CancellationToken::new();
                {
                    let mut inner = self.inner.lock().await;
                    inner.running = true;
                    inner.interface_name = interface_name.clone();
                    inner.auto_mode = auto_mode;
                    inner.cancel = Some(cancel.clone());
                    // 直连模式（进程已特权）：启动伪互联网加速设备联网自检
                    inner.fake = Some(crate::fakeinet::FakeInternet::start({
                        let app2 = app.clone();
                        move |m| {
                            let _ = app2.emit(events::UDP_LOG, m);
                        }
                    }));
                }
                self.emit_status(&app).await;

                let svc = app.state::<crate::state::AppState>().dhcp.clone();
                tokio::spawn(serve_loop(svc, app, socket, cancel));
                if !nic_warning.is_empty() {
                    return Err(nic_warning);
                }
                Ok(())
            }
            Err(e) => Err(format!("绑定 67 端口失败: {e}")),
        }
    }

    /// 中继模式启动：主程序绑定回环转发端口，提权拉起助手进程并等待 READY
    async fn start_relay(
        &self,
        app: &AppHandle,
        interface_name: String,
        auto_mode: bool,
    ) -> Result<(), String> {
        let nic = resolve_interface(&interface_name)?;
        let relay_sock = Arc::new(
            UdpSocket::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, RELAY_APP_PORT)))
                .await
                .map_err(|e| format!("绑定中继端口失败: {e}"))?,
        );
        let exe = std::env::current_exe()
            .map_err(|e| format!("获取程序路径失败: {e}"))?
            .display()
            .to_string();
        let args = [
            "--dhcp-relay".to_string(),
            RELAY_APP_PORT.to_string(),
            RELAY_HELPER_PORT.to_string(),
            nic.clone(),
        ];
        let child = crate::elevate::spawn_elevated(&exe, &args)?;
        // 等待助手就绪（包含用户输入密码的时间）；取消密码框会尽快报错
        wait_relay_ready(&relay_sock, child).await.map_err(|_| {
            "未获得管理员授权或 DHCP 助手启动失败，请重试并输入密码".to_string()
        })?;

        let cancel = CancellationToken::new();
        {
            let mut inner = self.inner.lock().await;
            inner.running = true;
            inner.interface_name = nic;
            inner.auto_mode = auto_mode;
            inner.cancel = Some(cancel.clone());
            inner.relay_socket = Some(relay_sock.clone());
        }
        self.emit_status(app).await;

        let svc = app.state::<crate::state::AppState>().dhcp.clone();
        tokio::spawn(serve_loop_relay(svc, app.clone(), relay_sock, cancel));
        Ok(())
    }

    pub async fn stop(&self, app: &AppHandle) {
        self.stop_inner(app).await;
    }

    async fn stop_inner(&self, app: &AppHandle) {
        let mut inner = self.inner.lock().await;
        // 停止伪互联网服务（drop 即停线程）
        inner.fake.take();
        // 移除启动时自动添加的网卡地址（仅直连模式；中继模式由助手自行还原）
        if let Some(nic) = inner.nic_ip_added.take() {
            remove_subnet_ip(&nic, inner.nic_was_dhcp);
            inner.nic_was_dhcp = false;
        }
        // 中继模式：向助手发退出指令（目标端口 0），助手移除网卡地址后退出
        if let Some(sock) = inner.relay_socket.take() {
            let mut cmd = [0u8; 6];
            cmd[5] = 0;
            let helper: SocketAddr = (Ipv4Addr::LOCALHOST, RELAY_HELPER_PORT).into();
            let _ = sock.send_to(&cmd, helper).await;
            // 同步等待助手释放控制端口（最多 3s），避免立即重开 DHCP 时新助手绑定失败
            for _ in 0..15 {
                if std::net::UdpSocket::bind(SocketAddr::from((
                    Ipv4Addr::LOCALHOST,
                    RELAY_HELPER_PORT,
                )))
                .is_ok()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
        if !inner.running {
            return;
        }
        if let Some(c) = inner.cancel.take() {
            c.cancel();
        }
        inner.running = false;
        // 租约持久化保留：停止不清空，列表展示的是真实已分配状态而非本次会话
        drop(inner);
        self.emit_status(app).await;
    }

    pub async fn status(&self, app: &AppHandle) -> DhcpStatusView {
        self.ensure_loaded(app).await;
        let inner = self.inner.lock().await;
        DhcpStatusView {
            running: inner.running,
            interface_name: inner.interface_name.clone(),
            server_ip: SUBNET_IP.to_string(),
            lease_count: inner.leases.len(),
            auto_mode: inner.auto_mode,
        }
    }

    async fn emit_status(&self, app: &AppHandle) {
        let _ = app.emit(events::DHCP_STATUS, self.status(app).await);
    }

    /// 从磁盘加载持久化租约（进程内只加载一次）；
    /// 租约跨停止/重启保留，同一设备再次请求时拿回同一 IP
    async fn ensure_loaded(&self, app: &AppHandle) {
        let mut inner = self.inner.lock().await;
        if inner.loaded {
            return;
        }
        inner.loaded = true;
        if inner.leases.is_empty() {
            inner.leases = load_leases_from_disk(app);
        }
    }

    /// 查询租约：只返回当前真实在线的（ARP 存活探测），断开的设备不展示
    pub async fn leases(&self, app: &AppHandle) -> Vec<DhcpLeaseView> {
        self.ensure_loaded(app).await;
        let all: Vec<DhcpLeaseView> = self.inner.lock().await.leases.values().cloned().collect();
        if all.is_empty() {
            return Vec::new();
        }
        let table = arp_probe(&all).await;
        let mut out: Vec<DhcpLeaseView> = all
            .into_iter()
            // ARP 表中存在该 IP 且 MAC 一致才视为在线
            .filter(|l| table.get(&l.ip).is_some_and(|m| m == &l.mac))
            .collect();
        out.sort_by(|a, b| a.ip.cmp(&b.ip));
        out
    }
}

/// 尝试绑定特权端口 67（直连模式），失败返回原始 IO 错误供上层区分权限问题
fn try_bind_67() -> Result<Arc<UdpSocket>, std::io::Error> {
    let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    let _ = sock.set_reuse_address(true);
    // 注意：不能设 SO_REUSEPORT——macOS 上它会让非特权进程也绑定成功，
    // 导致误入直连模式（随后配网卡 IP 因无 root 权限失败）
    let _ = sock.set_broadcast(true);
    sock.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, DHCP_SERVER_PORT)).into())?;
    sock.set_nonblocking(true)?;
    Ok(Arc::new(
        UdpSocket::from_std(sock.into()).map_err(|e| std::io::Error::other(e.to_string()))?,
    ))
}

/// 等待助手 READY 信号；macOS 上 osascript 取消密码框会提前退出，据此尽快报错
async fn wait_relay_ready(
    sock: &UdpSocket,
    mut child: Option<std::process::Child>,
) -> Result<(), ()> {
    let mut buf = vec![0u8; 1600];
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        if Instant::now() >= deadline {
            return Err(());
        }
        // 提权拉起的进程（osascript/pkexec/sudo/powershell -Wait）在助手存活期间保持存活；
        // 提前退出 = 用户取消授权或助手启动失败，尽快报错
        if let Some(c) = child.as_mut() {
            if let Ok(Some(_)) = c.try_wait() {
                return Err(());
            }
        }
        match tokio::time::timeout(Duration::from_secs(1), sock.recv_from(&mut buf)).await {
            Ok(Ok((len, _))) if len >= 5 && &buf[..5] == b"READY" => return Ok(()),
            Ok(_) => continue,
            Err(_) => continue,
        }
    }
}

/// 本机所有网卡 MAC 的十六进制串（大写无分隔符）：
/// DHCP 服务器不得给本机自身网卡分配地址
fn local_mac_hexes() -> &'static [String] {
    static MACS: std::sync::LazyLock<Vec<String>> = std::sync::LazyLock::new(|| {
        local_ip_address::list_afinet_netifas()
            .map(|list| {
                let mut seen = std::collections::HashSet::new();
                let mut out = Vec::new();
                for (name, _) in list {
                    if !seen.insert(name.clone()) {
                        continue;
                    }
                    if let Ok(Some(m)) = mac_address::mac_address_by_name(&name) {
                        out.push(m.to_string().to_uppercase().replace(':', ""));
                    }
                }
                out
            })
            .unwrap_or_default()
    });
    &MACS
}

/// BOOTP 请求处理：地址分配与租约管理（直连/中继两种模式共用），
/// 返回 (分配 IP, 客户端 MAC, xid, 应答类型)

async fn process_bootp(
    svc: &DhcpService,
    app: &AppHandle,
    data: &[u8],
) -> Option<(Ipv4Addr, [u8; 6], [u8; 4], u8)> {
    let (mac, msg_type, xid) = parse_request(data)?;
    let mac_str = hex::encode_upper(mac);

    // 不响应本机自身 MAC 的 DISCOVER/REQUEST：
    // Windows DHCP 客户端会从自己的服务器租到地址，停止后租约残留造成网卡列表混乱
    if local_mac_hexes().iter().any(|m| m == &mac_str) {
        return None;
    }

    let mut inner = svc.inner.lock().await;
    // 分配或复用地址
    let ip = match inner.leases.get(&mac_str) {
        Some(l) => l.ip.parse().unwrap_or(*SUBNET_IP),
        None => {
            let used: Vec<u8> = inner
                .leases
                .values()
                .filter_map(|l| l.ip.split('.').nth(3).and_then(|s| s.parse().ok()))
                .collect();
            let next = (LEASE_START..=LEASE_END)
                .find(|b| !used.contains(b))
                .unwrap_or(LEASE_START);
            Ipv4Addr::new(SUBNET[0], SUBNET[1], SUBNET[2], next)
        }
    };
    // DISCOVER(1) -> OFFER(2)，REQUEST(3) -> ACK(5)
    let reply_type = match msg_type {
        1 => 2,
        3 => 5,
        _ => return None,
    };
    if msg_type == 3 {
        let lease = DhcpLeaseView {
            mac: mac_str.clone(),
            ip: ip.to_string(),
            ts: now_ms(),
        };
        inner.leases.insert(mac_str.clone(), lease.clone());
        let snapshot = inner.leases.clone();
        drop(inner);
        save_leases_to_disk(app, &snapshot);
        let _ = app.emit(events::DHCP_LEASE, lease);
        let _ = app.emit(events::DHCP_STATUS, svc.status(app).await);
    }
    Some((ip, mac, xid, reply_type))
}

async fn serve_loop(
    svc: Arc<DhcpService>,
    app: AppHandle,
    socket: Arc<UdpSocket>,
    cancel: CancellationToken,
) {
    let mut buf = vec![0u8; 1500];
    let bcast: SocketAddr = (Ipv4Addr::new(255, 255, 255, 255), DHCP_CLIENT_PORT).into();
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            res = socket.recv_from(&mut buf) => {
                let Ok((len, _src)) = res else { break };
                let Some((ip, mac, xid, reply_type)) = process_bootp(&svc, &app, &buf[..len]).await else { continue };
                let reply = build_reply(xid, mac, ip, reply_type);
                let _ = socket.send_to(&reply, bcast).await;
            }
        }
    }
}

/// 中继模式收发循环：经助手转发，包格式 = 6 字节地址头 + BOOTP；
/// 同时每 RELAY_PING_SECS 秒向助手发心跳（目标端口 1），防助手成孤儿进程
async fn serve_loop_relay(
    svc: Arc<DhcpService>,
    app: AppHandle,
    socket: Arc<UdpSocket>,
    cancel: CancellationToken,
) {
    let mut buf = vec![0u8; 1600];
    let helper: SocketAddr = (Ipv4Addr::LOCALHOST, RELAY_HELPER_PORT).into();
    let mut ping = tokio::time::interval(Duration::from_secs(RELAY_PING_SECS));
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = ping.tick() => {
                let mut hb = [0u8; 6];
                hb[5] = 1;
                let _ = socket.send_to(&hb, helper).await;
            }
            res = socket.recv_from(&mut buf) => {
                let Ok((len, _)) = res else { break };
                if len < 6 || &buf[..5] == b"READY" {
                    continue;
                }
                let Some((ip, mac, xid, reply_type)) = process_bootp(&svc, &app, &buf[6..len]).await else { continue };
                let reply = build_reply(xid, mac, ip, reply_type);
                // 目标头：广播 255.255.255.255:68，由助手从 67 端口发出
                let mut out = Vec::with_capacity(6 + reply.len());
                out.extend_from_slice(&[255, 255, 255, 255]);
                out.extend_from_slice(&DHCP_CLIENT_PORT.to_be_bytes());
                out.extend_from_slice(&reply);
                let _ = socket.send_to(&out, helper).await;
            }
        }
    }
}

/// 租约持久化文件：记录历史分配，重启后同一设备拿回同一 IP
fn leases_file(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|p| p.join("dhcp_leases.json"))
}

/// 从磁盘加载租约，丢弃超过 24h 有效期的记录
fn load_leases_from_disk(app: &AppHandle) -> HashMap<String, DhcpLeaseView> {
    let Some(file) = leases_file(app) else {
        return HashMap::new();
    };
    let Ok(content) = std::fs::read_to_string(&file) else {
        return HashMap::new();
    };
    let Ok(list) = serde_json::from_str::<Vec<DhcpLeaseView>>(&content) else {
        return HashMap::new();
    };
    let now = now_ms();
    list.into_iter()
        .filter(|l| now.saturating_sub(l.ts) < LEASE_TTL_MS)
        .map(|l| (l.mac.clone(), l))
        .collect()
}

fn save_leases_to_disk(app: &AppHandle, leases: &HashMap<String, DhcpLeaseView>) {
    let Some(file) = leases_file(app) else { return };
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let list: Vec<&DhcpLeaseView> = leases.values().collect();
    if let Ok(json) = serde_json::to_string(&list) {
        let _ = std::fs::write(&file, json);
    }
}

/// ARP 存活探测：先向每个租约 IP 发一个 UDP 触发 ARP 解析，再读系统 ARP 表；
/// 返回 IP -> MAC（大写无分隔符十六进制），不在表中的设备视为已断开
async fn arp_probe(leases: &[DhcpLeaseView]) -> HashMap<String, String> {
    if let Ok(sock) = UdpSocket::bind("0.0.0.0:0").await {
        for l in leases {
            if let Ok(ip) = l.ip.parse::<Ipv4Addr>() {
                let addr: SocketAddr = (ip, DHCP_CLIENT_PORT).into();
                let _ = sock.send_to(&[0], addr).await;
            }
        }
    }
    tokio::time::sleep(Duration::from_millis(600)).await;
    tokio::task::spawn_blocking(read_arp_table)
        .await
        .unwrap_or_default()
}

/// 读系统 ARP 表（Linux /proc/net/arp；其余平台 arp 命令）
#[cfg(target_os = "linux")]
fn read_arp_table() -> HashMap<String, String> {
    let mut out = HashMap::new();
    if let Ok(content) = std::fs::read_to_string("/proc/net/arp") {
        for line in content.lines().skip(1) {
            parse_arp_line(line, &mut out);
        }
    }
    out
}

#[cfg(not(target_os = "linux"))]
fn read_arp_table() -> HashMap<String, String> {
    let mut out = HashMap::new();
    // macOS 用 -an（含接口名），Windows 用 -a
    let arg = if cfg!(target_os = "windows") { "-a" } else { "-an" };
    let mut cmd = std::process::Command::new("arp");
    cmd.arg(arg);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW：避免每次进设置页执行 arp 时控制台窗口闪现
        cmd.creation_flags(0x0800_0000);
    }
    if let Ok(o) = cmd.output() {
        for line in String::from_utf8_lossy(&o.stdout).lines() {
            parse_arp_line(line, &mut out);
        }
    }
    out
}

/// 解析 ARP 表单行提取 IP 与 MAC；跳过全零（未完成解析）与全 F（广播）条目
fn parse_arp_line(line: &str, out: &mut HashMap<String, String>) {
    let mut ip = None;
    let mut mac = None;
    for tok in line.split_whitespace() {
        let t = tok.trim_matches(|c| c == '(' || c == ')');
        if ip.is_none() {
            if t.parse::<Ipv4Addr>().is_ok() {
                ip = Some(t.to_string());
            }
            continue;
        }
        if mac.is_none() && is_mac_token(t) {
            mac = Some(t.replace([':', '-'], "").to_uppercase());
        }
    }
    if let (Some(ip), Some(mac)) = (ip, mac) {
        if mac != "000000000000" && mac != "FFFFFFFFFFFF" {
            out.insert(ip, mac);
        }
    }
}

fn is_mac_token(t: &str) -> bool {
    let sep = t.chars().filter(|&c| c == ':' || c == '-').count();
    t.len() == 17
        && sep == 5
        && t.chars()
            .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '-')
}

#[tauri::command]
pub async fn start_dhcp(
    app: AppHandle,
    state: State<'_, crate::state::AppState>,
    interface_name: String,
    auto_mode: bool,
    force: bool,
) -> Result<(), String> {
    state
        .dhcp
        .start(app, interface_name, auto_mode, force)
        .await
}

#[tauri::command]
pub async fn stop_dhcp(
    app: AppHandle,
    state: State<'_, crate::state::AppState>,
) -> Result<(), String> {
    state.dhcp.stop(&app).await;
    Ok(())
}

#[tauri::command]
pub async fn get_dhcp_status(
    app: AppHandle,
    state: State<'_, crate::state::AppState>,
) -> Result<DhcpStatusView, String> {
    Ok(state.dhcp.status(&app).await)
}

/// 查询当前真实在线的租约列表（ARP 存活过滤，断开的设备不返回）
#[tauri::command]
pub async fn get_dhcp_leases(
    app: AppHandle,
    state: State<'_, crate::state::AppState>,
) -> Result<Vec<DhcpLeaseView>, String> {
    Ok(state.dhcp.leases(&app).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_arp_line_macos() {
        let mut out = HashMap::new();
        parse_arp_line(
            "? (192.168.134.100) at aa:bb:cc:dd:ee:ff on en0 ifscope [ethernet]",
            &mut out,
        );
        assert_eq!(out.get("192.168.134.100").map(String::as_str), Some("AABBCCDDEEFF"));
    }

    #[test]
    fn parse_arp_line_windows_dash_sep() {
        let mut out = HashMap::new();
        parse_arp_line("  192.168.134.101          11-22-33-44-55-66     dynamic", &mut out);
        assert_eq!(out.get("192.168.134.101").map(String::as_str), Some("112233445566"));
    }

    #[test]
    fn parse_arp_line_skips_incomplete_and_broadcast() {
        let mut out = HashMap::new();
        parse_arp_line("  192.168.134.102          00-00-00-00-00-00     invalid", &mut out);
        parse_arp_line("  255.255.255.255          ff-ff-ff-ff-ff-ff     static", &mut out);
        assert!(out.is_empty());
    }
}
