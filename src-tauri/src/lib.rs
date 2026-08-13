//! 肯天玉佩 Tauri 2.0 桌面端库入口

pub mod auth;
pub mod commands;
pub mod db;
pub mod dhcp;
pub mod events;
pub mod filestore;
pub mod hotel;
pub mod netif;
pub mod protocol;
pub mod state;
pub mod telnet;
pub mod udp;
pub mod upgrade;

use state::AppState;
use std::sync::Arc;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let db = Arc::new(db::DbService::default());
            let state = AppState {
                udp: Arc::new(udp::UdpService::default()),
                auth: Arc::new(auth::AuthService::default()),
                dhcp: Arc::new(dhcp::DhcpService::default()),
                filestore: Arc::new(filestore::FileStore::new(db.clone())),
                update: Arc::new(upgrade::UpdateService::default()),
                hotel: Arc::new(hotel::HotelService::default()),
                telnet: Arc::new(telnet::TelnetService::default()),
                db,
            };
            // 启动即恢复本地登录态（供 Splash 校验）
            state.auth.load(&app.handle());
            // 预初始化本地库（check_environment 会再次确认）
            state.db.init(&app.handle());
            // 恢复酒店同步缓存
            state.hotel.load(&state.db);
            app.manage(state);

            // 主窗口（conf 中不预建）：macOS 用原生 overlay 标题栏，
            // 获得系统圆角/阴影/真红绿灯（微信式观感）；其他平台自定义无边框（自绘三键）
            let builder = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("/".into()),
            )
            .title("肯天玉佩")
            .center()
            .resizable(false)
            .inner_size(480.0, 344.0);
            #[cfg(target_os = "macos")]
            let builder = builder
                .decorations(true)
                .title_bar_style(tauri::TitleBarStyle::Overlay)
                .hidden_title(true);
            #[cfg(not(target_os = "macos"))]
            let builder = builder.decorations(false);
            builder.build()?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // 启动检查 / 检查更新 / 外链
            commands::check_environment,
            commands::check_update,
            commands::open_url,
            // 网卡
            netif::list_network_interfaces,
            // 登录
            auth::get_login_state,
            auth::refresh_user,
            auth::logout,
            auth::begin_qr_login,
            auth::cancel_qr_login,
            auth::build_login_code,
            // UDP 服务
            udp::start_udp_server,
            udp::stop_udp_server,
            udp::get_server_status,
            udp::list_devices,
            udp::clear_devices,
            udp::list_lock_packets,
            // 指令下发
            udp::send_network_config,
            udp::send_base_info,
            udp::send_revert_cmd,
            // DHCP
            dhcp::start_dhcp,
            dhcp::stop_dhcp,
            dhcp::get_dhcp_status,
            // 文件库
            filestore::fetch_file,
            filestore::list_files,
            filestore::delete_file,
            // 升级/配置任务
            upgrade::start_file_update,
            upgrade::list_update_tasks,
            upgrade::cancel_update_task,
            // 酒店同步 / 批量授权
            hotel::sync_hotel,
            hotel::search_hotel,
            hotel::get_hotel_state,
            hotel::send_base_info_batch,
            hotel::get_auth_info,
            hotel::send_auth_batch,
            // Telnet
            telnet::telnet_connect,
            telnet::telnet_write,
            telnet::telnet_close,
            telnet::telnet_list,
        ])
        .run(tauri::generate_context!())
        .expect("应用启动失败");
}
