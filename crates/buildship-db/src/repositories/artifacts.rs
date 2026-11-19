//! 发布包 Repository。

use super::{now_str, ts};
use buildship_common::{AppError, AppResult};
use buildship_domain::{Artifact, ArtifactStatus};
use sqlx::{Executor, Sqlite};

const COLS: &str =
    "id, project_id, build_task_id, file_name, file_path, size_bytes, sha256, file_count, \
manifest_path, manifest_sha256, status, git_commit, created_at, updated_at";

pub async fn insert<'e, E>(exec: E, artifact: &Artifact) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "INSERT INTO artifacts (id, project_id, build_task_id, file_name, file_path, size_bytes, sha256,
            file_count, manifest_path, manifest_sha256, status, git_commit, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
    )
    .bind(&artifact.id)
    .bind(&artifact.project_id)
    .bind(&artifact.build_task_id)
    .bind(&artifact.file_name)
    .bind(&artifact.file_path)
    .bind(artifact.size_bytes)
    .bind(&artifact.sha256)
    .bind(artifact.file_count)
    .bind(&artifact.manifest_path)
    .bind(&artifact.manifest_sha256)
    .bind(artifact.status)
    .bind(&artifact.git_commit)
    .bind(ts(artifact.created_at))
    .bind(ts(artifact.updated_at))
    .execute(exec)
    .await?;
    Ok(())
}

pub async fn get<'e, E>(exec: E, id: &str) -> AppResult<Option<Artifact>>
where
    E: Executor<'e, Database = Sqlite>,
{
    let sql = format!("SELECT {COLS} FROM artifacts WHERE id = ?1");
    Ok(sqlx::query_as::<_, Artifact>(&sql)
        .bind(id)
        .fetch_optional(exec)
        .await?)
}

pub async fn require<'e, E>(exec: E, id: &str) -> AppResult<Artifact>
where
    E: Executor<'e, Database = Sqlite>,
{
    get(exec, id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("发布包不存在：{id}")))
}

pub async fn list_by_project<'e, E>(
    exec: E,
    project_id: Option<&str>,
    limit: i64,
) -> AppResult<Vec<Artifact>>
where
    E: Executor<'e, Database = Sqlite>,
{
    let rows = match project_id {
        Some(project_id) => {
            let sql = format!(
                "SELECT {COLS} FROM artifacts WHERE project_id = ?1
                 AND status != 'purged' ORDER BY created_at DESC LIMIT ?2"
            );
            sqlx::query_as::<_, Artifact>(&sql)
                .bind(project_id)
                .bind(limit)
                .fetch_all(exec)
                .await?
        }
        None => {
            let sql = format!(
                "SELECT {COLS} FROM artifacts WHERE status != 'purged' ORDER BY created_at DESC LIMIT ?1"
            );
            sqlx::query_as::<_, Artifact>(&sql)
                .bind(limit)
                .fetch_all(exec)
                .await?
        }
    };
    Ok(rows)
}

pub async fn for_build_task<'e, E>(exec: E, build_task_id: &str) -> AppResult<Vec<Artifact>>
where
    E: Executor<'e, Database = Sqlite>,
{
    let sql =
        format!("SELECT {COLS} FROM artifacts WHERE build_task_id = ?1 ORDER BY created_at DESC");
    Ok(sqlx::query_as::<_, Artifact>(&sql)
        .bind(build_task_id)
        .fetch_all(exec)
        .await?)
}

pub async fn set_status<'e, E>(exec: E, id: &str, status: ArtifactStatus) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query("UPDATE artifacts SET status = ?2, updated_at = ?3 WHERE id = ?1")
        .bind(id)
        .bind(status)
        .bind(now_str())
        .execute(exec)
        .await?;
    Ok(())
}

pub async fn set_sha256<'e, E>(exec: E, id: &str, sha256: &str, size_bytes: i64) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query("UPDATE artifacts SET sha256 = ?2, size_bytes = ?3, updated_at = ?4 WHERE id = ?1")
        .bind(id)
        .bind(sha256)
        .bind(size_bytes)
        .bind(now_str())
        .execute(exec)
        .await?;
    Ok(())
}

pub async fn count<'e, E>(exec: E, project_id: Option<&str>) -> AppResult<i64>
where
    E: Executor<'e, Database = Sqlite>,
{
    let count: i64 = match project_id {
        Some(project_id) => {
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM artifacts WHERE project_id = ?1 AND status = 'ready'",
            )
            .bind(project_id)
            .fetch_one(exec)
            .await?
        }
        None => {
            sqlx::query_scalar("SELECT COUNT(*) FROM artifacts WHERE status = 'ready'")
                .fetch_one(exec)
                .await?
        }
    };
    Ok(count)
}

pub async fn total_size<'e, E>(exec: E) -> AppResult<i64>
where
    E: Executor<'e, Database = Sqlite>,
{
    let total: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(size_bytes), 0) FROM artifacts WHERE status = 'ready'",
    )
    .fetch_one(exec)
    .await?;
    Ok(total)
}

/// 发布包是否被某个 Release 引用（被引用的包不允许物理删除）
pub async fn referenced_by_release<'e, E>(exec: E, id: &str) -> AppResult<bool>
where
    E: Executor<'e, Database = Sqlite>,
{
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM releases WHERE artifact_id = ?1")
        .bind(id)
        .fetch_one(exec)
        .await?;
    Ok(count > 0)
}
