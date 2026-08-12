// 桌面端可执行入口（库逻辑见 lib.rs）
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    kt_mange_pc_lib::run()
}
