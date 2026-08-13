//! 肯天 RCU UDP 协议栈
//!
//! 对应原 C# 项目 `Tool/UdpServer/NewProtocols`：
//! - 12 字节大端 Header：serial(2) + deviceType(2) + dataLen(2) + slaveAddr(1) + funcCode(1) + regAddr(2) + regNum(2)
//! - 寄存器地址表 RegisterAddress
//! - 设备信息载荷 UdpModel 解析
//! - 下行报文构造（网络配置 / 基础信息 / UdpRevert 指令）

use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;

pub const HEADER_LEN: usize = 12;

/// 寄存器地址（与原 RegisterAddress.cs 一致）
#[allow(dead_code)]
pub mod reg {
    pub const UDP_SEARCH: u16 = 0xFE00;
    pub const UDP_REVERT: u16 = 0xFE01;
    pub const NET_WORK_CONFIGURE: u16 = 0xFE10;
    pub const READ_NET_WORK_CONFIGURE: u16 = 0xFE11;
    pub const UDP_SEND_BASE_INFO: u16 = 0xFE12;
    pub const UDP_READ_BASE_INFO: u16 = 0xFE13;
    pub const SEND_RCU_START_UPDATE: u16 = 0x02F2;
    pub const REQUEST_UPDATE_FILE: u16 = 0x02F3;
    pub const SEND_RCU_UPDATE_FILE: u16 = 0x02F4;
    pub const SEND_RCU_START_CONFIG: u16 = 0xFD01;
    pub const REQUEST_CONFIG_FILE: u16 = 0xFD02;
    pub const SEND_RCU_CONFIG_FILE: u16 = 0xFD03;
    pub const SEND_AUTH: u16 = 0xF102;
}

pub const FUNC_READ_RESP: u8 = 0x03;
pub const FUNC_READ_RESP2: u8 = 0x63;
pub const FUNC_WRITE: u8 = 0x10;

/// 协议头（12 字节，大端序）
#[derive(Debug, Clone, Copy)]
pub struct Header {
    pub serial: u16,
    pub device_type: u16,
    pub data_len: u16,
    pub slave_addr: u8,
    pub func_code: u8,
    pub reg_addr: u16,
    pub reg_num: u16,
}

impl Header {
    pub fn parse(buf: &[u8]) -> Option<Header> {
        if buf.len() < HEADER_LEN {
            return None;
        }
        Some(Header {
            serial: u16::from_be_bytes([buf[0], buf[1]]),
            device_type: u16::from_be_bytes([buf[2], buf[3]]),
            data_len: u16::from_be_bytes([buf[4], buf[5]]),
            slave_addr: buf[6],
            func_code: buf[7],
            reg_addr: u16::from_be_bytes([buf[8], buf[9]]),
            reg_num: u16::from_be_bytes([buf[10], buf[11]]),
        })
    }

    pub fn to_bytes(&self) -> [u8; HEADER_LEN] {
        let mut b = [0u8; HEADER_LEN];
        b[0..2].copy_from_slice(&self.serial.to_be_bytes());
        b[2..4].copy_from_slice(&self.device_type.to_be_bytes());
        b[4..6].copy_from_slice(&self.data_len.to_be_bytes());
        b[6] = self.slave_addr;
        b[7] = self.func_code;
        b[8..10].copy_from_slice(&self.reg_addr.to_be_bytes());
        b[10..12].copy_from_slice(&self.reg_num.to_be_bytes());
        b
    }
}

/// 原 GetSendHeader 映射：下行报文头构造
/// (serial, deviceType, dataLen, slaveAddr, funcCode, regAddr, regNum)
pub fn send_header(reg_addr: u16) -> Header {
    match reg_addr {
        reg::UDP_REVERT => Header {
            serial: 1,
            device_type: 1,
            data_len: 0,
            slave_addr: 0,
            func_code: FUNC_WRITE,
            reg_addr,
            reg_num: 40,
        },
        reg::NET_WORK_CONFIGURE => Header {
            serial: 1,
            device_type: 1,
            data_len: 0,
            slave_addr: 0,
            func_code: FUNC_WRITE,
            reg_addr,
            reg_num: 100,
        },
        reg::UDP_SEND_BASE_INFO => Header {
            serial: 1,
            device_type: 1,
            data_len: 0,
            slave_addr: 0,
            func_code: FUNC_WRITE,
            reg_addr,
            reg_num: 100,
        },
        _ => Header {
            serial: 1,
            device_type: 1,
            data_len: 0,
            slave_addr: 0,
            func_code: FUNC_WRITE,
            reg_addr,
            reg_num: 40,
        },
    }
}

/// 原 GetSendHeader 映射：下行报文寄存器字数（regNum）
pub fn reg_num_of(reg_addr: u16) -> u16 {
    match reg_addr {
        reg::NET_WORK_CONFIGURE | reg::UDP_SEND_BASE_INFO => 100,
        _ => 40,
    }
}

/// 组包（与原 sendMessage 线格式完全一致）：
/// serial=regNum、dataLen=regNum+14、载荷补齐/截断至 regNum 字节、尾部追加 0D0A
pub fn build_packet_dyn(reg_addr: u16, reg_num: u16, payload_hex: &str) -> Result<Vec<u8>, String> {
    let payload = hex::decode(payload_hex).map_err(|e| format!("载荷 HEX 非法: {e}"))?;
    let mut body = vec![0u8; reg_num as usize];
    let n = payload.len().min(reg_num as usize);
    body[..n].copy_from_slice(&payload[..n]);
    let mut out = Vec::with_capacity(HEADER_LEN + reg_num as usize + 2);
    let header = Header {
        serial: reg_num,
        device_type: 1,
        data_len: reg_num + 14,
        slave_addr: 0,
        func_code: FUNC_WRITE,
        reg_addr,
        reg_num,
    };
    out.extend_from_slice(&header.to_bytes());
    out.extend_from_slice(&body);
    out.extend_from_slice(b"\r\n");
    Ok(out)
}

/// 组包：Header + hex 载荷 + "0D0A" 结尾（固定寄存器用默认 regNum）
pub fn build_packet(reg_addr: u16, payload_hex: &str) -> Result<Vec<u8>, String> {
    build_packet_dyn(reg_addr, reg_num_of(reg_addr), payload_hex)
}

/// 0x02F2 固件升级首指令组包：在通用组包基础上，
/// 将数据区正数第 37 个字节（第一个字节从 1 开始数）设置为 1（设备侧协议要求）
pub fn build_start_update_packet(payload_hex: &str) -> Result<Vec<u8>, String> {
    let mut pkt = build_packet_dyn(reg::SEND_RCU_START_UPDATE, 40, payload_hex)?;
    // 报文前 12 字节为 Header，数据区第 37 字节 = 偏移 HEADER_LEN + 36
    pkt[HEADER_LEN + 36] = 1;
    Ok(pkt)
}

/// 设备发现广播包（原 UdpFind.cs，目标端口 4667）
pub fn find_packet() -> Vec<u8> {
    hex::decode("0002000100220010FE0000144B696E67696E745263758F933D6D0000000000000D0A")
        .expect("内置发现报文 HEX 恒定合法")
}

/// 设备型号映射（原 equipmentModel short）
pub fn model_name(code: u16) -> String {
    match code {
        0x009F => "大板机".into(),
        0x00AF => "无线网关".into(),
        0x00BF => "模块机".into(),
        0x00CF => "LORA主机".into(),
        0x00DF => "ZIGBEE主机".into(),
        0x00EF => "MINI主机".into(),
        0x008F => "模块一体机".into(),
        0x007F => "模块机【24款】".into(),
        other => format!("未知型号(0x{other:04X})"),
    }
}

/// 授权方式/状态显示（与原 NetWorkMsg.cs 一致）
pub fn author_text(way: u8, state: u8) -> String {
    let way_s = match way {
        4 => "[永久]",
        2 => "[到期]",
        _ => "[未知]",
    };
    let state_s = if state == 0 { "[正常]" } else { "[过期]" };
    format!("{way_s}\n{state_s}")
}

/// 版本号：protocolVersion(2 字节 HEX).hardVersion.softVersion(换字节)，与原显示一致
pub fn version_text(protocol_version: u16, hard_version: u8, soft_version: u16) -> String {
    let soft = soft_version.swap_bytes();
    format!("{protocol_version:04X}.{hard_version}.{soft}")
}

/// 端口字段按“每字节十进制”拼接显示（如 0x2B26 → 4338，与原 NetWorkMsg 一致）
fn port_text(port: u16) -> String {
    format!("{}{}", port >> 8, port & 0xFF)
}

fn read_ipv4(p: &[u8], off: usize) -> Ipv4Addr {
    Ipv4Addr::new(p[off], p[off + 1], p[off + 2], p[off + 3])
}

fn read_ascii(p: &[u8], off: usize, len: usize) -> String {
    let end = off + len.min(p.len().saturating_sub(off));
    let slice = &p[off..end];
    let cut = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
    String::from_utf8_lossy(&slice[..cut]).trim().to_string()
}

/// 设备信息载荷（原 UdpModel.cs 字段顺序）
#[derive(Debug, Clone)]
pub struct UdpModel {
    pub equip_id: String,
    pub equipment_model: u16,
    pub protocol_version: u16,
    pub soft_version: u16,
    pub hard_version: u8,
    pub authorization_way: u8,
    pub authorization_state: u8,
    pub hotel_id: u32,
    pub build_num: u8,
    pub floor_num: u8,
    pub room_num: u8,
    pub door_model: u8,
    pub rcu_ip_flag: u8,
    pub rcu_ip: Ipv4Addr,
    pub rcu_mask: Ipv4Addr,
    pub rcu_gateway: Ipv4Addr,
    pub rcu_mac: String,
    pub server_flag: u8,
    pub url: String,
    pub server_ip: Ipv4Addr,
    pub server_port: u16,
    pub rcu_dns: Ipv4Addr,
    pub run_server_flag: u8,
    pub run_url: String,
    pub run_ip: Ipv4Addr,
    pub run_port: u16,
}

impl UdpModel {
    /// 解析 Header 之后的二进制载荷（布局与原 UdpModel.cs 一致：protocolVersion 2 字节、含 rcuDns）
    pub fn parse(p: &[u8]) -> Option<UdpModel> {
        // 最小长度 = 8+2+2+2+1+1+1+4+1+1+1+1+1+4+4+4+6+1+32+4+2+4+1+32+4+2 = 125
        if p.len() < 125 {
            return None;
        }
        let mut off = 0usize;
        let equip_id = hex::encode_upper(&p[off..off + 8]);
        off += 8;
        let equipment_model = u16::from_be_bytes([p[off], p[off + 1]]);
        off += 2;
        let protocol_version = u16::from_be_bytes([p[off], p[off + 1]]);
        off += 2;
        let soft_version = u16::from_be_bytes([p[off], p[off + 1]]);
        off += 2;
        let hard_version = p[off];
        off += 1;
        let authorization_way = p[off];
        off += 1;
        let authorization_state = p[off];
        off += 1;
        let hotel_id = u32::from_be_bytes([p[off], p[off + 1], p[off + 2], p[off + 3]]);
        off += 4;
        let build_num = p[off];
        off += 1;
        let floor_num = p[off];
        off += 1;
        let room_num = p[off];
        off += 1;
        let door_model = p[off];
        off += 1;
        let rcu_ip_flag = p[off];
        off += 1;
        let rcu_ip = read_ipv4(p, off);
        off += 4;
        let rcu_mask = read_ipv4(p, off);
        off += 4;
        let rcu_gateway = read_ipv4(p, off);
        off += 4;
        let rcu_mac = hex::encode_upper(&p[off..off + 6]);
        off += 6;
        let server_flag = p[off];
        off += 1;
        let url = read_ascii(p, off, 32);
        off += 32;
        let server_ip = read_ipv4(p, off);
        off += 4;
        let server_port = u16::from_be_bytes([p[off], p[off + 1]]);
        off += 2;
        let rcu_dns = read_ipv4(p, off);
        off += 4;
        let run_server_flag = p[off];
        off += 1;
        let run_url = read_ascii(p, off, 32);
        off += 32;
        let run_ip = read_ipv4(p, off);
        off += 4;
        let run_port = u16::from_be_bytes([p[off], p[off + 1]]);

        Some(UdpModel {
            equip_id,
            equipment_model,
            protocol_version,
            soft_version,
            hard_version,
            authorization_way,
            authorization_state,
            hotel_id,
            build_num,
            floor_num,
            room_num,
            door_model,
            rcu_ip_flag,
            rcu_ip,
            rcu_mask,
            rcu_gateway,
            rcu_mac,
            server_flag,
            url,
            server_ip,
            server_port,
            rcu_dns,
            run_server_flag,
            run_url,
            run_ip,
            run_port,
        })
    }

    /// 设置服务器显示：flag=1 用 IP，否则域名；端口按字节十进制拼接；附 DNS（与原显示一致）
    pub fn server_text(&self) -> String {
        let head = if self.server_flag == 1 {
            format!("【IP方式】{}:{}", self.server_ip, port_text(self.server_port))
        } else {
            format!("【域名方式】{}:{}", self.url, port_text(self.server_port))
        };
        format!("{head}\n【DNS】{}", self.rcu_dns)
    }

    pub fn run_server_text(&self) -> String {
        if self.run_ip.octets()[0] == 0 {
            "未连接到服务器".into()
        } else {
            format!("{}:{}", self.run_ip, port_text(self.run_port))
        }
    }

    /// 网络配置显示：flag=1 静态，否则 DHCP；附 网关/掩码（与原显示一致）
    pub fn network_text(&self) -> String {
        let head = if self.rcu_ip_flag == 1 { "【静态】" } else { "【DHCP】" };
        format!("{head}{}\n【网/掩】{}/{}", self.rcu_ip, self.rcu_gateway, self.rcu_mask)
    }

    pub fn base_num_text(&self) -> String {
        format!(
            "{}/{}/{}/{}/{}",
            self.hotel_id, self.build_num, self.floor_num, self.room_num, self.door_model
        )
    }
}

/// 前端契约类型（与 src/types/index.ts RcuDevice 对齐）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RcuDeviceView {
    pub equip_id: String,
    pub room_num: String,
    pub room_type_name: String,
    pub build_name: String,
    pub floor_name: String,
    pub rcu_mac: String,
    pub equipment_model: String,
    pub version: String,
    pub author: String,
    pub base_num: String,
    pub network: String,
    pub ip: String,
    pub server: String,
    pub run_server: String,
    pub count: String,
    pub last_seen: u64,
}

/// 网络配置下发参数（与前端 NetworkConfig 对齐）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkConfigDto {
    pub ip_flag: u8,
    pub ip: String,
    pub mask: String,
    pub gateway: String,
    pub dns: String,
    pub server_flag: u8,
    pub server_url: String,
    pub server_ip: String,
    pub server_port: u16,
}

fn ipv4_hex(s: &str) -> Result<String, String> {
    let ip: Ipv4Addr = s.parse().map_err(|_| format!("IP 非法: {s}"))?;
    Ok(hex::encode_upper(ip.octets()))
}

/// 网络配置下发载荷（原 NetworkDialog，逐字段对齐）：
/// ipFlag + ip + mask + gateway + FFFFFFFFFFFF + serverFlag + url+09补齐32 + serverIp
/// + port[2]（每字节十进制：高字节=port/100、低字节=port%100，与端口显示映射互逆） + dns
pub fn build_network_payload(cfg: &NetworkConfigDto) -> Result<String, String> {
    let mut s = String::with_capacity(128);
    s.push_str(&format!("{:02X}", cfg.ip_flag));
    s.push_str(&ipv4_hex(&cfg.ip)?);
    s.push_str(&ipv4_hex(&cfg.mask)?);
    s.push_str(&ipv4_hex(&cfg.gateway)?);
    s.push_str("FFFFFFFFFFFF"); // MAC 保持不变
    s.push_str(&format!("{:02X}", cfg.server_flag));
    // 原 buildUrl(url,1)：url ASCII 后追加 0x09 结尾符，再补 0 至 32 字节
    let url_bytes = cfg.server_url.as_bytes();
    if url_bytes.len() > 31 {
        return Err("服务器域名超过 31 字节".into());
    }
    let mut url_buf = [0u8; 32];
    url_buf[..url_bytes.len()].copy_from_slice(url_bytes);
    url_buf[url_bytes.len()] = 0x09;
    s.push_str(&hex::encode_upper(url_buf));
    s.push_str(&ipv4_hex(&cfg.server_ip)?);
    if cfg.server_port >= 10000 {
        return Err("端口必须小于 10000（按每字节十进制编码）".into());
    }
    s.push_str(&format!(
        "{:02X}{:02X}",
        (cfg.server_port / 100) as u8,
        (cfg.server_port % 100) as u8
    ));
    s.push_str(&ipv4_hex(&cfg.dns)?);
    Ok(s)
}

/// 基础信息下发载荷（原 BaseInfoDialog）：
/// appid(32 hex) + hotelId(X8) + buildNum(X2) + floorNum(X2) + roomNum(X2) + doorModel(X2)
pub fn build_base_info_payload(
    hotel_id: u32,
    build_num: u8,
    floor_num: u8,
    room_num: u8,
    door_model: u8,
) -> String {
    format!(
        "{}{:08X}{:02X}{:02X}{:02X}{:02X}",
        "54a750a0d76d34d8ddfe81a5cf81c74e", hotel_id, build_num, floor_num, room_num, door_model
    )
}

/// UdpRevert 指令载荷：cmd 占寄存器首字
pub fn build_revert_payload(cmd: u8) -> String {
    format!("{:02X}00", cmd)
}

/// 固件升级首指令载荷（原 UpdateFileDialog.StartButton，0x02F2）：
/// "0000" + hotelFlag(X2) + v0(X2) + v2(X4) + v1(X2) + updateTime(X8) + fileId(X4) + total(X8) + num(X4) + md5
/// 注：原实现将 v0 裸拼接，单数字版本会产生奇数长度 hex 导致字节错位，此处修正为 X2
pub fn build_start_update_payload(
    hotel_id: u32,
    version: &str,
    update_time: u64,
    file_id: u16,
    total_len: u64,
    num: u16,
    md5: &str,
) -> Result<String, String> {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() < 3 {
        return Err(format!("固件版本号格式非法（需 x.y.z）: {version}"));
    }
    let v0: u8 = u8::from_str_radix(parts[0], 10).map_err(|_| format!("版本号主段非法: {version}"))?;
    let v1: u8 = u8::from_str_radix(parts[1], 10).map_err(|_| format!("版本号次段非法: {version}"))?;
    let v2: u16 = parts[2]
        .parse()
        .map_err(|_| format!("版本号修订段非法: {version}"))?;
    Ok(format!(
        "0000{:02X}{:02X}{:04X}{:02X}{:08X}{:04X}{:08X}{:04X}{}",
        if hotel_id == 0 { 0u8 } else { 1u8 },
        v0,
        v2,
        v1,
        update_time,
        file_id,
        total_len,
        num,
        md5
    ))
}

/// 配置下发首指令载荷（原 ConfigFileDialog.StartButton，0xFD01）：
/// fileId(X4) + total(X8) + num(X4) + md5 + "07" + version(X4) + updateTime(X8)
pub fn build_start_config_payload(
    file_id: u16,
    total_len: u64,
    num: u16,
    md5: &str,
    version: &str,
    update_time: u64,
) -> Result<String, String> {
    // 原实现 Convert.ToInt32(version)，用 u32 保持取值范围一致
    let ver: u32 = version
        .parse()
        .map_err(|_| format!("配置版本号非法: {version}"))?;
    Ok(format!(
        "{:04X}{:08X}{:04X}{}{:02X}{:04X}{:08X}",
        file_id, total_len, num, md5, 7u8, ver, update_time
    ))
}

/// 设备拉取分包请求载荷（0x02F3 / 0xFD02，原 UpdateFileModel）：
/// equipmentModel(2) + equipId(8) + uid(2) + dataIndex(2)
pub struct FileRequest {
    pub equip_id: String,
    pub uid: u16,
    pub data_index: u16,
}

pub fn parse_file_request(p: &[u8]) -> Option<FileRequest> {
    if p.len() < 14 {
        return None;
    }
    Some(FileRequest {
        equip_id: hex::encode_upper(&p[2..10]),
        uid: u16::from_be_bytes([p[10], p[11]]),
        data_index: u16::from_be_bytes([p[12], p[13]]),
    })
}

/// CRC16（原 AuthBatch.getCRC：初值 0xFFFF、多项式 0xA001 反射）
pub fn crc16_a001(bytes: &[u8]) -> u16 {
    let mut crc: u32 = 0x0000FFFF;
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc >>= 1;
                crc ^= 0x0000A001;
            } else {
                crc >>= 1;
            }
        }
    }
    crc as u16
}

/// XOR 校验（原 AuthBatch.getXORCheck）
pub fn xor_check(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0u8, |acc, &b| acc ^ b)
}

/// 批量授权载荷（原 AuthBatch.sendAuthTime，0xF102）：
/// 当前时间(6B) + "01" + 授权方式(04永久/02限期) + 截止时间(6B) + 开机时长(X4) + 剩余时长(X4) + 随机(X2) + XOR(X2) + CRC16(X4)
/// 校验基于 key(0x3039) + 内容
pub fn build_auth_payload(
    now: (u16, u8, u8, u8, u8, u8),
    permanent: bool,
    deadline: (u16, u8, u8, u8, u8, u8),
    random_byte: u8,
) -> String {
    const KEY: &str = "3039"; // 12345 的 X4
    let boot_hours: u16 = 90 * 24;
    let mut content = format!(
        "01{}{:04X}{:02X}{:02X}{:02X}{:02X}{:02X}{:04X}{:04X}{:02X}",
        if permanent { "04" } else { "02" },
        deadline.0,
        deadline.1,
        deadline.2,
        deadline.3,
        deadline.4,
        deadline.5,
        boot_hours,
        boot_hours,
        random_byte
    );
    let xor_src = hex::decode(format!("{KEY}{content}")).expect("授权 HEX 构造恒合法");
    let xor = xor_check(&xor_src);
    content.push_str(&format!("{:02X}", xor));
    let crc_src = hex::decode(format!("{KEY}{content}")).expect("授权 HEX 构造恒合法");
    let crc = crc16_a001(&crc_src);
    content.push_str(&format!("{:04X}", crc));
    format!(
        "{:04X}{:02X}{:02X}{:02X}{:02X}{:02X}{}",
        now.0, now.1, now.2, now.3, now.4, now.5, content
    )
}

/// 授权下发组包：原 GetSendHeader 无 SendAuth 分支，走 default → 线上寄存器为
/// 0xFE12（基础信息）、regNum=100，此处忠实复刻该行为，勿改为 0xF102
pub fn build_auth_packet(payload_hex: &str) -> Result<Vec<u8>, String> {
    build_packet_dyn(reg::UDP_SEND_BASE_INFO, 100, payload_hex)
}

/// 设备回复报文中取出载荷区（跳过 12 字节 Header，按 dataLen 截断，容忍 0D0A 尾）
pub fn payload_of<'a>(buf: &'a [u8], header: &Header) -> &'a [u8] {
    let start = HEADER_LEN;
    if buf.len() <= start {
        return &[];
    }
    let len = if header.data_len > 0 {
        (header.data_len as usize).min(buf.len() - start)
    } else {
        buf.len() - start
    };
    // 去除可能的 0D0A 结尾
    let mut end = start + len;
    if end >= start + 2 && &buf[end - 2..end] == b"\r\n" {
        end -= 2;
    }
    &buf[start..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_roundtrip() {
        let h = Header {
            serial: 2,
            device_type: 1,
            data_len: 0x22,
            slave_addr: 0,
            func_code: 0x10,
            reg_addr: reg::UDP_SEARCH,
            reg_num: 20,
        };
        let b = h.to_bytes();
        let p = Header::parse(&b).unwrap();
        assert_eq!(p.reg_addr, 0xFE00);
        assert_eq!(p.data_len, 0x22);
    }

    #[test]
    fn find_packet_len() {
        // 12 字节 Header + 20 字节载荷(regNum=0x14) + 0D0A
        assert_eq!(find_packet().len(), 34);
    }

    #[test]
    fn build_packet_wire_format() {
        // 原 sendMessage：serial=regNum、dataLen=regNum+14、载荷补齐至 regNum、尾 0D0A
        let pkt = build_packet(reg::UDP_REVERT, "0100").unwrap();
        assert_eq!(pkt.len(), 12 + 40 + 2);
        let h = Header::parse(&pkt).unwrap();
        assert_eq!(h.serial, 40);
        assert_eq!(h.data_len, 54);
        assert_eq!(h.reg_num, 40);
        assert_eq!(&pkt[pkt.len() - 2..], b"\r\n");
    }

    #[test]
    fn auth_payload_structure() {
        // 原 AuthBatch 注释金样本：07e802170d0927 01 02 07e805170d0927 0870 0870 7b af 5653
        let p = build_auth_payload(
            (0x07E8, 2, 0x17, 0x0D, 9, 0x27),
            false,
            (0x07E8, 5, 0x17, 0x0D, 9, 0x27),
            0x7B,
        );
        // 7B 时间 + 1 标记 + 1 方式 + 7B 截止 + 2+2 时长 + 1 随机 + 1 XOR + 2 CRC = 24 字节 = 48 hex
        assert_eq!(p.len(), 48);
        assert_eq!(p, "07E802170D0927010207E805170D0927087008707BAF5653");
        // 再用另一组参数重算 XOR/CRC 验证自洽
        let q = build_auth_payload((0x07EA, 7, 23, 1, 2, 3), true, (0x0C58, 5, 23, 1, 2, 3), 0x7B);
        let key_body = &q[14..q.len() - 6];
        let src = hex::decode(format!("3039{key_body}")).unwrap();
        assert_eq!(format!("{:02X}", xor_check(&src)), &q[q.len() - 6..q.len() - 4]);
        let src2 = hex::decode(format!("3039{key_body}{:02X}", xor_check(&src))).unwrap();
        assert_eq!(format!("{:04X}", crc16_a001(&src2)), &q[q.len() - 4..]);
    }

    #[test]
    fn network_payload_layout() {
        let cfg = NetworkConfigDto {
            ip_flag: 0,
            ip: "192.168.1.100".into(),
            mask: "255.255.255.0".into(),
            gateway: "192.168.1.1".into(),
            dns: "8.8.8.8".into(),
            server_flag: 1,
            server_url: "".into(),
            server_ip: "192.168.0.61".into(),
            server_port: 4338,
        };
        let p = build_network_payload(&cfg).unwrap();
        // ipFlag + 3 IP + MAC + serverFlag + url(32) + ip + port + dns = 1+12+6+1+32+4+2+4 = 62B
        assert_eq!(p.len(), 124);
        // url 区自 hex 索引 40 起：空域名 → 首字节为 09 结尾符，其余补 0
        assert_eq!(&p[40..42], "09");
        assert_eq!(&p[42..44], "00");
        // 端口 4338 → 每字节十进制 43/38 = 0x2B26（与显示映射 port_text 互逆）
        assert_eq!(&p[112..116], "2B26");
    }

    #[test]
    fn auth_packet_wire_register() {
        // 原 default 分支：线上寄存器 0xFE12、regNum=100（并非逻辑地址 0xF102）
        let p = build_auth_payload(
            (0x07E8, 2, 0x17, 0x0D, 9, 0x27),
            false,
            (0x07E8, 5, 0x17, 0x0D, 9, 0x27),
            0x7B,
        );
        let pkt = build_auth_packet(&p).unwrap();
        let h = Header::parse(&pkt).unwrap();
        assert_eq!(h.reg_addr, reg::UDP_SEND_BASE_INFO);
        assert_eq!(h.reg_num, 100);
        assert_eq!(pkt.len(), 12 + 100 + 2);
    }

    #[test]
    fn start_update_payload_len() {
        let md5 = "a".repeat(32);
        let p = build_start_update_payload(1395, "2.3.10", 0x66B0F000, 0x00AB, 123456, 121, &md5).unwrap();
        // 2+1+1+2+1+4+2+4+2+16 = 35 字节 = 70 hex
        assert_eq!(p.len(), 70);
    }

    #[test]
    fn start_update_packet_byte37() {
        // 0x02F2 报文：数据区正数第 37 字节（从 1 数）必须为 1
        let md5 = "a".repeat(32);
        let p = build_start_update_payload(1, "1.0.0", 0, 1, 100, 1, &md5).unwrap();
        let pkt = build_start_update_packet(&p).unwrap();
        assert_eq!(pkt[HEADER_LEN + 36], 1);
        // 其余补位字节不受影响
        assert_eq!(pkt[HEADER_LEN + 35], 0);
        assert_eq!(pkt[HEADER_LEN + 38], 0);
    }
}
