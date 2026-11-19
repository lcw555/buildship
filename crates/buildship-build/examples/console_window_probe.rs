//! 手动探针：验证构建子进程不会弹出控制台窗口。
//!
//! 这个 example 本身是 GUI 子系统进程（没有控制台），和正式应用一致；
//! 它用真实的 `CommandSpec::spawn` 启动一个控制台子进程（`cmd.exe /d /c ping`），
//! 期间用外部脚本统计桌面上 `ConsoleWindowClass` 的数量即可确认有没有黑框。
//!
//! 用法：
//! ```text
//! cargo build --example console_window_probe -p buildship-build
//! # 另开一个进程统计窗口数量，然后：
//! ./target/debug/examples/console_window_probe.exe
//! ```

#![windows_subsystem = "windows"]

use buildship_build::process::CommandSpec;

#[tokio::main]
async fn main() {
    let spec = CommandSpec::new(
        "cmd.exe",
        vec![
            "/d".to_string(),
            "/c".to_string(),
            "ping -n 4 127.0.0.1".to_string(),
        ],
        ".",
    );

    match spec.spawn() {
        Ok(mut child) => {
            // 子进程存活约 3 秒，足够外部探针采样
            let _ = child.wait().await;
        }
        Err(err) => {
            // GUI 进程没有 stderr，写到文件里便于排查
            let _ = std::fs::write("probe-error.txt", format!("{err}"));
        }
    }
}
