//! ADB 终端：内置 adb 的完整系统 shell 会话
//!
//! 使用 portable-pty 提供真 PTY（Ctrl+C / 行编辑 / 颜色等完整交互）：
//! - macOS/Linux 启动用户 $SHELL（缺省 /bin/zsh），Windows 启动 cmd.exe
//! - PATH 首位注入打包内置的 platform-tools 目录，直接敲 adb 即用
//! - 单会话、关闭即销毁；输出以 base64 事件透传，终端渲染交给前端 xterm.js

use crate::events;
use base64::Engine;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Serialize;
use std::io::{Read, Write};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex;

const SESSION_ID: &str = "adb";
const DEFAULT_ROWS: u16 = 32;
const DEFAULT_COLS: u16 = 120;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdbDataPayload {
    id: String,
    data: String, // base64
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdbClosedPayload {
    id: String,
    reason: String,
}

struct ShellSession {
    writer: Box<dyn Write + Send>,
    killer: Box<dyn portable_pty::ChildKiller + Send>,
}

#[derive(Default)]
pub struct AdbShellService {
    session: Mutex<Option<ShellSession>>,
}

impl AdbShellService {
    /// 打开 shell 会话（已存在时幂等返回）
    pub async fn open(&self, app: &AppHandle) -> Result<(), String> {
        {
            let session = self.session.lock().await;
            if session.is_some() {
                return Ok(());
            }
        }

        let adb_dir = resolve_adb_dir(app)?;
        let shell = shell_program();

        let mut cmd = CommandBuilder::new(&shell);
        if let Some(home) = home_dir() {
            cmd.cwd(home);
        }
        // PATH 首位注入内置 adb 目录（Windows 下为 Path）
        let path_var = if cfg!(windows) { "Path" } else { "PATH" };
        let old_path = std::env::var(path_var).unwrap_or_default();
        let sep = if cfg!(windows) { ";" } else { ":" };
        cmd.env(path_var, format!("{}{}{}", adb_dir.display(), sep, old_path));

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: DEFAULT_ROWS,
                cols: DEFAULT_COLS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("终端创建失败: {e}"))?;

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("启动 {shell} 失败: {e}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("终端写入端创建失败: {e}"))?;
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("终端读取端创建失败: {e}"))?;

        {
            let mut session = self.session.lock().await;
            *session = Some(ShellSession {
                writer,
                killer: child.clone_killer(),
            });
        }

        // 读循环：PTY 输出 → base64 事件；读到 EOF（shell 退出）后通知前端
        let app2 = app.clone();
        std::thread::spawn(move || {
            let mut buf = vec![0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let payload = AdbDataPayload {
                            id: SESSION_ID.to_string(),
                            data: base64::engine::general_purpose::STANDARD.encode(&buf[..n]),
                        };
                        let _ = app2.emit(events::ADB_DATA, payload);
                    }
                    Err(_) => break,
                }
            }
            let svc = app2.state::<crate::state::AppState>().adbshell.clone();
            let mut session = svc.session.blocking_lock();
            *session = None;
            drop(session);
            let _ = app2.emit(
                events::ADB_CLOSED,
                AdbClosedPayload {
                    id: SESSION_ID.to_string(),
                    reason: "会话已结束".into(),
                },
            );
        });
        Ok(())
    }

    /// 写入终端（前端已按原样发送按键字节）
    pub async fn write(&self, data: Vec<u8>) -> Result<(), String> {
        let mut session = self.session.lock().await;
        let session = session
            .as_mut()
            .ok_or_else(|| "终端会话不存在".to_string())?;
        session
            .writer
            .write_all(&data)
            .map_err(|e| format!("写入终端失败: {e}"))?;
        session
            .writer
            .flush()
            .map_err(|e| format!("写入终端失败: {e}"))?;
        Ok(())
    }

    /// 关闭并销毁会话（杀进程）
    pub async fn close(&self) {
        let mut session = self.session.lock().await;
        if let Some(mut s) = session.take() {
            let _ = s.killer.kill();
        }
    }
}

/// 定位内置 adb 所在资源目录（打包：Resources/adb；dev：resources/adb）
pub(crate) fn resolve_adb_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let exe = if cfg!(windows) { "adb/adb.exe" } else { "adb/adb" };
    let path: std::path::PathBuf = app
        .path()
        .resolve(exe, tauri::path::BaseDirectory::Resource)
        .map_err(|e| format!("定位内置 adb 失败: {e}"))?;
    if !path.exists() {
        return Err(format!("内置 adb 不存在: {}", path.display()));
    }
    path.parent()
        .map(|p: &std::path::Path| p.to_path_buf())
        .ok_or_else(|| "内置 adb 路径异常".into())
}

fn shell_program() -> String {
    if cfg!(windows) {
        "cmd.exe".to_string()
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string())
    }
}

fn home_dir() -> Option<String> {
    std::env::var("HOME").ok().or_else(|| std::env::var("USERPROFILE").ok())
}

#[tauri::command]
pub async fn adb_shell_open(
    app: AppHandle,
    state: State<'_, crate::state::AppState>,
) -> Result<(), String> {
    state.adbshell.open(&app).await
}

#[tauri::command]
pub async fn adb_shell_write(
    state: State<'_, crate::state::AppState>,
    data: Vec<u8>,
) -> Result<(), String> {
    state.adbshell.write(data).await
}

#[tauri::command]
pub async fn adb_shell_close(state: State<'_, crate::state::AppState>) -> Result<(), String> {
    state.adbshell.close().await;
    Ok(())
}
