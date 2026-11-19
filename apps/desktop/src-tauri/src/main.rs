// 桌面端是 GUI 程序：声明 windows 子系统，双击运行时不会附带控制台窗口。
// 发布版不挂任何控制台，日志只写 %APPDATA%\Buildship\logs（按天滚动、保留 7 天）；
// 开发期（debug 构建）或显式设置 BUILDSHIP_CONSOLE=1 时，才会挂回父控制台方便看输出。
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg_attr(mobile, tauri::mobile_entry_point)]
fn main() {
    buildship_lib::run()
}
