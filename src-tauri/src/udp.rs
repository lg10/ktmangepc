//! UDP 服务：RCU 设备发现/监控 + 门锁报文监听
//!
//! 对应原 UdpServer.cs / UdpFind.cs / UdpHeart.cs / NetWorkMsg.cs：
//! - 绑定：RCU 从 4668 起顺延尝试 100 个端口，失败给出明确提示；门锁固定 8787
//! - 发现：每 3 秒广播 0xFE00 报文到 255.255.255.255:4667；超级模式先探测目标网段再单播
//! - 心跳：每 5 秒检查，>10s 未上报计一次掉线，掉线计数 >5 移除设备
//! - 事件：udp://device、udp://device-offline、udp://lock-packet、udp://server-status、udp://log

use crate::events;
use crate::protocol::{
    self, build_packet, find_packet, payload_of, Header, NetworkConfigDto, RcuDeviceView,
    UdpModel,
};
use serde::Serialize;
use socket2::{Domain, Protocol, Socket, Type};
use std::collections::{HashMap, VecDeque};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub const BASE_PORT: u16 = 4668;
pub const MAX_ATTEMPTS: u16 = 100;
pub const LOCK_PORT: u16 = 8787;
pub const FIND_PORT: u16 = 4667;

const HEART_TICK_SECS: u64 = 5;
const STALE_SECS: u64 = 10;
const REMOVE_COUNT: u32 = 5;
const MAX_LOCK_PACKETS: usize = 500;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatusView {
    pub running: bool,
    pub mode: u8,
    pub ip: String,
    pub port: u16,
    pub device_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LockPacketView {
    pub src_ip: String,
    pub src_port: u16,
    pub hex: String,
    pub ts: u64,
}

struct DeviceEntry {
    model: UdpModel,
    addr: SocketAddr,
    last_seen: Instant,
    offline_count: u32,
}

impl DeviceEntry {
    fn view(&self) -> RcuDeviceView {
        let m = &self.model;
        RcuDeviceView {
            equip_id: m.equip_id.clone(),
            room_num: m.room_num.to_string(),
            room_type_name: m.door_model.to_string(),
            build_name: m.build_num.to_string(),
            floor_name: m.floor_num.to_string(),
            rcu_mac: m.rcu_mac.clone(),
            equipment_model: protocol::model_name(m.equipment_model),
            version: protocol::version_text(m.protocol_version, m.hard_version, m.soft_version),
            author: protocol::author_text(m.authorization_way, m.authorization_state),
            base_num: m.base_num_text(),
            network: m.network_text(),
            ip: m.rcu_ip.to_string(),
            server: m.server_text(),
            run_server: m.run_server_text(),
            ip_flag: m.rcu_ip_flag,
            mask: m.rcu_mask.to_string(),
            gateway: m.rcu_gateway.to_string(),
            dns: m.rcu_dns.to_string(),
            server_flag: m.server_flag,
            server_url: m.url.clone(),
            server_ip: m.server_ip.to_string(),
            server_port: protocol::port_value(m.server_port),
            count: if self.offline_count == 0 {
                "正常".into()
            } else {
                format!("[{}/6] 掉线", self.offline_count)
            },
            last_seen: now_ms(),
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Default)]
struct ServiceInner {
    running: bool,
    mode: u8,
    ip: String,
    port: u16,
    devices: HashMap<String, DeviceEntry>,
    lock_packets: VecDeque<LockPacketView>,
    socket: Option<Arc<UdpSocket>>,
    cancel: Option<CancellationToken>,
}

#[derive(Default)]
pub struct UdpService {
    inner: Mutex<ServiceInner>,
}

fn bind_socket(bind_ip: Ipv4Addr, base_port: u16, attempts: u16) -> Result<(UdpSocket, u16), String> {
    let mut last_err: Option<std::io::Error> = None;
    for i in 0..attempts {
        let port = base_port + i;
        let sock = match Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)) {
            Ok(s) => s,
            Err(e) => return Err(format!("创建套接字失败: {e}")),
        };
        let _ = sock.set_reuse_address(true);
        #[cfg(unix)]
        let _ = sock.set_reuse_port(true);
        let _ = sock.set_broadcast(true);
        match sock.bind(&SocketAddr::from((bind_ip, port)).into()) {
            Ok(()) => {
                if let Err(e) = sock.set_nonblocking(true) {
                    return Err(format!("设置非阻塞失败: {e}"));
                }
                return UdpSocket::from_std(sock.into())
                    .map(|s| (s, port))
                    .map_err(|e| format!("套接字转换失败: {e}"));
            }
            Err(e) => last_err = Some(e),
        }
    }
    Err(format!(
        "端口 {base_port}-{} 全部被占用，请关闭可能占用的程序后重试（最后错误: {}）",
        base_port + attempts - 1,
        last_err.map(|e| e.to_string()).unwrap_or_default()
    ))
}

fn emit_log(app: &AppHandle, msg: String) {
    let _ = app.emit(events::UDP_LOG, msg);
}

impl UdpService {
    /// 启动服务，返回实际绑定端口
    pub async fn start(
        &self,
        app: AppHandle,
        ip: String,
        mode: u8,
        segments: Vec<String>,
    ) -> Result<u16, String> {
        // 先停掉旧实例
        self.stop_inner(&app).await;

        let (socket, port) = if mode == 4 {
            // 门锁模式：0.0.0.0:8787
            let (s, p) = bind_socket(Ipv4Addr::UNSPECIFIED, LOCK_PORT, 1)?;
            (s, p)
        } else {
            let bind_ip: Ipv4Addr = ip
                .parse()
                .map_err(|_| format!("网卡 IP 非法: {ip}"))?;
            bind_socket(bind_ip, BASE_PORT, MAX_ATTEMPTS)?
        };
        let socket = Arc::new(socket);
        let cancel = CancellationToken::new();

        {
            let mut inner = self.inner.lock().await;
            inner.running = true;
            inner.mode = mode;
            inner.ip = ip.clone();
            inner.port = port;
            inner.devices.clear();
            inner.lock_packets.clear();
            inner.socket = Some(socket.clone());
            inner.cancel = Some(cancel.clone());
        }

        emit_log(&app, format!("UDP 服务已启动：{ip}:{port}（模式 {mode}）"));
        self.emit_status(&app).await;

        // 接收循环（从托管状态克隆所需服务 Arc，避免跨 await 持有 State 引用）
        let (udp_svc, update_svc, filestore_svc) = {
            let st = app.state::<crate::state::AppState>();
            (st.udp.clone(), st.update.clone(), st.filestore.clone())
        };
        tokio::spawn(recv_loop(
            udp_svc.clone(),
            update_svc,
            filestore_svc,
            app.clone(),
            socket.clone(),
            cancel.clone(),
        ));

        if mode != 4 {
            // 广播发现循环
            let bind_ip: Ipv4Addr = ip.parse().unwrap_or(Ipv4Addr::UNSPECIFIED);
            tokio::spawn(find_loop(
                udp_svc.clone(),
                app.clone(),
                socket.clone(),
                cancel.clone(),
                bind_ip,
                mode,
                segments,
            ));
            // 心跳检测循环
            tokio::spawn(heart_loop(udp_svc.clone(), app.clone(), cancel.clone()));
        }

        Ok(port)
    }

    pub async fn stop(&self, app: &AppHandle) {
        self.stop_inner(app).await;
    }

    async fn stop_inner(&self, app: &AppHandle) {
        let mut inner = self.inner.lock().await;
        if !inner.running {
            return;
        }
        if let Some(c) = inner.cancel.take() {
            c.cancel();
        }
        inner.socket = None;
        inner.running = false;
        inner.devices.clear();
        inner.lock_packets.clear();
        drop(inner);
        emit_log(app, "UDP 服务已停止".into());
        self.emit_status(app).await;
    }

    pub async fn status(&self) -> ServerStatusView {
        let inner = self.inner.lock().await;
        ServerStatusView {
            running: inner.running,
            mode: inner.mode,
            ip: inner.ip.clone(),
            port: inner.port,
            device_count: inner.devices.len(),
        }
    }

    pub async fn list_devices(&self) -> Vec<RcuDeviceView> {
        let inner = self.inner.lock().await;
        inner.devices.values().map(|d| d.view()).collect()
    }

    pub async fn clear_devices(&self) {
        let mut inner = self.inner.lock().await;
        inner.devices.clear();
    }

    pub async fn list_lock_packets(&self) -> Vec<LockPacketView> {
        let inner = self.inner.lock().await;
        inner.lock_packets.iter().cloned().collect()
    }

    /// 向指定设备下发报文
    async fn send_to_device(&self, equip_id: &str, packet: Vec<u8>) -> Result<(), String> {
        let inner = self.inner.lock().await;
        let socket = inner
            .socket
            .clone()
            .ok_or_else(|| "UDP 服务未启动".to_string())?;
        let addr = inner
            .devices
            .get(equip_id)
            .map(|d| d.addr)
            .ok_or_else(|| format!("设备不存在或已离线: {equip_id}"))?;
        drop(inner);
        socket
            .send_to(&packet, addr)
            .await
            .map_err(|e| format!("发送失败: {e}"))?;
        Ok(())
    }

    /// 对外暴露的原始报文下发（升级/授权等模块复用设备寻址）
    pub async fn send_raw(&self, equip_id: &str, packet: Vec<u8>) -> Result<(), String> {
        self.send_to_device(equip_id, packet).await
    }

    pub async fn send_network_config(
        &self,
        equip_id: &str,
        cfg: &NetworkConfigDto,
    ) -> Result<(), String> {
        let payload = protocol::build_network_payload(cfg)?;
        let packet = build_packet(protocol::reg::NET_WORK_CONFIGURE, &payload)?;
        self.send_to_device(equip_id, packet).await
    }

    pub async fn send_base_info(
        &self,
        equip_id: &str,
        hotel_id: u32,
        build_num: u8,
        floor_num: u8,
        room_num: u8,
        door_model: u8,
    ) -> Result<(), String> {
        let payload =
            protocol::build_base_info_payload(hotel_id, build_num, floor_num, room_num, door_model);
        let packet = build_packet(protocol::reg::UDP_SEND_BASE_INFO, &payload)?;
        self.send_to_device(equip_id, packet).await
    }

    pub async fn send_revert_cmd(&self, equip_id: &str, cmd: u8) -> Result<(), String> {
        let payload = protocol::build_revert_payload(cmd);
        let packet = build_packet(protocol::reg::UDP_REVERT, &payload)?;
        self.send_to_device(equip_id, packet).await
    }

    async fn emit_status(&self, app: &AppHandle) {
        let status = self.status().await;
        let _ = app.emit(events::SERVER_STATUS, status);
    }
}

/// 接收循环：解析设备回复 / 门锁报文
async fn recv_loop(
    udp: Arc<UdpService>,
    update: Arc<crate::upgrade::UpdateService>,
    filestore: Arc<crate::filestore::FileStore>,
    app: AppHandle,
    socket: Arc<UdpSocket>,
    cancel: CancellationToken,
) {
    let mut buf = vec![0u8; 4096];
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            res = socket.recv_from(&mut buf) => {
                match res {
                    Ok((len, src)) => {
                        let data = buf[..len].to_vec();
                        handle_packet(&udp, &update, &filestore, &app, &data, src).await;
                    }
                    Err(_) => {
                        // socket 已关闭
                        break;
                    }
                }
            }
        }
    }
}

async fn handle_packet(
    svc: &UdpService,
    update: &crate::upgrade::UpdateService,
    filestore: &crate::filestore::FileStore,
    app: &AppHandle,
    data: &[u8],
    src: SocketAddr,
) {
    let mode = { svc.inner.lock().await.mode };

    if mode == 4 {
        // 门锁模式：原样记录报文
        let view = LockPacketView {
            src_ip: src.ip().to_string(),
            src_port: src.port(),
            hex: hex::encode_upper(data),
            ts: now_ms(),
        };
        {
            let mut inner = svc.inner.lock().await;
            inner.lock_packets.push_back(view.clone());
            if inner.lock_packets.len() > MAX_LOCK_PACKETS {
                inner.lock_packets.pop_front();
            }
        }
        let _ = app.emit(events::LOCK_PACKET, view);
        return;
    }

    let header = match Header::parse(data) {
        Some(h) => h,
        None => return,
    };

    match header.reg_addr {
        protocol::reg::NET_WORK_CONFIGURE | protocol::reg::READ_NET_WORK_CONFIGURE => {
            // 设备对 0xFE00 发现的应答（0xFE10/0xFE11，func 0x03），载荷为 UdpModel
            let payload = payload_of(data, &header);
            let model = match UdpModel::parse(payload) {
                Some(m) => m,
                None => {
                    emit_log(
                        app,
                        format!("收到无法解析的设备报文: {} ({}字节)", src, payload.len()),
                    );
                    return;
                }
            };
            let view = {
                let mut inner = svc.inner.lock().await;
                // 去重过滤：同 MAC/IP 的设备若以新 equip_id 上报，移除旧键，避免每轮轮询重复添加
                let stale: Vec<String> = inner
                    .devices
                    .iter()
                    .filter(|(id, e)| {
                        id.as_str() != model.equip_id
                            && ((!model.rcu_mac.is_empty()
                                && e.model.rcu_mac == model.rcu_mac)
                                || e.model.rcu_ip == model.rcu_ip)
                    })
                    .map(|(id, _)| id.clone())
                    .collect();
                for k in stale {
                    inner.devices.remove(&k);
                }
                let entry = inner.devices.entry(model.equip_id.clone()).or_insert_with(|| {
                    DeviceEntry {
                        model: model.clone(),
                        addr: src,
                        last_seen: Instant::now(),
                        offline_count: 0,
                    }
                });
                entry.model = model;
                entry.addr = src;
                entry.last_seen = Instant::now();
                entry.offline_count = 0;
                entry.view()
            };
            let _ = app.emit(events::DEVICE, view);
        }
        protocol::reg::UDP_READ_BASE_INFO => {
            emit_log(app, format!("收到设备基础信息回读报文: {src}"));
        }
        protocol::reg::REQUEST_UPDATE_FILE | protocol::reg::REQUEST_CONFIG_FILE => {
            // 设备拉取升级/配置分包请求
            let kind = if header.reg_addr == protocol::reg::REQUEST_UPDATE_FILE {
                crate::filestore::FileKind::Upgrade
            } else {
                crate::filestore::FileKind::Config
            };
            let payload = payload_of(data, &header);
            match protocol::parse_file_request(payload) {
                Some(req) => {
                    update.on_file_request(app, filestore, svc, kind, &req).await;
                }
                None => {
                    emit_log(app, format!("拉包请求解析失败: {src} ({}字节)", payload.len()));
                }
            }
        }
        other => {
            emit_log(
                app,
                format!("收到未处理报文 reg=0x{other:04X} from {src}"),
            );
        }
    }
}

/// 广播发现循环（普通/超级/DHCP 模式）
async fn find_loop(
    svc: Arc<UdpService>,
    app: AppHandle,
    socket: Arc<UdpSocket>,
    cancel: CancellationToken,
    local_ip: Ipv4Addr,
    mode: u8,
    segments: Vec<String>,
) {
    let packet = find_packet();
    let bcast: SocketAddr = (Ipv4Addr::new(255, 255, 255, 255), FIND_PORT).into();

    // 超级模式：先并发探测目标网段，向存活主机单播发现报文
    if mode == 2 && !segments.is_empty() {
        let subnet_prefix = {
            let o = local_ip.octets();
            format!("{}.{}", o[0], o[1])
        };
        let mut join = tokio::task::JoinSet::new();
        for seg in &segments {
            for j in 1u16..=254 {
                let ip = format!("{subnet_prefix}.{seg}.{j}");
                let sock = socket.clone();
                let pkt = packet.clone();
                let token = cancel.clone();
                join.spawn(async move {
                    // TCP 探测替代原 ICMP Ping（免特权）
                    let probe = tokio::time::timeout(
                        Duration::from_millis(400),
                        TcpStream::connect((ip.as_str(), FIND_PORT)),
                    )
                    .await;
                    if matches!(probe, Ok(Ok(_))) {
                        let target: SocketAddr = format!("{ip}:{FIND_PORT}")
                            .parse()
                            .expect("IP 由内部构造");
                        let _ = sock.send_to(&pkt, target).await;
                    }
                    drop(token);
                });
            }
        }
        tokio::spawn(async move {
            while join.join_next().await.is_some() {}
        });
        emit_log(&app, format!("超级模式：开始扫描网段 {:?}", segments));
    }

    let mut interval = tokio::time::interval(Duration::from_secs(3));
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = interval.tick() => {
                let _ = socket.send_to(&packet, bcast).await;
                // 超级模式追加向已发现的异网段设备单播（对应原 SouceIpList 逻辑）
                if mode == 2 {
                    let local_oct = local_ip.octets();
                    let targets: Vec<SocketAddr> = {
                        let guard = svc.inner.lock().await;
                        guard
                            .devices
                            .values()
                            .map(|d| d.addr)
                            .filter(|a| {
                                if let std::net::IpAddr::V4(v4) = a.ip() {
                                    let o = v4.octets();
                                    !(o[0] == local_oct[0] && o[1] == local_oct[1])
                                } else {
                                    false
                                }
                            })
                            .map(|mut a| {
                                a.set_port(FIND_PORT);
                                a
                            })
                            .collect()
                    };
                    for t in targets {
                        let _ = socket.send_to(&packet, t).await;
                    }
                }
            }
        }
    }
}

/// 心跳检测循环：5 秒一轮，>10s 未上报计一次掉线，>5 次移除
async fn heart_loop(svc: Arc<UdpService>, app: AppHandle, cancel: CancellationToken) {
    let mut interval = tokio::time::interval(Duration::from_secs(HEART_TICK_SECS));
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = interval.tick() => {
                let mut removed: Vec<String> = Vec::new();
                let mut updated: Vec<RcuDeviceView> = Vec::new();
                {
                    let mut inner = svc.inner.lock().await;
                    inner.devices.retain(|id, entry| {
                        if entry.last_seen.elapsed().as_secs() > STALE_SECS {
                            entry.offline_count += 1;
                            if entry.offline_count > REMOVE_COUNT {
                                removed.push(id.clone());
                                return false;
                            }
                            updated.push(entry.view());
                        }
                        true
                    });
                }
                for view in updated {
                    let _ = app.emit(events::DEVICE, view);
                }
                for id in removed {
                    emit_log(&app, format!("设备掉线移除: {id}"));
                    let _ = app.emit(events::DEVICE_OFFLINE, id);
                }
            }
        }
    }
}

#[tauri::command]
pub async fn start_udp_server(
    app: AppHandle,
    state: State<'_, crate::state::AppState>,
    ip: String,
    mode: u8,
    segments: Vec<String>,
) -> Result<u16, String> {
    state.udp.start(app, ip, mode, segments).await
}

#[tauri::command]
pub async fn stop_udp_server(
    app: AppHandle,
    state: State<'_, crate::state::AppState>,
) -> Result<(), String> {
    state.udp.stop(&app).await;
    Ok(())
}

#[tauri::command]
pub async fn get_server_status(
    state: State<'_, crate::state::AppState>,
) -> Result<ServerStatusView, String> {
    Ok(state.udp.status().await)
}

#[tauri::command]
pub async fn list_devices(
    state: State<'_, crate::state::AppState>,
) -> Result<Vec<RcuDeviceView>, String> {
    Ok(state.udp.list_devices().await)
}

#[tauri::command]
pub async fn clear_devices(state: State<'_, crate::state::AppState>) -> Result<(), String> {
    state.udp.clear_devices().await;
    Ok(())
}

#[tauri::command]
pub async fn list_lock_packets(
    state: State<'_, crate::state::AppState>,
) -> Result<Vec<LockPacketView>, String> {
    Ok(state.udp.list_lock_packets().await)
}

#[tauri::command]
pub async fn send_network_config(
    state: State<'_, crate::state::AppState>,
    equip_id: String,
    config: NetworkConfigDto,
) -> Result<(), String> {
    state.udp.send_network_config(&equip_id, &config).await
}

#[tauri::command]
pub async fn send_base_info(
    state: State<'_, crate::state::AppState>,
    equip_id: String,
    hotel_id: u32,
    build_num: u8,
    floor_num: u8,
    room_num: u8,
    door_model: u8,
) -> Result<(), String> {
    state
        .udp
        .send_base_info(&equip_id, hotel_id, build_num, floor_num, room_num, door_model)
        .await
}

#[tauri::command]
pub async fn send_revert_cmd(
    state: State<'_, crate::state::AppState>,
    equip_id: String,
    cmd: u8,
) -> Result<(), String> {
    state.udp.send_revert_cmd(&equip_id, cmd).await
}
