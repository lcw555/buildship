//! 构建相关 Repository：构建配置、构建任务、日志分段。

use super::{new_id, now_str, ts};
use buildship_common::{AppError, AppResult};
use buildship_domain::{BuildLogSegment, BuildProfile, BuildQuery, BuildStatus, BuildTask};
use chrono::{DateTime, Utc};
use sqlx::{Executor, QueryBuilder, Sqlite};

const PROFILE_COLS: &str =
    "id, project_id, name, build_script, build_command, build_args_json, output_dir, \
env_json, is_default, created_at, updated_at";

const TASK_COLS: &str = "id, project_id, profile_id, status, command, args_json, exit_code, started_at, \
finished_at, duration_ms, log_path, log_size, log_sha256, error_summary, triggered_by, created_at, updated_at";

// ---------------------------------------------------------------------------
// 构建配置
// ---------------------------------------------------------------------------

/// 清掉项目下的默认标记（保证 `is_default` 唯一）
pub async fn clear_default_profile<'e, E>(exec: E, project_id: &str) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "UPDATE project_build_profiles SET is_default = 0 WHERE project_id = ?1 AND is_default = 1",
    )
    .bind(project_id)
    .execute(exec)
    .await?;
    Ok(())
}

pub async fn insert_profile<'e, E>(exec: E, profile: &BuildProfile) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "INSERT INTO project_build_profiles
            (id, project_id, name, build_script, build_command, build_args_json, output_dir, env_json, is_default, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    )
    .bind(&profile.id)
    .bind(&profile.project_id)
    .bind(&profile.name)
    .bind(&profile.build_script)
    .bind(&profile.build_command)
    .bind(&profile.build_args_json)
    .bind(&profile.output_dir)
    .bind(&profile.env_json)
    .bind(profile.is_default)
    .bind(ts(profile.created_at))
    .bind(ts(profile.updated_at))
    .execute(exec)
    .await?;
    Ok(())
}

pub async fn update_profile<'e, E>(
    exec: E,
    id: &str,
    build_script: Option<&str>,
    build_command: &str,
    build_args_json: &str,
    output_dir: &str,
    env_json: &str,
) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    let affected = sqlx::query(
        "UPDATE project_build_profiles
         SET build_script = ?2, build_command = ?3, build_args_json = ?4, output_dir = ?5, env_json = ?6,
             updated_at = ?7
         WHERE id = ?1",
    )
    .bind(id)
    .bind(build_script)
    .bind(build_command)
    .bind(build_args_json)
    .bind(output_dir)
    .bind(env_json)
    .bind(now_str())
    .execute(exec)
    .await?
    .rows_affected();
    if affected == 0 {
        return Err(AppError::not_found(format!("构建配置不存在：{id}")));
    }
    Ok(())
}

pub async fn list_profiles<'e, E>(exec: E, project_id: &str) -> AppResult<Vec<BuildProfile>>
where
    E: Executor<'e, Database = Sqlite>,
{
    let sql = format!(
        "SELECT {PROFILE_COLS} FROM project_build_profiles WHERE project_id = ?1
         ORDER BY is_default DESC, created_at ASC"
    );
    Ok(sqlx::query_as::<_, BuildProfile>(&sql)
        .bind(project_id)
        .fetch_all(exec)
        .await?)
}

pub async fn default_profile<'e, E>(exec: E, project_id: &str) -> AppResult<Option<BuildProfile>>
where
    E: Executor<'e, Database = Sqlite>,
{
    let sql = format!(
        "SELECT {PROFILE_COLS} FROM project_build_profiles WHERE project_id = ?1
         ORDER BY is_default DESC, created_at ASC LIMIT 1"
    );
    Ok(sqlx::query_as::<_, BuildProfile>(&sql)
        .bind(project_id)
        .fetch_optional(exec)
        .await?)
}

// ---------------------------------------------------------------------------
// 构建任务
// ---------------------------------------------------------------------------

pub async fn insert_task<'e, E>(exec: E, task: &BuildTask) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "INSERT INTO build_tasks
            (id, project_id, profile_id, status, command, args_json, exit_code, started_at, finished_at,
             duration_ms, log_path, log_size, log_sha256, error_summary, triggered_by, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
    )
    .bind(&task.id)
    .bind(&task.project_id)
    .bind(&task.profile_id)
    .bind(task.status)
    .bind(&task.command)
    .bind(&task.args_json)
    .bind(task.exit_code)
    .bind(task.started_at.map(ts))
    .bind(task.finished_at.map(ts))
    .bind(task.duration_ms)
    .bind(&task.log_path)
    .bind(task.log_size)
    .bind(&task.log_sha256)
    .bind(&task.error_summary)
    .bind(&task.triggered_by)
    .bind(ts(task.created_at))
    .bind(ts(task.updated_at))
    .execute(exec)
    .await?;
    Ok(())
}

pub async fn get_task<'e, E>(exec: E, id: &str) -> AppResult<Option<BuildTask>>
where
    E: Executor<'e, Database = Sqlite>,
{
    let sql = format!("SELECT {TASK_COLS} FROM build_tasks WHERE id = ?1");
    Ok(sqlx::query_as::<_, BuildTask>(&sql)
        .bind(id)
        .fetch_optional(exec)
        .await?)
}

pub async fn require_task<'e, E>(exec: E, id: &str) -> AppResult<BuildTask>
where
    E: Executor<'e, Database = Sqlite>,
{
    get_task(exec, id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("构建任务不存在：{id}")))
}

pub async fn mark_running<'e, E>(exec: E, id: &str, started_at: DateTime<Utc>) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "UPDATE build_tasks SET status = 'running', started_at = ?2, updated_at = ?2 WHERE id = ?1",
    )
    .bind(id)
    .bind(ts(started_at))
    .execute(exec)
    .await?;
    Ok(())
}

pub async fn finish_task<'e, E>(
    exec: E,
    id: &str,
    status: BuildStatus,
    exit_code: Option<i64>,
    finished_at: DateTime<Utc>,
    duration_ms: i64,
    error_summary: Option<&str>,
) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "UPDATE build_tasks SET status = ?2, exit_code = ?3, finished_at = ?4, duration_ms = ?5,
            error_summary = ?6, updated_at = ?4 WHERE id = ?1",
    )
    .bind(id)
    .bind(status)
    .bind(exit_code)
    .bind(ts(finished_at))
    .bind(duration_ms)
    .bind(error_summary)
    .execute(exec)
    .await?;
    Ok(())
}

pub async fn attach_log<'e, E>(
    exec: E,
    id: &str,
    log_path: &str,
    log_size: i64,
    log_sha256: &str,
) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "UPDATE build_tasks SET log_path = ?2, log_size = ?3, log_sha256 = ?4, updated_at = ?5 WHERE id = ?1",
    )
    .bind(id)
    .bind(log_path)
    .bind(log_size)
    .bind(log_sha256)
    .bind(now_str())
    .execute(exec)
    .await?;
    Ok(())
}

pub async fn list_tasks<'e, E>(exec: E, query: &BuildQuery) -> AppResult<Vec<BuildTask>>
where
    E: Executor<'e, Database = Sqlite>,
{
    let mut qb: QueryBuilder<Sqlite> =
        QueryBuilder::new(format!("SELECT {TASK_COLS} FROM build_tasks WHERE 1 = 1"));
    if let Some(project_id) = &query.project_id {
        qb.push(" AND project_id = ").push_bind(project_id);
    }
    if let Some(status) = query.status {
        qb.push(" AND status = ").push_bind(status);
    }
    qb.push(" ORDER BY created_at DESC LIMIT ")
        .push_bind(query.limit.unwrap_or(100));
    Ok(qb.build_query_as::<BuildTask>().fetch_all(exec).await?)
}

pub async fn recent_tasks<'e, E>(exec: E, limit: i64) -> AppResult<Vec<BuildTask>>
where
    E: Executor<'e, Database = Sqlite>,
{
    let sql = format!("SELECT {TASK_COLS} FROM build_tasks ORDER BY created_at DESC LIMIT ?1");
    Ok(sqlx::query_as::<_, BuildTask>(&sql)
        .bind(limit)
        .fetch_all(exec)
        .await?)
}

/// 项目当前的活动任务（queued / running）
pub async fn active_task<'e, E>(exec: E, project_id: &str) -> AppResult<Option<BuildTask>>
where
    E: Executor<'e, Database = Sqlite>,
{
    let sql = format!(
        "SELECT {TASK_COLS} FROM build_tasks WHERE project_id = ?1
         AND status IN ('queued', 'running') ORDER BY created_at DESC LIMIT 1"
    );
    Ok(sqlx::query_as::<_, BuildTask>(&sql)
        .bind(project_id)
        .fetch_optional(exec)
        .await?)
}

pub async fn count_active<'e, E>(exec: E) -> AppResult<i64>
where
    E: Executor<'e, Database = Sqlite>,
{
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM build_tasks WHERE status IN ('queued', 'running')",
    )
    .fetch_one(exec)
    .await?;
    Ok(count)
}

/// 应用启动时收尾上次异常退出残留的任务（方案 12：可识别未完成任务）
pub async fn recover_interrupted<'e, E>(exec: E) -> AppResult<u64>
where
    E: Executor<'e, Database = Sqlite>,
{
    let affected = sqlx::query(
        "UPDATE build_tasks
         SET status = 'failed',
             error_summary = COALESCE(error_summary, '应用异常退出，任务在重启后被标记为失败'),
             finished_at = ?1,
             updated_at = ?1
         WHERE status IN ('queued', 'running')",
    )
    .bind(now_str())
    .execute(exec)
    .await?
    .rows_affected();
    Ok(affected)
}

// ---------------------------------------------------------------------------
// 日志分段元数据
// ---------------------------------------------------------------------------

pub async fn insert_log_segment<'e, E>(exec: E, segment: &BuildLogSegment) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "INSERT INTO build_logs (id, task_id, segment_index, file_path, start_offset, end_offset, line_count, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )
    .bind(&segment.id)
    .bind(&segment.task_id)
    .bind(segment.segment_index)
    .bind(&segment.file_path)
    .bind(segment.start_offset)
    .bind(segment.end_offset)
    .bind(segment.line_count)
    .bind(ts(segment.created_at))
    .execute(exec)
    .await?;
    Ok(())
}

pub async fn list_log_segments<'e, E>(exec: E, task_id: &str) -> AppResult<Vec<BuildLogSegment>>
where
    E: Executor<'e, Database = Sqlite>,
{
    let rows = sqlx::query_as::<_, BuildLogSegment>(
        "SELECT id, task_id, segment_index, file_path, start_offset, end_offset, line_count, created_at
         FROM build_logs WHERE task_id = ?1 ORDER BY segment_index ASC",
    )
    .bind(task_id)
    .fetch_all(exec)
    .await?;
    Ok(rows)
}

/// 供服务层构造新分段
pub fn new_log_segment(
    task_id: &str,
    index: i64,
    file_path: &str,
    start_offset: i64,
    end_offset: i64,
    line_count: i64,
) -> BuildLogSegment {
    BuildLogSegment {
        id: new_id(),
        task_id: task_id.to_string(),
        segment_index: index,
        file_path: file_path.to_string(),
        start_offset,
        end_offset,
        line_count,
        created_at: super::now(),
    }
}
