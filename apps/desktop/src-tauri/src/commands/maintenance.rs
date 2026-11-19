//! 维护相关命令：清理本机数据、一键卸载。

use crate::services::maintenance::{self, CleanSummary};
use crate::state::SharedState;
use buildship_common::AppError;
use std::time::Duration;
use tauri::{AppHandle, State};

/// 清理本机数据并重启为全新状态。
#[tauri::command]
pub async fn maintenance_clean(
    app: AppHandle,
    state: State<'_, SharedState>,
    backup_first: bool,
) -> Result<CleanSummary, AppError> {
    let state = state.inner().clone();
    let summary = maintenance::clean_data(&state, backup_first).await?;
    // 先让前端把结果提示渲染出来，再重启进程
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(900)).await;
        app.restart();
    });
    Ok(summary)
}

/// 一键卸载：清数据 → 启动安装包自带的卸载程序 → 退出。
#[tauri::command]
pub async fn maintenance_uninstall(
    app: AppHandle,
    state: State<'_, SharedState>,
    backup_first: bool,
) -> Result<CleanSummary, AppError> {
    let state = state.inner().clone();
    let summary = maintenance::uninstall(&state, backup_first).await?;
    // 有卸载器时它会以静默模式结束本进程；这里再做一次兜底退出，避免应用卡在半路
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        app.exit(0);
    });
    Ok(summary)
}
