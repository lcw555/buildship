//! Windows 全局光标位置读取。

use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

/// 读取全局光标位置（物理像素，多显示器时为虚拟桌面坐标）
pub fn position() -> Option<(i32, i32)> {
    let mut point = POINT { x: 0, y: 0 };
    // SAFETY: GetCursorPos 只写入我们提供的栈上结构体
    let ok = unsafe { GetCursorPos(&mut point) };
    if ok != 0 {
        Some((point.x, point.y))
    } else {
        None
    }
}
