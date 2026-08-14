//! 前后端事件名常量（与 src/lib/api.ts EVENTS 严格一致）

pub const DEVICE: &str = "udp://device";
pub const DEVICE_OFFLINE: &str = "udp://device-offline";
pub const LOCK_PACKET: &str = "udp://lock-packet";
pub const SERVER_STATUS: &str = "udp://server-status";
pub const UDP_LOG: &str = "udp://log";
pub const LOGIN_SUCCESS: &str = "auth://login-success";
pub const LOGIN_FAILED: &str = "auth://login-failed";
pub const QR_STATE: &str = "auth://qr-state";
pub const SPLASH_LOG: &str = "splash://update";
pub const DHCP_STATUS: &str = "dhcp://status";
pub const DHCP_LEASE: &str = "dhcp://lease";
pub const INET_STATUS: &str = "inet://status";
// 阶段二
pub const FILE_FETCH_PROGRESS: &str = "file://fetch-progress";
pub const TASK_UPDATE: &str = "task://update";
pub const TELNET_DATA: &str = "telnet://data";
pub const TELNET_CLOSED: &str = "telnet://closed";
// 退出拦截：DHCP/中继运行中关窗被拦下，前端据此弹「正在恢复」等待弹窗
pub const EXIT_BLOCKED: &str = "app://exit-blocked";
