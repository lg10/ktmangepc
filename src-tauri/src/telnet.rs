//! Telnet 终端服务：多会话 TCP 透传
//!
//! 修复原 TelnetDialog 的缺陷（输出渲染被整体注释、KeyUp 逻辑导致整行重复发送）：
//! - 后端只负责 TCP 会话管理与字节透传，终端渲染交给前端 xterm.js
//! - 会话以 equipId 为键，支持多设备并存、单独关闭
//! - 数据事件 telnet://data 载荷为 base64，保持二进制透明

use crate::events;
use base64::Engine;
use serde::Serialize;
use std::collections::HashMap;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

pub const DEFAULT_PORT: u16 = 23;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TelnetSessionView {
    pub id: String,
    pub ip: String,
    pub port: u16,
    pub connected: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TelnetDataPayload {
    id: String,
    data: String, // base64
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TelnetClosedPayload {
    id: String,
    reason: String,
}

struct Session {
    ip: String,
    port: u16,
    writer: OwnedWriteHalf,
}

#[derive(Default)]
struct TelnetInner {
    sessions: HashMap<String, Session>,
}

#[derive(Default)]
pub struct TelnetService {
    inner: Mutex<TelnetInner>,
}

impl TelnetService {
    /// 连接会话（同 id 已连接时直接复用）
    pub async fn connect(&self, app: &AppHandle, id: String, ip: String, port: u16) -> Result<(), String> {
        {
            let inner = self.inner.lock().await;
            if inner.sessions.contains_key(&id) {
                return Ok(());
            }
        }
        let addr = format!("{ip}:{port}");
        let stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(&addr))
            .await
            .map_err(|_| format!("连接超时: {addr}"))?
            .map_err(|e| format!("连接失败 {addr}: {e}"))?;
        let _ = stream.set_nodelay(true);
        let (reader, writer) = stream.into_split();

        {
            let mut inner = self.inner.lock().await;
            inner.sessions.insert(
                id.clone(),
                Session {
                    ip: ip.clone(),
                    port,
                    writer,
                },
            );
        }

        // 读循环：字节透传 → base64 事件
        let app2 = app.clone();
        let id2 = id.clone();
        tokio::spawn(async move {
            read_loop(&app2, &id2, reader).await;
            // 连接断开：清理会话并通知前端
            {
                let svc = app2.state::<crate::state::AppState>().telnet.clone();
                let mut inner = svc.inner.lock().await;
                inner.sessions.remove(&id2);
            }
            let _ = app2.emit(
                events::TELNET_CLOSED,
                TelnetClosedPayload {
                    id: id2,
                    reason: "连接已断开".into(),
                },
            );
        });
        Ok(())
    }

    /// 写入数据（原样透传，不做行处理）
    pub async fn write(&self, id: &str, data: Vec<u8>) -> Result<(), String> {
        let mut inner = self.inner.lock().await;
        let session = inner
            .sessions
            .get_mut(id)
            .ok_or_else(|| format!("终端会话不存在: {id}"))?;
        session
            .writer
            .write_all(&data)
            .await
            .map_err(|e| format!("发送失败: {e}"))?;
        session
            .writer
            .flush()
            .await
            .map_err(|e| format!("发送失败: {e}"))?;
        Ok(())
    }

    pub async fn close(&self, id: &str) {
        let mut inner = self.inner.lock().await;
        inner.sessions.remove(id);
    }

    pub async fn list(&self) -> Vec<TelnetSessionView> {
        let inner = self.inner.lock().await;
        inner
            .sessions
            .iter()
            .map(|(id, s)| TelnetSessionView {
                id: id.clone(),
                ip: s.ip.clone(),
                port: s.port,
                connected: true,
            })
            .collect()
    }
}

async fn read_loop(app: &AppHandle, id: &str, mut reader: OwnedReadHalf) {
    let mut buf = vec![0u8; 4096];
    loop {
        match reader.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                let payload = TelnetDataPayload {
                    id: id.to_string(),
                    data: base64::engine::general_purpose::STANDARD.encode(&buf[..n]),
                };
                let _ = app.emit(events::TELNET_DATA, payload);
            }
            Err(_) => break,
        }
    }
}

#[tauri::command]
pub async fn telnet_connect(
    app: AppHandle,
    state: State<'_, crate::state::AppState>,
    id: String,
    ip: String,
    port: Option<u16>,
) -> Result<(), String> {
    if id.trim().is_empty() || ip.trim().is_empty() {
        return Err("会话 ID 与 IP 不能为空".into());
    }
    state
        .telnet
        .connect(&app, id, ip, port.unwrap_or(DEFAULT_PORT))
        .await
}

#[tauri::command]
pub async fn telnet_write(
    state: State<'_, crate::state::AppState>,
    id: String,
    data: Vec<u8>,
) -> Result<(), String> {
    state.telnet.write(&id, data).await
}

#[tauri::command]
pub async fn telnet_close(state: State<'_, crate::state::AppState>, id: String) -> Result<(), String> {
    state.telnet.close(&id).await;
    Ok(())
}

#[tauri::command]
pub async fn telnet_list(
    state: State<'_, crate::state::AppState>,
) -> Result<Vec<TelnetSessionView>, String> {
    Ok(state.telnet.list().await)
}
