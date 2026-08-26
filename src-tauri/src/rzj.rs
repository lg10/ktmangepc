//! 入住机一键安装：adb 连接设备列表 + 版本清单 + 下载/安装/拉起流水线
//!
//! - rzj_devices：执行内置 `adb devices`，解析序列号/状态/连接方式
//! - rzj_releases：拉取云端清单，解析 install 映射为可选版本列表
//! - rzj_install：共享下载（AppData/rzj-cache，下载新 URL 前清空旧 APK）
//!   → `adb -s <serial> install -r` → `monkey` 拉起固定包名；进度经
//!   `rzj://progress` 事件按设备序列号推送

use serde::Serialize;

/// 清单地址（注意：服务端文件名为 lastest.json，非拼写错误）
pub const MANIFEST_URL: &str = "https://d.kingint.com/app/rzj/lastest.json";
/// 入住机包名（安装后拉起用）
pub const PACKAGE: &str = "com.kingint.checkin4";

/// adb 连接设备（序列号即展示名）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RzjDevice {
    pub serial: String,
    pub status: String,
    /// "usb" | "tcp"（序列号含 ':' 判为网络连接）
    pub transport: String,
}

/// 可安装的入住机版本
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RzjRelease {
    pub key: String,
    pub name: String,
    pub version: String,
    pub url: String,
}

/// 解析 `adb devices` 输出：跳过 banner / server 提示，仅取「序列号 + 状态」两字段行
fn parse_devices(stdout: &str) -> Vec<RzjDevice> {
    stdout
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let serial = parts.next()?;
            let status = parts.next()?;
            if parts.next().is_some() {
                return None; // 三字段以上为干扰行（如 List of devices attached）
            }
            Some(RzjDevice {
                serial: serial.to_string(),
                status: status.to_string(),
                transport: if serial.contains(':') { "tcp".into() } else { "usb".into() },
            })
        })
        .collect()
}

/// 解析 `adb install` 流式输出中的末尾百分比（进度以 \r 分隔输出，如 "37%"）
fn parse_install_percent(chunk: &str) -> Option<u8> {
    let last = chunk
        .split('\r')
        .rev()
        .map(str::trim)
        .find(|s| !s.is_empty())?;
    let digits = last.strip_suffix('%')?;
    let tail: String = digits.chars().rev().take_while(|c| c.is_ascii_digit()).collect();
    if tail.is_empty() {
        return None;
    }
    tail.chars().rev().collect::<String>().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_devices_basic() {
        let out = "List of devices attached\nABC123\tdevice\n192.168.1.5:5555\tunauthorized\n\n";
        let list = parse_devices(out);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].serial, "ABC123");
        assert_eq!(list[0].status, "device");
        assert_eq!(list[0].transport, "usb");
        assert_eq!(list[1].serial, "192.168.1.5:5555");
        assert_eq!(list[1].status, "unauthorized");
        assert_eq!(list[1].transport, "tcp");
    }

    #[test]
    fn parse_devices_skips_banner() {
        let out = "* daemon not running; starting now at tcp:5037\n* daemon started successfully\nList of devices attached\n";
        assert!(parse_devices(out).is_empty());
    }

    #[test]
    fn parse_install_percent_variants() {
        assert_eq!(parse_install_percent("37%"), Some(37));
        assert_eq!(parse_install_percent("\r12%\r40%"), Some(40));
        assert_eq!(parse_install_percent("Performing Streamed Install\r55%"), Some(55));
        assert_eq!(parse_install_percent("Success"), None);
    }
}
