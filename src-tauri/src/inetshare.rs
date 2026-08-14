//! 网络中继（互联网共享）服务：把源网卡（如 Wi-Fi）的网络共享给目标网口，
//! 让网线直连设备获得真实互联网。
//!
//! 实现复用系统原生共享能力，不自行做 NAT：
//! - Windows：ICS（Internet 连接共享），设备分配 192.168.137.x
//! - macOS：互联网共享（Internet Sharing），设备分配 192.168.2.x
//! 两者均自带 DHCP，因此与内置 DHCP 服务器互斥（同一网口双 DHCP 会互相干扰）。
//!
//! 特权操作由 `--inet-share` 助手进程完成（提权拉起），主程序保持普通权限；
//! 助手内置 90 秒心跳看门狗——主程序退出/崩溃后自动停止共享恢复原样。

use crate::events;
use serde::Serialize;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// READY/ERR 控制端口（主程序侧）与助手控制端口（与 DHCP 中继 46367/46368 错开）
pub const INET_APP_PORT: u16 = 46370;
pub const INET_HELPER_PORT: u16 = 46371;

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct InetShareView {
    pub running: bool,
    pub src: String,
    pub dst: String,
}

struct Inner {
    running: bool,
    src: String,
    dst: String,
    /// 提权拉起的助手续约进程句柄（osascript/Start-Process 包装层）
    helper: Option<std::process::Child>,
    /// 绑定 INET_APP_PORT，接收助手 READY/ERR
    sock: Option<UdpSocket>,
    heartbeat_cancel: Option<CancellationToken>,
    monitor_cancel: Option<CancellationToken>,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            running: false,
            src: String::new(),
            dst: String::new(),
            helper: None,
            sock: None,
            heartbeat_cancel: None,
            monitor_cancel: None,
        }
    }
}

#[derive(Default)]
pub struct InetShareService {
    inner: Mutex<Inner>,
}

fn emit_log(app: &AppHandle, msg: String) {
    let _ = app.emit(events::UDP_LOG, msg);
}

impl InetShareService {
    pub async fn is_running(&self) -> bool {
        self.inner.lock().await.running
    }

    /// 同步版运行状态（窗口关闭拦截用；抢锁失败保守放行关闭）
    pub fn is_running_sync(&self) -> bool {
        self.inner.try_lock().map(|i| i.running).unwrap_or(false)
    }

    fn view(inner: &Inner) -> InetShareView {
        InetShareView {
            running: inner.running,
            src: inner.src.clone(),
            dst: inner.dst.clone(),
        }
    }

    pub async fn emit_status(&self, app: &AppHandle) {
        let inner = self.inner.lock().await;
        let _ = app.emit(events::INET_STATUS, Self::view(&inner));
    }

    async fn start_inner(
        &self,
        app: &AppHandle,
        src: String,
        dst: String,
    ) -> Result<(), String> {
        if src.trim().is_empty() || dst.trim().is_empty() {
            return Err("请选择源网卡和目标网口".into());
        }
        if src == dst {
            return Err("源网卡与目标网口不能相同".into());
        }
        {
            let inner = self.inner.lock().await;
            if inner.running {
                return Err("网络中继已在运行中".into());
            }
        }

        let relay_sock = UdpSocket::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, INET_APP_PORT)))
            .await
            .map_err(|e| format!("绑定中继控制端口失败: {e}"))?;
        let exe = std::env::current_exe()
            .map_err(|e| format!("获取程序路径失败: {e}"))?
            .display()
            .to_string();
        let args = [
            "--inet-share".to_string(),
            INET_APP_PORT.to_string(),
            INET_HELPER_PORT.to_string(),
            src.clone(),
            dst.clone(),
        ];
        let child = crate::elevate::spawn_elevated(&exe, &args)?
            .ok_or_else(|| "拉起提权进程失败".to_string())?;

        // 等待助手就绪（包含用户输入密码的时间）；ERR: 前缀为助手回传的具体失败原因
        let mut buf = vec![0u8; 1600];
        let deadline = std::time::Instant::now() + Duration::from_secs(120);
        let mut child = Some(child);
        let ready = loop {
            if std::time::Instant::now() >= deadline {
                break Err("等待中继助手就绪超时，请重试".to_string());
            }
            if let Some(c) = child.as_mut() {
                if let Ok(Some(_)) = c.try_wait() {
                    break Err("未获得管理员授权或中继助手启动失败，请重试".to_string());
                }
            }
            match tokio::time::timeout(Duration::from_secs(1), relay_sock.recv_from(&mut buf)).await
            {
                Ok(Ok((len, _))) if len >= 5 && &buf[..5] == b"READY" => break Ok(()),
                Ok(Ok((len, _))) if len >= 4 && &buf[..4] == b"ERR:" => {
                    break Err(String::from_utf8_lossy(&buf[4..len]).to_string());
                }
                Ok(_) => continue,
                Err(_) => continue,
            }
        };
        ready.map_err(|e| {
            // 启动失败：杀掉包装进程（助手若已起，看门狗会自行退出还原）
            if let Some(mut c) = child.take() {
                let _ = c.kill();
            }
            e
        })?;

        // 标记文件：被 kill/断电后据此检测残留并恢复
        crate::nicstate::update(app, |s| {
            s.inet_src = src.clone();
            s.inet_dst = dst.clone();
        });

        let hb_cancel = CancellationToken::new();
        let mon_cancel = CancellationToken::new();
        {
            let mut inner = self.inner.lock().await;
            inner.running = true;
            inner.src = src.clone();
            inner.dst = dst.clone();
            inner.helper = child;
            inner.sock = Some(relay_sock);
            inner.heartbeat_cancel = Some(hb_cancel.clone());
            inner.monitor_cancel = Some(mon_cancel.clone());
        }
        self.emit_status(app).await;
        emit_log(
            app,
            format!("网络中继已开启：{src} → {dst}，设备经系统共享获得地址后即可上网"),
        );

        // 心跳：20 秒一次（助手看门狗 90 秒），维持助手存活
        let helper_addr: SocketAddr = (Ipv4Addr::LOCALHOST, INET_HELPER_PORT).into();
        tokio::spawn(async move {
            let sock = match UdpSocket::bind("127.0.0.1:0").await {
                Ok(s) => s,
                Err(_) => return,
            };
            let mut cmd = [0u8; 6];
            cmd[5] = 1; // 端口 1 = 心跳
            loop {
                tokio::select! {
                    _ = hb_cancel.cancelled() => break,
                    _ = tokio::time::sleep(Duration::from_secs(20)) => {
                        let _ = sock.send_to(&cmd, helper_addr).await;
                    }
                }
            }
        });

        // 源网卡断线监控：Wi-Fi 掉线仅告警，恢复联网后自动续网
        let app_mon = app.clone();
        tokio::spawn(async move {
            let mut warned = false;
            loop {
                tokio::select! {
                    _ = mon_cancel.cancelled() => break,
                    _ = tokio::time::sleep(Duration::from_secs(15)) => {}
                }
                match crate::netif::adapter_ipv4(&src) {
                    None if !warned => {
                        warned = true;
                        emit_log(
                            &app_mon,
                            format!("警告：中继源网卡 {src} 当前无网络（Wi-Fi 断线？），设备暂时无法上网，网络恢复后自动续网"),
                        );
                    }
                    Some(_) if warned => {
                        warned = false;
                        emit_log(&app_mon, format!("中继源网卡 {src} 网络已恢复，设备上网恢复"));
                    }
                    _ => {}
                }
            }
        });
        Ok(())
    }

    async fn stop_inner(&self, app: &AppHandle, silent: bool) {
        let view;
        {
            let mut inner = self.inner.lock().await;
            if !inner.running {
                return;
            }
            if let Some(c) = inner.heartbeat_cancel.take() {
                c.cancel();
            }
            if let Some(c) = inner.monitor_cancel.take() {
                c.cancel();
            }
            // 向助手发退出指令（目标端口 0），助手停止共享后自行退出
            if let Some(sock) = inner.sock.take() {
                if let Ok(std_sock) = sock.into_std() {
                    let cmd = [0u8; 6];
                    let helper: SocketAddr = (Ipv4Addr::LOCALHOST, INET_HELPER_PORT).into();
                    let _ = std_sock.send_to(&cmd, helper);
                }
            }
            // 等待助手完成「停止共享」（ICS COM 调用需数秒），超时兜底 kill
            if let Some(mut child) = inner.helper.take() {
                for _ in 0..40 {
                    if matches!(child.try_wait(), Ok(Some(_))) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
                let _ = child.kill();
            }
            inner.running = false;
            inner.src.clear();
            inner.dst.clear();
            view = Self::view(&inner);
        }
        // 干净停止：清除标记文件中继部分
        crate::nicstate::update(app, |s| {
            s.inet_src.clear();
            s.inet_dst.clear();
        });
        let _ = app.emit(events::INET_STATUS, view);
        if !silent {
            emit_log(app, "网络中继已停止，网络配置已恢复".into());
        }
    }

    /// 应用退出清理（同步、尽力而为）：发退出指令并短暂等待助手停止共享。
    /// 即使此路径失败，助手 90 秒看门狗也会兜底恢复原样
    pub fn exit_cleanup(&self, app: &AppHandle) {
        let Ok(mut inner) = self.inner.try_lock() else {
            return;
        };
        if !inner.running {
            return;
        }
        if let Some(c) = inner.heartbeat_cancel.take() {
            c.cancel();
        }
        if let Some(c) = inner.monitor_cancel.take() {
            c.cancel();
        }
        if let Some(sock) = inner.sock.take() {
            if let Ok(std_sock) = sock.into_std() {
                let cmd = [0u8; 6];
                let helper: SocketAddr = (Ipv4Addr::LOCALHOST, INET_HELPER_PORT).into();
                let _ = std_sock.send_to(&cmd, helper);
            }
        }
        if let Some(mut child) = inner.helper.take() {
            for _ in 0..40 {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            let _ = child.kill();
        }
        inner.running = false;
        // 已发出停止指令，同步清除标记文件中继部分
        crate::nicstate::update(app, |s| {
            s.inet_src.clear();
            s.inet_dst.clear();
        });
    }
}

#[tauri::command]
pub async fn start_inet_share(
    app: AppHandle,
    state: State<'_, crate::state::AppState>,
    src: String,
    dst: String,
) -> Result<(), String> {
    if state.dhcp.is_running().await {
        return Err("内置 DHCP 正在运行（与中继互斥），请先在设置页停止 DHCP".into());
    }
    state.inetshare.start_inner(&app, src, dst).await
}

#[tauri::command]
pub async fn stop_inet_share(
    app: AppHandle,
    state: State<'_, crate::state::AppState>,
) -> Result<(), String> {
    state.inetshare.stop_inner(&app, false).await;
    Ok(())
}

#[tauri::command]
pub async fn get_inet_share_status(
    state: State<'_, crate::state::AppState>,
) -> Result<InetShareView, String> {
    let inner = state.inetshare.inner.lock().await;
    Ok(InetShareService::view(&inner))
}
