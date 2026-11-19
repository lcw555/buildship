//! Agent 通信协议（桌面端与 Agent 共用同一份结构，避免协议漂移）。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 协议版本
pub const API_VERSION: &str = "1";

pub const HEADER_TOKEN: &str = "x-buildship-token";
pub const HEADER_REQUEST_ID: &str = "x-buildship-request-id";
pub const HEADER_API_VERSION: &str = "x-buildship-api-version";
pub const HEADER_RELEASE_ID: &str = "x-buildship-release-id";
pub const HEADER_SHA256: &str = "x-buildship-sha256";

/// 站点当前状态
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteStatus {
    pub site: String,
    pub physical_path: String,
    pub current_release: Option<String>,
    pub previous_release: Option<String>,
    pub release_count: i64,
    pub updated_at: Option<DateTime<Utc>>,
}

/// Agent 概览
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    pub api_version: String,
    pub agent_version: String,
    pub hostname: String,
    pub os: String,
    pub uptime_seconds: i64,
    pub server_time: DateTime<Utc>,
    pub sites: Vec<SiteStatus>,
}

/// 已部署版本条目
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseInfo {
    pub release_id: String,
    pub path: String,
    pub size_bytes: i64,
    pub file_count: i64,
    pub is_current: bool,
    pub applied_at: Option<DateTime<Utc>>,
    pub tree_sha256: Option<String>,
}

/// 上传响应
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadResponse {
    pub release_id: String,
    pub stored_path: String,
    pub size_bytes: i64,
    pub sha256: String,
    pub file_count: i64,
    pub tree_sha256: Option<String>,
    pub already_existed: bool,
}

/// 应用（切换线上版本）请求
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyRequest {
    pub release_id: String,
    pub site: String,
    /// 站点物理根目录（Agent 会做路径规范化与白名单校验）
    pub physical_path: String,
    pub health_check_url: Option<String>,
    /// 期望的目录树哈希，用于二次确认包内容
    pub tree_sha256: Option<String>,
    pub request_id: String,
}

/// 回滚请求
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RollbackRequest {
    pub to_release_id: String,
    pub site: String,
    pub physical_path: String,
    pub health_check_url: Option<String>,
    pub reason: String,
    pub request_id: String,
}

/// 应用 / 回滚结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResponse {
    pub release_id: String,
    pub previous_release_id: Option<String>,
    pub current_path: String,
    pub health_status: String,
    pub health_message: Option<String>,
    pub applied_at: DateTime<Utc>,
    /// 失败时是否已自动恢复到上一健康版本
    pub rolled_back: bool,
}

/// 健康检查报告
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthReport {
    pub ok: bool,
    pub status_code: Option<u16>,
    pub message: String,
    pub checked_at: DateTime<Utc>,
}

/// 结构化错误体
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentErrorBody {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub request_id: String,
}
