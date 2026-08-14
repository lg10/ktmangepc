// 桌面端可执行入口（库逻辑见 lib.rs）
//
// --dhcp-relay <app_port> <helper_port> <nic_name>：DHCP 特权助手中继模式，
// 由主程序以提权方式拉起（不启动 GUI，避开 macOS WKWebView 的 root 限制）
// --inet-share <app_port> <helper_port> <src_if> <dst_if>：网络中继特权助手，
// 调用系统互联网共享（Windows ICS / macOS 互联网共享）给设备供网
// --nic-restore <残留网卡csv> <标记网卡> <was_dhcp>：一次性网卡恢复助手（清理异常退出残留）
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 4 && args[1] == "--dhcp-relay" {
        let app_port = args[2].parse().unwrap_or(0);
        let helper_port = args[3].parse().unwrap_or(0);
        let nic = args.get(4).cloned().unwrap_or_default();
        std::process::exit(kt_mange_pc_lib::dhcp_relay::run_relay(
            app_port,
            helper_port,
            &nic,
        ));
    }
    if args.len() >= 6 && args[1] == "--inet-share" {
        let app_port = args[2].parse().unwrap_or(0);
        let helper_port = args[3].parse().unwrap_or(0);
        let src = args.get(4).cloned().unwrap_or_default();
        let dst = args.get(5).cloned().unwrap_or_default();
        std::process::exit(kt_mange_pc_lib::inetshare_helper::run(
            app_port,
            helper_port,
            &src,
            &dst,
        ));
    }
    if args.len() >= 2 && args[1] == "--nic-restore" {
        // --nic-restore <残留网卡csv> <标记网卡> <was_dhcp 0/1>：一次性网卡恢复助手
        let residual_csv = args.get(2).cloned().unwrap_or_default();
        let dhcp_iface = args.get(3).cloned().unwrap_or_default();
        let was_dhcp = args.get(4).map(|s| s == "1").unwrap_or(false);
        std::process::exit(kt_mange_pc_lib::restore::run_restore(
            &residual_csv,
            &dhcp_iface,
            was_dhcp,
        ));
    }
    kt_mange_pc_lib::run()
}
