//! 发布包相关命令。

use crate::services::artifact as artifact_service;
use crate::state::SharedState;
use buildship_common::AppError;
use buildship_domain::{Artifact, ArtifactVerifyResult};
use tauri::{AppHandle, State};

#[tauri::command]
pub async fn artifact_list(
    state: State<'_, SharedState>,
    project_id: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<Artifact>, AppError> {
    let state = state.inner().clone();
    artifact_service::list(&state, project_id, limit.unwrap_or(100)).await
}

#[tauri::command]
pub async fn artifact_create(
    app: AppHandle,
    state: State<'_, SharedState>,
    build_task_id: String,
) -> Result<Artifact, AppError> {
    let state = state.inner().clone();
    artifact_service::create(&app, &state, &build_task_id).await
}

#[tauri::command]
pub async fn artifact_get(state: State<'_, SharedState>, id: String) -> Result<Artifact, AppError> {
    let state = state.inner().clone();
    artifact_service::get(&state, &id).await
}

#[tauri::command]
pub async fn artifact_verify(
    state: State<'_, SharedState>,
    id: String,
) -> Result<ArtifactVerifyResult, AppError> {
    let state = state.inner().clone();
    artifact_service::verify(&state, &id).await
}

#[tauri::command]
pub async fn artifact_delete(state: State<'_, SharedState>, id: String) -> Result<(), AppError> {
    let state = state.inner().clone();
    artifact_service::remove(&state, &id).await
}

/// 在资源管理器中定位发布包
#[tauri::command]
pub async fn artifact_open_dir(
    app: AppHandle,
    state: State<'_, SharedState>,
    id: String,
) -> Result<(), AppError> {
    use tauri_plugin_opener::OpenerExt;
    let state = state.inner().clone();
    let artifact = artifact_service::get(&state, &id).await?;
    let dir = std::path::Path::new(&artifact.file_path)
        .parent()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|| state.paths.artifacts_dir().to_string_lossy().to_string());
    app.opener()
        .open_path(dir, None::<&str>)
        .map_err(|e| AppError::io(format!("打开目录失败：{e}")))
}
