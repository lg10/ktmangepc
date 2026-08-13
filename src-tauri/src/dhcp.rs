//! DHCP 服务器：网线直连场景为设备自动分配 192.168.134.x 地址
//!
//! 对应原 MainWindow.axaml.cs 中的 DhcpDotNet 实现：
//! - 绑定 67 端口（需要管理员/root 权限），响应 DISCOVER/REQUEST，分配 192.168.134.100~200
//! - 智能开关：无论自动/手动模式启动都检测真实网络环境（排除本机 192.168.134.x 接口）；
//!   auto 模式直接拒绝，手动模式返回 REAL_NETWORK: 前缀错误由前端确认后 force 重试
//! - 启动时自动给所选网卡配置 192.168.134.1（需 root/管理员），停止时移除还原
//! - 事件：dhcp://status、dhcp://lease

use crate::events;
use serde::Serialize;
use socket2::{Domain, Protocol, Socket, Type};
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

const DHCP_SERVER_PORT: u16 = 67;
const DHCP_CLIENT_PORT: u16 = 68;
const SUBNET: [u8; 3] = [192, 168, 134];
const LEASE_START: u8 = 100;
const LEASE_END: u8 = 200;

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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DhcpLeaseView {
    pub mac: String,
    pub ip: String,
    pub ts: u64,
}

struct DhcpInner {
    running: bool,
    interface_name: String,
    auto_mode: bool,
    leases: HashMap<String, DhcpLeaseView>,
    cancel: Option<CancellationToken>,
    /// 启动时由程序自动添加的 192.168.134.1 网卡地址（停止时移除还原）
    nic_ip_added: Option<String>,
}

impl Default for DhcpInner {
    fn default() -> Self {
        Self {
            running: false,
            interface_name: String::new(),
            auto_mode: false,
            leases: HashMap::new(),
            cancel: None,
            nic_ip_added: None,
        }
    }
}

#[derive(Default)]
pub struct DhcpService {
    inner: Mutex<DhcpInner>,
}

/// 检测是否存在真实网络环境（外网/内网），需排除本机作为 DHCP 服务器时的 192.168.134.x 接口
pub async fn has_real_network() -> bool {
    // 1. 接口检查：存在非回环且非 192.168.134.0/24 的 IPv4 接口
    if let Ok(list) = local_ip_address::list_afinet_netifas() {
        for (_, ip) in list {
            if let std::net::IpAddr::V4(v4) = ip {
                if v4.is_loopback() {
                    continue;
                }
                let o = v4.octets();
                if !(o[0] == SUBNET[0] && o[1] == SUBNET[1] && o[2] == SUBNET[2]) {
                    return true;
                }
            }
        }
    }
    // 2. 路由检查：UDP connect 到公共 DNS（不真正发包），有默认路由则视为有网
    let probe = tokio::time::timeout(
        Duration::from_millis(600),
        async {
            let sock = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
            sock.connect("223.5.5.5:53").await
        },
    )
    .await;
    matches!(probe, Ok(Ok(())))
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

/// 确定目标网卡：传入名为准；未传入时仅当只有唯一非回环 IPv4 接口时自动选中
fn resolve_interface(requested: &str) -> Result<String, String> {
    let list = local_ip_address::list_afinet_netifas()
        .map_err(|e| format!("枚举网卡失败: {e}"))?;
    let v4: Vec<(String, Ipv4Addr)> = list
        .into_iter()
        .filter_map(|(n, ip)| match ip {
            std::net::IpAddr::V4(v4) if !v4.is_loopback() => Some((n, v4)),
            _ => None,
        })
        .collect();
    if !requested.is_empty() {
        if v4.iter().any(|(n, _)| n == requested) {
            return Ok(requested.to_string());
        }
        return Err(format!("未找到名为 {requested} 的 IPv4 网卡"));
    }
    let mut uniq: Vec<&(String, Ipv4Addr)> = Vec::new();
    for e in &v4 {
        if !uniq.iter().any(|u| u.0 == e.0) {
            uniq.push(e);
        }
    }
    if uniq.len() == 1 {
        return Ok(uniq[0].0.clone());
    }
    Err("请先在顶部网卡选择中选中直连设备的那块网卡".into())
}

/// 确保目标网卡持有 192.168.134.x 地址（设备回包目标）；无则自动添加。
/// 返回是否由本次调用新增（新增的在停止时移除还原）。需要 root/管理员权限，失败返回提示文案
fn ensure_subnet_ip(requested: &str) -> Result<bool, String> {
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

/// 移除启动时自动添加的 192.168.134.1（停止服务时还原现场），失败忽略
fn remove_subnet_ip(name: &str) {
    let _ = run_ip_cmd(name, false);
}

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

        // 为所选网卡自动配置 192.168.134.1（设备回包目标），失败不阻断但给出警告
        let nic_warning = match ensure_subnet_ip(&interface_name) {
            Ok(added) => {
                if added {
                    let mut inner = self.inner.lock().await;
                    inner.nic_ip_added = Some(interface_name.clone());
                }
                String::new()
            }
            Err(e) => e,
        };

        let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
            .map_err(|e| format!("创建套接字失败: {e}"))?;
        let _ = sock.set_reuse_address(true);
        #[cfg(unix)]
        let _ = sock.set_reuse_port(true);
        let _ = sock.set_broadcast(true);
        let bind_addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, DHCP_SERVER_PORT));
        sock.bind(&bind_addr.into()).map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                "绑定 67 端口需要管理员/root 权限，请在设置页使用『以管理员身份重启』".to_string()
            } else {
                format!("绑定 67 端口失败: {e}")
            }
        })?;
        sock.set_nonblocking(true)
            .map_err(|e| format!("设置非阻塞失败: {e}"))?;
        let socket = Arc::new(UdpSocket::from_std(sock.into()).map_err(|e| e.to_string())?);

        let cancel = CancellationToken::new();
        {
            let mut inner = self.inner.lock().await;
            inner.running = true;
            inner.interface_name = interface_name.clone();
            inner.auto_mode = auto_mode;
            inner.leases.clear();
            inner.cancel = Some(cancel.clone());
        }
        self.emit_status(&app).await;

        let svc = app.state::<crate::state::AppState>().dhcp.clone();
        tokio::spawn(serve_loop(svc, app, socket, cancel));
        if !nic_warning.is_empty() {
            return Err(nic_warning);
        }
        Ok(())
    }

    pub async fn stop(&self, app: &AppHandle) {
        self.stop_inner(app).await;
    }

    async fn stop_inner(&self, app: &AppHandle) {
        let mut inner = self.inner.lock().await;
        // 移除启动时自动添加的网卡地址（还原现场）
        if let Some(nic) = inner.nic_ip_added.take() {
            remove_subnet_ip(&nic);
        }
        if !inner.running {
            return;
        }
        if let Some(c) = inner.cancel.take() {
            c.cancel();
        }
        inner.running = false;
        inner.leases.clear();
        drop(inner);
        self.emit_status(app).await;
    }

    pub async fn status(&self) -> DhcpStatusView {
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
        let _ = app.emit(events::DHCP_STATUS, self.status().await);
    }
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
                let Some((mac, msg_type, xid)) = parse_request(&buf[..len]) else { continue };
                let mac_str = hex::encode_upper(mac);

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
                    _ => continue,
                };
                if msg_type == 3 {
                    let lease = DhcpLeaseView {
                        mac: mac_str.clone(),
                        ip: ip.to_string(),
                        ts: now_ms(),
                    };
                    inner.leases.insert(mac_str.clone(), lease.clone());
                    drop(inner);
                    let _ = app.emit(events::DHCP_LEASE, lease);
                    let _ = app.emit(events::DHCP_STATUS, svc.status().await);
                } else {
                    drop(inner);
                }
                let reply = build_reply(xid, mac, ip, reply_type);
                let _ = socket.send_to(&reply, bcast).await;
            }
        }
    }
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
    state: State<'_, crate::state::AppState>,
) -> Result<DhcpStatusView, String> {
    Ok(state.dhcp.status().await)
}
