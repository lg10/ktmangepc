//! 入住机 mDNS 发现服务：浏览 `_kingint-kcd._tcp.local.`
//!
//! 入住机（com.kingint.checkin4）启动后在局域网广播 DNS-SD 服务（端口 7271，
//! 无 TXT 记录）。后端只负责持续发现与在场维护，设备详情（型号/版本等）由
//! 前端登录设备管理接口（/api/auth/login → /api/device/info）自行获取。

use crate::events;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::Serialize;
use std::collections::HashMap;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex;

pub const SERVICE_TYPE: &str = "_kingint-kcd._tcp.local.";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckinDevice {
    /// mDNS 实例全名（多台同名设备时 Android NSD 自动加后缀，可作唯一键）
    pub full_name: String,
    /// 服务实例名（如 checkin / checkin (2)）
    pub name: String,
    pub ip: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckinStatus {
    pub running: bool,
    pub device_count: usize,
}

#[derive(Default)]
struct CheckinInner {
    running: bool,
    devices: HashMap<String, CheckinDevice>,
    daemon: Option<ServiceDaemon>,
}

#[derive(Default)]
pub struct CheckinService {
    inner: Mutex<CheckinInner>,
}

impl CheckinService {
    /// 启动持续发现（已运行时幂等）
    pub async fn start(&self, app: &AppHandle) -> Result<(), String> {
        {
            let inner = self.inner.lock().await;
            if inner.running {
                return Ok(());
            }
        }
        let daemon = ServiceDaemon::new().map_err(|e| format!("mDNS 服务启动失败: {e}"))?;
        let receiver = daemon
            .browse(SERVICE_TYPE)
            .map_err(|e| format!("mDNS 浏览启动失败: {e}"))?;

        {
            let mut inner = self.inner.lock().await;
            inner.daemon = Some(daemon);
            inner.running = true;
        }
        self.emit_status(app).await;

        // 独立线程阻塞接收事件（daemon 关闭后通道断开，循环自然退出）
        let app2 = app.clone();
        std::thread::spawn(move || {
            while let Ok(event) = receiver.recv() {
                match event {
                    ServiceEvent::ServiceResolved(info) => {
                        let Some(device) = to_device(&info) else { continue };
                        let svc = app2.state::<crate::state::AppState>().checkin.clone();
                        let inserted = {
                            let mut inner = svc.inner.blocking_lock();
                            if !inner.running {
                                false
                            } else {
                                inner.devices.insert(device.full_name.clone(), device.clone());
                                true
                            }
                        };
                        if inserted {
                            let _ = app2.emit(events::CHECKIN_DEVICE, device);
                            let _ = svc.emit_status_sync(&app2);
                        }
                    }
                    ServiceEvent::ServiceRemoved(_, full_name) => {
                        let svc = app2.state::<crate::state::AppState>().checkin.clone();
                        let removed = {
                            let mut inner = svc.inner.blocking_lock();
                            inner.devices.remove(&full_name).is_some()
                        };
                        if removed {
                            let _ = app2.emit(events::CHECKIN_DEVICE_OFFLINE, full_name);
                            let _ = svc.emit_status_sync(&app2);
                        }
                    }
                    _ => {}
                }
            }
        });
        Ok(())
    }

    /// 停止发现并清空列表
    pub async fn stop(&self, app: &AppHandle) {
        let daemon = {
            let mut inner = self.inner.lock().await;
            inner.running = false;
            inner.devices.clear();
            inner.daemon.take()
        };
        if let Some(d) = daemon {
            // shutdown 后事件通道断开，接收线程退出
            let _ = d.shutdown();
        }
        self.emit_status(app).await;
    }

    pub async fn status(&self) -> CheckinStatus {
        let inner = self.inner.lock().await;
        CheckinStatus {
            running: inner.running,
            device_count: inner.devices.len(),
        }
    }

    pub async fn list(&self) -> Vec<CheckinDevice> {
        let inner = self.inner.lock().await;
        inner.devices.values().cloned().collect()
    }

    async fn emit_status(&self, app: &AppHandle) {
        let _ = app.emit(events::CHECKIN_STATUS, self.status().await);
    }

    /// 供事件接收线程（非 async 上下文）使用
    fn emit_status_sync(&self, app: &AppHandle) -> Result<(), String> {
        let inner = self.inner.blocking_lock();
        let status = CheckinStatus {
            running: inner.running,
            device_count: inner.devices.len(),
        };
        app.emit(events::CHECKIN_STATUS, status)
            .map_err(|e| e.to_string())
    }
}

/// 从解析结果提取首个 IPv4 地址构造设备视图；无 IPv4 时丢弃
fn to_device(info: &ServiceInfo) -> Option<CheckinDevice> {
    let ip = info
        .get_addresses()
        .iter()
        .find(|a| a.is_ipv4())
        .map(|a| a.to_string())?;
    // fullName 形如 checkin._kingint-kcd._tcp.local.，实例名为第一段
    let name = info
        .get_fullname()
        .split('.')
        .next()
        .unwrap_or(info.get_fullname())
        .to_string();
    Some(CheckinDevice {
        full_name: info.get_fullname().to_string(),
        name,
        ip,
        port: info.get_port(),
    })
}

#[tauri::command]
pub async fn checkin_start(
    app: AppHandle,
    state: State<'_, crate::state::AppState>,
) -> Result<(), String> {
    state.checkin.start(&app).await
}

#[tauri::command]
pub async fn checkin_stop(
    app: AppHandle,
    state: State<'_, crate::state::AppState>,
) -> Result<(), String> {
    state.checkin.stop(&app).await;
    Ok(())
}

#[tauri::command]
pub async fn checkin_status(
    state: State<'_, crate::state::AppState>,
) -> Result<CheckinStatus, String> {
    Ok(state.checkin.status().await)
}

#[tauri::command]
pub async fn checkin_list(
    state: State<'_, crate::state::AppState>,
) -> Result<Vec<CheckinDevice>, String> {
    Ok(state.checkin.list().await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// 回环验证：用 mdns-sd 注册一个假入住机服务，确认浏览能解析到
    #[test]
    fn browse_finds_registered_service() {
        let daemon = ServiceDaemon::new().expect("mDNS daemon");
        // mdns-sd 默认禁用 loopback 接口，回环测试需显式启用
        daemon
            .enable_interface(mdns_sd::IfKind::LoopbackV4)
            .expect("enable loopback");
        let service = ServiceInfo::new(
            SERVICE_TYPE,
            "checkin-test",
            "checkin-test.local.",
            "127.0.0.1",
            7271,
            None,
        )
        .expect("service info");
        daemon.register(service).expect("register");
        let receiver = daemon.browse(SERVICE_TYPE).expect("browse");
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut found = false;
        while Instant::now() < deadline {
            match receiver.recv_timeout(Duration::from_millis(500)) {
                Ok(ServiceEvent::ServiceResolved(info))
                    if info.get_fullname().starts_with("checkin-test") =>
                {
                    assert_eq!(info.get_port(), 7271);
                    found = true;
                    break;
                }
                _ => {}
            }
        }
        let _ = daemon.shutdown();
        assert!(found, "应能发现本机注册的 _kingint-kcd._tcp 服务（若失败请确认系统已授予本地网络权限）");
    }
}
