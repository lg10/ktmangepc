//! 肯天 RCU 设备协议级模拟器
//!
//! 无硬件联调时的替代手段：
//! - 监听 4667：响应工具的 0xFE00 广播发现，回复 0xFE01 + UdpModel 载荷
//! - 监听 4668：接收工具下发的网络配置(0xFE10)、基础信息(0xFE12)、UdpRevert(0xFE01) 指令
//!   并更新内部状态后回复最新设备信息，便于在监控表格中观察下发效果
//! - 升级联调：收到固件首指令 0x02F2 / 配置首指令 0xFD01 后，自动循环发送
//!   0x02F3/0xFD02 拉包请求并接收 0x02F4/0xFD03 分包应答，模拟完整升级流程
//!
//! 两个端口共用同一套报文处理：工具按发现应答的源地址回发所有指令，
//! 因此分包应答可能落在 4667 端口上。
//!
//! 用法：cargo run --release -- --dev-ip 192.168.134.88 --dev-port 4668

use std::net::{Ipv4Addr, SocketAddr};
use tokio::net::UdpSocket;

const FIND_PORT: u16 = 4667;
const REG_REVERT: u16 = 0xFE01;
const REG_NET_CONFIG: u16 = 0xFE10;
const REG_BASE_INFO: u16 = 0xFE12;
const REG_START_UPDATE: u16 = 0x02F2;
const REG_REQUEST_UPDATE: u16 = 0x02F3;
const REG_SEND_UPDATE: u16 = 0x02F4;
const REG_START_CONFIG: u16 = 0xFD01;
const REG_REQUEST_CONFIG: u16 = 0xFD02;
const REG_SEND_CONFIG: u16 = 0xFD03;

#[derive(Clone, Copy, PartialEq)]
enum FileKind {
    Firmware,
    Config,
}

impl FileKind {
    fn label(self) -> &'static str {
        match self {
            FileKind::Firmware => "固件",
            FileKind::Config => "配置",
        }
    }
}

/// 进行中的升级会话（同一时间只模拟一个文件）
struct UpgradeSession {
    kind: FileKind,
    uid: u16,
    num: u16,
    /// 下一个待请求的分包序号（从 1 开始）
    next: u16,
}

#[derive(Clone)]
struct DeviceState {
    equip_id: [u8; 8],
    model: u16,
    protocol_version: u8,
    soft_version: u16,
    hard_version: u8,
    auth_way: u8,
    auth_state: u8,
    hotel_id: u32,
    build_num: u8,
    floor_num: u8,
    room_num: u8,
    door_model: u8,
    ip_flag: u8,
    ip: Ipv4Addr,
    mask: Ipv4Addr,
    gateway: Ipv4Addr,
    mac: [u8; 6],
    server_flag: u8,
    url: [u8; 32],
    server_ip: Ipv4Addr,
    server_port: u16,
}

impl Default for DeviceState {
    fn default() -> Self {
        Self {
            equip_id: [0xAA, 0xBB, 0xCC, 0xDD, 0x11, 0x22, 0x33, 0x44],
            model: 0x00EF, // MINI主机
            protocol_version: 1,
            soft_version: 0x0108,
            hard_version: 2,
            auth_way: 4, // 永久授权
            auth_state: 0,
            hotel_id: 1001,
            build_num: 1,
            floor_num: 3,
            room_num: 5,
            door_model: 1,
            ip_flag: 0,
            ip: Ipv4Addr::new(192, 168, 134, 88),
            mask: Ipv4Addr::new(255, 255, 255, 0),
            gateway: Ipv4Addr::new(192, 168, 134, 1),
            mac: [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x01],
            server_flag: 0,
            url: [0u8; 32],
            server_ip: Ipv4Addr::new(192, 168, 134, 1),
            server_port: 8899,
        }
    }
}

impl DeviceState {
    /// 121 字节 UdpModel 载荷（字段顺序与主程序 protocol.rs 一致）
    fn payload(&self) -> Vec<u8> {
        let mut p = Vec::with_capacity(121);
        p.extend_from_slice(&self.equip_id);
        p.extend_from_slice(&self.model.to_be_bytes());
        p.push(self.protocol_version);
        p.extend_from_slice(&self.soft_version.to_be_bytes());
        p.push(self.hard_version);
        p.push(self.auth_way);
        p.push(self.auth_state);
        p.extend_from_slice(&self.hotel_id.to_be_bytes());
        p.push(self.build_num);
        p.push(self.floor_num);
        p.push(self.room_num);
        p.push(self.door_model);
        p.push(self.ip_flag);
        p.extend_from_slice(&self.ip.octets());
        p.extend_from_slice(&self.mask.octets());
        p.extend_from_slice(&self.gateway.octets());
        p.extend_from_slice(&self.mac);
        p.push(self.server_flag);
        p.extend_from_slice(&self.url);
        p.extend_from_slice(&self.server_ip.octets());
        p.extend_from_slice(&self.server_port.to_be_bytes());
        // runServer 段（模拟器固定为空配置）
        p.push(0);
        p.extend_from_slice(&[0u8; 32]);
        p.extend_from_slice(&[0u8; 4]);
        p.extend_from_slice(&0u16.to_be_bytes());
        debug_assert_eq!(p.len(), 121);
        p
    }

    /// 设备应答报文：Header(func=0x03, reg=0xFE01) + 载荷 + 0D0A
    fn reply_packet(&self) -> Vec<u8> {
        let payload = self.payload();
        let mut out = Vec::with_capacity(payload.len() + 14);
        let mut h = [0u8; 12];
        h[0..2].copy_from_slice(&1u16.to_be_bytes()); // serial
        h[2..4].copy_from_slice(&1u16.to_be_bytes()); // deviceType
        h[4..6].copy_from_slice(&(payload.len() as u16).to_be_bytes());
        h[7] = 0x03; // funcCode: 读响应
        h[8..10].copy_from_slice(&REG_REVERT.to_be_bytes());
        h[10..12].copy_from_slice(&40u16.to_be_bytes());
        out.extend_from_slice(&h);
        out.extend_from_slice(&payload);
        out.extend_from_slice(b"\r\n");
        out
    }
}

fn parse_header(buf: &[u8]) -> Option<(u8, u16, u16)> {
    if buf.len() < 12 {
        return None;
    }
    Some((
        buf[7],
        u16::from_be_bytes([buf[8], buf[9]]),
        u16::from_be_bytes([buf[4], buf[5]]),
    ))
}

fn revert_name(cmd: u8) -> &'static str {
    match cmd {
        0 => "退出查找",
        1 => "查找设备",
        2 => "硬件测试",
        3 => "老化测试",
        4 => "全开",
        5 => "全关",
        _ => "未知指令",
    }
}

/// 解析升级/配置首指令载荷，返回 (uid, 总字节数, 分包数)
/// 固件 0x02F2：  "0000"+hotelFlag(2)+v0(2)+v2(4)+v1(2)+updateTime(8)+fileId(4)+total(8)+num(4)+md5
/// 配置 0xFD01： fileId(4)+total(8)+num(4)+md5+…
fn parse_start_cmd(kind: FileKind, payload: &[u8]) -> Option<(u16, u64, u16)> {
    let h = hex::encode_upper(payload);
    let (uid, total, num) = match kind {
        FileKind::Firmware => {
            if h.len() < 38 {
                return None;
            }
            (&h[22..26], &h[26..34], &h[34..38])
        }
        FileKind::Config => {
            if h.len() < 16 {
                return None;
            }
            (&h[0..4], &h[4..12], &h[12..16])
        }
    };
    Some((
        u16::from_str_radix(uid, 16).ok()?,
        u64::from_str_radix(total, 16).ok()?,
        u16::from_str_radix(num, 16).ok()?,
    ))
}

/// 拉包请求报文：equipmentModel(2) + equipId(8) + uid(2) + dataIndex(2)
fn build_file_request(kind: FileKind, state: &DeviceState, uid: u16, index: u16) -> Vec<u8> {
    let reg = if kind == FileKind::Firmware {
        REG_REQUEST_UPDATE
    } else {
        REG_REQUEST_CONFIG
    };
    let mut body = Vec::with_capacity(14);
    body.extend_from_slice(&state.model.to_be_bytes());
    body.extend_from_slice(&state.equip_id);
    body.extend_from_slice(&uid.to_be_bytes());
    body.extend_from_slice(&index.to_be_bytes());
    let reg_num = body.len() as u16;
    let mut out = Vec::with_capacity(12 + body.len() + 2);
    let mut h = [0u8; 12];
    h[0..2].copy_from_slice(&reg_num.to_be_bytes()); // serial=regNum
    h[2..4].copy_from_slice(&1u16.to_be_bytes()); // deviceType
    h[4..6].copy_from_slice(&(reg_num + 14).to_be_bytes()); // dataLen
    h[7] = 0x10; // funcCode: 写
    h[8..10].copy_from_slice(&reg.to_be_bytes());
    h[10..12].copy_from_slice(&reg_num.to_be_bytes());
    out.extend_from_slice(&h);
    out.extend_from_slice(&body);
    out.extend_from_slice(b"\r\n");
    out
}

/// 统一处理两个端口收到的报文（工具按发现应答源地址回发，分包应答可能落在 4667）
async fn handle_datagram(
    sock: &UdpSocket,
    data: &[u8],
    len: usize,
    src: SocketAddr,
    state: &mut DeviceState,
    session: &mut Option<UpgradeSession>,
) -> std::io::Result<()> {
    let Some((_func, reg, data_len)) = parse_header(&data[..len]) else {
        return Ok(());
    };
    let payload_start = 12usize.min(len);
    // 按头部 data_len 裁剪载荷（与主程序 payload_of 一致），再去除可能的 0D0A 尾
    let mut end = if data_len > 0 {
        (payload_start + data_len as usize).min(len)
    } else {
        len
    };
    if end >= payload_start + 2 && &data[end - 2..end] == b"\r\n" {
        end -= 2;
    }
    let payload = &data[payload_start..end];

    match reg {
        0xFE00 => {
            println!("[模拟器] 收到发现请求 from {src}，回复设备信息");
            let reply = state.reply_packet();
            sock.send_to(&reply, src).await?;
        }
        REG_REVERT => {
            let cmd = payload.first().copied().unwrap_or(0xFF);
            println!("[模拟器] 收到 UdpRevert 指令: {cmd} ({})", revert_name(cmd));
        }
        REG_START_UPDATE | REG_START_CONFIG => {
            let kind = if reg == REG_START_UPDATE {
                FileKind::Firmware
            } else {
                FileKind::Config
            };
            match parse_start_cmd(kind, payload) {
                Some((uid, total, num)) => {
                    // 同文件会话进行中时忽略工具守护重发的首指令
                    if let Some(s) = session {
                        if s.kind == kind && s.uid == uid {
                            return Ok(());
                        }
                    }
                    println!(
                        "[模拟器] 收到{}首指令: uid={} 总字节={} 分包数={}，开始拉包",
                        kind.label(),
                        uid,
                        total,
                        num
                    );
                    *session = Some(UpgradeSession { kind, uid, num, next: 1 });
                    let req = build_file_request(kind, state, uid, 1);
                    sock.send_to(&req, src).await?;
                }
                None => {
                    println!("[模拟器] 首指令载荷解析失败 ({}字节)", payload.len());
                }
            }
        }
        REG_SEND_UPDATE | REG_SEND_CONFIG => {
            let Some(mut s) = session.take() else {
                return Ok(());
            };
            // 应答载荷：uid(X4) + index(X4) + len(X4) + hex 数据 + md5
            let h = hex::encode_upper(payload);
            if h.len() < 12 {
                *session = Some(s);
                return Ok(());
            }
            let uid = u16::from_str_radix(&h[0..4], 16).unwrap_or(0);
            let idx = u16::from_str_radix(&h[4..8], 16).unwrap_or(0);
            if uid != s.uid || idx != s.next {
                println!(
                    "[模拟器] 收到乱序分包应答 uid={uid} idx={idx}（期望 uid={} idx={}），忽略",
                    s.uid, s.next
                );
                *session = Some(s);
                return Ok(());
            }
            if s.next % (s.num / 10).max(1) == 0 || s.next == s.num {
                println!(
                    "[模拟器] {}下载进度 {}% ({}/{})",
                    s.kind.label(),
                    (s.next as u32 * 100 / s.num as u32).min(100),
                    s.next,
                    s.num
                );
            }
            if s.next >= s.num {
                println!(
                    "[模拟器] {}升级完成：{} 个分包全部接收",
                    s.kind.label(),
                    s.num
                );
                return Ok(());
            }
            s.next += 1;
            let req = build_file_request(s.kind, state, s.uid, s.next);
            sock.send_to(&req, src).await?;
            *session = Some(s);
        }
        other => {
            if handle_state_command(state, other, payload) {
                // 状态变更后回发最新设备信息，让工具表格立即刷新
                let reply = state.reply_packet();
                sock.send_to(&reply, src).await?;
            }
        }
    }
    Ok(())
}

/// 处理会改变设备状态的控制指令，返回是否需要回包
fn handle_state_command(state: &mut DeviceState, reg: u16, payload: &[u8]) -> bool {
    match reg {
        REG_BASE_INFO if payload.len() >= 23 => {
            let mut off = 16; // 跳过 appid
            state.hotel_id = u32::from_be_bytes(payload[off..off + 4].try_into().unwrap());
            off += 4;
            state.build_num = payload[off];
            state.floor_num = payload[off + 1];
            state.room_num = payload[off + 2];
            state.door_model = payload[off + 3];
            println!(
                "[模拟器] 基础信息已更新: 酒店={} 栋={} 层={} 房={} 户型={}",
                state.hotel_id, state.build_num, state.floor_num, state.room_num, state.door_model
            );
            true
        }
        REG_NET_CONFIG if payload.len() >= 59 => {
            state.ip_flag = payload[0];
            state.ip = Ipv4Addr::new(payload[1], payload[2], payload[3], payload[4]);
            state.mask = Ipv4Addr::new(payload[5], payload[6], payload[7], payload[8]);
            state.gateway = Ipv4Addr::new(payload[9], payload[10], payload[11], payload[12]);
            // payload[13..19] 为 MAC，FFFFFFFFFFFF 表示保持不变
            if payload[13..19] != [0xFF; 6] {
                state.mac.copy_from_slice(&payload[13..19]);
            }
            state.server_flag = payload[19];
            state.url.copy_from_slice(&payload[20..52]);
            state.server_ip = Ipv4Addr::new(payload[52], payload[53], payload[54], payload[55]);
            state.server_port = u16::from_be_bytes([payload[56], payload[57]]);
            println!(
                "[模拟器] 网络配置已更新: ip={} mask={} gw={} server={}:{}",
                state.ip, state.mask, state.gateway, state.server_ip, state.server_port
            );
            true
        }
        other => {
            println!(
                "[模拟器] 收到未识别报文 reg=0x{other:04X} len={}",
                payload.len()
            );
            false
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut dev_ip = Ipv4Addr::new(192, 168, 134, 88);
    let mut dev_port: u16 = 4668;
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--dev-ip" => {
                i += 1;
                dev_ip = args[i].parse()?;
            }
            "--dev-port" => {
                i += 1;
                dev_port = args[i].parse()?;
            }
            _ => {}
        }
        i += 1;
    }

    let mut state = DeviceState {
        ip: dev_ip,
        ..Default::default()
    };
    let mut session: Option<UpgradeSession> = None;

    // 发现端口：响应广播/单播 0xFE00
    let find_sock = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, FIND_PORT)).await?;
    // 指令端口：接收工具下发（与工具绑定顺延的起始端口一致）
    let cmd_sock = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, dev_port)).await?;

    println!("==========================================");
    println!(" 肯天 RCU 设备模拟器已启动");
    println!(" 发现端口: {FIND_PORT}  指令端口: {dev_port}");
    println!(" 模拟设备: {} @ {dev_ip}", hex::encode_upper(state.equip_id));
    println!(" 支持：发现应答 / 指令回显 / 固件与配置升级拉包");
    println!(" 请启动主程序并选择同一网段网卡进入普通扫描");
    println!("==========================================");

    let mut find_buf = vec![0u8; 4096];
    let mut cmd_buf = vec![0u8; 4096];
    loop {
        tokio::select! {
            res = find_sock.recv_from(&mut find_buf) => {
                let (len, src) = res?;
                handle_datagram(&find_sock, &find_buf, len, src, &mut state, &mut session).await?;
            }
            res = cmd_sock.recv_from(&mut cmd_buf) => {
                let (len, src) = res?;
                handle_datagram(&cmd_sock, &cmd_buf, len, src, &mut state, &mut session).await?;
            }
        }
    }
}
