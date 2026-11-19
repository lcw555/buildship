//! 发布环境、部署目标、部署任务 Repository。

use super::{now_str, ts};
use buildship_common::{AppError, AppResult};
use buildship_domain::{
    DeploymentEnvironment, DeploymentStatus, DeploymentTarget, DeploymentTask, RecordStatus,
};
use chrono::{DateTime, Utc};
use sqlx::{Executor, QueryBuilder, Sqlite};

const ENV_COLS: &str = "id, name, kind, description, status, created_at, updated_at";

const TARGET_COLS: &str = "id, environment_id, name, server_host, agent_base_url, iis_site, physical_path, \
health_check_url, credential_ref, tls_verify, status, last_checked_at, last_check_ok, created_at, updated_at";

const TASK_COLS: &str =
    "id, release_id, target_id, project_id, kind, status, stage, progress, request_id, \
idempotency_key, error_code, error_message, started_at, finished_at, created_at, updated_at";

// ---------------------------------------------------------------------------
// 环境
// ---------------------------------------------------------------------------

pub async fn insert_env<'e, E>(exec: E, env: &DeploymentEnvironment) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "INSERT INTO deployment_environments (id, name, kind, description, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )
    .bind(&env.id)
    .bind(&env.name)
    .bind(env.kind)
    .bind(&env.description)
    .bind(env.status)
    .bind(ts(env.created_at))
    .bind(ts(env.updated_at))
    .execute(exec)
    .await?;
    Ok(())
}

pub async fn update_env<'e, E>(
    exec: E,
    id: &str,
    name: &str,
    kind: buildship_domain::EnvironmentKind,
    description: Option<&str>,
) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    let affected = sqlx::query(
        "UPDATE deployment_environments SET name = ?2, kind = ?3, description = ?4, updated_at = ?5
         WHERE id = ?1",
    )
    .bind(id)
    .bind(name)
    .bind(kind)
    .bind(description)
    .bind(now_str())
    .execute(exec)
    .await?
    .rows_affected();
    if affected == 0 {
        return Err(AppError::not_found(format!("环境不存在：{id}")));
    }
    Ok(())
}

pub async fn get_env<'e, E>(exec: E, id: &str) -> AppResult<Option<DeploymentEnvironment>>
where
    E: Executor<'e, Database = Sqlite>,
{
    let sql = format!("SELECT {ENV_COLS} FROM deployment_environments WHERE id = ?1");
    Ok(sqlx::query_as::<_, DeploymentEnvironment>(&sql)
        .bind(id)
        .fetch_optional(exec)
        .await?)
}

pub async fn list_envs<'e, E>(
    exec: E,
    include_archived: bool,
) -> AppResult<Vec<DeploymentEnvironment>>
where
    E: Executor<'e, Database = Sqlite>,
{
    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(format!(
        "SELECT {ENV_COLS} FROM deployment_environments WHERE 1 = 1"
    ));
    if !include_archived {
        qb.push(" AND status = 'active'");
    }
    qb.push(" ORDER BY CASE kind WHEN 'prod' THEN 0 WHEN 'staging' THEN 1 WHEN 'test' THEN 2 ELSE 3 END, name ASC");
    Ok(qb
        .build_query_as::<DeploymentEnvironment>()
        .fetch_all(exec)
        .await?)
}

pub async fn set_env_status<'e, E>(exec: E, id: &str, status: RecordStatus) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query("UPDATE deployment_environments SET status = ?2, updated_at = ?3 WHERE id = ?1")
        .bind(id)
        .bind(status)
        .bind(now_str())
        .execute(exec)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 部署目标
// ---------------------------------------------------------------------------

pub async fn insert_target<'e, E>(exec: E, target: &DeploymentTarget) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "INSERT INTO deployment_targets (id, environment_id, name, server_host, agent_base_url, iis_site,
            physical_path, health_check_url, credential_ref, tls_verify, status, last_checked_at,
            last_check_ok, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
    )
    .bind(&target.id)
    .bind(&target.environment_id)
    .bind(&target.name)
    .bind(&target.server_host)
    .bind(&target.agent_base_url)
    .bind(&target.iis_site)
    .bind(&target.physical_path)
    .bind(&target.health_check_url)
    .bind(&target.credential_ref)
    .bind(target.tls_verify)
    .bind(target.status)
    .bind(target.last_checked_at.map(ts))
    .bind(target.last_check_ok)
    .bind(ts(target.created_at))
    .bind(ts(target.updated_at))
    .execute(exec)
    .await?;
    Ok(())
}

pub async fn update_target<'e, E>(exec: E, target: &DeploymentTarget) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    let affected = sqlx::query(
        "UPDATE deployment_targets SET environment_id = ?2, name = ?3, server_host = ?4, agent_base_url = ?5,
            iis_site = ?6, physical_path = ?7, health_check_url = ?8, credential_ref = ?9, tls_verify = ?10,
            updated_at = ?11
         WHERE id = ?1",
    )
    .bind(&target.id)
    .bind(&target.environment_id)
    .bind(&target.name)
    .bind(&target.server_host)
    .bind(&target.agent_base_url)
    .bind(&target.iis_site)
    .bind(&target.physical_path)
    .bind(&target.health_check_url)
    .bind(&target.credential_ref)
    .bind(target.tls_verify)
    .bind(now_str())
    .execute(exec)
    .await?
    .rows_affected();
    if affected == 0 {
        return Err(AppError::not_found(format!(
            "部署目标不存在：{}",
            target.id
        )));
    }
    Ok(())
}

pub async fn get_target<'e, E>(exec: E, id: &str) -> AppResult<Option<DeploymentTarget>>
where
    E: Executor<'e, Database = Sqlite>,
{
    let sql = format!("SELECT {TARGET_COLS} FROM deployment_targets WHERE id = ?1");
    Ok(sqlx::query_as::<_, DeploymentTarget>(&sql)
        .bind(id)
        .fetch_optional(exec)
        .await?)
}

pub async fn require_target<'e, E>(exec: E, id: &str) -> AppResult<DeploymentTarget>
where
    E: Executor<'e, Database = Sqlite>,
{
    get_target(exec, id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("部署目标不存在：{id}")))
}

pub async fn list_targets<'e, E>(
    exec: E,
    environment_id: Option<&str>,
    include_archived: bool,
) -> AppResult<Vec<DeploymentTarget>>
where
    E: Executor<'e, Database = Sqlite>,
{
    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(format!(
        "SELECT {TARGET_COLS} FROM deployment_targets WHERE 1 = 1"
    ));
    if let Some(environment_id) = environment_id {
        qb.push(" AND environment_id = ").push_bind(environment_id);
    }
    if !include_archived {
        qb.push(" AND status = 'active'");
    }
    qb.push(" ORDER BY name ASC");
    Ok(qb
        .build_query_as::<DeploymentTarget>()
        .fetch_all(exec)
        .await?)
}

pub async fn set_target_status<'e, E>(exec: E, id: &str, status: RecordStatus) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query("UPDATE deployment_targets SET status = ?2, updated_at = ?3 WHERE id = ?1")
        .bind(id)
        .bind(status)
        .bind(now_str())
        .execute(exec)
        .await?;
    Ok(())
}

pub async fn update_target_check<'e, E>(
    exec: E,
    id: &str,
    ok: bool,
    checked_at: DateTime<Utc>,
) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "UPDATE deployment_targets SET last_checked_at = ?2, last_check_ok = ?3, updated_at = ?2 WHERE id = ?1",
    )
    .bind(id)
    .bind(ts(checked_at))
    .bind(ok)
    .execute(exec)
    .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 部署任务
// ---------------------------------------------------------------------------

pub async fn insert_deployment<'e, E>(exec: E, task: &DeploymentTask) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "INSERT INTO deployment_tasks (id, release_id, target_id, project_id, kind, status, stage, progress,
            request_id, idempotency_key, error_code, error_message, started_at, finished_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
    )
    .bind(&task.id)
    .bind(&task.release_id)
    .bind(&task.target_id)
    .bind(&task.project_id)
    .bind(task.kind)
    .bind(task.status)
    .bind(&task.stage)
    .bind(task.progress)
    .bind(&task.request_id)
    .bind(&task.idempotency_key)
    .bind(&task.error_code)
    .bind(&task.error_message)
    .bind(task.started_at.map(ts))
    .bind(task.finished_at.map(ts))
    .bind(ts(task.created_at))
    .bind(ts(task.updated_at))
    .execute(exec)
    .await?;
    Ok(())
}

/// 状态推进需要同时写入阶段、进度与错误信息，参数虽多但都直接映射到列，
/// 拆成结构体只会让调用点更啰嗦，这里显式豁免。
#[allow(clippy::too_many_arguments)]
pub async fn update_deployment<'e, E>(
    exec: E,
    id: &str,
    status: DeploymentStatus,
    stage: Option<&str>,
    progress: i64,
    error_code: Option<&str>,
    error_message: Option<&str>,
    finished_at: Option<DateTime<Utc>>,
) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "UPDATE deployment_tasks SET status = ?2, stage = ?3, progress = ?4, error_code = ?5,
            error_message = ?6, finished_at = COALESCE(?7, finished_at), updated_at = ?8
         WHERE id = ?1",
    )
    .bind(id)
    .bind(status)
    .bind(stage)
    .bind(progress)
    .bind(error_code)
    .bind(error_message)
    .bind(finished_at.map(ts))
    .bind(now_str())
    .execute(exec)
    .await?;
    Ok(())
}

pub async fn get_deployment<'e, E>(exec: E, id: &str) -> AppResult<Option<DeploymentTask>>
where
    E: Executor<'e, Database = Sqlite>,
{
    let sql = format!("SELECT {TASK_COLS} FROM deployment_tasks WHERE id = ?1");
    Ok(sqlx::query_as::<_, DeploymentTask>(&sql)
        .bind(id)
        .fetch_optional(exec)
        .await?)
}

pub async fn deployment_for_release<'e, E>(
    exec: E,
    release_id: &str,
) -> AppResult<Option<DeploymentTask>>
where
    E: Executor<'e, Database = Sqlite>,
{
    let sql = format!(
        "SELECT {TASK_COLS} FROM deployment_tasks WHERE release_id = ?1 ORDER BY created_at DESC LIMIT 1"
    );
    Ok(sqlx::query_as::<_, DeploymentTask>(&sql)
        .bind(release_id)
        .fetch_optional(exec)
        .await?)
}

pub async fn deployment_by_idempotency<'e, E>(
    exec: E,
    key: &str,
) -> AppResult<Option<DeploymentTask>>
where
    E: Executor<'e, Database = Sqlite>,
{
    let sql = format!("SELECT {TASK_COLS} FROM deployment_tasks WHERE idempotency_key = ?1");
    Ok(sqlx::query_as::<_, DeploymentTask>(&sql)
        .bind(key)
        .fetch_optional(exec)
        .await?)
}

/// 应用启动时把未完成的部署任务标记为失败（异常退出恢复）
pub async fn recover_interrupted<'e, E>(exec: E) -> AppResult<u64>
where
    E: Executor<'e, Database = Sqlite>,
{
    let affected = sqlx::query(
        "UPDATE deployment_tasks
         SET status = 'failed',
             error_code = 'INTERRUPTED',
             error_message = COALESCE(error_message, '应用异常退出，部署任务未完成'),
             finished_at = ?1, updated_at = ?1
         WHERE status IN ('queued', 'uploading', 'deploying', 'health_check')",
    )
    .bind(now_str())
    .execute(exec)
    .await?
    .rows_affected();
    Ok(affected)
}
