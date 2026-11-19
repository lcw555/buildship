//! 发布包服务：打包、校验、清理。

use crate::services::audit::{self, AuditInput};
use crate::state::AppState;
use buildship_artifact::{archive, verify_package, PackageOptions};
use buildship_build::scanner;
use buildship_common::{time, AppError, AppResult, ErrorCode};
use buildship_db::repositories as repo;
use buildship_domain::{Artifact, ArtifactStatus, ArtifactVerifyResult, AuditAction, BuildStatus};
use std::path::{Path, PathBuf};
use tauri::AppHandle;

/// 由成功的构建任务生成发布包
pub async fn create(app: &AppHandle, state: &AppState, build_task_id: &str) -> AppResult<Artifact> {
    let _ = app;
    let db = state.db().await;
    let pool = db.pool();

    let task = repo::builds::require_task(pool, build_task_id).await?;
    if task.status != BuildStatus::Succeeded {
        return Err(AppError::conflict("只有构建成功的任务才能生成发布包"));
    }
    let project = repo::projects::require(pool, &task.project_id).await?;
    let profile = match &task.profile_id {
        Some(profile_id) => repo::builds::list_profiles(pool, &project.id)
            .await?
            .into_iter()
            .find(|item| &item.id == profile_id),
        None => repo::builds::default_profile(pool, &project.id).await?,
    }
    .ok_or_else(|| AppError::invalid("项目缺少构建配置，无法确定产物目录"))?;

    // 同一构建任务重复打包时直接复用已有发布包
    if let Some(existing) = repo::artifacts::for_build_task(pool, build_task_id)
        .await?
        .into_iter()
        .find(|item| item.status == ArtifactStatus::Ready)
    {
        return Ok(existing);
    }

    let source_dir = Path::new(&project.path).join(&profile.output_dir);
    if !source_dir.is_dir() {
        return Err(AppError::not_found(format!(
            "构建产物目录不存在：{}（请检查项目配置的产物目录）",
            source_dir.display()
        )));
    }

    let artifact_id = uuid::Uuid::new_v4().to_string();
    let options = PackageOptions {
        artifact_id: artifact_id.clone(),
        project_id: project.id.clone(),
        project_name: project.name.clone(),
        project_slug: project.slug.clone(),
        build_task_id: task.id.clone(),
        build_command: profile.build_command.clone(),
        output_dir: profile.output_dir.clone(),
        source_dir: source_dir.clone(),
        dest_dir: state.paths.artifacts_dir(),
        file_name: None,
        git_commit: scanner::git_commit(Path::new(&project.path)),
        packaged_by: state.operator.clone(),
    };

    // 打包是阻塞 IO，放到阻塞线程池执行，避免卡住异步运行时
    let result = tokio::task::spawn_blocking(move || archive::package(&options))
        .await
        .map_err(|err| AppError::internal(format!("打包任务异常：{err}")))??;

    let now = time::now();
    let artifact = Artifact {
        id: artifact_id,
        project_id: project.id.clone(),
        build_task_id: task.id.clone(),
        file_name: result.file_name.clone(),
        file_path: result.file_path.to_string_lossy().to_string(),
        size_bytes: result.size_bytes,
        sha256: result.sha256.clone(),
        file_count: result.file_count,
        manifest_path: result.manifest_path.to_string_lossy().to_string(),
        manifest_sha256: Some(result.manifest_sha256.clone()),
        status: ArtifactStatus::Ready,
        git_commit: scanner::git_commit(Path::new(&project.path)),
        created_at: now,
        updated_at: now,
    };

    let mut tx = pool.begin().await?;
    repo::artifacts::insert(&mut *tx, &artifact).await?;
    repo::audit::append(
        &mut *tx,
        &audit::event(AuditInput {
            actor: &state.operator,
            action: AuditAction::ArtifactCreated,
            entity_type: "artifact",
            entity_id: &artifact.id,
            project_id: Some(&project.id),
            summary: format!("生成发布包：{}", artifact.file_name),
            detail: Some(serde_json::json!({
                "sha256": artifact.sha256,
                "fileCount": artifact.file_count,
                "sizeBytes": artifact.size_bytes,
                "excludedRules": result.excluded_rules,
                "treeSha256": result.tree_sha256,
            })),
        }),
    )
    .await?;
    tx.commit().await?;

    tracing::info!(artifact = %artifact.id, file = %artifact.file_name, "发布包已生成");
    Ok(artifact)
}

pub async fn list(
    state: &AppState,
    project_id: Option<String>,
    limit: i64,
) -> AppResult<Vec<Artifact>> {
    let db = state.db().await;
    repo::artifacts::list_by_project(db.pool(), project_id.as_deref(), limit.clamp(1, 500)).await
}

pub async fn get(state: &AppState, id: &str) -> AppResult<Artifact> {
    let db = state.db().await;
    repo::artifacts::require(db.pool(), id).await
}

/// 校验发布包完整性（哈希 + manifest 逐文件比对）
pub async fn verify(state: &AppState, id: &str) -> AppResult<ArtifactVerifyResult> {
    let artifact = {
        let db = state.db().await;
        repo::artifacts::require(db.pool(), id).await?
    };

    let path = PathBuf::from(&artifact.file_path);
    let report = tokio::task::spawn_blocking({
        let path = path.clone();
        let expected = artifact.sha256.clone();
        move || verify_package(&path, &expected)
    })
    .await
    .map_err(|err| AppError::internal(format!("校验任务异常：{err}")))??;

    let (status, action_summary) = if report.ok {
        (ArtifactStatus::Ready, "发布包校验通过")
    } else if path.is_file() {
        (ArtifactStatus::Corrupt, "发布包校验失败")
    } else {
        (ArtifactStatus::Missing, "发布包文件缺失")
    };

    {
        let db = state.db().await;
        let mut tx = db.pool().begin().await?;
        repo::artifacts::set_status(&mut *tx, id, status).await?;
        repo::audit::append(
            &mut *tx,
            &audit::event(AuditInput {
                actor: &state.operator,
                action: AuditAction::ArtifactVerified,
                entity_type: "artifact",
                entity_id: id,
                project_id: Some(&artifact.project_id),
                summary: format!("{action_summary}：{}", artifact.file_name),
                detail: Some(serde_json::json!({
                    "ok": report.ok,
                    "message": report.message,
                    "actualSha256": report.actual_sha256,
                })),
            }),
        )
        .await?;
        tx.commit().await?;
    }

    Ok(ArtifactVerifyResult {
        artifact_id: id.to_string(),
        ok: report.ok,
        expected_sha256: artifact.sha256,
        actual_sha256: report.actual_sha256,
        message: report.message,
        checked_at: time::now(),
    })
}

/// 删除发布包文件（保留记录，标记 purged；被 Release 引用的包禁止删除）
pub async fn remove(state: &AppState, id: &str) -> AppResult<()> {
    {
        let db = state.db().await;
        let pool = db.pool();
        let artifact = repo::artifacts::require(pool, id).await?;
        if repo::artifacts::referenced_by_release(pool, id).await? {
            return Err(AppError::conflict(
                "该发布包已被发布记录引用，不能删除；如需变更请重新生成新的发布包",
            ));
        }

        let file = PathBuf::from(&artifact.file_path);
        let manifest = PathBuf::from(&artifact.manifest_path);
        if file.exists() {
            std::fs::remove_file(&file)
                .map_err(|e| AppError::new(ErrorCode::IoError, format!("删除发布包失败：{e}")))?;
        }
        if manifest.exists() {
            let _ = std::fs::remove_file(&manifest);
        }

        let mut tx = pool.begin().await?;
        repo::artifacts::set_status(&mut *tx, id, ArtifactStatus::Purged).await?;
        repo::audit::append(
            &mut *tx,
            &audit::event(AuditInput {
                actor: &state.operator,
                action: AuditAction::ArtifactDeleted,
                entity_type: "artifact",
                entity_id: id,
                project_id: Some(&artifact.project_id),
                summary: format!("删除发布包文件：{}", artifact.file_name),
                detail: None,
            }),
        )
        .await?;
        tx.commit().await?;
    }
    Ok(())
}
