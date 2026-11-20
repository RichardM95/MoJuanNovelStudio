#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#[cfg(target_os = "windows")]
fn main() {
    novel_core::desktop::run();
}
#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("墨卷桌面版本目前针对 Windows；此平台仅验证核心逻辑。");
}
