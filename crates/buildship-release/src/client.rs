//! Agent HTTP 客户端。
//!
//! - 统一附加协议版本、请求 ID、认证 Token。
//! - 上传用流式请求体，避免把大包一次性读进内存。
//! - 非 2xx 响应解析为结构化错误码，映射回 `ErrorCode`。

use crate::protocol::*;
use buildship_common::{AppError, AppResult, ErrorCode};
use std::path::Path;
use std::time::Duration;
use tokio_util::io::ReaderStream;

/// 部署通道客户端
#[derive(Clone)]
pub struct DeployClient {
    base_url: String,
    token: String,
    http: reqwest::Client,
}

impl DeployClient {
    pub fn new(base_url: &str, token: &str, tls_verify: bool) -> AppResult<Self> {
        let base_url = base_url.trim_end_matches('/').to_string();
        if base_url.is_empty() {
            return Err(AppError::invalid("Agent 地址不能为空"));
        }
        if !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
            return Err(AppError::invalid(
                "Agent 地址必须以 http:// 或 https:// 开头",
            ));
        }
        if token.trim().is_empty() {
            return Err(AppError::new(
                ErrorCode::CredentialUnavailable,
                "缺少 Agent 访问凭据，请在部署目标中配置",
            ));
        }
        let http = reqwest::Client::builder()
            .danger_accept_invalid_certs(!tls_verify)
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .user_agent(concat!("Buildship/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self {
            base_url,
            token: token.to_string(),
            http,
        })
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}{}", self.base_url, path))
            .header(HEADER_TOKEN, &self.token)
            .header(HEADER_API_VERSION, API_VERSION)
            .header(HEADER_REQUEST_ID, uuid::Uuid::new_v4().to_string())
    }

    async fn parse<T: serde::de::DeserializeOwned>(response: reqwest::Response) -> AppResult<T> {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if !status.is_success() {
            let body: Option<AgentErrorBody> = serde_json::from_str(&text).ok();
            let (code, message) = match body {
                Some(err) => (
                    map_code(&err.code),
                    format!("Agent 拒绝请求：{}", err.message),
                ),
                None => (
                    if status.as_u16() == 401 || status.as_u16() == 403 {
                        ErrorCode::CredentialUnavailable
                    } else {
                        ErrorCode::AgentUnavailable
                    },
                    format!("Agent 返回 HTTP {status}：{}", truncate(&text)),
                ),
            };
            return Err(AppError::new(code, message));
        }
        serde_json::from_str(&text).map_err(|e| {
            AppError::new(
                ErrorCode::NetworkError,
                format!("Agent 响应格式不合法：{e}（原始内容：{}）", truncate(&text)),
            )
        })
    }

    /// 查询 Agent 概览
    pub async fn status(&self) -> AppResult<AgentStatus> {
        let response = self
            .request(reqwest::Method::GET, "/v1/status")
            .send()
            .await?;
        Self::parse(response).await
    }

    /// 列出某个站点已部署版本
    pub async fn list_releases(&self, site: &str) -> AppResult<Vec<ReleaseInfo>> {
        let response = self
            .request(reqwest::Method::GET, "/v1/releases")
            .query(&[("site", site)])
            .send()
            .await?;
        Self::parse(response).await
    }

    /// 上传发布包（流式）
    pub async fn upload(
        &self,
        release_id: &str,
        zip_path: &Path,
        sha256: &str,
    ) -> AppResult<UploadResponse> {
        let file = tokio::fs::File::open(zip_path)
            .await
            .map_err(|e| AppError::io(format!("打开发布包失败：{e}")))?;
        let size = file
            .metadata()
            .await
            .map(|m| m.len())
            .map_err(|e| AppError::io(format!("读取发布包大小失败：{e}")))?;

        let body = reqwest::Body::wrap_stream(ReaderStream::new(file));
        // 上传比常规请求更宽松的超时
        let response = self
            .request(reqwest::Method::POST, "/v1/upload")
            .header(HEADER_RELEASE_ID, release_id)
            .header(HEADER_SHA256, sha256)
            .header(reqwest::header::CONTENT_LENGTH, size)
            .header(reqwest::header::CONTENT_TYPE, "application/zip")
            .timeout(Duration::from_secs(30 * 60))
            .body(body)
            .send()
            .await?;
        Self::parse(response).await
    }

    /// 应用版本（原子切换 + 健康检查）
    pub async fn apply(&self, request: &ApplyRequest) -> AppResult<ApplyResponse> {
        let response = self
            .request(reqwest::Method::POST, "/v1/apply")
            .json(request)
            .timeout(Duration::from_secs(300))
            .send()
            .await?;
        Self::parse(response).await
    }

    /// 回滚到指定版本
    pub async fn rollback(&self, request: &RollbackRequest) -> AppResult<ApplyResponse> {
        let response = self
            .request(reqwest::Method::POST, "/v1/rollback")
            .json(request)
            .timeout(Duration::from_secs(300))
            .send()
            .await?;
        Self::parse(response).await
    }

    /// 健康检查（Agent 侧代检，用于发布前连通性测试）
    pub async fn health_check(&self, url: &str) -> AppResult<HealthReport> {
        let response = self
            .request(reqwest::Method::GET, "/v1/health")
            .query(&[("url", url)])
            .timeout(Duration::from_secs(30))
            .send()
            .await?;
        Self::parse(response).await
    }
}

fn map_code(code: &str) -> ErrorCode {
    match code {
        "CHECKSUM_MISMATCH" => ErrorCode::ChecksumMismatch,
        "HEALTH_CHECK_FAILED" => ErrorCode::HealthCheckFailed,
        "INVALID_INPUT" => ErrorCode::InvalidInput,
        "NOT_FOUND" => ErrorCode::NotFound,
        "CONFLICT" => ErrorCode::Conflict,
        "PERMISSION_DENIED" => ErrorCode::PermissionDenied,
        _ => ErrorCode::AgentUnavailable,
    }
}

fn truncate(text: &str) -> String {
    let mut out: String = text.chars().take(300).collect();
    if text.chars().count() > 300 {
        out.push('…');
    }
    out
}
