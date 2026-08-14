//! DHCP 中继助手（root/管理员权限运行，由主程序以提权方式拉起）
//!
//! 背景：macOS 上 WKWebView 无法在 root 进程中运行，整个 GUI 应用提权重启会闪退；
//! 因此只有需要特权的部分（绑定 67 端口、配置网卡 IP）放到本助手进程，GUI 保持普通权限。
//!
//! 用法：`kt-mange-pc --dhcp-relay <app_port> <helper_port> <nic_name>`
//! - 绑定 0.0.0.0:67，把收到的 BOOTP 包加 6 字节源地址头（IP4+端口BE）转发到 127.0.0.1:app_port
//! - 监听 127.0.0.1:helper_port，接收主程序的回包（6 字节目标地址头 + BOOTP）从 67 端口发出
//! - 目标端口 0 = 退出指令（移除自动添加的网卡地址后退出），1 = 心跳
//! - 主程序心跳丢失超过 90 秒自动退出并还原网卡配置

use crate::dhcp::{ensure_subnet_ip, nic_dhcp_enabled, remove_subnet_ip};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const IDLE_LIMIT_SECS: u64 = 90;

/// Windows 防火墙入站规则管理：默认入站策略会丢弃局域网发来的 UDP 67，
/// 助手以管理员运行，启动时加规则、退出时移除（按名称幂等）
#[cfg(windows)]
fn firewall_rule(add: bool) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let args: Vec<String> = if add {
        let exe = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let mut v: Vec<String> = [
            "advfirewall", "firewall", "add", "rule", "name", "KT Device Scan DHCP",
            "dir", "in", "action", "allow", "protocol", "UDP", "localport", "67",
            "program",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        v.push(exe);
        v.extend(["enable", "yes"].iter().map(|s| s.to_string()));
        v
    } else {
        vec![
            "advfirewall",
            "firewall",
            "delete",
            "rule",
            "name",
            "KT Device Scan DHCP",
        ]
        .into_iter()
        .map(String::from)
        .collect()
    };
    let _ = std::process::Command::new("netsh")
        .args(&args)
        .creation_flags(CREATE_NO_WINDOW)
        .output();
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 助手中继主循环；返回进程退出码
pub fn run_relay(app_port: u16, helper_port: u16, nic: &str) -> i32 {
    let mut bind_retry = 0u8;
    // 1. 网卡 192.168.134.1 配置（助手以特权运行，退出时还原本次新增的地址）；
    // 先记录添加前的 DHCP 模式：Windows 增删辅助地址可能把网卡切成静态，退出时据此恢复
    let was_dhcp = nic_dhcp_enabled(nic);
    let nic_added = match ensure_subnet_ip(nic) {
        Ok(added) => added,
        Err(e) => {
            eprintln!("[dhcp-relay] 网卡配置警告: {e}");
            false
        }
    };
    let cleanup = |added: bool, nic: &str| {
        if added {
            remove_subnet_ip(nic, was_dhcp);
        }
    };

    // 2. 特权端口与控制端口；旧助手退出释放 67 需要数秒，
    // 立即重开 DHCP 时重试绑定吸收该竞态窗口
    let s67 = loop {
        match UdpSocket::bind("0.0.0.0:67") {
            Ok(s) => break s,
            Err(e) => {
                let kind = e.kind();
                if kind == std::io::ErrorKind::AddrInUse && bind_retry < 8 {
                    bind_retry += 1;
                    std::thread::sleep(Duration::from_secs(1));
                    continue;
                }
                eprintln!("[dhcp-relay] 绑定 67 端口失败: {e}");
                cleanup(nic_added, nic);
                return 1;
            }
        }
    };
    // 回包需广播到 255.255.255.255（客户端尚未取得 IP），未设置该选项时发送会静默失败
    let _ = s67.set_broadcast(true);
    #[cfg(windows)]
    firewall_rule(true);
    let hs = match UdpSocket::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, helper_port))) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[dhcp-relay] 绑定控制端口失败: {e}");
            cleanup(nic_added, nic);
            return 1;
        }
    };
    let app_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, app_port));
    // 就绪信号（主程序据此确认提权成功且端口就绪）
    let _ = hs.send_to(b"READY", app_addr);
    // 伪互联网服务（DNS/HTTP 模拟）：助手以特权运行可绑 53/80，
    // 让设备联网自检快速通过，缩短 DHCP 模式下发现等待；进程退出时自动停止
    let _fake = crate::fakeinet::FakeInternet::start(|m| eprintln!("[dhcp-relay] {m}"));

    let done = Arc::new(AtomicBool::new(false));
    let last_seen = Arc::new(AtomicU64::new(now_secs()));

    // 上行线程用克隆句柄，主循环保留原句柄发下行包
    let s67_up = s67
        .try_clone()
        .expect("克隆 67 端口 socket 失败");
    let hs_up = hs
        .try_clone()
        .expect("克隆控制端口 socket 失败");

    // 3. 上行：客户端 BOOTP -> 加源地址头 -> 主程序
    let (d1, done1) = (done.clone(), done.clone());
    let t_up = std::thread::spawn(move || {
        let mut buf = vec![0u8; 1600];
        // 读超时 + 循环检查 done，保证主循环退出后本线程能及时结束（join 不悬挂）
        s67_up
            .set_read_timeout(Some(Duration::from_secs(1)))
            .ok();
        loop {
            if d1.load(Ordering::Relaxed) {
                break;
            }
            match s67_up.recv_from(&mut buf) {
                Ok((len, src)) => {
                    let (ip, port) = match src {
                        SocketAddr::V4(v4) => (v4.ip().octets(), v4.port()),
                        _ => continue,
                    };
                    let mut out = Vec::with_capacity(6 + len);
                    out.extend_from_slice(&ip);
                    out.extend_from_slice(&port.to_be_bytes());
                    out.extend_from_slice(&buf[..len]);
                    let _ = hs_up.send_to(&out, app_addr);
                }
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    continue
                }
                Err(_) => break,
            }
        }
        done1.store(true, Ordering::Relaxed);
    });

    // 4. 下行：主程序回包/指令 -> 按目标地址从 67 端口发出
    let mut buf = vec![0u8; 1600];
    loop {
        if done.load(Ordering::Relaxed) {
            break;
        }
        // 心跳看门狗：主程序退出/崩溃后自动还原并退出
        if now_secs().saturating_sub(last_seen.load(Ordering::Relaxed)) > IDLE_LIMIT_SECS {
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
        let mut ip = [0u8; 4];
        ip.copy_from_slice(&buf[..4]);
        let port = u16::from_be_bytes([buf[4], buf[5]]);
        match port {
            0 => break, // 退出指令
            1 => continue, // 心跳
            _ => {
                let dst: SocketAddr = SocketAddrV4::new(Ipv4Addr::from(ip), port).into();
                let _ = s67.send_to(&buf[6..len], dst);
            }
        }
    }
    // 通知上行线程退出，避免 join 悬挂
    done.store(true, Ordering::Relaxed);
    let _ = t_up.join();
    #[cfg(windows)]
    firewall_rule(false);
    cleanup(nic_added, nic);
    0
}