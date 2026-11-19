//! 应用设置与凭据引用 Repository。
//!
//! 注意：`secret_references` 只存引用，正文在系统凭据库（见 `buildship-build::toolchain`
//! 与桌面端 `platform` 模块）。任何情况下不允许把 Token / 密码写进 SQLite。

use super::{now_str, ts};
use buildship_common::AppResult;
use buildship_domain::{AppSetting, SecretReference};
use sqlx::{Executor, Sqlite};

// ---------------------------------------------------------------------------
// 设置
// ---------------------------------------------------------------------------

pub async fn all<'e, E>(exec: E) -> AppResult<Vec<AppSetting>>
where
    E: Executor<'e, Database = Sqlite>,
{
    Ok(sqlx::query_as::<_, AppSetting>(
        "SELECT key, value, updated_at FROM app_settings ORDER BY key ASC",
    )
    .fetch_all(exec)
    .await?)
}

pub async fn get<'e, E>(exec: E, key: &str) -> AppResult<Option<String>>
where
    E: Executor<'e, Database = Sqlite>,
{
    Ok(
        sqlx::query_scalar::<_, String>("SELECT value FROM app_settings WHERE key = ?1")
            .bind(key)
            .fetch_optional(exec)
            .await?,
    )
}

pub async fn upsert<'e, E>(exec: E, key: &str, value: &str) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "INSERT INTO app_settings (key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )
    .bind(key)
    .bind(value)
    .bind(now_str())
    .execute(exec)
    .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 凭据引用
// ---------------------------------------------------------------------------

pub async fn upsert_secret_ref<'e, E>(exec: E, reference: &SecretReference) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query(
        "INSERT INTO secret_references (id, label, provider, account, created_at, last_used_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET label = excluded.label, provider = excluded.provider,
             account = excluded.account",
    )
    .bind(&reference.id)
    .bind(&reference.label)
    .bind(reference.provider)
    .bind(&reference.account)
    .bind(ts(reference.created_at))
    .bind(reference.last_used_at.map(ts))
    .execute(exec)
    .await?;
    Ok(())
}

pub async fn get_secret_ref<'e, E>(exec: E, id: &str) -> AppResult<Option<SecretReference>>
where
    E: Executor<'e, Database = Sqlite>,
{
    Ok(sqlx::query_as::<_, SecretReference>(
        "SELECT id, label, provider, account, created_at, last_used_at FROM secret_references WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(exec)
    .await?)
}

pub async fn list_secret_refs<'e, E>(exec: E) -> AppResult<Vec<SecretReference>>
where
    E: Executor<'e, Database = Sqlite>,
{
    Ok(sqlx::query_as::<_, SecretReference>(
        "SELECT id, label, provider, account, created_at, last_used_at FROM secret_references ORDER BY label ASC",
    )
    .fetch_all(exec)
    .await?)
}

pub async fn touch_secret_ref<'e, E>(exec: E, id: &str) -> AppResult<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    sqlx::query("UPDATE secret_references SET last_used_at = ?2 WHERE id = ?1")
        .bind(id)
        .bind(now_str())
        .execute(exec)
        .await?;
    Ok(())
}
