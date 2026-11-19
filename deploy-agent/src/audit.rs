//! 追加式审计日志（JSON Lines）。
//!
//! Agent 侧不依赖数据库：每次关键操作（上传 / 应用 / 回滚 / 清理）都追加一行，
//! 只追加不修改，便于运维直接用 `findstr` / `grep` 或导入 SIEM。

use crate::store::Layout;
use buildship_common::{AppError, AppResult};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::io::Write;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry<'a> {
    pub ts: DateTime<Utc>,
    pub action: &'a str,
    pub outcome: &'a str,
    pub site: &'a str,
    pub release_id: Option<&'a str>,
    pub request_id: &'a str,
    pub remote: Option<&'a str>,
    pub message: Option<&'a str>,
    /// 补充字段（旧/新版本、文件数、耗时等）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
}

pub fn append(layout: &Layout, entry: AuditEntry<'_>) -> AppResult<()> {
    layout.ensure()?;
    let mut line = serde_json::to_string(&entry)
        .map_err(|e| AppError::internal(format!("审计日志序列化失败：{e}")))?;
    line.push('\n');
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(layout.audit_file())
        .map_err(|e| AppError::io(format!("打开审计日志失败：{e}")))?;
    file.write_all(line.as_bytes())
        .map_err(|e| AppError::io(format!("写入审计日志失败：{e}")))?;
    // 部署动作必须落稳，避免断电丢失审计
    file.flush()
        .map_err(|e| AppError::io(format!("刷新审计日志失败：{e}")))?;
    Ok(())
}
