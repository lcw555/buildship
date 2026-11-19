//! 发布清单 manifest。
//!
//! manifest 同时写进 ZIP 根目录与包外同名 `.manifest.json`，
//! 服务端 Agent 校验时会重新计算每个文件的 SHA-256 并与清单比对。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 清单结构版本，Agent 与桌面端按此协商
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub schema_version: u32,
    pub artifact: ManifestArtifact,
    pub source: ManifestSource,
    pub files: Vec<ManifestFile>,
    pub summary: ManifestSummary,
}

/// 发布包身份信息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestArtifact {
    pub id: String,
    pub project_id: String,
    pub project_name: String,
    pub project_slug: String,
    pub build_task_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_commit: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// 打包来源信息（便于回溯"这个包是怎么来的"）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestSource {
    pub build_command: String,
    pub output_dir: String,
    pub packaged_at: DateTime<Utc>,
    pub packaged_by: String,
}

/// 单个文件条目
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestFile {
    /// ZIP 内路径（统一 `/` 分隔，UTF-8）
    pub path: String,
    pub size: i64,
    pub sha256: String,
}

/// 汇总
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestSummary {
    pub file_count: i64,
    pub total_bytes: i64,
    /// 目录树一致性哈希
    pub tree_sha256: String,
    /// 打包时被排除的规则名（便于用户理解为什么少了文件）
    pub excluded_rules: Vec<String>,
}

impl Manifest {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    pub fn parse(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }
}
