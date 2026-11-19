//! 项目相关命令。

use crate::services::{project as project_service, quick};
use crate::state::SharedState;
use buildship_common::AppError;
use buildship_db::repositories as repo;
use buildship_domain::{
    AddProjectInput, BuildProfile, Project, QuickState, RecordStatus, ScanResult,
    UpdateProjectInput,
};
use tauri::{AppHandle, State};

#[tauri::command]
pub async fn project_list(
    state: State<'_, SharedState>,
    status: Option<RecordStatus>,
) -> Result<Vec<Project>, AppError> {
    let state = state.inner().clone();
    project_service::list(&state, status).await
}

#[tauri::command]
pub async fn project_scan(path: String) -> Result<ScanResult, AppError> {
    project_service::scan(&path).await
}

#[tauri::command]
pub async fn project_add(
    app: AppHandle,
    state: State<'_, SharedState>,
    input: AddProjectInput,
) -> Result<Project, AppError> {
    let state = state.inner().clone();
    let project = project_service::add(&state, input).await?;
    state
        .set_selected_project(Some(project.id.clone()), Some(project.name.clone()))
        .await;
    {
        let db = state.db().await;
        repo::settings::upsert(db.pool(), "selected_project_id", &project.id).await?;
    }
    quick::emit(&app, &state).await;
    Ok(project)
}

#[tauri::command]
pub async fn project_update(
    state: State<'_, SharedState>,
    input: UpdateProjectInput,
) -> Result<Project, AppError> {
    let state = state.inner().clone();
    project_service::update(&state, input).await
}

#[tauri::command]
pub async fn project_archive(
    app: AppHandle,
    state: State<'_, SharedState>,
    id: String,
) -> Result<(), AppError> {
    let state = state.inner().clone();
    project_service::archive(&state, &id).await?;
    let context = state.quick_snapshot().await;
    if context.project_id.as_deref() == Some(id.as_str()) {
        state.set_selected_project(None, None).await;
    }
    quick::emit(&app, &state).await;
    Ok(())
}

#[tauri::command]
pub async fn project_profiles(
    state: State<'_, SharedState>,
    project_id: String,
) -> Result<Vec<BuildProfile>, AppError> {
    let state = state.inner().clone();
    project_service::profiles(&state, &project_id).await
}

#[tauri::command]
pub async fn quick_state_get(state: State<'_, SharedState>) -> Result<QuickState, AppError> {
    let state = state.inner().clone();
    Ok(quick::snapshot(&state).await)
}

/// 切换当前项目（悬浮栏与快捷构建都依赖它）
#[tauri::command]
pub async fn quick_set_project(
    app: AppHandle,
    state: State<'_, SharedState>,
    project_id: String,
) -> Result<(), AppError> {
    let state = state.inner().clone();
    let project = {
        let db = state.db().await;
        repo::projects::require(db.pool(), &project_id).await?
    };
    state
        .set_selected_project(Some(project.id.clone()), Some(project.name.clone()))
        .await;
    {
        let db = state.db().await;
        repo::settings::upsert(db.pool(), "selected_project_id", &project.id).await?;
    }
    quick::emit(&app, &state).await;
    Ok(())
}
