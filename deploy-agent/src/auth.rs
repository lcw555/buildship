//! 请求认证与上下文提取。
//!
//! 安全要点：
//! - Token 比较固定时间（先各自 SHA-256 再逐字节异或累加），不泄露长度与命中位置。
//! - 协议版本必须匹配，避免旧客户端误对接新 Agent 造成半成功状态。
//! - 请求 ID 由客户端提供并原样回传，便于跨端串联日志。

use crate::config::Config;
use crate::error::{AgentError, AgentResult};
use axum::http::HeaderMap;
use buildship_release::protocol::{
    API_VERSION, HEADER_API_VERSION, HEADER_REQUEST_ID, HEADER_TOKEN,
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// 认证并解析出请求上下文
pub fn authorize(config: &Config, headers: &HeaderMap) -> AgentResult<RequestCtx> {
    let request_id = header_str(headers, HEADER_REQUEST_ID)
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    // 协议版本：缺省视为 v1，显式给了就必须一致
    if let Some(version) = header_str(headers, HEADER_API_VERSION) {
        if version != API_VERSION {
            return Err(AgentError::invalid(
                &request_id,
                format!("协议版本不匹配：Agent 支持 v{API_VERSION}，请求声明 v{version}"),
            ));
        }
    }

    let token = header_str(headers, HEADER_TOKEN).ok_or_else(|| {
        AgentError::new(
            buildship_common::AppError::new(
                buildship_common::ErrorCode::CredentialUnavailable,
                "缺少访问凭据（x-buildship-token）",
            ),
            &request_id,
        )
    })?;

    if !constant_time_eq(token.as_bytes(), config.token.as_bytes()) {
        return Err(AgentError::new(
            buildship_common::AppError::new(
                buildship_common::ErrorCode::CredentialUnavailable,
                "访问凭据无效",
            ),
            &request_id,
        ));
    }

    Ok(RequestCtx { request_id })
}

pub struct RequestCtx {
    pub request_id: String,
}

pub fn header_str(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim().to_string())
}

/// 来源地址：优先取反向代理头，便于审计里还原真实调用方。
///
/// Agent 生产部署通常在 nginx / IIS 之后，直接取 TCP 对端只能看到代理地址，
/// 因此从 `x-forwarded-for` / `x-real-ip` 取，取不到就返回 None（不伪造）。
pub fn remote_of(headers: &HeaderMap) -> Option<String> {
    if let Some(forwarded) = header_str(headers, "x-forwarded-for") {
        if let Some(first) = forwarded.split(',').next() {
            let value = first.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    header_str(headers, "x-real-ip").filter(|value| !value.is_empty())
}

/// 固定时间比较：先做摘要对齐长度，再全量遍历。
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let a = Sha256::digest(left);
    let b = Sha256::digest(right);
    let mut diff = 0u8;
    for index in 0..a.len() {
        diff |= a[index] ^ b[index];
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_compare() {
        assert!(constant_time_eq(b"abcdef1234567890", b"abcdef1234567890"));
        assert!(!constant_time_eq(b"abcdef1234567890", b"abcdef1234567891"));
        assert!(!constant_time_eq(b"short", b"a-much-longer-secret"));
    }
}
