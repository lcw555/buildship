//! 发布服务：环境/目标管理、发布编排、健康检查、自动与手动回滚。
//!
//! 发布流程（方案 4.5）：
//!   校验包 → 检查 Agent → 建 Release → 上传 → Agent 校验解压 → 切换 → 健康检查
//!   → 成功标记 healthy 并切 current；失败则恢复上一健康版本。

use crate::events;
use crate::infrastructure::credential;
use crate::services::audit::{self, AuditInput};
use crate::state::{AppState, SharedState};
use buildship_common::{time, AppError, AppResult, ErrorCode};
use buildship_db::repositories as repo;
use buildship_domain::{
    state as fsm, Artifact, AuditAction, CreateReleaseInput, DeployEvent, DeploymentKind,
    DeploymentStatus, DeploymentTarget, EnvironmentWithTargets, EventLevel, HealthStatus,
    RecordStatus, Release, ReleaseDetail, ReleaseStatus, RollbackInput, SaveEnvironmentInput,
    SaveTargetInput,
};
use buildship_release::{ApplyRequest, DeployClient, RollbackRequest};
use std::path::Path;
use tauri::AppHandle;

// ---------------------------------------------------------------------------
// 环境与目标
// ---------------------------------------------------------------------------

pub async fn list_environments(
    state: &AppState,
    include_archived: bool,
) -> AppResult<Vec<EnvironmentWithTargets>> {
    let db = state.db().await;
    let pool = db.pool();
    let environments = repo::deployments::list_envs(pool, include_archived).await?;
    let mut result = Vec::with_capacity(environments.len());
    for environment in environments {
        let targets =
            repo::deployments::list_targets(pool, Some(&environment.id), include_archived).await?;
        result.push(EnvironmentWithTargets {
            environment,
            targets,
        });
    }
    Ok(result)
}

pub async fn save_environment(state: &AppState, input: SaveEnvironmentInput) -> AppResult<()> {
    if input.name.trim().is_empty() {
        return Err(AppError::invalid("环境名称不能为空"));
    }
    let db = state.db().await;
    let pool = db.pool();
    let now = time::now();

    let mut tx = pool.begin().await?;
    match input.id.clone() {
        Some(id) => {
            repo::deployments::update_env(
                &mut *tx,
                &id,
                input.name.trim(),
                input.kind,
                input.description.as_deref(),
            )
            .await?;
            repo::audit::append(
                &mut *tx,
                &audit::event(AuditInput {
                    actor: &state.operator,
                    action: AuditAction::EnvironmentSaved,
                    entity_type: "deployment_environment",
                    entity_id: &id,
                    project_id: None,
                    summary: format!("保存环境：{}", input.name),
                    detail: None,
                }),
            )
            .await?;
        }
        None => {
            let environment = buildship_domain::DeploymentEnvironment {
                id: uuid::Uuid::new_v4().to_string(),
                name: input.name.trim().to_string(),
                kind: input.kind,
                description: input.description.clone(),
                status: RecordStatus::Active,
                created_at: now,
                updated_at: now,
            };
            repo::deployments::insert_env(&mut *tx, &environment).await?;
            repo::audit::append(
                &mut *tx,
                &audit::event(AuditInput {
                    actor: &state.operator,
                    action: AuditAction::EnvironmentSaved,
                    entity_type: "deployment_environment",
                    entity_id: &environment.id,
                    project_id: None,
                    summary: format!("新增环境：{}", environment.name),
                    detail: Some(serde_json::json!({ "kind": input.kind })),
                }),
            )
            .await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

pub async fn archive_environment(state: &AppState, id: &str) -> AppResult<()> {
    let db = state.db().await;
    let pool = db.pool();
    if repo::releases::count_current_by_env(pool, id).await? > 0 {
        return Err(AppError::conflict(
            "该环境下仍有线上版本，请先切换或回滚后再归档",
        ));
    }
    let mut tx = pool.begin().await?;
    repo::deployments::set_env_status(&mut *tx, id, RecordStatus::Archived).await?;
    repo::audit::append(
        &mut *tx,
        &audit::event(AuditInput {
            actor: &state.operator,
            action: AuditAction::EnvironmentArchived,
            entity_type: "deployment_environment",
            entity_id: id,
            project_id: None,
            summary: "归档发布环境".to_string(),
            detail: None,
        }),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn save_target(state: &AppState, input: SaveTargetInput) -> AppResult<()> {
    if input.name.trim().is_empty() {
        return Err(AppError::invalid("目标名称不能为空"));
    }
    if input.iis_site.trim().is_empty() {
        return Err(AppError::invalid("IIS 站点名称不能为空"));
    }
    if input.physical_path.trim().is_empty() {
        return Err(AppError::invalid("站点物理目录不能为空"));
    }
    if !input.agent_base_url.starts_with("http://") && !input.agent_base_url.starts_with("https://")
    {
        return Err(AppError::invalid(
            "Agent 地址必须以 http:// 或 https:// 开头",
        ));
    }

    let db = state.db().await;
    let pool = db.pool();
    let environment = repo::deployments::get_env(pool, &input.environment_id)
        .await?
        .ok_or_else(|| AppError::not_found("发布环境不存在"))?;

    let now = time::now();
    let target = DeploymentTarget {
        id: input
            .id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        environment_id: environment.id.clone(),
        name: input.name.trim().to_string(),
        server_host: input.server_host.clone(),
        agent_base_url: input.agent_base_url.trim_end_matches('/').to_string(),
        iis_site: input.iis_site.trim().to_string(),
        physical_path: input.physical_path.trim().to_string(),
        health_check_url: input
            .health_check_url
            .clone()
            .filter(|value| !value.trim().is_empty()),
        credential_ref: input.credential_ref.clone(),
        tls_verify: input.tls_verify,
        status: RecordStatus::Active,
        last_checked_at: None,
        last_check_ok: None,
        created_at: now,
        updated_at: now,
    };

    let mut tx = pool.begin().await?;
    if input.id.is_some() {
        repo::deployments::update_target(&mut *tx, &target).await?;
    } else {
        repo::deployments::insert_target(&mut *tx, &target).await?;
    }
    repo::audit::append(
        &mut *tx,
        &audit::event(AuditInput {
            actor: &state.operator,
            action: AuditAction::EnvironmentSaved,
            entity_type: "deployment_target",
            entity_id: &target.id,
            project_id: None,
            summary: format!("保存部署目标：{}", target.name),
            detail: Some(serde_json::json!({
                "agentBaseUrl": target.agent_base_url,
                "iisSite": target.iis_site,
                "physicalPath": target.physical_path,
            })),
        }),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn archive_target(state: &AppState, id: &str) -> AppResult<()> {
    let db = state.db().await;
    let mut tx = db.pool().begin().await?;
    repo::deployments::set_target_status(&mut *tx, id, RecordStatus::Archived).await?;
    repo::audit::append(
        &mut *tx,
        &audit::event(AuditInput {
            actor: &state.operator,
            action: AuditAction::EnvironmentArchived,
            entity_type: "deployment_target",
            entity_id: id,
            project_id: None,
            summary: "归档部署目标".to_string(),
            detail: None,
        }),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// 连通性检查：读取 Agent 状态
pub async fn check_target(state: &AppState, id: &str) -> AppResult<serde_json::Value> {
    let target = {
        let db = state.db().await;
        repo::deployments::require_target(db.pool(), id).await?
    };
    let result = async {
        let client = build_client(state, &target).await?;
        let status = client.status().await?;
        Ok::<_, AppError>(status)
    }
    .await;

    let (ok, message) = match &result {
        Ok(status) => (
            true,
            format!(
                "Agent {} 在线，已纳管 {} 个站点",
                status.agent_version,
                status.sites.len()
            ),
        ),
        Err(err) => (false, err.to_string()),
    };

    let db = state.db().await;
    repo::deployments::update_target_check(db.pool(), id, ok, time::now()).await?;

    Ok(serde_json::json!({ "ok": ok, "message": message }))
}

// ---------------------------------------------------------------------------
// 发布
// ---------------------------------------------------------------------------

async fn build_client(state: &AppState, target: &DeploymentTarget) -> AppResult<DeployClient> {
    let token = match &target.credential_ref {
        Some(reference_id) => {
            let reference = {
                let db = state.db().await;
                repo::settings::get_secret_ref(db.pool(), reference_id).await?
            };
            let reference = reference.ok_or_else(|| {
                AppError::new(
                    ErrorCode::CredentialUnavailable,
                    "部署目标引用的凭据已不存在",
                )
            })?;
            credential::load(&reference.label)?
        }
        None => {
            return Err(AppError::new(
                ErrorCode::CredentialUnavailable,
                "部署目标未配置访问凭据，请先在环境管理中保存 Agent Token",
            ))
        }
    };
    DeployClient::new(&target.agent_base_url, &token, target.tls_verify)
}

/// 创建发布并异步执行
pub async fn create_release(
    app: AppHandle,
    state: SharedState,
    input: CreateReleaseInput,
) -> AppResult<Release> {
    let artifact = {
        let db = state.db().await;
        repo::artifacts::require(db.pool(), &input.artifact_id).await?
    };
    let target = {
        let db = state.db().await;
        repo::deployments::require_target(db.pool(), &input.target_id).await?
    };
    let project = {
        let db = state.db().await;
        repo::projects::require(db.pool(), &artifact.project_id).await?
    };

    // 1. 包完整性（哈希 + manifest）
    let report = {
        let path = artifact.file_path.clone();
        let expected = artifact.sha256.clone();
        tokio::task::spawn_blocking(move || {
            buildship_artifact::verify_package(Path::new(&path), &expected)
        })
        .await
        .map_err(|err| AppError::internal(format!("校验任务异常：{err}")))??
    };
    if !report.ok {
        return Err(AppError::new(ErrorCode::ChecksumMismatch, report.message));
    }

    // 2. Agent 在线检查
    let client = build_client(&state, &target).await?;
    let status = client.status().await.map_err(|err| {
        AppError::new(
            ErrorCode::AgentUnavailable,
            format!("部署目标 Agent 不可用：{}", err.message),
        )
    })?;

    // 3. 版本号
    let version = input
        .version
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| buildship_common::time::compact_stamp(buildship_common::time::now()));

    let previous = {
        let db = state.db().await;
        repo::releases::current(db.pool(), &project.id, &target.environment_id).await?
    };

    let now = time::now();
    let release = Release {
        id: uuid::Uuid::new_v4().to_string(),
        project_id: project.id.clone(),
        artifact_id: artifact.id.clone(),
        environment_id: target.environment_id.clone(),
        target_id: target.id.clone(),
        version: version.clone(),
        deploy_path: Some(target.physical_path.clone()),
        status: ReleaseStatus::Draft,
        previous_release_id: previous.as_ref().map(|item| item.id.clone()),
        rolled_back_from: None,
        rollback_reason: None,
        health_status: HealthStatus::Unknown,
        health_message: None,
        is_current: false,
        operator: Some(state.operator.clone()),
        started_at: Some(now),
        finished_at: None,
        created_at: now,
        updated_at: now,
    };

    let deployment = buildship_domain::DeploymentTask {
        id: uuid::Uuid::new_v4().to_string(),
        release_id: release.id.clone(),
        target_id: target.id.clone(),
        project_id: project.id.clone(),
        kind: DeploymentKind::Deploy,
        status: DeploymentStatus::Queued,
        stage: Some("排队中".to_string()),
        progress: 0,
        request_id: uuid::Uuid::new_v4().to_string(),
        idempotency_key: Some(format!("{}:{}", artifact.sha256, target.id)),
        error_code: None,
        error_message: None,
        started_at: None,
        finished_at: None,
        created_at: now,
        updated_at: now,
    };

    {
        let db = state.db().await;
        let mut tx = db.pool().begin().await?;
        repo::releases::insert(&mut *tx, &release).await?;
        repo::deployments::insert_deployment(&mut *tx, &deployment).await?;
        repo::releases::append_event(
            &mut *tx,
            &release.id,
            "create",
            EventLevel::Info,
            &format!("创建发布 {} → {}", release.version, target.iis_site),
            Some(
                &serde_json::json!({
                    "artifact": artifact.file_name,
                    "agent": status.hostname,
                    "previous": previous.as_ref().map(|item| item.version.clone()),
                })
                .to_string(),
            ),
        )
        .await?;
        repo::audit::append(
            &mut *tx,
            &audit::event(AuditInput {
                actor: &state.operator,
                action: AuditAction::ReleaseCreated,
                entity_type: "release",
                entity_id: &release.id,
                project_id: Some(&project.id),
                summary: format!(
                    "创建发布：{} → {}（{}）",
                    project.name, version, target.iis_site
                ),
                detail: Some(serde_json::json!({
                    "artifactSha256": artifact.sha256,
                    "environmentId": target.environment_id,
                    "targetId": target.id,
                })),
            }),
        )
        .await?;
        tx.commit().await?;
    }

    let app_handle = app.clone();
    let state_clone = state.clone();
    let release_run = release.clone();
    let deployment_run = deployment.clone();
    let artifact_run = artifact.clone();
    let target_run = target.clone();
    let project_name = project.name.clone();
    tokio::spawn(async move {
        let result = deploy_release(
            app_handle.clone(),
            state_clone.clone(),
            release_run.clone(),
            deployment_run.clone(),
            target_run,
            artifact_run,
            previous,
            project_name,
        )
        .await;
        if let Err(err) = result {
            tracing::error!(release = %release_run.id, error = %err, "发布执行异常");
            let _ = fail_release(
                &app_handle,
                &state_clone,
                &release_run.id,
                &deployment_run.id,
                &err,
                false,
            )
            .await;
        }
    });

    Ok(release)
}

#[allow(clippy::too_many_arguments)]
async fn deploy_release(
    app: AppHandle,
    state: SharedState,
    release: Release,
    deployment: buildship_domain::DeploymentTask,
    target: DeploymentTarget,
    artifact: Artifact,
    previous: Option<Release>,
    project_name: String,
) -> AppResult<()> {
    let client = build_client(&state, &target).await?;

    // 上传
    advance(
        &state,
        &app,
        &release.id,
        &deployment.id,
        ReleaseStatus::Uploading,
        DeploymentStatus::Uploading,
        "上传发布包",
        10,
    )
    .await?;

    let upload = client
        .upload(
            &release.id,
            Path::new(&artifact.file_path),
            &artifact.sha256,
        )
        .await;
    let upload = match upload {
        Ok(value) => value,
        Err(err) => {
            fail_release(&app, &state, &release.id, &deployment.id, &err, false).await?;
            return Ok(());
        }
    };

    // 切换版本
    advance(
        &state,
        &app,
        &release.id,
        &deployment.id,
        ReleaseStatus::Deploying,
        DeploymentStatus::Deploying,
        "切换 IIS 站点版本",
        60,
    )
    .await?;

    let apply = ApplyRequest {
        release_id: release.id.clone(),
        site: target.iis_site.clone(),
        physical_path: target.physical_path.clone(),
        health_check_url: target.health_check_url.clone(),
        tree_sha256: upload.tree_sha256.clone(),
        request_id: deployment.request_id.clone(),
    };

    let response = match client.apply(&apply).await {
        Ok(value) => value,
        Err(err) => {
            fail_release(&app, &state, &release.id, &deployment.id, &err, false).await?;
            return Ok(());
        }
    };

    // 健康检查结果
    {
        let db = state.db().await;
        let mut tx = db.pool().begin().await?;
        repo::releases::append_event(
            &mut *tx,
            &release.id,
            "health_check",
            if response.health_status == "passed" {
                EventLevel::Info
            } else {
                EventLevel::Error
            },
            &format!(
                "健康检查{}：{}",
                if response.health_status == "passed" {
                    "通过"
                } else {
                    "失败"
                },
                response
                    .health_message
                    .clone()
                    .unwrap_or_else(|| "无详情".to_string())
            ),
            None,
        )
        .await?;
        repo::releases::set_health(
            &mut *tx,
            &release.id,
            if response.health_status == "passed" {
                HealthStatus::Passed
            } else {
                HealthStatus::Failed
            },
            response.health_message.as_deref(),
        )
        .await?;
        repo::releases::set_deploy_path(&mut *tx, &release.id, &response.current_path).await?;
        tx.commit().await?;
    }

    if response.health_status != "passed" {
        let err = AppError::new(
            ErrorCode::HealthCheckFailed,
            response
                .health_message
                .clone()
                .unwrap_or_else(|| "健康检查未通过".to_string()),
        );
        fail_release(
            &app,
            &state,
            &release.id,
            &deployment.id,
            &err,
            response.rolled_back,
        )
        .await?;
        return Ok(());
    }

    // 成功：切换 current 指针
    let finished_at = time::now();
    {
        let db = state.db().await;
        let mut tx = db.pool().begin().await?;
        fsm::ensure_release_transition(ReleaseStatus::Deploying, ReleaseStatus::HealthCheck)?;
        repo::releases::update_status(&mut *tx, &release.id, ReleaseStatus::HealthCheck, None)
            .await?;
        repo::releases::update_status(
            &mut *tx,
            &release.id,
            ReleaseStatus::Healthy,
            Some(finished_at),
        )
        .await?;
        repo::releases::clear_current(&mut *tx, &release.project_id, &release.environment_id)
            .await?;
        repo::releases::mark_current(&mut *tx, &release.id).await?;
        repo::deployments::update_deployment(
            &mut *tx,
            &deployment.id,
            DeploymentStatus::Succeeded,
            Some("发布完成"),
            100,
            None,
            None,
            Some(finished_at),
        )
        .await?;
        repo::releases::append_event(
            &mut *tx,
            &release.id,
            "healthy",
            EventLevel::Info,
            "发布完成，站点已切换到新版本",
            None,
        )
        .await?;
        repo::audit::append(
            &mut *tx,
            &audit::event(AuditInput {
                actor: &state.operator,
                action: AuditAction::ReleaseSucceeded,
                entity_type: "release",
                entity_id: &release.id,
                project_id: Some(&release.project_id),
                summary: format!(
                    "发布成功：{} {} → {}",
                    project_name, release.version, target.iis_site
                ),
                detail: Some(serde_json::json!({
                    "artifactId": artifact.id,
                    "sha256": artifact.sha256,
                    "deployPath": response.current_path,
                    "previousRelease": previous.as_ref().map(|item| item.version.clone()),
                })),
            }),
        )
        .await?;
        tx.commit().await?;
    }

    emit(
        &app,
        &release.id,
        &deployment.id,
        DeploymentStatus::Succeeded,
        ReleaseStatus::Healthy,
        "发布完成",
        100,
        Some("站点已切换到新版本".to_string()),
        None,
    );
    Ok(())
}

/// 推进阶段（同时写 Release 与 DeploymentTask，并推送事件）
#[allow(clippy::too_many_arguments)]
async fn advance(
    state: &AppState,
    app: &AppHandle,
    release_id: &str,
    deployment_id: &str,
    release_status: ReleaseStatus,
    deployment_status: DeploymentStatus,
    stage: &str,
    progress: i64,
) -> AppResult<()> {
    {
        let db = state.db().await;
        let mut tx = db.pool().begin().await?;
        let current = repo::releases::require(&mut *tx, release_id).await?;
        fsm::ensure_release_transition(current.status, release_status)?;
        repo::releases::update_status(&mut *tx, release_id, release_status, None).await?;
        repo::deployments::update_deployment(
            &mut *tx,
            deployment_id,
            deployment_status,
            Some(stage),
            progress,
            None,
            None,
            None,
        )
        .await?;
        repo::releases::append_event(&mut *tx, release_id, stage, EventLevel::Info, stage, None)
            .await?;
        tx.commit().await?;
    }
    emit(
        app,
        release_id,
        deployment_id,
        deployment_status,
        release_status,
        stage,
        progress,
        Some(stage.to_string()),
        None,
    );
    Ok(())
}

/// 失败收尾：记录错误，必要时恢复上一健康版本
async fn fail_release(
    app: &AppHandle,
    state: &AppState,
    release_id: &str,
    deployment_id: &str,
    err: &AppError,
    agent_rolled_back: bool,
) -> AppResult<()> {
    let finished_at = time::now();
    let release = {
        let db = state.db().await;
        let mut tx = db.pool().begin().await?;
        let release = repo::releases::require(&mut *tx, release_id).await?;
        repo::releases::update_status(
            &mut *tx,
            release_id,
            ReleaseStatus::Failed,
            Some(finished_at),
        )
        .await?;
        repo::deployments::update_deployment(
            &mut *tx,
            deployment_id,
            DeploymentStatus::Failed,
            Some("发布失败"),
            100,
            Some("RELEASE_FAILED"),
            Some(&err.message),
            Some(finished_at),
        )
        .await?;
        repo::releases::append_event(
            &mut *tx,
            release_id,
            "failed",
            EventLevel::Error,
            &format!("发布失败：{}", err.message),
            None,
        )
        .await?;

        // Agent 已自动恢复上一版本时，本地指针同步回退
        if agent_rolled_back {
            if let Some(previous_id) = release.previous_release_id.clone() {
                repo::releases::clear_current(
                    &mut *tx,
                    &release.project_id,
                    &release.environment_id,
                )
                .await?;
                repo::releases::mark_current(&mut *tx, &previous_id).await?;
            }
        }

        repo::audit::append(
            &mut *tx,
            &audit::event(AuditInput {
                actor: &state.operator,
                action: AuditAction::ReleaseFailed,
                entity_type: "release",
                entity_id: release_id,
                project_id: Some(&release.project_id),
                summary: format!("发布失败：{}（{}）", release.version, err.message),
                detail: Some(serde_json::json!({
                    "code": err.code.as_str(),
                    "detail": err.detail,
                    "agentRolledBack": agent_rolled_back,
                })),
            }),
        )
        .await?;
        tx.commit().await?;
        release
    };

    emit(
        app,
        release_id,
        deployment_id,
        DeploymentStatus::Failed,
        ReleaseStatus::Failed,
        "发布失败",
        100,
        Some(err.message.clone()),
        Some(err.code.as_str().to_string()),
    );
    tracing::error!(release = %release.id, error = %err, "发布失败");
    Ok(())
}

/// 手动回滚到历史健康版本
pub async fn rollback(
    app: AppHandle,
    state: SharedState,
    input: RollbackInput,
) -> AppResult<Release> {
    if input.reason.trim().len() < 2 {
        return Err(AppError::invalid("请填写回滚原因（至少 2 个字符）"));
    }

    let (target_release, current, target, deployment) = {
        let db = state.db().await;
        let pool = db.pool();
        let target_release = repo::releases::require(pool, &input.target_release_id).await?;
        let current = repo::releases::current(
            pool,
            &target_release.project_id,
            &target_release.environment_id,
        )
        .await?;
        fsm::ensure_rollback_target(
            target_release.status,
            target_release.is_current,
            &target_release.id,
            current.as_ref().map(|item| item.id.as_str()),
        )?;
        let target = repo::deployments::require_target(pool, &target_release.target_id).await?;
        let now = time::now();
        let deployment = buildship_domain::DeploymentTask {
            id: uuid::Uuid::new_v4().to_string(),
            release_id: target_release.id.clone(),
            target_id: target.id.clone(),
            project_id: target_release.project_id.clone(),
            kind: DeploymentKind::Rollback,
            status: DeploymentStatus::Queued,
            stage: Some("回滚排队".to_string()),
            progress: 0,
            request_id: uuid::Uuid::new_v4().to_string(),
            idempotency_key: Some(format!(
                "rollback:{}:{}",
                target_release.id,
                now.timestamp()
            )),
            error_code: None,
            error_message: None,
            started_at: None,
            finished_at: None,
            created_at: now,
            updated_at: now,
        };

        let mut tx = pool.begin().await?;
        repo::deployments::insert_deployment(&mut *tx, &deployment).await?;
        repo::releases::append_event(
            &mut *tx,
            &target_release.id,
            "rollback",
            EventLevel::Warn,
            &format!(
                "发起回滚：{} → 目标版本 {}，原因：{}",
                current
                    .as_ref()
                    .map(|item| item.version.clone())
                    .unwrap_or_default(),
                target_release.version,
                input.reason
            ),
            None,
        )
        .await?;
        repo::audit::append(
            &mut *tx,
            &audit::event(AuditInput {
                actor: &state.operator,
                action: AuditAction::ReleaseRolledBack,
                entity_type: "release",
                entity_id: &target_release.id,
                project_id: Some(&target_release.project_id),
                summary: format!("回滚到版本 {}", target_release.version),
                detail: Some(serde_json::json!({
                    "from": current.as_ref().map(|item| item.version.clone()),
                    "to": target_release.version,
                    "reason": input.reason,
                })),
            }),
        )
        .await?;
        tx.commit().await?;
        (target_release, current, target, deployment)
    };

    let app_handle = app.clone();
    let state_clone = state.clone();
    let release_run = target_release.clone();
    let deployment_run = deployment.clone();
    let current_run = current.clone();
    let reason = input.reason.clone();
    tokio::spawn(async move {
        if let Err(err) = run_rollback(
            app_handle.clone(),
            state_clone.clone(),
            release_run.clone(),
            deployment_run.clone(),
            current_run,
            reason,
            target,
        )
        .await
        {
            tracing::error!(release = %release_run.id, error = %err, "回滚执行异常");
            let _ = fail_rollback(
                &app_handle,
                &state_clone,
                &release_run.id,
                &deployment_run.id,
                &err,
            )
            .await;
        }
    });

    Ok(target_release)
}

async fn run_rollback(
    app: AppHandle,
    state: SharedState,
    target_release: Release,
    deployment: buildship_domain::DeploymentTask,
    current: Option<Release>,
    reason: String,
    target: DeploymentTarget,
) -> AppResult<()> {
    let client = build_client(&state, &target).await?;

    advance(
        &state,
        &app,
        &target_release.id,
        &deployment.id,
        ReleaseStatus::HealthCheck,
        DeploymentStatus::Deploying,
        "回滚切换版本",
        40,
    )
    .await
    .ok();

    let request = RollbackRequest {
        to_release_id: target_release.id.clone(),
        site: target.iis_site.clone(),
        physical_path: target.physical_path.clone(),
        health_check_url: target.health_check_url.clone(),
        reason: reason.clone(),
        request_id: deployment.request_id.clone(),
    };

    let response = match client.rollback(&request).await {
        Ok(value) => value,
        Err(err) => {
            fail_rollback(&app, &state, &target_release.id, &deployment.id, &err).await?;
            return Ok(());
        }
    };

    let finished_at = time::now();
    {
        let db = state.db().await;
        let mut tx = db.pool().begin().await?;

        if response.health_status == "passed" {
            repo::releases::clear_current(
                &mut *tx,
                &target_release.project_id,
                &target_release.environment_id,
            )
            .await?;
            repo::releases::mark_current(&mut *tx, &target_release.id).await?;
            repo::releases::set_health(
                &mut *tx,
                &target_release.id,
                HealthStatus::Passed,
                Some("回滚后健康检查通过"),
            )
            .await?;
            repo::releases::set_deploy_path(&mut *tx, &target_release.id, &response.current_path)
                .await?;

            if let Some(current) = &current {
                if current.id != target_release.id {
                    repo::releases::update_status(
                        &mut *tx,
                        &current.id,
                        ReleaseStatus::RolledBack,
                        Some(finished_at),
                    )
                    .await?;
                    repo::releases::set_rollback_info(
                        &mut *tx,
                        &current.id,
                        &target_release.id,
                        &reason,
                    )
                    .await?;
                    repo::releases::append_event(
                        &mut *tx,
                        &current.id,
                        "rolled_back",
                        EventLevel::Warn,
                        &format!("已被回滚到 {}，原因：{}", target_release.version, reason),
                        None,
                    )
                    .await?;
                }
            }

            repo::deployments::update_deployment(
                &mut *tx,
                &deployment.id,
                DeploymentStatus::Succeeded,
                Some("回滚完成"),
                100,
                None,
                None,
                Some(finished_at),
            )
            .await?;
            repo::releases::append_event(
                &mut *tx,
                &target_release.id,
                "rollback_done",
                EventLevel::Info,
                "回滚完成，健康检查通过",
                None,
            )
            .await?;
            repo::audit::append(
                &mut *tx,
                &audit::event(AuditInput {
                    actor: &state.operator,
                    action: AuditAction::ReleaseRolledBack,
                    entity_type: "release",
                    entity_id: &target_release.id,
                    project_id: Some(&target_release.project_id),
                    summary: format!("回滚成功：当前版本 {}（原因：{}）", target_release.version, reason),
                    detail: Some(serde_json::json!({ "from": current.as_ref().map(|item| item.version.clone()) })),
                }),
            )
            .await?;
            tx.commit().await?;

            emit(
                &app,
                &target_release.id,
                &deployment.id,
                DeploymentStatus::Succeeded,
                ReleaseStatus::Healthy,
                "回滚完成",
                100,
                Some("回滚后健康检查通过".to_string()),
                None,
            );
        } else {
            let err = AppError::new(
                ErrorCode::HealthCheckFailed,
                response
                    .health_message
                    .clone()
                    .unwrap_or_else(|| "回滚后健康检查未通过".to_string()),
            );
            tx.rollback().await.ok();
            fail_rollback(&app, &state, &target_release.id, &deployment.id, &err).await?;
        }
    }

    Ok(())
}

async fn fail_rollback(
    app: &AppHandle,
    state: &AppState,
    release_id: &str,
    deployment_id: &str,
    err: &AppError,
) -> AppResult<()> {
    let finished_at = time::now();
    let db = state.db().await;
    let mut tx = db.pool().begin().await?;
    repo::deployments::update_deployment(
        &mut *tx,
        deployment_id,
        DeploymentStatus::Failed,
        Some("回滚失败"),
        100,
        Some("ROLLBACK_FAILED"),
        Some(&err.message),
        Some(finished_at),
    )
    .await?;
    repo::releases::append_event(
        &mut *tx,
        release_id,
        "rollback_failed",
        EventLevel::Error,
        &format!("回滚失败：{}（线上版本保持不变，请人工确认）", err.message),
        None,
    )
    .await?;
    repo::audit::append(
        &mut *tx,
        &audit::event(AuditInput {
            actor: &state.operator,
            action: AuditAction::RollbackFailed,
            entity_type: "release",
            entity_id: release_id,
            project_id: None,
            summary: format!("回滚失败：{}", err.message),
            detail: Some(serde_json::json!({ "code": err.code.as_str() })),
        }),
    )
    .await?;
    tx.commit().await?;

    emit(
        app,
        release_id,
        deployment_id,
        DeploymentStatus::Failed,
        ReleaseStatus::Healthy,
        "回滚失败",
        100,
        Some(err.message.clone()),
        Some(err.code.as_str().to_string()),
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// 查询
// ---------------------------------------------------------------------------

pub async fn list_releases(
    state: &AppState,
    project_id: Option<String>,
    environment_id: Option<String>,
    limit: i64,
) -> AppResult<Vec<Release>> {
    let db = state.db().await;
    repo::releases::list(
        db.pool(),
        project_id.as_deref(),
        environment_id.as_deref(),
        limit.clamp(1, 500),
    )
    .await
}

pub async fn release_detail(state: &AppState, id: &str) -> AppResult<ReleaseDetail> {
    let db = state.db().await;
    let pool = db.pool();
    let release = repo::releases::require(pool, id).await?;
    let events = repo::releases::list_events(pool, id).await?;
    let artifact = repo::artifacts::get(pool, &release.artifact_id).await?;
    let environment = repo::deployments::get_env(pool, &release.environment_id).await?;
    let target = repo::deployments::get_target(pool, &release.target_id).await?;
    let deployment_task = repo::deployments::deployment_for_release(pool, id).await?;
    let previous = match &release.previous_release_id {
        Some(previous_id) => repo::releases::get(pool, previous_id).await?,
        None => None,
    };
    Ok(ReleaseDetail {
        release,
        events,
        artifact,
        environment,
        target,
        deployment_task,
        previous,
    })
}

pub async fn healthy_candidates(
    state: &AppState,
    project_id: &str,
    environment_id: &str,
) -> AppResult<Vec<Release>> {
    let db = state.db().await;
    repo::releases::list_healthy(db.pool(), project_id, environment_id, 50).await
}

// ---------------------------------------------------------------------------
// 事件推送
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn emit(
    app: &AppHandle,
    release_id: &str,
    deployment_id: &str,
    status: DeploymentStatus,
    release_status: ReleaseStatus,
    stage: &str,
    progress: i64,
    message: Option<String>,
    error_code: Option<String>,
) {
    events::emit_deploy(
        app,
        DeployEvent {
            release_id: release_id.to_string(),
            deployment_task_id: deployment_id.to_string(),
            status,
            release_status,
            stage: Some(stage.to_string()),
            progress,
            message,
            error_code,
        },
    );
}
