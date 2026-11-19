//! 发布 / 环境相关命令。

use crate::services::release as release_service;
use crate::state::SharedState;
use buildship_common::AppError;
use buildship_domain::{
    CreateReleaseInput, EnvironmentWithTargets, Release, ReleaseDetail, RollbackInput,
    SaveEnvironmentInput, SaveTargetInput,
};
use tauri::{AppHandle, State};

#[tauri::command]
pub async fn env_list(
    state: State<'_, SharedState>,
    include_archived: Option<bool>,
) -> Result<Vec<EnvironmentWithTargets>, AppError> {
    let state = state.inner().clone();
    release_service::list_environments(&state, include_archived.unwrap_or(false)).await
}

#[tauri::command]
pub async fn env_save(
    state: State<'_, SharedState>,
    input: SaveEnvironmentInput,
) -> Result<(), AppError> {
    let state = state.inner().clone();
    release_service::save_environment(&state, input).await
}

#[tauri::command]
pub async fn env_archive(state: State<'_, SharedState>, id: String) -> Result<(), AppError> {
    let state = state.inner().clone();
    release_service::archive_environment(&state, &id).await
}

#[tauri::command]
pub async fn target_save(
    state: State<'_, SharedState>,
    input: SaveTargetInput,
) -> Result<(), AppError> {
    let state = state.inner().clone();
    release_service::save_target(&state, input).await
}

#[tauri::command]
pub async fn target_archive(state: State<'_, SharedState>, id: String) -> Result<(), AppError> {
    let state = state.inner().clone();
    release_service::archive_target(&state, &id).await
}

#[tauri::command]
pub async fn target_check(
    state: State<'_, SharedState>,
    id: String,
) -> Result<serde_json::Value, AppError> {
    let state = state.inner().clone();
    release_service::check_target(&state, &id).await
}

#[tauri::command]
pub async fn release_list(
    state: State<'_, SharedState>,
    project_id: Option<String>,
    environment_id: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<Release>, AppError> {
    let state = state.inner().clone();
    release_service::list_releases(&state, project_id, environment_id, limit.unwrap_or(100)).await
}

#[tauri::command]
pub async fn release_detail(
    state: State<'_, SharedState>,
    id: String,
) -> Result<ReleaseDetail, AppError> {
    let state = state.inner().clone();
    release_service::release_detail(&state, &id).await
}

#[tauri::command]
pub async fn release_create(
    app: AppHandle,
    state: State<'_, SharedState>,
    input: CreateReleaseInput,
) -> Result<Release, AppError> {
    let state = state.inner().clone();
    release_service::create_release(app, state, input).await
}

#[tauri::command]
pub async fn release_rollback(
    app: AppHandle,
    state: State<'_, SharedState>,
    input: RollbackInput,
) -> Result<Release, AppError> {
    let state = state.inner().clone();
    release_service::rollback(app, state, input).await
}

#[tauri::command]
pub async fn release_healthy_candidates(
    state: State<'_, SharedState>,
    project_id: String,
    environment_id: String,
) -> Result<Vec<Release>, AppError> {
    let state = state.inner().clone();
    release_service::healthy_candidates(&state, &project_id, &environment_id).await
}
