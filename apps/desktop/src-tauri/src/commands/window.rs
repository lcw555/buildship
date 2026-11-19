//! 窗口相关命令：主面板显示、悬浮栏展开/收起。

use crate::platform::floating;
use buildship_common::AppError;
use tauri::{AppHandle, Manager};

/// 显示并聚焦主面板
#[tauri::command]
pub fn window_show_main(app: AppHandle) -> Result<(), AppError> {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    Ok(())
}

/// 展开悬浮栏（向左展开横向胶囊）
#[tauri::command]
pub fn floating_expand(app: AppHandle) -> Result<(), AppError> {
    floating::expand(&app).map_err(|e| AppError::internal(format!("展开悬浮栏失败：{e}")))
}

/// 收起悬浮栏（只保留图标）
#[tauri::command]
pub fn floating_collapse(app: AppHandle) -> Result<(), AppError> {
    floating::collapse(&app).map_err(|e| AppError::internal(format!("收起悬浮栏失败：{e}")))
}

/// 允许 / 禁止鼠标穿透（前端在过渡动画期间临时切换）
#[tauri::command]
pub fn floating_set_ignore_cursor(app: AppHandle, ignore: bool) -> Result<(), AppError> {
    if let Some(window) = app.get_webview_window("floating") {
        window
            .set_ignore_cursor_events(ignore)
            .map_err(|e| AppError::internal(format!("设置鼠标穿透失败：{e}")))?;
    }
    Ok(())
}

/// 主面板最小化到托盘
#[tauri::command]
pub fn window_hide_main(app: AppHandle) -> Result<(), AppError> {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    Ok(())
}
