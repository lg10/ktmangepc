//! 伪互联网服务：让 DHCP 直连设备「以为能上网」，跳过联网自检快速进入本地发现
//!
//! 背景：设备通过本机 DHCP 拿到 134.x 地址后，固件通常会先做联网自检
//! （DNS 解析 / HTTP 探测云平台），失败后反复重试直到超时（数十秒级），
//! 期间不响应本地发现广播，表现为「DHCP 模式下扫描要等很久」。
//! 本模块绑定 192.168.134.1 的 53/80 端口（仅该地址，不影响主机其他网络）：
//! - DNS：任意域名 A 查询应答为本机 134.1（把后续 HTTP 探测引到本机）
//! - HTTP：任意请求返回 200，模拟云平台可达
//! 绑定失败（无权限/端口占用）仅告警不阻断。

use crate::dhcp::SUBNET_IP;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

pub struct FakeInternet {
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl FakeInternet {
    /// 启动伪 DNS / 伪 HTTP 服务；log 用于把设备探测行为上报到日志面板
    pub fn start<L>(log: L) -> Self
    where
        L: Fn(String) + Send + Sync + 'static,
    {
        let stop = Arc::new(AtomicBool::new(false));
        let log = Arc::new(log);
        let mut threads = Vec::new();

        // 伪 DNS：53/UDP
        let addr53 = SocketAddr::from((*SUBNET_IP, 53));
        match UdpSocket::bind(addr53) {
            Ok(sock) => {
                let _ = sock.set_read_timeout(Some(Duration::from_secs(1)));
                let (stop, log) = (stop.clone(), log.clone());
                threads.push(std::thread::spawn(move || {
                    dns_loop(sock, stop, log);
                }));
            }
            Err(e) => log(format!("伪互联网：绑定 {addr53} 失败（{e}），DNS 模拟不可用")),
        }

        // 伪 HTTP：80/TCP
        let addr80 = SocketAddr::from((*SUBNET_IP, 80));
        match TcpListener::bind(addr80) {
            Ok(l) => {
                let _ = l.set_nonblocking(true);
                let (stop, log) = (stop.clone(), log.clone());
                threads.push(std::thread::spawn(move || {
                    http_loop(l, stop, log);
                }));
            }
            Err(e) => log(format!("伪互联网：绑定 {addr80} 失败（{e}），HTTP 模拟不可用")),
        }

        FakeInternet { stop, threads }
    }
}

impl Drop for FakeInternet {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

/// 解析 DNS 查询并应答：A 记录返回本机 134.1，其余类型返回空应答（NODATA）
fn dns_loop(sock: UdpSocket, stop: Arc<AtomicBool>, log: Arc<dyn Fn(String) + Send>) {
    let mut buf = [0u8; 512];
    loop {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let Ok((n, src)) = sock.recv_from(&mut buf) else {
            continue;
        };
        let Some(resp) = build_dns_reply(&buf[..n], log.as_ref()) else {
            continue;
        };
        let _ = sock.send_to(&resp, src);
    }
}

fn build_dns_reply(q: &[u8], log: &dyn Fn(String)) -> Option<Vec<u8>> {
    if q.len() < 12 {
        return None;
    }
    let qdcount = u16::from_be_bytes([q[4], q[5]]) as usize;
    if qdcount < 1 {
        return None;
    }
    // 解析第一个问题的域名与类型
    let mut i = 12;
    let mut name = String::new();
    while i < q.len() {
        let len = q[i] as usize;
        if len == 0 {
            i += 1;
            break;
        }
        if len & 0xC0 == 0xC0 {
            return None; // 查询中不应出现压缩指针，保守放弃
        }
        if i + 1 + len > q.len() {
            return None;
        }
        if !name.is_empty() {
            name.push('.');
        }
        name.push_str(&String::from_utf8_lossy(&q[i + 1..i + 1 + len]));
        i += 1 + len;
    }
    if i + 4 > q.len() {
        return None;
    }
    let qtype = u16::from_be_bytes([q[i], q[i + 1]]);
    log(format!("伪互联网：设备查询 DNS {name}（type {qtype}）"));

    let mut r = Vec::with_capacity(q.len() + 16);
    r.extend_from_slice(&q[..2]); // transaction id
    r.extend_from_slice(&0x8180u16.to_be_bytes()); // qr=1 aa=1 rcode=0
    r.extend_from_slice(&q[4..8]); // qdcount/ancount 占位，下面覆写 ancount
    r[6..8].copy_from_slice(&1u16.to_be_bytes()); // ancount=1（A 查询时）
    r.extend_from_slice(&q[12..i + 4]); // 问题段原样回显
    if qtype == 1 {
        // A 记录应答：指向本机，把设备后续 HTTP 探测引到伪 HTTP 服务
        r.extend_from_slice(&[0xC0, 0x0C]); // 名字指针
        r.extend_from_slice(&1u16.to_be_bytes()); // type A
        r.extend_from_slice(&1u16.to_be_bytes()); // class IN
        r.extend_from_slice(&60u32.to_be_bytes()); // ttl
        r.extend_from_slice(&4u16.to_be_bytes()); // rdlength
        r.extend_from_slice(&SUBNET_IP.octets());
    } else {
        // 非 A 查询（如 AAAA）：NODATA 空应答
        r[6..8].copy_from_slice(&0u16.to_be_bytes());
    }
    Some(r)
}

/// 伪 HTTP：读取请求头后统一返回 200，模拟云平台可达
fn http_loop(l: TcpListener, stop: Arc<AtomicBool>, log: Arc<dyn Fn(String) + Send>) {
    const RESP: &[u8] =
        b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";
    loop {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let Ok((mut stream, _)) = l.accept() else {
            std::thread::sleep(Duration::from_millis(200));
            continue;
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let mut head = [0u8; 2048];
        let mut filled = 0usize;
        // 读到请求头结束或缓冲满/超时，避免设备不发包时阻塞
        while filled < head.len() {
            match stream.read(&mut head[filled..]) {
                Ok(0) => break,
                Ok(n) => {
                    filled += n;
                    if head[..filled].windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let text = String::from_utf8_lossy(&head[..filled]);
        let first = text.lines().next().unwrap_or_default().to_string();
        let host = text
            .lines()
            .find_map(|ln| {
                let ln = ln.trim();
                ln.to_lowercase()
                    .strip_prefix("host:")
                    .map(|h| h.trim().to_string())
            })
            .unwrap_or_default();
        log(format!("伪互联网：设备 HTTP 探测 {first}（Host: {host}）"));
        let _ = stream.write_all(RESP);
        let _ = stream.shutdown(std::net::Shutdown::Both);
    }
}
