//! 系统级命令：启动引导、工具链、工作台汇总、设置、审计、备份。

use crate::services::settings as settings_service;
use crate::state::{AppState, SharedState};
use buildship_artifact::sha256_file;
use buildship_build::toolchain;
use buildship_common::AppError;
use buildship_db::repositories as repo;
use buildship_domain::{
    AppSetting, AuditEvent, AuditQuery, BackupEntry, BackupResult, BootstrapInfo, DashboardSummary,
    Page, SecretReference, SettingsPatch, ToolchainReport,
};
use std::path::Path;
use tauri::State;

fn platform_label() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// 启动引导：前端首屏需要的全部上下文一次返回
#[tauri::command]
pub async fn system_bootstrap(state: State<'_, SharedState>) -> Result<BootstrapInfo, AppError> {
    let state = state.inner().clone();
    let settings = settings_service::all(&state).await?;
    let toolchain = toolchain::detect().await;
    let (schema_version, db_path) = {
        let db = state.db().await;
        (db.schema_version(), state.paths.db_file())
    };

    // 前端首次成功拿到引导信息，说明 IPC 通路与运行时状态都已就绪；
    // 启动卡白屏/报 "state not managed" 时，这条日志有没有出现就是最快的判据。
    tracing::info!(schema = schema_version, "前端引导完成");

    Ok(BootstrapInfo {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        platform: platform_label().to_string(),
        data_dir: state.paths.root().to_string_lossy().to_string(),
        db_path: db_path.to_string_lossy().to_string(),
        schema_version,
        toolchain,
        settings,
        operator: state.operator.clone(),
    })
}

/// 重新体检工具链
#[tauri::command]
pub async fn system_toolchain() -> Result<ToolchainReport, AppError> {
    Ok(toolchain::detect().await)
}

/// 工作台汇总
#[tauri::command]
pub async fn dashboard_summary(
    state: State<'_, SharedState>,
) -> Result<DashboardSummary, AppError> {
    let state = state.inner().clone();
    let db = state.db().await;
    let pool = db.pool();
    Ok(DashboardSummary {
        project_count: repo::projects::count(pool, Some(buildship_domain::RecordStatus::Active))
            .await?,
        active_build_count: repo::builds::count_active(pool).await?,
        artifact_count: repo::artifacts::count(pool, None).await?,
        healthy_release_count: repo::releases::count_healthy(pool).await?,
        total_artifact_bytes: repo::artifacts::total_size(pool).await?,
        recent_projects: repo::projects::recent(pool, 6).await?,
        recent_builds: repo::builds::recent_tasks(pool, 6).await?,
        recent_releases: repo::releases::recent(pool, 6).await?,
    })
}

/// 设置列表
#[tauri::command]
pub async fn settings_get(state: State<'_, SharedState>) -> Result<Vec<AppSetting>, AppError> {
    let state = state.inner().clone();
    settings_service::all(&state).await
}

/// 批量写入设置
#[tauri::command]
pub async fn settings_update(
    state: State<'_, SharedState>,
    patch: SettingsPatch,
) -> Result<Vec<AppSetting>, AppError> {
    let state = state.inner().clone();
    settings_service::update(&state, patch).await
}

/// 审计列表
#[tauri::command]
pub async fn audit_list(
    state: State<'_, SharedState>,
    query: AuditQuery,
) -> Result<Page<AuditEvent>, AppError> {
    let state = state.inner().clone();
    let db = state.db().await;
    let items = repo::audit::list(db.pool(), &query).await?;
    let total = repo::audit::count(db.pool(), &query).await?;
    Ok(Page { items, total })
}

#[tauri::command]
pub async fn backup_list(state: State<'_, SharedState>) -> Result<Vec<BackupEntry>, AppError> {
    let state = state.inner().clone();
    settings_service::list_backups(&state).await
}

#[tauri::command]
pub async fn backup_create(state: State<'_, SharedState>) -> Result<BackupResult, AppError> {
    let state = state.inner().clone();
    settings_service::create_backup(&state).await
}

#[tauri::command]
pub async fn backup_restore(
    state: State<'_, SharedState>,
    path: String,
) -> Result<BackupResult, AppError> {
    let state = state.inner().clone();
    if !Path::new(&path).is_file() {
        return Err(AppError::not_found("备份文件不存在"));
    }
    settings_service::restore_backup(&state, &path).await
}

#[tauri::command]
pub async fn secret_list(state: State<'_, SharedState>) -> Result<Vec<SecretReference>, AppError> {
    let state = state.inner().clone();
    settings_service::list_secrets(&state).await
}

/// 保存 Agent Token（正文进系统凭据库）
#[tauri::command]
pub async fn secret_save(
    state: State<'_, SharedState>,
    label: String,
    token: String,
) -> Result<SecretReference, AppError> {
    let state = state.inner().clone();
    settings_service::save_secret(&state, &label, &token).await
}

/// 计算任意文件哈希（设置页做备份校验用）
#[tauri::command]
pub async fn file_sha256(path: String) -> Result<String, AppError> {
    tokio::task::spawn_blocking(move || sha256_file(Path::new(&path)))
        .await
        .map_err(|err| AppError::internal(format!("计算哈希失败：{err}")))?
}

/// 打开数据目录（便于用户备份/排障）
#[tauri::command]
pub async fn open_data_dir(
    app: tauri::AppHandle,
    state: State<'_, SharedState>,
) -> Result<(), AppError> {
    use tauri_plugin_opener::OpenerExt;
    let state = state.inner().clone();
    let path = state.paths.root().to_string_lossy().to_string();
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| AppError::io(format!("打开目录失败：{e}")))
}

/// 当前运行时状态（供设置页展示）
#[tauri::command]
pub async fn runtime_info(state: State<'_, SharedState>) -> Result<serde_json::Value, AppError> {
    let state: std::sync::Arc<AppState> = state.inner().clone();
    let active = state.builds.active().await;
    Ok(serde_json::json!({
        "operator": state.operator,
        "dataDir": state.paths.root().to_string_lossy(),
        "activeBuild": active.map(|item| serde_json::json!({
            "taskId": item.task_id,
            "projectId": item.project_id,
            "projectName": item.project_name,
        })),
    }))
}
