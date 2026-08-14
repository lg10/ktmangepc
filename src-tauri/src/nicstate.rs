//! 网卡状态标记文件：DHCP/中继开启时写入，干净关闭后删除。
//! 被 kill、崩溃、强制关机时文件残留，作为启动残留检测与「恢复网络原状」的线索。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct NicState {
    /// DHCP 模式占用的网卡名（空 = 未开启）
    pub dhcp_iface: String,
    /// 该网卡原本是否 DHCP 取址（恢复时据此还原，防止 netsh 操作遗留静态模式）
    pub dhcp_was_dhcp: bool,
    /// 中继模式源网卡（空 = 未开启）
    pub inet_src: String,
    /// 中继模式目标网口
    pub inet_dst: String,
}

impl NicState {
    pub fn is_empty(&self) -> bool {
        self.dhcp_iface.is_empty() && self.inet_src.is_empty()
    }
}

fn marker_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("nic-state.json"))
}

pub fn load(app: &AppHandle) -> NicState {
    marker_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// 读取-修改-写回；文件为空且无内容时直接删除
pub fn update(app: &AppHandle, f: impl FnOnce(&mut NicState)) {
    let Some(path) = marker_path(app) else {
        return;
    };
    let mut state = load(app);
    f(&mut state);
    if state.is_empty() {
        let _ = std::fs::remove_file(&path);
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(&state) {
        let _ = std::fs::write(&path, json);
    }
}
