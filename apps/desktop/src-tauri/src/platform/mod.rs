//! 平台适配层：所有 Windows / macOS 专属逻辑集中在此，业务代码不直接调用系统 API。

pub mod floating;

/// 挂回父控制台（仅 Windows 需要，其它平台是空操作）。
///
/// 发布版默认什么都不做：桌面端是纯后台 GUI，日志只进文件。
/// 开发期（debug 构建）或设置 `BUILDSHIP_CONSOLE=1` 时才会真正挂回终端。
pub fn init_console() {
    #[cfg(windows)]
    {
        windows::console::init();
    }
}

#[cfg(windows)]
pub mod windows;

#[cfg(target_os = "macos")]
pub mod macos;

/// 启动阶段的致命错误提示。
///
/// 此时窗口还不存在，日志目录也可能不可用，所以必须有一条不依赖 WebView 与 tracing
/// 的通道：Windows 用系统对话框，其它平台写 stderr。
pub fn fatal(message: &str) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
        let title: Vec<u16> = "Buildship 启动失败\0".encode_utf16().collect();
        let body: Vec<u16> = format!("{message}\0").encode_utf16().collect();
        // SAFETY: 两个宽字符串均以 NUL 结尾，生命周期覆盖调用过程
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                body.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
    }
    #[cfg(not(windows))]
    {
        eprintln!("Buildship 启动失败：{message}");
    }
}

/// 全局光标位置（物理像素）。
///
/// Windows 通过 `GetCursorPos` 获取，用于悬浮窗"透明区域鼠标穿透 + 悬浮展开"的热区判定；
/// macOS 返回 None，交由 WKWebView 的 hover 事件处理（窗口本身足够小，不会遮挡桌面）。
pub fn cursor_position() -> Option<(i32, i32)> {
    #[cfg(windows)]
    {
        windows::cursor::position()
    }
    #[cfg(not(windows))]
    {
        None
    }
}
