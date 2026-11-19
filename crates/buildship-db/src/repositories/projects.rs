//! 项目 Repository。

use super::{new_id, now_str, ts};
use buildship_common::{AppError, AppResult};
use buildship_domain::{PackageManager, Project, RecordStatus};
use chrono::{DateTime, Utc};
use sqlx::{Executor, QueryBuilder, Sqlite};

const COLS: &str = "id, name, slug, path, package_manager, package_manager_version, node_version, \
framework, status, last_build_status, last_build_at, created_at, updated_at";

/// 新增项目
pub async fn insert<'e, E>(exec: E, project: &Project) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "INSERT INTO projects (id, name, slug, path, package_manager, package_manager_version,
            node_version, framework, status, last_build_status, last_build_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
    )
    .bind(&project.id)
    .bind(&project.name)
    .bind(&project.slug)
    .bind(&project.path)
    .bind(project.package_manager)
    .bind(&project.package_manager_version)
    .bind(&project.node_version)
    .bind(&project.framework)
    .bind(project.status)
    .bind(project.last_build_status)
    .bind(project.last_build_at.map(ts))
    .bind(ts(project.created_at))
    .bind(ts(project.updated_at))
    .execute(exec)
    .await?;
    Ok(())
}

/// 按 ID 查询
pub async fn get<'e, E>(exec: E, id: &str) -> AppResult<Option<Project>>
where
    E: Executor<'e, Database = Sqlite>,
{
    let sql = format!("SELECT {COLS} FROM projects WHERE id = ?1");
    Ok(sqlx::query_as::<_, Project>(&sql)
        .bind(id)
        .fetch_optional(exec)
        .await?)
}

/// 必须存在
pub async fn require<'e, E>(exec: E, id: &str) -> AppResult<Project>
where
    E: Executor<'e, Database = Sqlite>,
{
    get(exec, id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("项目不存在：{id}")))
}

/// 按路径查询（用于"重复添加同一目录"判定）
pub async fn find_by_path<'e, E>(exec: E, path: &str) -> AppResult<Option<Project>>
where
    E: Executor<'e, Database = Sqlite>,
{
    let sql = format!("SELECT {COLS} FROM projects WHERE path = ?1");
    Ok(sqlx::query_as::<_, Project>(&sql)
        .bind(path)
        .fetch_optional(exec)
        .await?)
}

/// 按标识查询
pub async fn find_by_slug<'e, E>(exec: E, slug: &str) -> AppResult<Option<Project>>
where
    E: Executor<'e, Database = Sqlite>,
{
    let sql = format!("SELECT {COLS} FROM projects WHERE slug = ?1");
    Ok(sqlx::query_as::<_, Project>(&sql)
        .bind(slug)
        .fetch_optional(exec)
        .await?)
}

/// 列表（默认只返回 active）
pub async fn list<'e, E>(exec: E, status: Option<RecordStatus>) -> AppResult<Vec<Project>>
where
    E: Executor<'e, Database = Sqlite>,
{
    let mut qb: QueryBuilder<Sqlite> =
        QueryBuilder::new(format!("SELECT {COLS} FROM projects WHERE 1 = 1"));
    if let Some(status) = status {
        qb.push(" AND status = ").push_bind(status);
    } else {
        qb.push(" AND status = 'active'");
    }
    qb.push(" ORDER BY updated_at DESC");
    Ok(qb.build_query_as::<Project>().fetch_all(exec).await?)
}

/// 工作台"最近项目"
pub async fn recent<'e, E>(exec: E, limit: i64) -> AppResult<Vec<Project>>
where
    E: Executor<'e, Database = Sqlite>,
{
    let sql = format!(
        "SELECT {COLS} FROM projects WHERE status = 'active' ORDER BY updated_at DESC LIMIT ?1"
    );
    Ok(sqlx::query_as::<_, Project>(&sql)
        .bind(limit)
        .fetch_all(exec)
        .await?)
}

/// 编辑项目主体字段
pub async fn update_basic<'e, E>(
    exec: E,
    id: &str,
    name: &str,
    package_manager: PackageManager,
    package_manager_version: Option<&str>,
) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    let affected = sqlx::query(
        "UPDATE projects SET name = ?2, package_manager = ?3, package_manager_version = ?4,
            updated_at = ?5 WHERE id = ?1",
    )
    .bind(id)
    .bind(name)
    .bind(package_manager)
    .bind(package_manager_version)
    .bind(now_str())
    .execute(exec)
    .await?
    .rows_affected();
    if affected == 0 {
        return Err(AppError::not_found(format!("项目不存在：{id}")));
    }
    Ok(())
}

/// 写入扫描得到的元信息
pub async fn update_scan_meta<'e, E>(
    exec: E,
    id: &str,
    node_version: Option<&str>,
    framework: Option<&str>,
    package_manager: Option<PackageManager>,
    package_manager_version: Option<&str>,
) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "UPDATE projects SET
            node_version = COALESCE(?2, node_version),
            framework = COALESCE(?3, framework),
            package_manager = COALESCE(?4, package_manager),
            package_manager_version = COALESCE(?5, package_manager_version),
            updated_at = ?6
         WHERE id = ?1",
    )
    .bind(id)
    .bind(node_version)
    .bind(framework)
    .bind(package_manager)
    .bind(package_manager_version)
    .bind(now_str())
    .execute(exec)
    .await?;
    Ok(())
}

/// 记录最近一次构建结果
pub async fn set_last_build<'e, E>(
    exec: E,
    id: &str,
    status: buildship_domain::BuildStatus,
    at: DateTime<Utc>,
) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query("UPDATE projects SET last_build_status = ?2, last_build_at = ?3, updated_at = ?3 WHERE id = ?1")
        .bind(id)
        .bind(status)
        .bind(ts(at))
        .execute(exec)
        .await?;
    Ok(())
}

/// 归档（软删除）
pub async fn set_status<'e, E>(exec: E, id: &str, status: RecordStatus) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    let affected = sqlx::query("UPDATE projects SET status = ?2, updated_at = ?3 WHERE id = ?1")
        .bind(id)
        .bind(status)
        .bind(now_str())
        .execute(exec)
        .await?
        .rows_affected();
    if affected == 0 {
        return Err(AppError::not_found(format!("项目不存在：{id}")));
    }
    Ok(())
}

/// 统计
pub async fn count<'e, E>(exec: E, status: Option<RecordStatus>) -> AppResult<i64>
where
    E: Executor<'e, Database = Sqlite>,
{
    let count: i64 = match status {
        Some(status) => {
            sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE status = ?1")
                .bind(status)
                .fetch_one(exec)
                .await?
        }
        None => {
            sqlx::query_scalar("SELECT COUNT(*) FROM projects")
                .fetch_one(exec)
                .await?
        }
    };
    Ok(count)
}

/// 需要一个空 id（测试/占位用）
pub fn placeholder_id() -> String {
    new_id()
}
