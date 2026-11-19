//! 领域实体。
//!
//! 与 `migrations/0001_init.sql` 中的表结构一一对应。字段名即列名（snake_case），
//! 序列化给前端时统一 camelCase，避免前端出现两套命名。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::enums::*;

/// 前端项目
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub path: String,
    pub package_manager: PackageManager,
    pub package_manager_version: Option<String>,
    pub node_version: Option<String>,
    pub framework: Option<String>,
    pub status: ProjectStatus,
    pub last_build_status: Option<BuildStatus>,
    pub last_build_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 项目构建配置（一个项目可有多套，`is_default` 指定默认）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct BuildProfile {
    pub id: String,
    pub project_id: String,
    pub name: String,
    /// package.json 中的 script 名，例如 `build`
    pub build_script: Option<String>,
    /// 实际执行的命令行摘要，仅用于展示；真实执行使用参数数组
    pub build_command: String,
    /// 附加参数（JSON 数组字符串），逐个传给包管理器
    pub build_args_json: String,
    /// 相对项目根目录的产物目录，例如 `dist`
    pub output_dir: String,
    /// JSON 对象，构建时注入的环境变量（值不得含密钥）
    pub env_json: String,
    pub is_default: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 构建任务
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct BuildTask {
    pub id: String,
    pub project_id: String,
    pub profile_id: Option<String>,
    pub status: BuildStatus,
    pub command: String,
    pub args_json: String,
    pub exit_code: Option<i64>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub duration_ms: Option<i64>,
    pub log_path: Option<String>,
    pub log_size: Option<i64>,
    pub log_sha256: Option<String>,
    pub error_summary: Option<String>,
    pub triggered_by: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 构建日志分段元数据（正文落在日志文件，避免数据库膨胀）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct BuildLogSegment {
    pub id: String,
    pub task_id: String,
    pub segment_index: i64,
    pub file_path: String,
    pub start_offset: i64,
    pub end_offset: i64,
    pub line_count: i64,
    pub created_at: DateTime<Utc>,
}

/// 发布包（ZIP + manifest + SHA-256）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub id: String,
    pub project_id: String,
    pub build_task_id: String,
    pub file_name: String,
    pub file_path: String,
    pub size_bytes: i64,
    pub sha256: String,
    pub file_count: i64,
    pub manifest_path: String,
    pub manifest_sha256: Option<String>,
    pub status: ArtifactStatus,
    pub git_commit: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 发布环境（开发 / 测试 / 预发布 / 生产）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentEnvironment {
    pub id: String,
    pub name: String,
    pub kind: EnvironmentKind,
    pub description: Option<String>,
    pub status: RecordStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 部署目标：一台 Windows Server 上的一个 IIS 站点
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentTarget {
    pub id: String,
    pub environment_id: String,
    pub name: String,
    pub server_host: Option<String>,
    pub agent_base_url: String,
    pub iis_site: String,
    pub physical_path: String,
    pub health_check_url: Option<String>,
    pub credential_ref: Option<String>,
    pub tls_verify: bool,
    pub status: RecordStatus,
    pub last_checked_at: Option<DateTime<Utc>>,
    pub last_check_ok: Option<bool>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 部署任务（一次上传+切换的执行记录，带幂等键）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentTask {
    pub id: String,
    pub release_id: String,
    pub target_id: String,
    pub project_id: String,
    pub kind: DeploymentKind,
    pub status: DeploymentStatus,
    pub stage: Option<String>,
    pub progress: i64,
    pub request_id: String,
    pub idempotency_key: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 发布版本
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub id: String,
    pub project_id: String,
    pub artifact_id: String,
    pub environment_id: String,
    pub target_id: String,
    pub version: String,
    pub deploy_path: Option<String>,
    pub status: ReleaseStatus,
    pub previous_release_id: Option<String>,
    pub rolled_back_from: Option<String>,
    pub rollback_reason: Option<String>,
    pub health_status: HealthStatus,
    pub health_message: Option<String>,
    pub is_current: bool,
    pub operator: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 发布过程事件（时间线）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseEvent {
    pub id: String,
    pub release_id: String,
    pub seq: i64,
    pub phase: String,
    pub level: EventLevel,
    pub message: String,
    pub detail_json: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// 操作审计（只允许新增）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AuditEvent {
    pub id: String,
    pub actor: String,
    pub action: AuditAction,
    pub entity_type: String,
    pub entity_id: String,
    pub project_id: Option<String>,
    pub summary: String,
    pub detail_json: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// 非敏感应用设置
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AppSetting {
    pub key: String,
    pub value: String,
    pub updated_at: DateTime<Utc>,
}

/// 系统凭据库中的凭据引用（不存明文）
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SecretReference {
    pub id: String,
    pub label: String,
    pub provider: SecretProvider,
    pub account: String,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}
