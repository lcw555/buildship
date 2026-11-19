//! 数据库备份 / 恢复支撑。
//!
//! 方案 5.6 要求：
//! - 一致性备份（用 `VACUUM INTO`，可安全在 WAL 模式下执行）
//! - 备份文件校验（SHA-256 + SQLite 完整性检查）
//! - 恢复前自动备份当前库，恢复后检查 schema version
//!
//! 真正的"替换数据库文件"由服务层在关闭连接池之后完成，这里只负责准备与校验。

use crate::db::Db;
use buildship_common::{time, AppError, AppResult};
use buildship_domain::BackupEntry;
use sha2::{Digest, Sha256};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::Row;
use std::io::Read;
use std::path::{Path, PathBuf};

/// 计算文件 SHA-256
pub fn sha256_file(path: &Path) -> AppResult<String> {
    let mut file = std::fs::File::open(path)
        .map_err(|e| AppError::io(format!("打开文件失败 {}：{e}", path.display())))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|e| AppError::io(format!("读取文件失败 {}：{e}", path.display())))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn entry_for(path: &Path) -> AppResult<BackupEntry> {
    let meta =
        std::fs::metadata(path).map_err(|e| AppError::io(format!("读取备份文件信息失败：{e}")))?;
    let created = meta
        .modified()
        .ok()
        .map(chrono::DateTime::<chrono::Utc>::from)
        .unwrap_or_else(time::now);
    Ok(BackupEntry {
        file_name: path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default(),
        path: path.to_string_lossy().to_string(),
        size_bytes: meta.len() as i64,
        sha256: sha256_file(path)?,
        created_at: created,
    })
}

/// 创建一致性备份，返回备份条目（含 SHA-256）。
pub async fn create(db: &Db, dir: &Path) -> AppResult<BackupEntry> {
    std::fs::create_dir_all(dir).map_err(|e| AppError::io(format!("创建备份目录失败：{e}")))?;
    let stamp = time::compact_stamp(time::now());
    let file_name = format!(
        "buildship-{stamp}-{}.db",
        &uuid::Uuid::new_v4().to_string()[..6]
    );
    let target: PathBuf = dir.join(&file_name);

    if target.exists() {
        return Err(AppError::conflict(format!(
            "备份文件已存在：{}",
            target.display()
        )));
    }
    // VACUUM INTO 会生成一份事务一致的新库文件
    sqlx::query("VACUUM INTO ?1")
        .bind(target.to_string_lossy().to_string())
        .execute(db.pool())
        .await?;

    let entry = entry_for(&target)?;
    tracing::info!(path = %target.display(), size = entry.size_bytes, "数据库备份完成");
    Ok(entry)
}

/// 列出备份目录中的备份文件（按时间倒序）。
pub async fn list(dir: &Path) -> AppResult<Vec<BackupEntry>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    let reader =
        std::fs::read_dir(dir).map_err(|e| AppError::io(format!("读取备份目录失败：{e}")))?;
    for item in reader {
        let item = item.map_err(|e| AppError::io(format!("遍历备份目录失败：{e}")))?;
        let path = item.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("db") {
            continue;
        }
        match entry_for(&path) {
            Ok(entry) => entries.push(entry),
            Err(err) => {
                tracing::warn!(path = %path.display(), error = %err, "跳过无法读取的备份文件")
            }
        }
    }
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.created_at));
    Ok(entries)
}

/// 校验备份文件：能打开、完整性 OK、返回其 schema version。
pub async fn validate(path: &Path) -> AppResult<i64> {
    if !path.exists() {
        return Err(AppError::not_found(format!(
            "备份文件不存在：{}",
            path.display()
        )));
    }
    let options = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(|e| {
            AppError::new(
                buildship_common::ErrorCode::DbError,
                format!("备份文件无法打开：{e}"),
            )
        })?;

    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|_| "unknown".to_string());
    if !integrity.eq_ignore_ascii_case("ok") {
        pool.close().await;
        return Err(AppError::new(
            buildship_common::ErrorCode::ChecksumMismatch,
            format!("备份文件完整性检查失败：{integrity}"),
        ));
    }

    let version: i64 = sqlx::query("SELECT COALESCE(MAX(version), 0) AS v FROM schema_migrations")
        .fetch_one(&pool)
        .await
        .map(|row| row.get::<i64, _>("v"))
        .unwrap_or(0);
    pool.close().await;
    Ok(version)
}

/// 恢复前把当前数据库另存一份（"恢复的撤销点"）。
pub async fn snapshot_before_restore(db_path: &Path, dir: &Path) -> AppResult<PathBuf> {
    std::fs::create_dir_all(dir).map_err(|e| AppError::io(format!("创建备份目录失败：{e}")))?;
    let stamp = time::compact_stamp(time::now());
    let target = dir.join(format!("pre-restore-{stamp}.db"));
    std::fs::copy(db_path, &target)
        .map_err(|e| AppError::io(format!("复制当前数据库失败：{e}")))?;
    // WAL 模式下还需要把 -wal/-shm 一并清理，避免恢复后状态错乱
    for suffix in ["-wal", "-shm"] {
        let extra = PathBuf::from(format!("{}{suffix}", db_path.display()));
        if extra.exists() {
            let _ = std::fs::remove_file(extra);
        }
    }
    Ok(target)
}
