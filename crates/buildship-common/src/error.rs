//! 统一错误模型。
//!
//! 设计要点（对应方案第 12 章安全要求）：
//! - 错误码是稳定的机器可读枚举，前端按 code 分支处理，不解析文案。
//! - `detail` 只放可公开的诊断信息，敏感内容必须先脱敏。
//! - 所有 IPC 命令统一返回 `Result<T, AppError>`，Tauri 会把它序列化成
//!   `{ code, message, detail }` 结构，前端 `invoke` 的 reject 值即可直接消费。

use serde::{Deserialize, Serialize};
use std::fmt;

/// 稳定的错误码枚举。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    /// 入参不合法（路径为空、字段越界等）
    InvalidInput,
    /// 实体不存在
    NotFound,
    /// 与当前状态冲突（重复添加、唯一约束）
    Conflict,
    /// 本地文件系统权限不足
    PermissionDenied,
    /// 未检测到 Node.js / 包管理器
    ToolchainMissing,
    /// 同一项目已有活动构建任务
    BuildAlreadyRunning,
    /// 子进程执行失败
    ProcessFailed,
    /// 任务被用户取消
    Cancelled,
    /// IO 错误
    IoError,
    /// 数据库错误
    DbError,
    /// 网络错误（访问 Agent）
    NetworkError,
    /// Agent 不可达或未就绪
    AgentUnavailable,
    /// 发布包内容与 manifest/哈希不一致
    ChecksumMismatch,
    /// 健康检查未通过
    HealthCheckFailed,
    /// 状态机非法流转
    StateInvalid,
    /// 凭据缺失或不可读取
    CredentialUnavailable,
    /// 功能未实现（骨架阶段占位，必须显式暴露）
    Unimplemented,
    /// 兜底内部错误
    Internal,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidInput => "INVALID_INPUT",
            Self::NotFound => "NOT_FOUND",
            Self::Conflict => "CONFLICT",
            Self::PermissionDenied => "PERMISSION_DENIED",
            Self::ToolchainMissing => "TOOLCHAIN_MISSING",
            Self::BuildAlreadyRunning => "BUILD_ALREADY_RUNNING",
            Self::ProcessFailed => "PROCESS_FAILED",
            Self::Cancelled => "CANCELLED",
            Self::IoError => "IO_ERROR",
            Self::DbError => "DB_ERROR",
            Self::NetworkError => "NETWORK_ERROR",
            Self::AgentUnavailable => "AGENT_UNAVAILABLE",
            Self::ChecksumMismatch => "CHECKSUM_MISMATCH",
            Self::HealthCheckFailed => "HEALTH_CHECK_FAILED",
            Self::StateInvalid => "STATE_INVALID",
            Self::CredentialUnavailable => "CREDENTIAL_UNAVAILABLE",
            Self::Unimplemented => "UNIMPLEMENTED",
            Self::Internal => "INTERNAL",
        }
    }
}

/// 对外暴露的应用错误。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidInput, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Conflict, message)
    }

    pub fn io(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::IoError, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }

    pub fn unimplemented(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Unimplemented, message)
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.detail {
            Some(detail) => write!(f, "[{}] {}: {}", self.code.as_str(), self.message, detail),
            None => write!(f, "[{}] {}", self.code.as_str(), self.message),
        }
    }
}

impl std::error::Error for AppError {}

pub type AppResult<T> = Result<T, AppError>;

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        let code = match err.kind() {
            std::io::ErrorKind::NotFound => ErrorCode::NotFound,
            std::io::ErrorKind::PermissionDenied => ErrorCode::PermissionDenied,
            _ => ErrorCode::IoError,
        };
        Self::new(code, err.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        Self::new(ErrorCode::Internal, format!("JSON 序列化失败：{err}"))
    }
}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        match &err {
            sqlx::Error::RowNotFound => Self::not_found("记录不存在"),
            sqlx::Error::Database(db) if db.is_unique_violation() => {
                Self::conflict(format!("唯一约束冲突：{}", db.message()))
            }
            sqlx::Error::Database(db) if db.is_foreign_key_violation() => {
                Self::conflict(format!("外键约束失败：{}", db.message()))
            }
            _ => Self::new(ErrorCode::DbError, err.to_string()),
        }
    }
}

impl From<sqlx::migrate::MigrateError> for AppError {
    fn from(err: sqlx::migrate::MigrateError) -> Self {
        Self::new(ErrorCode::DbError, format!("数据库迁移失败：{err}"))
    }
}

impl From<reqwest::Error> for AppError {
    fn from(err: reqwest::Error) -> Self {
        let code = if err.is_connect() || err.is_timeout() {
            ErrorCode::AgentUnavailable
        } else {
            ErrorCode::NetworkError
        };
        Self::new(code, err.to_string())
    }
}
