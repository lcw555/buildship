//! 项目服务：扫描、添加、编辑、归档。

use crate::services::audit::{self, AuditInput};
use crate::state::AppState;
use buildship_build::{scanner, toolchain};
use buildship_common::{time, AppError, AppResult};
use buildship_db::repositories as repo;
use buildship_domain::{
    AddProjectInput, AuditAction, BuildProfile, PackageManager, Project, RecordStatus, ScanResult,
    UpdateProjectInput,
};
use sqlx::SqlitePool;
use std::path::Path;

/// 扫描项目目录（只读配置文件，不执行脚本）
pub async fn scan(path: &str) -> AppResult<ScanResult> {
    if path.trim().is_empty() {
        return Err(AppError::invalid("请选择项目目录"));
    }
    scanner::scan(Path::new(path)).await
}

/// 保证 slug 唯一：重复时追加 -2、-3…
async fn unique_slug(pool: &SqlitePool, base: &str) -> AppResult<String> {
    let mut candidate = base.to_string();
    let mut counter = 1;
    loop {
        if repo::projects::find_by_slug(pool, &candidate)
            .await?
            .is_none()
        {
            return Ok(candidate);
        }
        counter += 1;
        candidate = format!("{base}-{counter}");
        if counter > 999 {
            return Err(AppError::conflict("项目标识生成失败，请手动指定项目名称"));
        }
    }
}

fn build_command(pm: PackageManager, script: Option<&str>) -> String {
    format!("{} run {}", pm.binary(), script.unwrap_or("build"))
}

fn empty_profile(
    project_id: &str,
    script: Option<&str>,
    output_dir: &str,
    command: &str,
) -> BuildProfile {
    let now = time::now();
    BuildProfile {
        id: uuid::Uuid::new_v4().to_string(),
        project_id: project_id.to_string(),
        name: "default".to_string(),
        build_script: script.map(str::to_string),
        build_command: command.to_string(),
        build_args_json: "[]".to_string(),
        output_dir: output_dir.to_string(),
        env_json: "{}".to_string(),
        is_default: true,
        created_at: now,
        updated_at: now,
    }
}

/// 添加项目。同一目录重复添加会返回冲突；已归档的同路径项目会被重新激活。
pub async fn add(state: &AppState, input: AddProjectInput) -> AppResult<Project> {
    let scanned = scanner::scan(Path::new(&input.path)).await?;
    let db = state.db().await;
    let pool = db.pool();

    let name = input
        .name
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| scanned.name.clone());
    let package_manager = input.package_manager.unwrap_or(scanned.package_manager);
    let output_dir = input
        .output_dir
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| scanned.suggested_output_dir.clone());
    let script = scanned.suggested_script.clone();
    let command = build_command(package_manager, script.as_deref());
    let pm_version = toolchain::package_manager_version(package_manager).await;
    let now = time::now();

    if let Some(existing) = repo::projects::find_by_path(pool, &scanned.path).await? {
        if existing.status == RecordStatus::Active {
            return Err(AppError::conflict(format!(
                "该目录已在项目列表中：{}",
                existing.name
            )));
        }

        // 重新激活历史项目
        let mut tx = pool.begin().await?;
        repo::projects::update_scan_meta(
            &mut *tx,
            &existing.id,
            scanned.node_version.as_deref(),
            scanned.framework.as_deref(),
            Some(package_manager),
            pm_version.as_deref(),
        )
        .await?;
        repo::projects::update_basic(
            &mut *tx,
            &existing.id,
            &name,
            package_manager,
            pm_version.as_deref(),
        )
        .await?;
        repo::projects::set_status(&mut *tx, &existing.id, RecordStatus::Active).await?;
        if repo::builds::default_profile(&mut *tx, &existing.id)
            .await?
            .is_none()
        {
            let profile = empty_profile(&existing.id, script.as_deref(), &output_dir, &command);
            repo::builds::insert_profile(&mut *tx, &profile).await?;
        }
        repo::audit::append(
            &mut *tx,
            &audit::event(AuditInput {
                actor: &state.operator,
                action: AuditAction::ProjectAdded,
                entity_type: "project",
                entity_id: &existing.id,
                project_id: Some(&existing.id),
                summary: format!("重新激活项目：{name}"),
                detail: Some(serde_json::json!({ "path": scanned.path })),
            }),
        )
        .await?;
        tx.commit().await?;
        return repo::projects::require(pool, &existing.id).await;
    }

    let slug = unique_slug(pool, &scanned.slug).await?;
    let project = Project {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        slug,
        path: scanned.path.clone(),
        package_manager,
        package_manager_version: pm_version,
        node_version: scanned.node_version.clone(),
        framework: scanned.framework.clone(),
        status: RecordStatus::Active,
        last_build_status: None,
        last_build_at: None,
        created_at: now,
        updated_at: now,
    };
    let profile = empty_profile(&project.id, script.as_deref(), &output_dir, &command);

    let mut tx = pool.begin().await?;
    repo::projects::insert(&mut *tx, &project).await?;
    repo::builds::insert_profile(&mut *tx, &profile).await?;
    repo::audit::append(
        &mut *tx,
        &audit::event(AuditInput {
            actor: &state.operator,
            action: AuditAction::ProjectAdded,
            entity_type: "project",
            entity_id: &project.id,
            project_id: Some(&project.id),
            summary: format!("添加项目：{}", project.name),
            detail: Some(serde_json::json!({
                "path": project.path,
                "packageManager": package_manager.binary(),
                "outputDir": profile.output_dir,
                "buildCommand": profile.build_command,
            })),
        }),
    )
    .await?;
    tx.commit().await?;

    tracing::info!(project = %project.id, path = %project.path, "项目已添加");
    Ok(project)
}

/// 编辑项目与默认构建配置
pub async fn update(state: &AppState, input: UpdateProjectInput) -> AppResult<Project> {
    if input.name.trim().is_empty() {
        return Err(AppError::invalid("项目名称不能为空"));
    }
    if input.output_dir.trim().is_empty() {
        return Err(AppError::invalid("产物目录不能为空"));
    }

    let db = state.db().await;
    let pool = db.pool();
    let project = repo::projects::require(pool, &input.id).await?;

    let script = input
        .build_script
        .clone()
        .filter(|value| !value.trim().is_empty());
    let command = build_command(input.package_manager, script.as_deref());
    let args_json = serde_json::to_string(&input.build_args)?;
    let env_json = serde_json::to_string(&input.env)?;
    let existing = repo::builds::default_profile(pool, &input.id).await?;

    let mut tx = pool.begin().await?;
    repo::projects::update_basic(
        &mut *tx,
        &input.id,
        input.name.trim(),
        input.package_manager,
        input.package_manager_version.as_deref(),
    )
    .await?;

    match existing {
        Some(profile) => {
            repo::builds::update_profile(
                &mut *tx,
                &profile.id,
                script.as_deref(),
                &command,
                &args_json,
                input.output_dir.trim(),
                &env_json,
            )
            .await?;
        }
        None => {
            repo::builds::clear_default_profile(&mut *tx, &input.id).await?;
            let profile = empty_profile(
                &input.id,
                script.as_deref(),
                input.output_dir.trim(),
                &command,
            );
            repo::builds::insert_profile(&mut *tx, &profile).await?;
        }
    }

    repo::audit::append(
        &mut *tx,
        &audit::event(AuditInput {
            actor: &state.operator,
            action: AuditAction::ProjectUpdated,
            entity_type: "project",
            entity_id: &input.id,
            project_id: Some(&input.id),
            summary: format!("修改项目配置：{}", input.name),
            detail: Some(serde_json::json!({
                "buildCommand": command,
                "outputDir": input.output_dir,
                "args": input.build_args,
                "envKeys": input.env.keys().collect::<Vec<_>>(),
            })),
        }),
    )
    .await?;
    tx.commit().await?;

    let _ = &project;
    repo::projects::require(pool, &input.id).await
}

/// 归档项目（软删除，保留历史记录）
pub async fn archive(state: &AppState, id: &str) -> AppResult<()> {
    let db = state.db().await;
    let pool = db.pool();
    let project = repo::projects::require(pool, id).await?;

    if let Some(task) = repo::builds::active_task(pool, id).await? {
        return Err(AppError::conflict(format!(
            "项目正在构建中（任务 {}），请先取消或等待完成",
            &task.id[..8]
        )));
    }

    let mut tx = pool.begin().await?;
    repo::projects::set_status(&mut *tx, id, RecordStatus::Archived).await?;
    repo::audit::append(
        &mut *tx,
        &audit::event(AuditInput {
            actor: &state.operator,
            action: AuditAction::ProjectArchived,
            entity_type: "project",
            entity_id: id,
            project_id: Some(id),
            summary: format!("归档项目：{}", project.name),
            detail: None,
        }),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// 项目列表
pub async fn list(state: &AppState, status: Option<RecordStatus>) -> AppResult<Vec<Project>> {
    let db = state.db().await;
    repo::projects::list(db.pool(), status).await
}

/// 项目构建配置列表
pub async fn profiles(state: &AppState, project_id: &str) -> AppResult<Vec<BuildProfile>> {
    let db = state.db().await;
    repo::builds::list_profiles(db.pool(), project_id).await
}
