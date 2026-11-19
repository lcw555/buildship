//! 健康检查：由 Agent 代客户端探测站点 URL。
//!
//! 放在 Agent 侧做是因为只有服务器自身才具备"内网可达"的视角，
//! 桌面端在办公网里往往探测不到生产地址。

use buildship_common::{AppError, AppResult};
use buildship_release::protocol::HealthReport;
use chrono::Utc;
use std::time::Duration;

/// 执行一次健康检查。返回的 `ok` 只表示 2xx/3xx，具体文案给出诊断信息。
pub async fn check(url: &str, timeout: Duration) -> AppResult<HealthReport> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(AppError::invalid("健康检查地址不能为空"));
    }
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return Err(AppError::invalid(
            "健康检查地址必须以 http:// 或 https:// 开头",
        ));
    }

    let client = reqwest::Client::builder()
        .connect_timeout(timeout)
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent(concat!("Buildship-Agent/", env!("CARGO_PKG_VERSION")))
        .build()?;

    let started = std::time::Instant::now();
    let checked_at = Utc::now();
    match client.get(trimmed).send().await {
        Ok(response) => {
            let status = response.status();
            let code = status.as_u16();
            let ok = status.is_success() || status.is_redirection();
            let elapsed = started.elapsed().as_millis();
            let message = if ok {
                format!("HTTP {code}，耗时 {elapsed} ms")
            } else {
                format!("HTTP {code}，耗时 {elapsed} ms（期望 2xx/3xx）")
            };
            Ok(HealthReport {
                ok,
                status_code: Some(code),
                message,
                checked_at,
            })
        }
        Err(err) => {
            let detail = if err.is_timeout() {
                "请求超时".to_string()
            } else if err.is_connect() {
                "连接失败（端口未监听或防火墙拦截）".to_string()
            } else {
                err.to_string()
            };
            Ok(HealthReport {
                ok: false,
                status_code: None,
                message: format!("{detail}（{trimmed}）"),
                checked_at,
            })
        }
    }
}
