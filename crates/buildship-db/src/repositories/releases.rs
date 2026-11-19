//! Release / 发布事件 Repository。

use super::{new_id, now_str, ts};
use buildship_common::{AppError, AppResult};
use buildship_domain::{EventLevel, HealthStatus, Release, ReleaseEvent, ReleaseStatus};
use chrono::{DateTime, Utc};
use sqlx::{Executor, QueryBuilder, Sqlite};

const COLS: &str = "id, project_id, artifact_id, environment_id, target_id, version, deploy_path, status, \
previous_release_id, rolled_back_from, rollback_reason, health_status, health_message, is_current, operator, \
started_at, finished_at, created_at, updated_at";

pub async fn insert<'e, E>(exec: E, release: &Release) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "INSERT INTO releases (id, project_id, artifact_id, environment_id, target_id, version, deploy_path,
            status, previous_release_id, rolled_back_from, rollback_reason, health_status, health_message,
            is_current, operator, started_at, finished_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
    )
    .bind(&release.id)
    .bind(&release.project_id)
    .bind(&release.artifact_id)
    .bind(&release.environment_id)
    .bind(&release.target_id)
    .bind(&release.version)
    .bind(&release.deploy_path)
    .bind(release.status)
    .bind(&release.previous_release_id)
    .bind(&release.rolled_back_from)
    .bind(&release.rollback_reason)
    .bind(release.health_status)
    .bind(&release.health_message)
    .bind(release.is_current)
    .bind(&release.operator)
    .bind(release.started_at.map(ts))
    .bind(release.finished_at.map(ts))
    .bind(ts(release.created_at))
    .bind(ts(release.updated_at))
    .execute(exec)
    .await?;
    Ok(())
}

pub async fn get<'e, E>(exec: E, id: &str) -> AppResult<Option<Release>>
where
    E: Executor<'e, Database = Sqlite>,
{
    let sql = format!("SELECT {COLS} FROM releases WHERE id = ?1");
    Ok(sqlx::query_as::<_, Release>(&sql)
        .bind(id)
        .fetch_optional(exec)
        .await?)
}

pub async fn require<'e, E>(exec: E, id: &str) -> AppResult<Release>
where
    E: Executor<'e, Database = Sqlite>,
{
    get(exec, id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("发布记录不存在：{id}")))
}

pub async fn list<'e, E>(
    exec: E,
    project_id: Option<&str>,
    environment_id: Option<&str>,
    limit: i64,
) -> AppResult<Vec<Release>>
where
    E: Executor<'e, Database = Sqlite>,
{
    let mut qb: QueryBuilder<Sqlite> =
        QueryBuilder::new(format!("SELECT {COLS} FROM releases WHERE 1 = 1"));
    if let Some(project_id) = project_id {
        qb.push(" AND project_id = ").push_bind(project_id);
    }
    if let Some(environment_id) = environment_id {
        qb.push(" AND environment_id = ").push_bind(environment_id);
    }
    qb.push(" ORDER BY created_at DESC LIMIT ").push_bind(limit);
    Ok(qb.build_query_as::<Release>().fetch_all(exec).await?)
}

pub async fn recent<'e, E>(exec: E, limit: i64) -> AppResult<Vec<Release>>
where
    E: Executor<'e, Database = Sqlite>,
{
    let sql = format!("SELECT {COLS} FROM releases ORDER BY created_at DESC LIMIT ?1");
    Ok(sqlx::query_as::<_, Release>(&sql)
        .bind(limit)
        .fetch_all(exec)
        .await?)
}

/// 清掉某个项目+环境下所有 current 标记（切换前必须调用）
pub async fn clear_current<'e, E>(exec: E, project_id: &str, environment_id: &str) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "UPDATE releases SET is_current = 0, updated_at = ?3
         WHERE project_id = ?1 AND environment_id = ?2 AND is_current = 1",
    )
    .bind(project_id)
    .bind(environment_id)
    .bind(now_str())
    .execute(exec)
    .await?;
    Ok(())
}

/// 标记为当前线上版本（调用前必须已 clear_current）
pub async fn mark_current<'e, E>(exec: E, id: &str) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query("UPDATE releases SET is_current = 1, updated_at = ?2 WHERE id = ?1")
        .bind(id)
        .bind(now_str())
        .execute(exec)
        .await?;
    Ok(())
}

pub async fn current<'e, E>(
    exec: E,
    project_id: &str,
    environment_id: &str,
) -> AppResult<Option<Release>>
where
    E: Executor<'e, Database = Sqlite>,
{
    let sql = format!(
        "SELECT {COLS} FROM releases WHERE project_id = ?1 AND environment_id = ?2 AND is_current = 1 LIMIT 1"
    );
    Ok(sqlx::query_as::<_, Release>(&sql)
        .bind(project_id)
        .bind(environment_id)
        .fetch_optional(exec)
        .await?)
}

/// 上一健康版本（用于失败自动回滚）
pub async fn previous_healthy<'e, E>(
    exec: E,
    project_id: &str,
    environment_id: &str,
    exclude_release_id: &str,
) -> AppResult<Option<Release>>
where
    E: Executor<'e, Database = Sqlite>,
{
    let sql = format!(
        "SELECT {COLS} FROM releases
         WHERE project_id = ?1 AND environment_id = ?2 AND id != ?3
           AND status = 'healthy' AND health_status = 'passed'
         ORDER BY created_at DESC LIMIT 1"
    );
    Ok(sqlx::query_as::<_, Release>(&sql)
        .bind(project_id)
        .bind(environment_id)
        .bind(exclude_release_id)
        .fetch_optional(exec)
        .await?)
}

/// 历史健康版本列表（回滚目标候选）
pub async fn list_healthy<'e, E>(
    exec: E,
    project_id: &str,
    environment_id: &str,
    limit: i64,
) -> AppResult<Vec<Release>>
where
    E: Executor<'e, Database = Sqlite>,
{
    let sql = format!(
        "SELECT {COLS} FROM releases
         WHERE project_id = ?1 AND environment_id = ?2 AND status = 'healthy'
         ORDER BY created_at DESC LIMIT ?3"
    );
    Ok(sqlx::query_as::<_, Release>(&sql)
        .bind(project_id)
        .bind(environment_id)
        .bind(limit)
        .fetch_all(exec)
        .await?)
}

pub async fn update_status<'e, E>(
    exec: E,
    id: &str,
    status: ReleaseStatus,
    finished_at: Option<DateTime<Utc>>,
) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "UPDATE releases SET status = ?2, finished_at = COALESCE(?3, finished_at), updated_at = ?4
         WHERE id = ?1",
    )
    .bind(id)
    .bind(status)
    .bind(finished_at.map(ts))
    .bind(now_str())
    .execute(exec)
    .await?;
    Ok(())
}

pub async fn set_health<'e, E>(
    exec: E,
    id: &str,
    health: HealthStatus,
    message: Option<&str>,
) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query("UPDATE releases SET health_status = ?2, health_message = ?3, updated_at = ?4 WHERE id = ?1")
        .bind(id)
        .bind(health)
        .bind(message)
        .bind(now_str())
        .execute(exec)
        .await?;
    Ok(())
}

pub async fn set_deploy_path<'e, E>(exec: E, id: &str, path: &str) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query("UPDATE releases SET deploy_path = ?2, updated_at = ?3 WHERE id = ?1")
        .bind(id)
        .bind(path)
        .bind(now_str())
        .execute(exec)
        .await?;
    Ok(())
}

pub async fn set_rollback_info<'e, E>(
    exec: E,
    id: &str,
    rolled_back_from: &str,
    reason: &str,
) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "UPDATE releases SET rolled_back_from = ?2, rollback_reason = ?3, updated_at = ?4 WHERE id = ?1",
    )
    .bind(id)
    .bind(rolled_back_from)
    .bind(reason)
    .bind(now_str())
    .execute(exec)
    .await?;
    Ok(())
}

pub async fn count_healthy<'e, E>(exec: E) -> AppResult<i64>
where
    E: Executor<'e, Database = Sqlite>,
{
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM releases WHERE status = 'healthy' AND is_current = 1",
    )
    .fetch_one(exec)
    .await?;
    Ok(count)
}

pub async fn count_current_by_env<'e, E>(exec: E, environment_id: &str) -> AppResult<i64>
where
    E: Executor<'e, Database = Sqlite>,
{
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM releases WHERE environment_id = ?1 AND is_current = 1",
    )
    .bind(environment_id)
    .fetch_one(exec)
    .await?;
    Ok(count)
}

// ---------------------------------------------------------------------------
// 发布事件
// ---------------------------------------------------------------------------

/// 追加发布事件。
///
/// seq 在同一条 INSERT ... SELECT 内自增，避免"先查 max 再插入"的竞态；
/// `(release_id, seq)` 上有唯一索引兜底。
pub async fn append_event<'e, E>(
    exec: E,
    release_id: &str,
    phase: &str,
    level: EventLevel,
    message: &str,
    detail_json: Option<&str>,
) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "INSERT INTO release_events (id, release_id, seq, phase, level, message, detail_json, created_at)
         SELECT ?1, ?2, COALESCE(MAX(seq), 0) + 1, ?3, ?4, ?5, ?6, ?7
         FROM release_events WHERE release_id = ?2",
    )
    .bind(new_id())
    .bind(release_id)
    .bind(phase)
    .bind(level)
    .bind(message)
    .bind(detail_json)
    .bind(now_str())
    .execute(exec)
    .await?;
    Ok(())
}

pub async fn list_events<'e, E>(exec: E, release_id: &str) -> AppResult<Vec<ReleaseEvent>>
where
    E: Executor<'e, Database = Sqlite>,
{
    Ok(sqlx::query_as::<_, ReleaseEvent>(
        "SELECT id, release_id, seq, phase, level, message, detail_json, created_at
         FROM release_events WHERE release_id = ?1 ORDER BY seq ASC",
    )
    .bind(release_id)
    .fetch_all(exec)
    .await?)
}
