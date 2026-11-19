//! 应用日志初始化。
//!
//! 桌面端与 Agent 共用：输出到控制台（开发期）与按天滚动的文件（诊断用）。
//! 文件日志保留 7 天，避免无限增长。
//!
//! 两个 Windows 特有的坑在这里统一处理：
//! 1. 控制台默认不解析 ANSI 转义 → 先开启 `ENABLE_VIRTUAL_TERMINAL_PROCESSING`，开不了就不着色；
//! 2. `stdout` 写的是 UTF-8 字节，而 cmd/powershell 按本地代码页（936/GBK）解码 → 中文乱码，
//!    因此控制台场景改用 `WriteConsoleW` 直接写 UTF-16，管道/文件场景仍走字节流。

use once_cell::sync::OnceCell;
use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt::writer::BoxMakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

static GUARD: OnceCell<WorkerGuard> = OnceCell::new();

/// 初始化全局日志。重复调用是安全的（仅首次生效）。
///
/// `log_dir` 为空时只输出到控制台（deploy-agent 前台运行场景）。
pub fn init(log_dir: Option<&Path>, default_level: &str) -> std::io::Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(format!(
            "buildship={default_level},deploy_agent={default_level},info"
        ))
    });

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_ansi(console_ansi())
        .with_writer(console_writer())
        .with_target(false)
        .with_thread_ids(false);

    match log_dir {
        Some(dir) => {
            std::fs::create_dir_all(dir)?;
            let appender = tracing_appender::rolling::Builder::new()
                .rotation(tracing_appender::rolling::Rotation::DAILY)
                .filename_prefix("buildship")
                .filename_suffix("log")
                .max_log_files(7)
                .build(dir)
                // InitError 不是 io::Error，这里显式转换，保证调用方签名简洁
                .map_err(|e| std::io::Error::other(format!("初始化日志文件失败：{e}")))?;
            let (non_blocking, guard) = tracing_appender::non_blocking(appender);
            let _ = GUARD.set(guard);
            let _ = tracing_subscriber::registry()
                .with(filter)
                .with(fmt_layer)
                .with(
                    tracing_subscriber::fmt::layer()
                        .with_writer(non_blocking)
                        .with_ansi(false)
                        .with_target(false),
                )
                .try_init();
        }
        None => {
            let _ = tracing_subscriber::registry()
                .with(filter)
                .with(fmt_layer)
                .try_init();
        }
    }
    Ok(())
}

/// 当前进程的 stdout 是否真的连着一个控制台（重定向到管道/文件时为 false）。
fn console_attached() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
        use windows_sys::Win32::System::Console::{
            GetConsoleMode, GetStdHandle, STD_OUTPUT_HANDLE,
        };
        // SAFETY: 只查询标准输出句柄的控制台模式，句柄做了有效性判断
        unsafe {
            let handle = GetStdHandle(STD_OUTPUT_HANDLE);
            if handle.is_null() || handle == INVALID_HANDLE_VALUE {
                return false;
            }
            let mut mode = 0u32;
            GetConsoleMode(handle, &mut mode) != 0
        }
    }
    #[cfg(not(windows))]
    {
        use std::io::IsTerminal;
        std::io::stdout().is_terminal()
    }
}

/// 控制台是否支持 ANSI 着色：连着控制台 + 未设置 `NO_COLOR` + VT 处理已开启。
fn console_ansi() -> bool {
    if !console_attached() || std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    #[cfg(windows)]
    {
        enable_windows_vt()
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// 开启当前进程标准输出的虚拟终端处理，返回是否成功。
#[cfg(windows)]
fn enable_windows_vt() -> bool {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{
        GetConsoleMode, GetStdHandle, SetConsoleMode, ENABLE_VIRTUAL_TERMINAL_PROCESSING,
        STD_OUTPUT_HANDLE,
    };

    // SAFETY: 仅读取/设置当前进程标准输出的控制台模式，句柄做了有效性判断
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut mode = 0u32;
        if GetConsoleMode(handle, &mut mode) == 0 {
            return false;
        }
        SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) != 0
    }
}

/// 标准输出句柄是否可用。
///
/// GUI 子系统进程由资源管理器双击启动时，三个标准句柄都是空的；
/// 此时再往里写只会白白失败一次，日志只该进文件。
#[cfg(windows)]
fn stdout_available() -> bool {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{GetStdHandle, STD_OUTPUT_HANDLE};
    // SAFETY: 只查询标准输出句柄，未解引用
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        !handle.is_null() && handle != INVALID_HANDLE_VALUE
    }
}

/// 控制台场景用 `WriteConsoleW`，其余场景写标准输出字节流。
fn console_writer() -> BoxMakeWriter {
    #[cfg(windows)]
    {
        if console_attached() {
            return BoxMakeWriter::new(|| ConsoleWriter);
        }
        if !stdout_available() {
            // 双击启动、且没有终端：丢弃控制台输出，只保留文件日志
            return BoxMakeWriter::new(std::io::sink);
        }
    }
    BoxMakeWriter::new(std::io::stdout)
}

/// 直接写 Unicode 控制台，避免控制台按本地代码页把 UTF-8 解成乱码。
#[cfg(windows)]
struct ConsoleWriter;

#[cfg(windows)]
impl std::io::Write for ConsoleWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        use windows_sys::Win32::System::Console::{GetStdHandle, WriteConsoleW, STD_OUTPUT_HANDLE};

        let text = String::from_utf8_lossy(buf);
        let wide: Vec<u16> = text.encode_utf16().collect();
        let mut written = 0u32;
        // SAFETY: wide 缓冲区长度与传入长度一致；句柄由系统提供，失败时回退到字节流
        let ok = unsafe {
            WriteConsoleW(
                GetStdHandle(STD_OUTPUT_HANDLE),
                wide.as_ptr(),
                wide.len() as u32,
                &mut written,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            // 不是控制台（或被重定向）：按原样写字节，保持管道语义
            let mut out = std::io::stdout();
            return std::io::Write::write(&mut out, buf);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
