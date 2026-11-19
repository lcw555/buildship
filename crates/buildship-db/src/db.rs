//! 连接池、PRAGMA 与迁移执行。

use buildship_common::{AppError, AppResult};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, SqlitePool};
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

/// SQLite 连接封装。
#[derive(Clone)]
pub struct Db {
    pool: SqlitePool,
    schema_version: i64,
}

impl Db {
    /// 打开（必要时创建）磁盘数据库。
    ///
    /// PRAGMA 按方案 5.1 设置：WAL、外键开启、busy_timeout 5s、synchronous=NORMAL。
    pub async fn open(path: &Path) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AppError::io(format!("创建数据库目录失败：{e}")))?;
        }
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));

        let pool = SqlitePoolOptions::new()
            .max_connections(8)
            .acquire_timeout(Duration::from_secs(10))
            .connect_with(options)
            .await?;

        Self::after_connect(pool).await
    }

    /// 内存数据库，仅用于测试。连接池固定 1 连接，否则每个连接是独立库。
    pub async fn open_memory() -> AppResult<Self> {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .map_err(|e| AppError::internal(format!("构造内存数据库连接串失败：{e}")))?
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        Self::after_connect(pool).await
    }

    async fn after_connect(pool: SqlitePool) -> AppResult<Self> {
        // 池化连接建立后再保险地设一遍外键（部分平台驱动会重置）
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await?;
        let mut db = Self {
            pool,
            schema_version: 0,
        };
        db.migrate().await?;
        Ok(db)
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub fn schema_version(&self) -> i64 {
        self.schema_version
    }

    /// 执行内嵌迁移（编译期打包，安装后无需外部 SQL 文件）。
    pub async fn migrate(&mut self) -> AppResult<()> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;

        // 把 sqlx 的迁移记录镜像到业务可见的 schema_migrations 表
        let rows =
            sqlx::query("SELECT version, description FROM _sqlx_migrations ORDER BY version")
                .fetch_all(&self.pool)
                .await?;
        for row in rows {
            let version: i64 = row.try_get("version")?;
            let description: String = row.try_get("description")?;
            sqlx::query(
                "INSERT INTO schema_migrations (version, description, applied_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(version) DO NOTHING",
            )
            .bind(version)
            .bind(description)
            .bind(buildship_common::time::to_db(buildship_common::time::now()))
            .execute(&self.pool)
            .await?;
        }

        let version: Option<i64> = sqlx::query_scalar("SELECT MAX(version) FROM schema_migrations")
            .fetch_one(&self.pool)
            .await?;
        self.schema_version = version.unwrap_or(0);
        Ok(())
    }

    /// 完整性检查，返回 (ok, 说明)。
    pub async fn integrity_check(&self) -> AppResult<(bool, String)> {
        let result: String = sqlx::query_scalar("PRAGMA integrity_check")
            .fetch_one(&self.pool)
            .await?;
        let fk_violations: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM pragma_foreign_key_check")
                .fetch_one(&self.pool)
                .await?;
        let ok = result.eq_ignore_ascii_case("ok") && fk_violations == 0;
        let message = if ok {
            "数据库完整性检查通过".to_string()
        } else {
            format!("integrity_check={result}，外键冲突 {fk_violations} 条")
        };
        Ok((ok, message))
    }

    /// 便于集成测试：直接执行一段 SQL。
    pub async fn execute(&self, sql: &str) -> AppResult<()> {
        sqlx::query(sql).execute(&self.pool).await?;
        Ok(())
    }
}
