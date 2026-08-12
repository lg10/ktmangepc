//! 网卡枚举（对应原 MainWindowViewModel.cs 的接口列表）

use serde::Serialize;
use std::net::IpAddr;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetInterfaceView {
    pub name: String,
    pub ip: String,
    pub mac: String,
    pub is_loopback: bool,
}

fn mac_of(name: &str) -> String {
    mac_address::mac_address_by_name(name)
        .ok()
        .flatten()
        .map(|m: mac_address::MacAddress| m.to_string().to_uppercase())
        .unwrap_or_default()
}

#[tauri::command]
pub async fn list_network_interfaces() -> Result<Vec<NetInterfaceView>, String> {
    let list = local_ip_address::list_afinet_netifas().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for (name, ip) in list {
        // 仅保留 IPv4，避免前端展示噪声
        let IpAddr::V4(v4) = ip else { continue };
        let is_loopback = v4.is_loopback();
        if is_loopback {
            continue;
        }
        out.push(NetInterfaceView {
            mac: mac_of(&name),
            name,
            ip: v4.to_string(),
            is_loopback,
        });
    }
    Ok(out)
}
