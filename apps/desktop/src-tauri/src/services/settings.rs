//! 设置、备份恢复、系统凭据。

use crate::infrastructure::credential;
use crate::services::audit::{self, AuditInput};
use crate::state::AppState;
use buildship_common::{time, AppError, AppResult, ErrorCode};
use buildship_db::{backup, repositories as repo, Db};
use buildship_domain::{
    AppSetting, AuditAction, BackupEntry, BackupResult, SecretProvider, SecretReference,
    SettingsPatch,
};
use std::path::Path;

pub async fn all(state: &AppState) -> AppResult<Vec<AppSetting>> {
    let db = state.db().await;
    repo::settings::all(db.pool()).await
}

/// 批量写入设置。只允许非敏感键值，凭据必须走 `save_secret`。
pub async fn update(state: &AppState, patch: SettingsPatch) -> AppResult<Vec<AppSetting>> {
    let db = state.db().await;
    let mut tx = db.pool().begin().await?;
    for (key, value) in &patch.entries {
        if key.contains("password") || key.contains("token") || key.contains("secret") {
            return Err(AppError::invalid(format!(
                "设置项 {key} 看起来是敏感信息，请使用凭据管理保存"
            )));
        }
        repo::settings::upsert(&mut *tx, key, value).await?;
    }
    repo::audit::append(
        &mut *tx,
        &audit::event(AuditInput {
            actor: &state.operator,
            action: AuditAction::SettingsUpdated,
            entity_type: "app_settings",
            entity_id: "settings",
            project_id: None,
            summary: format!(
                "修改设置项：{}",
                patch.entries.keys().cloned().collect::<Vec<_>>().join("、")
            ),
            detail: Some(serde_json::json!({ "keys": patch.entries.keys().collect::<Vec<_>>() })),
        }),
    )
    .await?;
    tx.commit().await?;
    repo::settings::all(db.pool()).await
}

pub async fn get_value(state: &AppState, key: &str) -> AppResult<Option<String>> {
    let db = state.db().await;
    repo::settings::get(db.pool(), key).await
}

// ---------------------------------------------------------------------------
// 备份
// ---------------------------------------------------------------------------

pub async fn list_backups(state: &AppState) -> AppResult<Vec<BackupEntry>> {
    backup::list(&state.paths.backups_dir()).await
}

pub async fn create_backup(state: &AppState) -> AppResult<BackupResult> {
    let entry = {
        let db = state.db().await;
        backup::create(&db, &state.paths.backups_dir()).await?
    };

    {
        let db = state.db().await;
        let mut tx = db.pool().begin().await?;
        repo::audit::append(
            &mut *tx,
            &audit::event(AuditInput {
                actor: &state.operator,
                action: AuditAction::BackupCreated,
                entity_type: "database_backup",
                entity_id: &entry.file_name,
                project_id: None,
                summary: format!("创建数据库备份：{}", entry.file_name),
                detail: Some(serde_json::json!({
                    "sha256": entry.sha256,
                    "sizeBytes": entry.size_bytes,
                })),
            }),
        )
        .await?;
        tx.commit().await?;
    }

    Ok(BackupResult {
        message: format!("备份完成：{}（{} 字节）", entry.file_name, entry.size_bytes),
        entry,
    })
}

/// 从备份恢复数据库。
///
/// 步骤：校验备份 → 记录当前库快照 → 关闭连接池 → 替换文件 → 重新打开并迁移 → 完整性检查。
pub async fn restore_backup(state: &AppState, backup_path: &str) -> AppResult<BackupResult> {
    let source = Path::new(backup_path);
    let version = backup::validate(source).await?;

    // 恢复前自动备份当前库
    let safety = {
        let db = state.db().await;
        backup::create(&db, &state.paths.backups_dir()).await?
    };

    // 关闭连接池（恢复期间独占数据库文件）
    {
        let db = state.db().await;
        db.pool().close().await;
    }

    let db_path = state.paths.db_file();
    for suffix in ["-wal", "-shm"] {
        let extra = format!("{}{suffix}", db_path.display());
        if Path::new(&extra).exists() {
            let _ = std::fs::remove_file(extra);
        }
    }

    if db_path.exists() {
        std::fs::remove_file(&db_path)
            .map_err(|e| AppError::new(ErrorCode::IoError, format!("替换数据库文件失败：{e}")))?;
    }
    std::fs::copy(source, &db_path)
        .map_err(|e| AppError::new(ErrorCode::IoError, format!("复制备份文件失败：{e}")))?;

    // 重新打开（会自动执行迁移，兼容旧 schema）
    let reopened = Db::open(&db_path).await?;
    let (ok, message) = reopened.integrity_check().await?;
    let schema = reopened.schema_version();
    state.replace_db(reopened).await;

    if !ok {
        return Err(AppError::new(
            ErrorCode::ChecksumMismatch,
            format!("恢复后的数据库完整性检查失败：{message}"),
        ));
    }

    {
        let backup_name = source
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        let db = state.db().await;
        let mut tx = db.pool().begin().await?;
        repo::audit::append(
            &mut *tx,
            &audit::event(AuditInput {
                actor: &state.operator,
                action: AuditAction::BackupRestored,
                entity_type: "database_backup",
                entity_id: &backup_name,
                project_id: None,
                summary: format!(
                    "从备份恢复数据库（备份 schema v{version}，当前 v{schema}），恢复前已自动备份为 {}",
                    safety.file_name
                ),
                detail: Some(serde_json::json!({ "safetyBackup": safety.path })),
            }),
        )
        .await?;
        tx.commit().await?;
    }

    Ok(BackupResult {
        message: format!("恢复完成，schema v{schema}，完整性检查通过"),
        entry: safety,
    })
}

// ---------------------------------------------------------------------------
// 凭据
// ---------------------------------------------------------------------------

/// 保存 Agent Token：正文进系统凭据库，SQLite 只存引用
pub async fn save_secret(state: &AppState, label: &str, token: &str) -> AppResult<SecretReference> {
    let label = label.trim();
    if label.is_empty() {
        return Err(AppError::invalid("凭据名称不能为空"));
    }
    credential::save(label, token)?;

    let existing = {
        let db = state.db().await;
        repo::settings::list_secret_refs(db.pool()).await?
    }
    .into_iter()
    .find(|item| item.label == label);

    let reference = SecretReference {
        id: existing
            .as_ref()
            .map(|item| item.id.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        label: label.to_string(),
        provider: if cfg!(windows) {
            SecretProvider::WindowsCredentialManager
        } else {
            SecretProvider::MacosKeychain
        },
        account: label.to_string(),
        created_at: existing
            .map(|item| item.created_at)
            .unwrap_or_else(time::now),
        last_used_at: None,
    };

    let db = state.db().await;
    repo::settings::upsert_secret_ref(db.pool(), &reference).await?;
    Ok(reference)
}

pub async fn list_secrets(state: &AppState) -> AppResult<Vec<SecretReference>> {
    let db = state.db().await;
    repo::settings::list_secret_refs(db.pool()).await
}
