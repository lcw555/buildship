//! Windows 控制台装配。
//!
//! 桌面端以 `windows` 子系统（GUI）启动，双击运行时不会再弹出黑框；
//! 从终端（`cargo run` / cmd / PowerShell / Windows Terminal）启动时，
//! 再把标准流挂回父进程已有的控制台，否则开发期看不到任何日志。
//!
//! 关键细节：调用方可能已经把输出重定向到文件或管道（`buildship.exe > log.txt`），
//! 这种情况下必须尊重重定向、不能抢占 stdout，否则日志会"跑到控制台里、文件却是空的"。
//!
//! ANSI 着色与 UTF-8/UTF-16 的编码处理由 `buildship_common::logging` 统一负责。

use std::fs::OpenOptions;
use std::os::windows::io::AsRawHandle;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Storage::FileSystem::{GetFileType, FILE_TYPE_DISK, FILE_TYPE_PIPE};
use windows_sys::Win32::System::Console::{
    AttachConsole, GetStdHandle, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE,
    STD_OUTPUT_HANDLE,
};

/// 是否允许挂回父控制台。
///
/// 发布版是纯后台 GUI：双击运行不 attach、不占控制台，日志只落文件
/// （`%APPDATA%\Buildship\logs\buildship.YYYY-MM-DD.log`）。
/// 需要临时在终端看输出时，设置 `BUILDSHIP_CONSOLE=1` 再启动即可。
fn console_requested() -> bool {
    if cfg!(debug_assertions) {
        return true;
    }
    std::env::var_os("BUILDSHIP_CONSOLE").is_some()
}

/// 挂回父控制台；没有父控制台或输出已被重定向时静默返回。
pub fn init() {
    if !console_requested() {
        return;
    }
    // SAFETY: 只调用 Win32 控制台 API；句柄由系统返回并做了有效性判断
    unsafe {
        if stdout_redirected() {
            return;
        }
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            return;
        }
        redirect("CONOUT$", STD_OUTPUT_HANDLE);
        redirect("CONOUT$", STD_ERROR_HANDLE);
    }
}

/// 标准输出是否已被调用方重定向到文件 / 管道。
unsafe fn stdout_redirected() -> bool {
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        if handle.is_null() {
            return false;
        }
        // FILE_TYPE_DISK = 文件，FILE_TYPE_PIPE = 管道（含我们的输出采集）
        matches!(GetFileType(handle), FILE_TYPE_DISK | FILE_TYPE_PIPE)
    }
}

/// 把 `std::io::stdout()/stderr()` 指向已挂载的控制台设备。
unsafe fn redirect(device: &str, which: u32) {
    let Ok(file) = OpenOptions::new().write(true).open(device) else {
        return;
    };
    let handle = file.as_raw_handle() as HANDLE;
    // 句柄所有权交给进程标准流：故意泄漏 File，避免 drop 时 CloseHandle 让句柄失效
    std::mem::forget(file);
    unsafe { SetStdHandle(which, handle) };
}
