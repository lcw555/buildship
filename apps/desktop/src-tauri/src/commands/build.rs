//! 构建相关命令。

use crate::services::build as build_service;
use crate::state::SharedState;
use buildship_common::AppError;
use buildship_domain::{BuildLogChunk, BuildQuery, BuildTask, LogQuery};
use tauri::{AppHandle, State};

#[tauri::command]
pub async fn build_start(
    app: AppHandle,
    state: State<'_, SharedState>,
    project_id: String,
) -> Result<BuildTask, AppError> {
    let state = state.inner().clone();
    build_service::start(app, state, project_id).await
}

#[tauri::command]
pub async fn build_cancel(state: State<'_, SharedState>, task_id: String) -> Result<(), AppError> {
    let state = state.inner().clone();
    build_service::cancel(&state, &task_id).await
}

#[tauri::command]
pub async fn build_list(
    state: State<'_, SharedState>,
    query: BuildQuery,
) -> Result<Vec<BuildTask>, AppError> {
    let state = state.inner().clone();
    build_service::list(&state, query).await
}

#[tauri::command]
pub async fn build_get(
    state: State<'_, SharedState>,
    task_id: String,
) -> Result<BuildTask, AppError> {
    let state = state.inner().clone();
    build_service::get(&state, &task_id).await
}

#[tauri::command]
pub async fn build_logs(
    state: State<'_, SharedState>,
    query: LogQuery,
) -> Result<BuildLogChunk, AppError> {
    let state = state.inner().clone();
    build_service::read_log(&state, query).await
}

/// 打开日志所在目录
#[tauri::command]
pub async fn build_open_log_dir(
    app: AppHandle,
    state: State<'_, SharedState>,
    task_id: String,
) -> Result<(), AppError> {
    use tauri_plugin_opener::OpenerExt;
    let state = state.inner().clone();
    let task = build_service::get(&state, &task_id).await?;
    let dir = task
        .log_path
        .as_deref()
        .and_then(|path| {
            std::path::Path::new(path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| state.paths.build_logs_dir().to_string_lossy().to_string());
    app.opener()
        .open_path(dir, None::<&str>)
        .map_err(|e| AppError::io(format!("打开日志目录失败：{e}")))
}
