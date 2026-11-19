//! 审计 Repository。
//!
//! 审计表由触发器禁止 UPDATE / DELETE，本模块只提供"追加"与"查询"。

use super::ts;
use buildship_common::AppResult;
use buildship_domain::{AuditEvent, AuditQuery};
use sqlx::{Executor, QueryBuilder, Sqlite};

const COLS: &str =
    "id, actor, action, entity_type, entity_id, project_id, summary, detail_json, created_at";

/// 追加一条审计记录。调用方应将它和业务写入放在同一个事务里。
pub async fn append<'e, E>(exec: E, event: &AuditEvent) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "INSERT INTO audit_events (id, actor, action, entity_type, entity_id, project_id, summary, detail_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )
    .bind(&event.id)
    .bind(&event.actor)
    .bind(event.action)
    .bind(&event.entity_type)
    .bind(&event.entity_id)
    .bind(&event.project_id)
    .bind(&event.summary)
    .bind(&event.detail_json)
    .bind(ts(event.created_at))
    .execute(exec)
    .await?;
    Ok(())
}

pub async fn list<'e, E>(exec: E, query: &AuditQuery) -> AppResult<Vec<AuditEvent>>
where
    E: Executor<'e, Database = Sqlite>,
{
    let mut qb: QueryBuilder<Sqlite> =
        QueryBuilder::new(format!("SELECT {COLS} FROM audit_events WHERE 1 = 1"));
    if let Some(project_id) = &query.project_id {
        qb.push(" AND project_id = ").push_bind(project_id);
    }
    if let Some(action) = query.action {
        qb.push(" AND action = ").push_bind(action);
    }
    qb.push(" ORDER BY created_at DESC LIMIT ")
        .push_bind(query.limit.unwrap_or(200))
        .push(" OFFSET ")
        .push_bind(query.offset.unwrap_or(0));
    Ok(qb.build_query_as::<AuditEvent>().fetch_all(exec).await?)
}

pub async fn count<'e, E>(exec: E, query: &AuditQuery) -> AppResult<i64>
where
    E: Executor<'e, Database = Sqlite>,
{
    let mut qb: QueryBuilder<Sqlite> =
        QueryBuilder::new("SELECT COUNT(*) FROM audit_events WHERE 1 = 1");
    if let Some(project_id) = &query.project_id {
        qb.push(" AND project_id = ").push_bind(project_id);
    }
    if let Some(action) = query.action {
        qb.push(" AND action = ").push_bind(action);
    }
    let total: i64 = qb.build_query_scalar::<i64>().fetch_one(exec).await?;
    Ok(total)
}
