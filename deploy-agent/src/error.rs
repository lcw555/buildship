//! Agent 错误 → HTTP 响应。
//!
//! 所有失败都返回结构化错误体 + 稳定错误码，桌面端按 code 分支处理，
//! 不解析自然语言文案。

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use buildship_common::{AppError, ErrorCode};
use buildship_release::protocol::AgentErrorBody;

/// 带请求上下文的错误载体
#[derive(Debug)]
pub struct AgentError {
    pub inner: AppError,
    pub request_id: String,
}

impl AgentError {
    pub fn new(inner: AppError, request_id: impl Into<String>) -> Self {
        Self {
            inner,
            request_id: request_id.into(),
        }
    }

    pub fn invalid(request_id: &str, message: impl Into<String>) -> Self {
        Self::new(AppError::invalid(message), request_id)
    }

    pub fn not_found(request_id: &str, message: impl Into<String>) -> Self {
        Self::new(AppError::not_found(message), request_id)
    }

    pub fn conflict(request_id: &str, message: impl Into<String>) -> Self {
        Self::new(AppError::conflict(message), request_id)
    }
}

impl From<AppError> for AgentError {
    fn from(inner: AppError) -> Self {
        Self {
            inner,
            request_id: String::new(),
        }
    }
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.inner)
    }
}

fn status_for(code: ErrorCode) -> StatusCode {
    match code {
        ErrorCode::InvalidInput => StatusCode::BAD_REQUEST,
        ErrorCode::NotFound => StatusCode::NOT_FOUND,
        ErrorCode::Conflict => StatusCode::CONFLICT,
        ErrorCode::PermissionDenied | ErrorCode::CredentialUnavailable => StatusCode::FORBIDDEN,
        ErrorCode::ChecksumMismatch => StatusCode::UNPROCESSABLE_ENTITY,
        ErrorCode::HealthCheckFailed => StatusCode::BAD_GATEWAY,
        ErrorCode::AgentUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

impl IntoResponse for AgentError {
    fn into_response(self) -> Response {
        let status = status_for(self.inner.code);
        if status.is_server_error() {
            tracing::error!(
                request_id = %self.request_id,
                code = self.inner.code.as_str(),
                error = %self.inner,
                "Agent 处理失败"
            );
        } else {
            tracing::warn!(
                request_id = %self.request_id,
                code = self.inner.code.as_str(),
                error = %self.inner,
                "Agent 拒绝请求"
            );
        }
        let body = AgentErrorBody {
            code: self.inner.code.as_str().to_string(),
            message: self.inner.message,
            detail: self.inner.detail,
            request_id: self.request_id,
        };
        (status, Json(body)).into_response()
    }
}

pub type AgentResult<T> = Result<T, AgentError>;
