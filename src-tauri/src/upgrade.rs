//! 固件升级 / 配置下发任务引擎
//!
//! 对应原 TaskUpdate.cs / TaskConfig.cs / UpdateFileMsg.cs / ConfigFileMsg.cs：
//! - 首指令：固件 0x02F2、配置 0xFD01（构造后加入等待队列）
//! - 守护：每 10 秒对等待中任务重发首指令，5 分钟未推进判超时
//! - 设备拉包：0x02F3/0xFD02 → 查本地分包 → 0x02F4/0xFD03 应答并更新进度

use crate::events;
use crate::filestore::{FileKind, FileStore};
use crate::protocol::{self, build_packet_dyn, FileRequest};
use crate::state::AppState;
use crate::udp::UdpService;
use serde::Serialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Mutex;

const GUARD_TICK_SECS: u64 = 10;
const TASK_TIMEOUT_MS: u64 = 1000 * 60 * 5;
const MAX_FINISHED: usize = 200;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 任务状态：0 等待 1 进行 10 完成 999 超时 1999 重复（与原 RcuFileUpdate.State 对齐）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTaskView {
    pub kind: String,
    pub equip_id: String,
    pub uid: i64,
    pub file_name: String,
    pub total_num: i64,
    pub percent: u8,
    pub progress: String,
    pub state: i32,
    pub start_time: u64,
    pub end_time: u64,
}

#[derive(Clone)]
struct TaskInner {
    kind: FileKind,
    equip_id: String,
    uid: i64,
    file_name: String,
    first_cmd: String,
    total_num: i64,
    percent: u8,
    progress: String,
    state: i32,
    start_time: u64,
    end_time: u64,
}

impl TaskInner {
    fn view(&self) -> UpdateTaskView {
        UpdateTaskView {
            kind: match self.kind {
                FileKind::Upgrade => "firmware".into(),
                FileKind::Config => "config".into(),
            },
            equip_id: self.equip_id.clone(),
            uid: self.uid,
            file_name: self.file_name.clone(),
            total_num: self.total_num,
            percent: self.percent,
            progress: self.progress.clone(),
            state: self.state,
            start_time: self.start_time,
            end_time: self.end_time,
        }
    }
}

#[derive(Default)]
struct ServiceInner {
    active: Vec<TaskInner>,
    finished: Vec<TaskInner>,
    guard_started: bool,
}

#[derive(Default)]
pub struct UpdateService {
    inner: Mutex<ServiceInner>,
}

impl UpdateService {
    fn emit_tasks(&self, app: &AppHandle, inner: &ServiceInner) {
        let mut list: Vec<UpdateTaskView> = inner.active.iter().map(|t| t.view()).collect();
        list.extend(inner.finished.iter().rev().map(|t| t.view()));
        let _ = app.emit(events::TASK_UPDATE, list);
    }

    /// 创建下发任务（支持多设备批量）
    pub async fn start_tasks(
        &self,
        app: &AppHandle,
        state: &AppState,
        kind: FileKind,
        equip_ids: Vec<String>,
        uid: i64,
    ) -> Result<u32, String> {
        if equip_ids.is_empty() {
            return Err("未选择任何设备".into());
        }
        let (num, total, md5, version) = state
            .filestore
            .get_meta(kind, uid)?
            .ok_or("文件损坏，请删除后重新拉取")?;
        let file_name = state
            .filestore
            .list(kind, None)?
            .into_iter()
            .find(|f| f.uid == uid)
            .map(|f| {
                if f.hotel_name.is_empty() {
                    f.name
                } else {
                    format!("{}-{}", f.hotel_name, f.name)
                }
            })
            .unwrap_or_default();

        // 文件条目用于取 custom（hotelFlag 来源，与原实现对齐）
        let custom = state
            .filestore
            .list(kind, None)?
            .into_iter()
            .find(|f| f.uid == uid)
            .map(|f| f.custom)
            .unwrap_or(0);

        let update_time = now_ms() / 1000;
        let first_cmd = match kind {
            FileKind::Upgrade => protocol::build_start_update_payload(
                custom as u32,
                &version,
                update_time,
                uid as u16,
                total as u64,
                num as u16,
                &md5,
            )?,
            FileKind::Config => protocol::build_start_config_payload(
                uid as u16,
                total as u64,
                num as u16,
                &md5,
                &version,
                update_time,
            )?,
        };

        let mut created = 0u32;
        {
            let mut inner = self.inner.lock().await;
            for equip in equip_ids {
                // 同设备旧任务归档：未完成的标记重复（原 TaskUpdate.AddUpdateIng）
                let mut moved: Vec<TaskInner> =
                    take_matching(&mut inner.active, |t| t.equip_id == equip && t.kind == kind);
                for m in &mut moved {
                    if m.state <= 1 {
                        m.state = 1999;
                        m.progress = "重复".into();
                        m.end_time = now_ms();
                    }
                }
                inner.finished.append(&mut moved);

                inner.active.push(TaskInner {
                    kind,
                    equip_id: equip,
                    uid,
                    file_name: file_name.clone(),
                    first_cmd: first_cmd.clone(),
                    total_num: num,
                    percent: 0,
                    progress: "等待".into(),
                    state: 0,
                    start_time: now_ms(),
                    end_time: 0,
                });
                created += 1;
            }
            trim_finished(&mut inner);
            self.emit_tasks(app, &inner);
        }

        // 立即首发一次 + 启动守护
        self.guard_once(app, &state.udp).await;
        self.ensure_guard(app).await;
        Ok(created)
    }

    /// 守护一轮：等待中任务超时判定 + 重发首指令
    async fn guard_once(&self, app: &AppHandle, udp: &UdpService) {
        let resend: Vec<(FileKind, String, String)> = {
            let mut inner = self.inner.lock().await;
            let now = now_ms();
            let mut timed_out: Vec<TaskInner> = Vec::new();
            let mut resend = Vec::new();
            inner.active.retain(|t| {
                if t.state != 0 {
                    return true;
                }
                if now - t.start_time > TASK_TIMEOUT_MS {
                    let mut done = t.clone();
                    done.state = 999;
                    done.progress = "超时".into();
                    done.end_time = now;
                    timed_out.push(done);
                    return false;
                }
                resend.push((t.kind, t.equip_id.clone(), t.first_cmd.clone()));
                true
            });
            inner.finished.append(&mut timed_out);
            trim_finished(&mut inner);
            if !timed_out.is_empty() {
                self.emit_tasks(app, &inner);
            }
            resend
        };
        for (kind, equip, cmd) in resend {
            // 固件 0x02F2 用专用组包（数据区第 37 字节置 1）；配置 0xFD01 走通用组包
            let packet = match kind {
                FileKind::Upgrade => protocol::build_start_update_packet(&cmd),
                FileKind::Config => {
                    build_packet_dyn(protocol::reg::SEND_RCU_START_CONFIG, 40, &cmd)
                }
            };
            match packet {
                Ok(packet) => {
                    if let Err(e) = udp.send_raw(&equip, packet).await {
                        let _ = app.emit(events::UDP_LOG, format!("首指令重发失败 {equip}: {e}"));
                    }
                }
                Err(e) => {
                    let _ = app.emit(events::UDP_LOG, format!("首指令组包失败 {equip}: {e}"));
                }
            }
        }
    }

    async fn ensure_guard(&self, app: &AppHandle) {
        {
            let mut inner = self.inner.lock().await;
            if inner.guard_started {
                return;
            }
            inner.guard_started = true;
        }
        let svc = app.state::<AppState>().update.clone();
        let app2 = app.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(GUARD_TICK_SECS)).await;
                let udp = app2.state::<AppState>().udp.clone();
                let has_active = {
                    let inner = svc.inner.lock().await;
                    inner.active.iter().any(|t| t.state == 0)
                };
                if !has_active {
                    let mut inner = svc.inner.lock().await;
                    if !inner.active.iter().any(|t| t.state == 0) {
                        inner.guard_started = false;
                        return;
                    }
                }
                svc.guard_once(&app2, &udp).await;
            }
        });
    }

    /// 设备拉包请求（0x02F3 / 0xFD02）
    pub async fn on_file_request(
        &self,
        app: &AppHandle,
        filestore: &FileStore,
        udp: &UdpService,
        kind: FileKind,
        req: &FileRequest,
    ) {
        // 查分包
        let pkg = match filestore.get_package(kind, req.uid as i64, req.data_index as i64) {
            Ok(Some(p)) => p,
            _ => return,
        };
        let (hex_data, md5) = pkg;
        let byte_len = hex_data.len() / 2;
        let payload = format!(
            "{:04X}{:04X}{:04X}{}{}",
            req.uid, req.data_index, byte_len, hex_data, md5
        );
        let (reg, reg_num) = match kind {
            FileKind::Upgrade => (protocol::reg::SEND_RCU_UPDATE_FILE, 26 + byte_len as u16),
            FileKind::Config => (protocol::reg::SEND_RCU_CONFIG_FILE, 26 + byte_len as u16),
        };
        match build_packet_dyn(reg, reg_num, &payload) {
            Ok(packet) => {
                if let Err(e) = udp.send_raw(&req.equip_id, packet).await {
                    let _ = app.emit(
                        events::UDP_LOG,
                        format!("分包应答发送失败 {}: {e}", req.equip_id),
                    );
                }
            }
            Err(e) => {
                let _ = app.emit(events::UDP_LOG, format!("分包应答组包失败: {e}"));
            }
        }

        // 更新任务进度
        {
            let mut inner = self.inner.lock().await;
            if let Some(pos) = inner
                .active
                .iter()
                .position(|t| t.kind == kind && t.equip_id == req.equip_id && t.state <= 1)
            {
                let task = &mut inner.active[pos];
                task.first_cmd.clear(); // 首指令已确认（原 rcuFileUpdate.firstCmd=""）
                if task.total_num > 0 && req.data_index as i64 == task.total_num {
                    task.state = 10;
                    task.percent = 100;
                    task.progress = "完成".into();
                    task.end_time = now_ms();
                } else {
                    task.state = 1;
                    task.percent =
                        ((req.data_index as f64 / task.total_num as f64) * 100.0) as u8;
                    task.progress = format!(
                        "{}%({}/{})",
                        task.percent, req.data_index, task.total_num
                    );
                }
            }
            // 完成任务归档
            let mut moved: Vec<TaskInner> =
                take_matching(&mut inner.active, |t| t.kind == kind && t.state == 10);
            if !moved.is_empty() {
                inner.finished.append(&mut moved);
                trim_finished(&mut inner);
            }
            self.emit_tasks(app, &inner);
        }
    }

    /// 任务列表（进行中 + 已完成倒序）
    pub async fn list_tasks(&self) -> Vec<UpdateTaskView> {
        let inner = self.inner.lock().await;
        let mut list: Vec<UpdateTaskView> = inner.active.iter().map(|t| t.view()).collect();
        list.extend(inner.finished.iter().rev().map(|t| t.view()));
        list
    }

    /// 取消等待中的任务
    pub async fn cancel_task(&self, app: &AppHandle, equip_id: &str, kind: &str) -> Result<(), String> {
        let kind = match kind {
            "firmware" => FileKind::Upgrade,
            "config" => FileKind::Config,
            _ => return Err("任务类型非法".into()),
        };
        let mut inner = self.inner.lock().await;
        if let Some(pos) = inner
            .active
            .iter()
            .position(|t| t.kind == kind && t.equip_id == equip_id && t.state == 0)
        {
            let mut t = inner.active.remove(pos);
            t.state = 999;
            t.progress = "已取消".into();
            t.end_time = now_ms();
            inner.finished.push(t);
            trim_finished(&mut inner);
            self.emit_tasks(app, &inner);
            Ok(())
        } else {
            Err("任务不存在或已在进行".into())
        }
    }
}

fn trim_finished(inner: &mut ServiceInner) {
    while inner.finished.len() > MAX_FINISHED {
        inner.finished.remove(0);
    }
}

/// 从 vec 中取出满足条件的元素（稳定版无 Vec::extract_if）
fn take_matching<T>(v: &mut Vec<T>, mut f: impl FnMut(&T) -> bool) -> Vec<T> {
    let mut keep = Vec::with_capacity(v.len());
    let mut taken = Vec::new();
    for item in v.drain(..) {
        if f(&item) {
            taken.push(item);
        } else {
            keep.push(item);
        }
    }
    *v = keep;
    taken
}

#[tauri::command]
pub async fn start_file_update(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    kind: String,
    equip_ids: Vec<String>,
    uid: i64,
) -> Result<u32, String> {
    let kind = match kind.as_str() {
        "firmware" => FileKind::Upgrade,
        "config" => FileKind::Config,
        _ => return Err("任务类型非法（firmware/config）".into()),
    };
    state
        .update
        .start_tasks(&app, &state, kind, equip_ids, uid)
        .await
}

#[tauri::command]
pub async fn list_update_tasks(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<UpdateTaskView>, String> {
    Ok(state.update.list_tasks().await)
}

#[tauri::command]
pub async fn cancel_update_task(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    equip_id: String,
    kind: String,
) -> Result<(), String> {
    state.update.cancel_task(&app, &equip_id, &kind).await
}
