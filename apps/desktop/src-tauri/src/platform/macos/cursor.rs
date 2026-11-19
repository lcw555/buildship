//! macOS 光标位置。
//!
//! 说明：macOS 侧不轮询全局光标，悬浮窗收起态保持 104×104 的小窗口，
//! 由 WKWebView 的 hover 事件驱动展开（见 6.5 平台差异说明）。

pub fn position() -> Option<(i32, i32)> {
    None
}
