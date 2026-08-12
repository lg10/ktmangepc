//! 登录模块：扫码登录（与 kt-uniapp-mange3.0-pc pages/index 同源）
//!
//! - 前端生成 12 位随机 code，调 build_login_code 获取二维码内容并在本地渲染
//! - 二维码内容 substring(6,18) 为轮询 key（与原 H5 实现一致）
//! - 后端每 1.5 秒轮询 /kingint/UmsAdmin/watchLoginCode?key=<key>
//!   data=no 继续；wait/refuse/error 推送 auth://qr-state；长度 > 20 的 JSON 字符串为登录结果
//! - 登录态持久化于 AppData 下 login.json（异步 + 取消令牌）

use crate::events;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

const WATCH_URL: &str = "https://mange.kingint.com/kingint/UmsAdmin/watchLoginCode";
const BUILD_URL: &str = "https://mange.kingint.com/kingint/UmsAdmin/buildLoginCode";
const USER_DETAILS_URL: &str = "https://mange.kingint.com/kingint/UmsAdmin/getUserDetails";
const POLL_INTERVAL: Duration = Duration::from_millis(1500);
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);

pub struct AuthService {
    /// 登录态（serde_json::Value 保留云端完整字段）
    user: Mutex<Option<Value>>,
    /// 当前轮询取消令牌
    polling: Mutex<Option<CancellationToken>>,
    http: reqwest::Client,
}

impl Default for AuthService {
    fn default() -> Self {
        Self {
            user: Mutex::new(None),
            polling: Mutex::new(None),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("HTTP 客户端初始化失败"),
        }
    }
}

fn login_file(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|p| p.join("login.json"))
}

impl AuthService {
    /// 启动时从本地恢复登录态
    pub fn load(&self, app: &AppHandle) {
        let Some(file) = login_file(app) else { return };
        if let Ok(content) = std::fs::read_to_string(&file) {
            if let Ok(v) = serde_json::from_str::<Value>(&content) {
                self.user.try_lock().map(|mut g| *g = Some(v)).ok();
            }
        }
    }

    pub async fn get_login_state(&self) -> Option<Value> {
        self.user.lock().await.clone()
    }

    /// 每次启动拉取云端最新用户信息，验证登录态是否仍有效（避免过期未及时退出）
    ///
    /// 仅当云端明确拒绝（未认证/过期）才返回 Err 触发登出；
    /// 网络抖动/解析失败等暂时性错误直接沿用本地登录态，避免误登出
    pub async fn refresh_user(&self, app: &AppHandle) -> Result<Value, String> {
        let user = self.get_login_state().await.ok_or("无本地登录态")?;
        let token = user
            .get("token")
            .and_then(|t| t.as_str())
            .filter(|s| !s.is_empty())
            .ok_or("登录信息缺少 token")?
            .to_string();
        let resp = match self
            .http
            .get(USER_DETAILS_URL)
            .header("Authorization", &token)
            .send()
            .await
        {
            Ok(r) => r,
            // 网络异常：沿用本地登录态（后续联网接口仍会校验过期）
            Err(_) => return Ok(user),
        };
        let status = resp.status();
        let body: Value = match resp.json().await {
            Ok(v) => v,
            Err(_) => return Ok(user),
        };
        let ok = body.get("success").and_then(|s| s.as_bool()) == Some(true);
        if !ok {
            // 云端明确判定未认证/过期才登出；其他错误码视为暂时性异常，沿用本地登录态
            let code = body.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
            if status.as_u16() == 401 || code == 400001 || code == 401 {
                return Err("登录已过期，请重新扫码登录".into());
            }
            return Ok(user);
        }
        // 云端 success=true 但无 data 属异常响应，同样沿用本地登录态
        let Some(data) = body.get("data").filter(|d| d.is_object()) else {
            return Ok(user);
        };
        // 云端未回传 token 时保留本地 token，保证后续接口可用
        let mut new_user = data.clone();
        if new_user
            .get("token")
            .and_then(|t| t.as_str())
            .filter(|s| !s.is_empty())
            .is_none()
        {
            if let Some(o) = new_user.as_object_mut() {
                o.insert("token".into(), Value::String(token));
            }
        }
        self.save_user(app, new_user.clone()).await;
        Ok(new_user)
    }

    pub fn http_client(&self) -> &reqwest::Client {
        &self.http
    }

    pub fn is_logged_in(&self) -> bool {
        self.user
            .try_lock()
            .map(|g| g.is_some())
            .unwrap_or(false)
    }

    pub async fn save_user(&self, app: &AppHandle, user: Value) {
        if let Some(file) = login_file(app) {
            let _ = std::fs::create_dir_all(file.parent().unwrap());
            let _ = std::fs::write(&file, serde_json::to_string_pretty(&user).unwrap_or_default());
        }
        *self.user.lock().await = Some(user);
    }

    pub async fn logout(&self, app: &AppHandle) {
        self.cancel_polling().await;
        if let Some(file) = login_file(app) {
            let _ = std::fs::remove_file(file);
        }
        *self.user.lock().await = None;
    }

    /// 开始轮询扫码结果；重复调用会先取消上一轮
    pub async fn begin_polling(&self, app: AppHandle, key: String) {
        self.cancel_polling().await;
        let token = CancellationToken::new();
        *self.polling.lock().await = Some(token.clone());

        let http = self.http.clone();
        let svc = app.state::<crate::state::AppState>().auth.clone();
        tokio::spawn(async move {
            let deadline = tokio::time::Instant::now() + LOGIN_TIMEOUT;
            loop {
                tokio::select! {
                    _ = token.cancelled() => return,
                    _ = tokio::time::sleep(POLL_INTERVAL) => {}
                }
                if tokio::time::Instant::now() > deadline {
                    let _ = app.emit(
                        events::LOGIN_FAILED,
                        serde_json::json!({ "message": "登录超时，请重新扫码" }),
                    );
                    return;
                }
                let url = format!("{WATCH_URL}?key={key}");
                let resp = match http.get(&url).send().await {
                    Ok(r) => r,
                    Err(_) => continue, // 网络抖动，下一轮重试
                };
                let body: Value = match resp.json().await {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let data = match body.get("data") {
                    Some(Value::String(s)) => s.clone(),
                    _ => continue,
                };
                // 扫码状态：no=未扫 wait=已扫待确认 refuse=已拒绝 error=超时
                match data.as_str() {
                    "no" => continue,
                    "wait" | "refuse" | "error" => {
                        let _ = app.emit(events::QR_STATE, data.clone());
                        if data != "wait" {
                            *svc.polling.lock().await = None;
                            return;
                        }
                        continue;
                    }
                    _ if data.len() > 20 => {}
                    _ => continue,
                }
                let user: Value = match serde_json::from_str(&data) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                svc.save_user(&app, user.clone()).await;
                *svc.polling.lock().await = None;
                let _ = app.emit(events::LOGIN_SUCCESS, user);
                return;
            }
        });
    }

    pub async fn cancel_polling(&self) {
        if let Some(t) = self.polling.lock().await.take() {
            t.cancel();
        }
    }
}

#[tauri::command]
pub async fn get_login_state(
    state: State<'_, crate::state::AppState>,
) -> Result<Option<Value>, String> {
    Ok(state.auth.get_login_state().await)
}

/// 启动时拉取云端用户信息校验登录态
#[tauri::command]
pub async fn refresh_user(
    app: AppHandle,
    state: State<'_, crate::state::AppState>,
) -> Result<Value, String> {
    state.auth.refresh_user(&app).await
}

#[tauri::command]
pub async fn logout(
    app: AppHandle,
    state: State<'_, crate::state::AppState>,
) -> Result<(), String> {
    state.auth.logout(&app).await;
    Ok(())
}

#[tauri::command]
pub async fn begin_qr_login(
    app: AppHandle,
    state: State<'_, crate::state::AppState>,
    key: String,
) -> Result<(), String> {
    if key.trim().is_empty() {
        return Err("登录 key 不能为空".into());
    }
    state.auth.begin_polling(app, key).await;
    Ok(())
}

#[tauri::command]
pub async fn cancel_qr_login(state: State<'_, crate::state::AppState>) -> Result<(), String> {
    state.auth.cancel_polling().await;
    Ok(())
}

/// 获取扫码登录二维码内容（原 H5 _buildLoginCode）
#[tauri::command]
pub async fn build_login_code(
    state: State<'_, crate::state::AppState>,
    code: String,
) -> Result<String, String> {
    if code.trim().is_empty() {
        return Err("登录 code 不能为空".into());
    }
    let url = format!("{BUILD_URL}?code={code}");
    let resp = state
        .auth
        .http_client()
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("获取二维码失败：{e}"))?;
    let body: Value = resp
        .json()
        .await
        .map_err(|e| format!("二维码响应解析失败：{e}"))?;
    body.get("data")
        .and_then(|d| d.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| "云端未返回二维码内容".to_string())
}

/// 供 Splash 启动检查使用：登录态是否有效
#[allow(dead_code)]
pub fn login_valid_cached(svc: &Arc<AuthService>) -> bool {
    svc.is_logged_in()
}
