//! 领域枚举。
//!
//! 约束（方案 5.4）：状态字段只允许明确的枚举值，数据库列上带 CHECK 约束，
//! Rust 侧通过 `sqlx::Type` 与 serde 保证写入/输出的字符串与建表脚本完全一致。

use serde::{Deserialize, Serialize};

/// 包管理器
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
pub enum PackageManager {
    Npm,
    Pnpm,
    Yarn,
}

impl PackageManager {
    /// 可执行文件名（Windows 会自动补 `.cmd`）
    pub fn binary(self) -> &'static str {
        match self {
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Yarn => "yarn",
        }
    }

    pub fn run_subcommand(self) -> &'static str {
        match self {
            Self::Npm => "run",
            Self::Pnpm => "run",
            Self::Yarn => "run",
        }
    }

    pub fn as_str(self) -> &'static str {
        self.binary()
    }
}

/// 记录状态：删除采用受控归档（软删除），不做物理删除，避免破坏历史构建与发布记录
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum RecordStatus {
    Active,
    Archived,
}

/// 项目状态语义别名
pub type ProjectStatus = RecordStatus;

/// 构建任务状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum BuildStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl BuildStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

/// 日志流来源
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
pub enum LogStream {
    Stdout,
    Stderr,
    System,
}

/// 发布包状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum ArtifactStatus {
    Ready,
    Corrupt,
    Missing,
    Purged,
}

/// 环境类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
pub enum EnvironmentKind {
    Dev,
    Test,
    Staging,
    Prod,
}

/// Release 状态机（方案 4.6）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum ReleaseStatus {
    Draft,
    Uploading,
    Deploying,
    HealthCheck,
    Healthy,
    Failed,
    RolledBack,
    Cancelled,
}

impl ReleaseStatus {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Healthy | Self::Failed | Self::RolledBack | Self::Cancelled
        )
    }

    /// 是否可作为回滚目标：只有成功且健康的版本
    pub fn is_rollback_candidate(self) -> bool {
        matches!(self, Self::Healthy | Self::RolledBack)
    }
}

/// 部署任务类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
pub enum DeploymentKind {
    Deploy,
    Rollback,
}

/// 部署任务状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum DeploymentStatus {
    Queued,
    Uploading,
    Deploying,
    HealthCheck,
    Succeeded,
    Failed,
    Cancelled,
}

impl DeploymentStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

/// 健康检查结果
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum HealthStatus {
    Unknown,
    Passed,
    Failed,
    Skipped,
}

/// 发布事件级别
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
pub enum EventLevel {
    Info,
    Warn,
    Error,
}

/// 审计动作（机器可读，前端做本地化展示）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum AuditAction {
    ProjectAdded,
    ProjectUpdated,
    ProjectArchived,
    ProjectRescanned,
    BuildStarted,
    BuildSucceeded,
    BuildFailed,
    BuildCancelled,
    ArtifactCreated,
    ArtifactVerified,
    ArtifactDeleted,
    EnvironmentSaved,
    EnvironmentArchived,
    ReleaseCreated,
    ReleaseSucceeded,
    ReleaseFailed,
    ReleaseRolledBack,
    RollbackFailed,
    SettingsUpdated,
    BackupCreated,
    BackupRestored,
    ToolchainChecked,
}

impl AuditAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProjectAdded => "project_added",
            Self::ProjectUpdated => "project_updated",
            Self::ProjectArchived => "project_archived",
            Self::ProjectRescanned => "project_rescanned",
            Self::BuildStarted => "build_started",
            Self::BuildSucceeded => "build_succeeded",
            Self::BuildFailed => "build_failed",
            Self::BuildCancelled => "build_cancelled",
            Self::ArtifactCreated => "artifact_created",
            Self::ArtifactVerified => "artifact_verified",
            Self::ArtifactDeleted => "artifact_deleted",
            Self::EnvironmentSaved => "environment_saved",
            Self::EnvironmentArchived => "environment_archived",
            Self::ReleaseCreated => "release_created",
            Self::ReleaseSucceeded => "release_succeeded",
            Self::ReleaseFailed => "release_failed",
            Self::ReleaseRolledBack => "release_rolled_back",
            Self::RollbackFailed => "rollback_failed",
            Self::SettingsUpdated => "settings_updated",
            Self::BackupCreated => "backup_created",
            Self::BackupRestored => "backup_restored",
            Self::ToolchainChecked => "toolchain_checked",
        }
    }
}

/// 凭据存储提供方
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum SecretProvider {
    WindowsCredentialManager,
    MacosKeychain,
    /// 仅测试使用，正式流程禁止写入
    Memory,
}
