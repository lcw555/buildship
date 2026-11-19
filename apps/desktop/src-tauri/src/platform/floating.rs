//! 悬浮窗口管理：尺寸、位置记忆、光标热区与穿透。
//!
//! 设计（方案 6.5）：
//! - 收起态：104×104 的小窗，只显示 Aurora Loop 图标 + 状态点；
//! - 展开态：向左扩展成 372×104 的玻璃胶囊，图标固定在右侧；
//! - 通过 120ms 的光标轮询决定"是否让窗口接收鼠标事件"，
//!   既保证透明区域不拦截桌面点击，又能在光标进入时立刻恢复交互。
//!
//! 位置策略：
//! - 首次启动贴右下角（`auto` 模式）；
//! - 用户一旦拖动过，就转为自定义位置，展开/收起时保持**右边缘与底边不动**，
//!   否则每次悬停展开都会被拽回角落；
//! - 自定义位置持久化到 `app_settings.floating_anchor`，重启后恢复。

use buildship_db::repositories as repo;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, PhysicalPosition, WebviewWindow};

/// 收起态尺寸（逻辑像素）
pub const COLLAPSED_SIZE: (f64, f64) = (104.0, 104.0);
/// 展开态尺寸
pub const EXPANDED_SIZE: (f64, f64) = (372.0, 104.0);

const MARGIN_RIGHT: f64 = 18.0;
/// 距屏幕底部的偏移，避让任务栏 / Dock
const MARGIN_BOTTOM: f64 = 96.0;

/// 设置表中的键名
const ANCHOR_KEY: &str = "floating_anchor";

/// 自动贴边模式；用户拖过一次就置 false
static AUTO_ANCHOR: AtomicBool = AtomicBool::new(true);
/// 用户摆放的位置（物理像素）
static CUSTOM_POS: Mutex<Option<(i32, i32)>> = Mutex::new(None);
/// 程序自身 `set_position` 造成的 Moved 事件，在此时间点之前忽略
static SUPPRESS_MOVES_UNTIL: AtomicI64 = AtomicI64::new(0);

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 标记「接下来 800ms 内的 Moved 是程序改的，不是用户拖的」
fn suppress_moves() {
    SUPPRESS_MOVES_UNTIL.store(now_ms() + 800, Ordering::Relaxed);
}

/// 消耗一次抑制窗口（读一次即失效）
fn consume_suppress() -> bool {
    let deadline = SUPPRESS_MOVES_UNTIL.swap(0, Ordering::Relaxed);
    now_ms() < deadline
}

fn monitor_anchor(window: &WebviewWindow, size: (f64, f64)) -> tauri::Result<()> {
    let monitor = match window.current_monitor()? {
        Some(monitor) => Some(monitor),
        None => window.primary_monitor()?,
    };
    let Some(monitor) = monitor else {
        return Ok(());
    };
    let scale = monitor.scale_factor();
    let origin = monitor.position().to_logical::<f64>(scale);
    let area = monitor.size().to_logical::<f64>(scale);
    let x = origin.x + area.width - size.0 - MARGIN_RIGHT;
    let y = origin.y + area.height - size.1 - MARGIN_BOTTOM;
    suppress_moves();
    window.set_position(LogicalPosition::new(x, y))?;
    Ok(())
}

/// 把窗口限制在当前显示器可视区域内（拖到屏幕外就再也点不到了）
fn clamp_to_monitor(window: &WebviewWindow, size: (f64, f64)) -> tauri::Result<()> {
    let Some(monitor) = window.current_monitor()? else {
        return Ok(());
    };
    let scale = monitor.scale_factor();
    let origin = monitor.position().to_logical::<f64>(scale);
    let area = monitor.size().to_logical::<f64>(scale);
    let Ok(current) = window.outer_position() else {
        return Ok(());
    };
    let current = current.to_logical::<f64>(scale);
    let max_x = (origin.x + area.width - size.0).max(origin.x);
    let max_y = (origin.y + area.height - size.1).max(origin.y);
    let x = current.x.clamp(origin.x, max_x);
    let y = current.y.clamp(origin.y, max_y);
    if (x - current.x).abs() > 0.5 || (y - current.y).abs() > 0.5 {
        suppress_moves();
        window.set_position(LogicalPosition::new(x, y))?;
    }
    Ok(())
}

/// 调整尺寸。自定义位置下保持右边缘/底边不动，让图标停在原处向左展开。
fn resize(window: &WebviewWindow, size: (f64, f64), interactive: bool) -> tauri::Result<()> {
    let previous = window.outer_size().ok();
    window.set_size(LogicalSize::new(size.0, size.1))?;

    if AUTO_ANCHOR.load(Ordering::Relaxed) {
        monitor_anchor(window, size)?;
    } else if let (Some(previous), Ok(position)) = (previous, window.outer_position()) {
        let scale = window.scale_factor().unwrap_or(1.0);
        let previous = previous.to_logical::<f64>(scale);
        let delta_x = previous.width - size.0;
        let delta_y = previous.height - size.1;
        if delta_x.abs() > 0.5 || delta_y.abs() > 0.5 {
            let position = position.to_logical::<f64>(scale);
            suppress_moves();
            window.set_position(LogicalPosition::new(
                position.x + delta_x,
                position.y + delta_y,
            ))?;
        }
        clamp_to_monitor(window, size)?;
    }

    window.set_ignore_cursor_events(!interactive)?;
    Ok(())
}

/// 从设置表读回上次摆放的位置
fn load_saved_anchor(app: &AppHandle) -> Option<(i32, i32)> {
    let state = app.state::<crate::state::SharedState>().inner().clone();
    let raw = tauri::async_runtime::block_on(async move {
        let db = state.db().await;
        repo::settings::get(db.pool(), ANCHOR_KEY)
            .await
            .ok()
            .flatten()
    })?;
    let (x, y) = raw.split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

/// 初始化悬浮窗：恢复上次位置（没有就贴右下角）、默认穿透
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window("floating") {
        window.set_size(LogicalSize::new(COLLAPSED_SIZE.0, COLLAPSED_SIZE.1))?;
        // 双保险：除了 tauri.conf.json 的 transparent，再显式把窗口与
        // WebView2 的背景色置为全透明，避免图标背后出现一块白色矩形。
        let _ = window.set_background_color(Some(tauri::window::Color(0, 0, 0, 0)));

        match load_saved_anchor(app) {
            Some((x, y)) => {
                AUTO_ANCHOR.store(false, Ordering::Relaxed);
                *CUSTOM_POS.lock().expect("anchor lock") = Some((x, y));
                suppress_moves();
                window.set_position(PhysicalPosition::new(x, y))?;
                clamp_to_monitor(&window, COLLAPSED_SIZE)?;
                tracing::info!(
                    x,
                    y,
                    actual = ?window.outer_position().ok(),
                    "恢复悬浮窗位置"
                );
            }
            None => {
                monitor_anchor(&window, COLLAPSED_SIZE)?;
                tracing::info!("悬浮窗使用默认右下角锚点");
            }
        }

        // 初始穿透：避免启动时挡住桌面点击；光标轮询会按需恢复交互
        window.set_ignore_cursor_events(true)?;
        window.show()?;
    }
    Ok(())
}

/// 展开为横向胶囊
pub fn expand(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window("floating") {
        resize(&window, EXPANDED_SIZE, true)?;
        window.show()?;
        window.set_focus()?;
    }
    Ok(())
}

/// 收起为图标
pub fn collapse(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window("floating") {
        resize(&window, COLLAPSED_SIZE, false)?;
    }
    Ok(())
}

/// 复位到右下角并清除持久化位置（拖到屏幕外时用托盘菜单找回）
pub fn reset_position(app: &AppHandle) -> tauri::Result<()> {
    AUTO_ANCHOR.store(true, Ordering::Relaxed);
    *CUSTOM_POS.lock().expect("anchor lock") = None;

    let state = app.state::<crate::state::SharedState>().inner().clone();
    tauri::async_runtime::spawn(async move {
        let db = state.db().await;
        let _ = repo::settings::upsert(db.pool(), ANCHOR_KEY, "").await;
    });

    if let Some(window) = app.get_webview_window("floating") {
        let expanded = window
            .outer_size()
            .map(|size| size.width > (COLLAPSED_SIZE.0 as u32))
            .unwrap_or(false);
        let size = if expanded {
            EXPANDED_SIZE
        } else {
            COLLAPSED_SIZE
        };
        monitor_anchor(&window, size)?;
        window.show()?;
    }
    Ok(())
}

/// 监听用户拖动：把窗口从「自动贴边」切换成「记住这个位置」。
pub fn watch_moves(app: &AppHandle) {
    let Some(window) = app.get_webview_window("floating") else {
        return;
    };

    let handle = window.clone();
    window.on_window_event(move |event| {
        if !matches!(event, tauri::WindowEvent::Moved(_)) {
            return;
        }
        if consume_suppress() {
            return;
        }
        // 不用事件里带的坐标：部分 DPI 缩放下它与窗口真实物理位置差一个倍数，
        // 统一回读 `outer_position()`，保证与 setup 时的 `PhysicalPosition` 同一坐标系。
        let Ok(position) = handle.outer_position() else {
            return;
        };
        AUTO_ANCHOR.store(false, Ordering::Relaxed);
        *CUSTOM_POS.lock().expect("anchor lock") = Some((position.x, position.y));
    });

    // 记忆落盘：拖动过程中 Moved 会连续触发，只按 2s 周期写入最后一次
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut saved = String::new();
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let Some((x, y)) = *CUSTOM_POS.lock().expect("anchor lock") else {
                continue;
            };
            let value = format!("{x},{y}");
            if value == saved {
                continue;
            }
            let state = app.state::<crate::state::SharedState>().inner().clone();
            let db = state.db().await;
            if repo::settings::upsert(db.pool(), ANCHOR_KEY, &value)
                .await
                .is_ok()
            {
                saved = value;
            }
        }
    });
}

/// 启动光标热区轮询。
///
/// 光标进入窗口矩形 → 允许接收鼠标事件；离开 → 恢复穿透。
/// 只在 Windows 生效（macOS 无全局光标 API，且窗口足够小）。
pub fn start_hit_test(app: AppHandle) {
    if crate::platform::cursor_position().is_none() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let ignore = Arc::new(AtomicBool::new(true));
        loop {
            tokio::time::sleep(Duration::from_millis(120)).await;

            let Some(window) = app.get_webview_window("floating") else {
                continue;
            };
            let Ok(visible) = window.is_visible() else {
                continue;
            };
            if !visible {
                continue;
            }

            let Some((cursor_x, cursor_y)) = crate::platform::cursor_position() else {
                continue;
            };
            let (Ok(position), Ok(size)) = (window.outer_position(), window.outer_size()) else {
                continue;
            };

            let inside = cursor_x >= position.x
                && cursor_y >= position.y
                && cursor_x <= position.x + size.width as i32
                && cursor_y <= position.y + size.height as i32;

            let want_ignore = !inside;
            if want_ignore != ignore.load(Ordering::Relaxed)
                && window.set_ignore_cursor_events(want_ignore).is_ok()
            {
                ignore.store(want_ignore, Ordering::Relaxed);
            }
        }
    });
}
