//! 全局状态容器（Pinia 单一状态源的服务端对应物）

use crate::adbshell::AdbShellService;
use crate::auth::AuthService;
use crate::checkin::CheckinService;
use crate::db::DbService;
use crate::dhcp::DhcpService;
use crate::filestore::FileStore;
use crate::hotel::HotelService;
use crate::inetshare::InetShareService;
use crate::telnet::TelnetService;
use crate::udp::UdpService;
use crate::upgrade::UpdateService;
use std::sync::Arc;

#[derive(Default)]
pub struct AppState {
    pub udp: Arc<UdpService>,
    pub auth: Arc<AuthService>,
    pub dhcp: Arc<DhcpService>,
    pub db: Arc<DbService>,
    pub filestore: Arc<FileStore>,
    pub update: Arc<UpdateService>,
    pub hotel: Arc<HotelService>,
    pub telnet: Arc<TelnetService>,
    pub inetshare: Arc<InetShareService>,
    pub checkin: Arc<CheckinService>,
    pub adbshell: Arc<AdbShellService>,
}
