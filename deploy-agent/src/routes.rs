//! HTTP 路由与业务处理。
//!
//! 端点与桌面端 `buildship-release::client` 一一对应：
//!
//! | 方法 | 路径            | 说明                     |
//! |------|-----------------|--------------------------|
//! | GET  | /v1/status      | Agent 概览与站点列表     |
//! | GET  | /v1/releases    | 站点已部署版本           |
//! | POST | /v1/upload      | 上传发布包（流式）       |
//! | POST | /v1/apply       | 切换线上版本             |
//! | POST | /v1/rollback    | 回滚到指定版本           |
//! | GET  | /v1/health      | 代客户端做健康检查       |
//! | GET  | /healthz        | 无鉴权探活（容器/负载均衡）|

use crate::audit::{self, AuditEntry};
use crate::auth::{self, header_str};
use crate::config::{Config, Site};
use crate::error::{AgentError, AgentResult};
use crate::health;
use crate::store::{self, Layout, ReleaseMeta, SiteState};
use crate::switch;
use crate::upload;
use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use buildship_common::{AppError, ErrorCode};
use buildship_release::protocol::*;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Mutex, RwLock};

/// 进程内共享状态
pub struct AppCtx {
    pub config: Config,
    pub started: Instant,
    /// 每个站点一把锁，避免并发切换互相覆盖
    site_locks: RwLock<HashMap<String, Arc<Mutex<()>>>>,
    /// 保留的幂等记录条数（超出后清理最旧）
    idem_keep: usize,
    pub apply_count: AtomicU64,
}

impl AppCtx {
    pub fn new(config: Config) -> Arc<Self> {
        Arc::new(Self {
            config,
            started: Instant::now(),
            site_locks: RwLock::new(HashMap::new()),
            idem_keep: 200,
            apply_count: AtomicU64::new(0),
        })
    }

    async fn site_lock(&self, site: &str) -> Arc<Mutex<()>> {
        if let Some(existing) = self.site_locks.read().await.get(site) {
            return existing.clone();
        }
        let mut guard = self.site_locks.write().await;
        guard
            .entry(site.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}

pub fn router(ctx: Arc<AppCtx>) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/status", get(status))
        .route("/v1/releases", get(list_releases))
        .route("/v1/upload", post(upload_release))
        .route("/v1/apply", post(apply))
        .route("/v1/rollback", post(rollback))
        .route("/v1/health", get(health_probe))
        .with_state(ctx)
}

// ---------------------------------------------------------------- 无鉴权探活

async fn healthz() -> &'static str {
    "ok"
}

// ---------------------------------------------------------------- 概览

async fn status(
    State(ctx): State<Arc<AppCtx>>,
    headers: HeaderMap,
) -> AgentResult<Json<AgentStatus>> {
    let auth = auth::authorize(&ctx.config, &headers)?;

    let mut sites = Vec::new();
    for site in ctx.config.sites.values() {
        let layout = Layout::new(&site.root);
        let state = SiteState::load(&layout).map_err(|e| AgentError::new(e, &auth.request_id))?;
        let metas = store::list_metas(&layout).map_err(|e| AgentError::new(e, &auth.request_id))?;
        // 指针指向的目录可能被人工删除，这里以磁盘为准
        let current = state
            .current
            .clone()
            .filter(|id| store::validate_release_id(id).is_ok() && layout.release_dir(id).is_dir());
        sites.push(SiteStatus {
            site: site.name.clone(),
            physical_path: layout.root().display().to_string(),
            current_release: current,
            previous_release: state.previous.clone(),
            release_count: metas.len() as i64,
            updated_at: state.updated_at,
        });
    }

    Ok(Json(AgentStatus {
        api_version: API_VERSION.to_string(),
        agent_version: env!("CARGO_PKG_VERSION").to_string(),
        hostname: hostname(),
        os: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        uptime_seconds: ctx.started.elapsed().as_secs() as i64,
        server_time: store::now(),
        sites,
    }))
}

fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

#[derive(Debug, Deserialize)]
struct SiteQuery {
    site: String,
}

async fn list_releases(
    State(ctx): State<Arc<AppCtx>>,
    headers: HeaderMap,
    Query(query): Query<SiteQuery>,
) -> AgentResult<Json<Vec<ReleaseInfo>>> {
    let auth = auth::authorize(&ctx.config, &headers)?;
    let site = ctx
        .config
        .site(&query.site)
        .map_err(|e| AgentError::new(e, &auth.request_id))?;
    let layout = Layout::new(&site.root);
    let state = SiteState::load(&layout).map_err(|e| AgentError::new(e, &auth.request_id))?;
    let metas = store::list_metas(&layout).map_err(|e| AgentError::new(e, &auth.request_id))?;

    let items = metas
        .into_iter()
        .map(|meta| ReleaseInfo {
            release_id: meta.release_id.clone(),
            path: layout.release_dir(&meta.release_id).display().to_string(),
            size_bytes: meta.size_bytes,
            file_count: meta.file_count,
            is_current: state.current.as_deref() == Some(meta.release_id.as_str()),
            applied_at: meta.applied_at,
            tree_sha256: Some(meta.tree_sha256),
        })
        .collect();

    Ok(Json(items))
}

// ---------------------------------------------------------------- 上传

async fn upload_release(
    State(ctx): State<Arc<AppCtx>>,
    headers: HeaderMap,
    body: Body,
) -> AgentResult<Json<UploadResponse>> {
    let auth = auth::authorize(&ctx.config, &headers)?;

    let release_id = header_str(&headers, HEADER_RELEASE_ID)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AgentError::invalid(&auth.request_id, "缺少 x-buildship-release-id"))?;
    let sha256 = header_str(&headers, HEADER_SHA256)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AgentError::invalid(&auth.request_id, "缺少 x-buildship-sha256"))?;
    // 站点从查询参数取；缺省时使用唯一站点，多站点必须显式指定
    let site = resolve_site_for_upload(&ctx.config, &headers, &auth.request_id)?;

    let layout = Layout::new(&site.root);
    let lock = ctx.site_lock(&site.name).await;
    let _guard = lock.lock().await;

    let outcome = upload::receive(
        &layout,
        &release_id,
        &sha256,
        body,
        ctx.config.max_upload_bytes,
        &auth.request_id,
    )
    .await
    .map_err(|e| AgentError::new(e, &auth.request_id))?;

    let remote = crate::auth::remote_of(&headers);
    audit::append(
        &layout,
        AuditEntry {
            ts: store::now(),
            action: "upload",
            outcome: if outcome.already_existed {
                "reused"
            } else {
                "success"
            },
            site: &site.name,
            release_id: Some(&release_id),
            request_id: &auth.request_id,
            remote: remote.as_deref(),
            message: None,
            detail: Some(serde_json::json!({
                "sha256": outcome.sha256,
                "sizeBytes": outcome.size_bytes,
                "fileCount": outcome.file_count,
            })),
        },
    )
    .map_err(|e| AgentError::new(e, &auth.request_id))?;

    Ok(Json(UploadResponse {
        release_id: outcome.release_id,
        stored_path: outcome.stored_path.display().to_string(),
        size_bytes: outcome.size_bytes,
        sha256: outcome.sha256,
        file_count: outcome.file_count,
        tree_sha256: Some(outcome.tree_sha256),
        already_existed: outcome.already_existed,
    }))
}

fn resolve_site_for_upload<'a>(
    config: &'a Config,
    headers: &HeaderMap,
    request_id: &str,
) -> AgentResult<&'a Site> {
    if let Some(name) = header_str(headers, "x-buildship-site").filter(|v| !v.is_empty()) {
        return config
            .site(&name)
            .map_err(|e| AgentError::new(e, request_id));
    }
    if config.sites.len() == 1 {
        return Ok(config.sites.values().next().expect("非空"));
    }
    Err(AgentError::invalid(
        request_id,
        "Agent 配置了多个站点，上传时必须通过 x-buildship-site 指明目标站点",
    ))
}

// ---------------------------------------------------------------- 应用

async fn apply(
    State(ctx): State<Arc<AppCtx>>,
    headers: HeaderMap,
    Json(request): Json<ApplyRequest>,
) -> AgentResult<Json<ApplyResponse>> {
    let auth = auth::authorize(&ctx.config, &headers)?;
    let site = ctx
        .config
        .authorize_path(&request.site, &request.physical_path)
        .map_err(|e| AgentError::new(e, &auth.request_id))?;
    store::validate_release_id(&request.release_id)
        .map_err(|e| AgentError::new(e, &auth.request_id))?;

    let layout = Layout::new(&site.root);
    layout
        .ensure()
        .map_err(|e| AgentError::new(e, &auth.request_id))?;

    // 幂等重放：同一 requestId 直接返回上次结果
    if let Some(cached) = read_idempotent::<ApplyResponse>(&layout, &request.request_id) {
        tracing::info!(
            request_id = %request.request_id,
            "命中幂等记录，直接返回上次结果"
        );
        return Ok(Json(cached));
    }

    let lock = ctx.site_lock(&site.name).await;
    let _guard = lock.lock().await;

    let release_dir = layout.release_dir(&request.release_id);
    if !release_dir.is_dir() {
        return Err(AgentError::not_found(
            &auth.request_id,
            format!("版本 {} 尚未上传到本站点", request.release_id),
        ));
    }
    let meta = ReleaseMeta::load(&layout, &request.release_id)
        .map_err(|e| AgentError::new(e, &auth.request_id))?
        .ok_or_else(|| {
            AgentError::not_found(
                &auth.request_id,
                format!("版本 {} 缺少元信息，请重新上传", request.release_id),
            )
        })?;
    if let Some(expected) = request.tree_sha256.as_deref() {
        if !expected.eq_ignore_ascii_case(&meta.tree_sha256) {
            return Err(AgentError::new(
                AppError::new(
                    ErrorCode::ChecksumMismatch,
                    format!(
                        "目录树哈希不一致：请求 {expected}，Agent 记录 {}",
                        meta.tree_sha256
                    ),
                ),
                &auth.request_id,
            ));
        }
    }

    let mut state = SiteState::load(&layout).map_err(|e| AgentError::new(e, &auth.request_id))?;
    let previous = state.current.clone();

    // 已在目标版本上：补一次健康检查后返回，视为成功（幂等）
    if previous.as_deref() == Some(request.release_id.as_str()) && layout.current().exists() {
        let health_url = request
            .health_check_url
            .clone()
            .or_else(|| site.health_check_url.clone());
        let (health_status, health_message) =
            run_health(&health_url, ctx.config.health_timeout).await;
        let response = ApplyResponse {
            release_id: request.release_id.clone(),
            previous_release_id: state.previous.clone(),
            current_path: layout.current().display().to_string(),
            health_status,
            health_message,
            applied_at: state.applied_at.unwrap_or_else(store::now),
            rolled_back: false,
        };
        write_idempotent(&layout, &request.request_id, &response, ctx.idem_keep);
        return Ok(Json(response));
    }

    let switch_outcome = switch::switch(&layout, &request.release_id, ctx.config.link_strategy)
        .map_err(|e| AgentError::new(e, &auth.request_id))?;

    let applied_at = store::now();
    state.current = Some(request.release_id.clone());
    state.previous = previous.clone();
    state.applied_at = Some(applied_at);
    state.updated_at = Some(applied_at);
    state
        .save(&layout)
        .map_err(|e| AgentError::new(e, &auth.request_id))?;

    // 版本元信息补记应用时间
    let mut updated_meta = meta;
    updated_meta.applied_at = Some(applied_at);
    if let Err(err) = updated_meta.save(&layout) {
        tracing::warn!(error = %err, "更新版本元信息失败");
    }

    let health_url = request
        .health_check_url
        .clone()
        .or_else(|| site.health_check_url.clone());
    let (health_status, health_message) = run_health(&health_url, ctx.config.health_timeout).await;
    let mut rolled_back = false;

    if health_status == "failed" {
        // 自动回滚到上一版本，尽最大努力保持站点可用
        if let Some(prev) = previous.clone() {
            match switch::switch(&layout, &prev, ctx.config.link_strategy) {
                Ok(_) => {
                    state.current = Some(prev.clone());
                    state.previous = Some(request.release_id.clone());
                    state.applied_at = Some(store::now());
                    state.updated_at = Some(store::now());
                    let _ = state.save(&layout);
                    rolled_back = true;
                    tracing::error!(
                        release_id = %request.release_id,
                        previous = %prev,
                        "健康检查未通过，已自动回滚"
                    );
                }
                Err(err) => {
                    tracing::error!(error = %err, "自动回滚失败，需人工介入");
                }
            }
        } else {
            tracing::error!(
                release_id = %request.release_id,
                "健康检查未通过且无上一版本可回滚"
            );
        }
    }

    ctx.apply_count.fetch_add(1, Ordering::Relaxed);
    let remote = crate::auth::remote_of(&headers);
    let audit_result = audit::append(
        &layout,
        AuditEntry {
            ts: store::now(),
            action: "apply",
            outcome: if health_status == "failed" {
                "failed"
            } else {
                "success"
            },
            site: &site.name,
            release_id: Some(&request.release_id),
            request_id: &auth.request_id,
            remote: remote.as_deref(),
            message: health_message.as_deref(),
            detail: Some(serde_json::json!({
                "previousReleaseId": previous,
                "switchMode": switch_outcome.mode.as_str(),
                "currentPath": switch_outcome.current_path().display().to_string(),
                "healthStatus": health_status,
                "rolledBack": rolled_back,
                "treeSha256": updated_meta.tree_sha256,
            })),
        },
    );
    if let Err(err) = audit_result {
        tracing::warn!(error = %err, "写审计日志失败");
    }

    // 健康检查失败且回滚成功：仍返回 200，让桌面端拿到 rolled_back 与诊断文案
    if health_status == "failed" && !rolled_back && previous.is_none() {
        return Err(AgentError::new(
            AppError::new(
                ErrorCode::HealthCheckFailed,
                format!(
                    "版本已切换但健康检查未通过，且没有可回滚的历史版本：{}",
                    health_message.clone().unwrap_or_default()
                ),
            ),
            &auth.request_id,
        ));
    }

    let response = ApplyResponse {
        release_id: request.release_id.clone(),
        previous_release_id: previous,
        current_path: layout.current().display().to_string(),
        health_status,
        health_message,
        applied_at,
        rolled_back,
    };
    write_idempotent(&layout, &request.request_id, &response, ctx.idem_keep);
    prune_if_needed(&layout, site.keep_releases, &state);

    Ok(Json(response))
}

async fn run_health(
    url: &Option<String>,
    timeout: std::time::Duration,
) -> (String, Option<String>) {
    match url {
        None => ("skipped".to_string(), None),
        Some(target) => match health::check(target, timeout).await {
            Ok(report) => {
                let status = if report.ok { "passed" } else { "failed" }.to_string();
                (status, Some(report.message))
            }
            Err(err) => ("failed".to_string(), Some(err.to_string())),
        },
    }
}

// ---------------------------------------------------------------- 回滚

async fn rollback(
    State(ctx): State<Arc<AppCtx>>,
    headers: HeaderMap,
    Json(request): Json<RollbackRequest>,
) -> AgentResult<Json<ApplyResponse>> {
    let auth = auth::authorize(&ctx.config, &headers)?;
    let site = ctx
        .config
        .authorize_path(&request.site, &request.physical_path)
        .map_err(|e| AgentError::new(e, &auth.request_id))?;
    store::validate_release_id(&request.to_release_id)
        .map_err(|e| AgentError::new(e, &auth.request_id))?;
    if request.reason.trim().is_empty() {
        return Err(AgentError::invalid(&auth.request_id, "回滚必须填写原因"));
    }

    let layout = Layout::new(&site.root);
    layout
        .ensure()
        .map_err(|e| AgentError::new(e, &auth.request_id))?;

    if let Some(cached) = read_idempotent::<ApplyResponse>(&layout, &request.request_id) {
        return Ok(Json(cached));
    }

    let lock = ctx.site_lock(&site.name).await;
    let _guard = lock.lock().await;

    if !layout.release_dir(&request.to_release_id).is_dir() {
        return Err(AgentError::not_found(
            &auth.request_id,
            format!("目标版本不存在：{}", request.to_release_id),
        ));
    }

    let mut state = SiteState::load(&layout).map_err(|e| AgentError::new(e, &auth.request_id))?;
    let previous = state.current.clone();
    if previous.as_deref() == Some(request.to_release_id.as_str()) {
        return Err(AgentError::conflict(
            &auth.request_id,
            format!("{} 已是当前线上版本", request.to_release_id),
        ));
    }

    switch::switch(&layout, &request.to_release_id, ctx.config.link_strategy)
        .map_err(|e| AgentError::new(e, &auth.request_id))?;

    let applied_at = store::now();
    state.current = Some(request.to_release_id.clone());
    state.previous = previous.clone();
    state.applied_at = Some(applied_at);
    state.updated_at = Some(applied_at);
    state
        .save(&layout)
        .map_err(|e| AgentError::new(e, &auth.request_id))?;

    let health_url = request
        .health_check_url
        .clone()
        .or_else(|| site.health_check_url.clone());
    let (health_status, health_message) = run_health(&health_url, ctx.config.health_timeout).await;
    let mut rolled_back = false;

    // 回滚后仍不可用：再切回原版本，避免把站点留在坏状态
    if health_status == "failed" {
        if let Some(prev) = previous.clone() {
            if switch::switch(&layout, &prev, ctx.config.link_strategy).is_ok() {
                state.current = Some(prev.clone());
                state.previous = Some(request.to_release_id.clone());
                state.updated_at = Some(store::now());
                let _ = state.save(&layout);
                rolled_back = true;
            }
        }
    }

    let remote = crate::auth::remote_of(&headers);
    let audit_result = audit::append(
        &layout,
        AuditEntry {
            ts: store::now(),
            action: "rollback",
            outcome: if health_status == "failed" {
                "failed"
            } else {
                "success"
            },
            site: &site.name,
            release_id: Some(&request.to_release_id),
            request_id: &auth.request_id,
            remote: remote.as_deref(),
            message: Some(request.reason.trim()),
            detail: Some(serde_json::json!({
                "fromReleaseId": previous,
                "healthStatus": health_status,
                "revertedToOriginal": rolled_back,
                "healthMessage": health_message,
            })),
        },
    );
    if let Err(err) = audit_result {
        tracing::warn!(error = %err, "写审计日志失败");
    }

    let response = ApplyResponse {
        release_id: request.to_release_id.clone(),
        previous_release_id: previous,
        current_path: layout.current().display().to_string(),
        health_status,
        health_message,
        applied_at,
        rolled_back,
    };
    write_idempotent(&layout, &request.request_id, &response, ctx.idem_keep);
    Ok(Json(response))
}

// ---------------------------------------------------------------- 健康检查

#[derive(Debug, Deserialize)]
struct HealthQuery {
    url: String,
}

async fn health_probe(
    State(ctx): State<Arc<AppCtx>>,
    headers: HeaderMap,
    Query(query): Query<HealthQuery>,
) -> AgentResult<Json<HealthReport>> {
    let auth = auth::authorize(&ctx.config, &headers)?;
    let report = health::check(&query.url, ctx.config.health_timeout)
        .await
        .map_err(|e| AgentError::new(e, &auth.request_id))?;
    Ok(Json(report))
}

// ---------------------------------------------------------------- 幂等与清理

fn read_idempotent<T: serde::de::DeserializeOwned>(layout: &Layout, request_id: &str) -> Option<T> {
    if request_id.trim().is_empty() || store::validate_release_id(request_id).is_err() {
        return None;
    }
    let path: PathBuf = layout.idem_file(request_id);
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_idempotent<T: serde::Serialize>(
    layout: &Layout,
    request_id: &str,
    response: &T,
    keep: usize,
) {
    if request_id.trim().is_empty() || store::validate_release_id(request_id).is_err() {
        return;
    }
    let dir = layout.idem_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    if let Ok(text) = serde_json::to_string(response) {
        let _ = std::fs::write(layout.idem_file(request_id), text.as_bytes());
    }
    // 只保留最近 N 条，避免无限增长
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
            .filter_map(|e| {
                let modified = e.metadata().ok()?.modified().ok()?;
                Some((modified, e.path()))
            })
            .collect();
        if files.len() > keep {
            files.sort_by_key(|entry| entry.0);
            let remove_count = files.len() - keep;
            for (_, path) in files.into_iter().take(remove_count) {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

/// 按 `keepReleases` 清理历史版本（保留当前与上一版本）
fn prune_if_needed(layout: &Layout, keep: u32, state: &SiteState) {
    if keep == 0 {
        return;
    }
    let metas = match store::list_metas(layout) {
        Ok(items) => items,
        Err(err) => {
            tracing::warn!(error = %err, "读取版本列表失败，跳过清理");
            return;
        }
    };
    if metas.len() as u32 <= keep {
        return;
    }
    for meta in metas.into_iter().skip(keep as usize) {
        let is_protected = state.current.as_deref() == Some(meta.release_id.as_str())
            || state.previous.as_deref() == Some(meta.release_id.as_str());
        if is_protected {
            continue;
        }
        let dir = layout.release_dir(&meta.release_id);
        if let Err(err) = switch::remove_entry(&dir) {
            tracing::warn!(release_id = %meta.release_id, error = %err, "清理旧版本目录失败");
            continue;
        }
        let _ = std::fs::remove_file(layout.release_meta_file(&meta.release_id));
        let _ = std::fs::remove_file(layout.archive_file(&meta.release_id));
        tracing::info!(release_id = %meta.release_id, "已清理旧版本");
    }
}
