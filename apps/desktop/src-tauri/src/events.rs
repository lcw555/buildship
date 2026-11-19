//! IPC 事件名与推送封装。
//!
//! 前端只监听这些事件名，命令返回值只负责"一次性结果"，长流程状态一律走事件。

use buildship_domain::{BuildEvent, DeployEvent, QuickState};
use tauri::{AppHandle, Emitter};

pub const EV_BUILD_STATUS: &str = "build://status";
pub const EV_DEPLOY_STATUS: &str = "deploy://status";
pub const EV_QUICK_STATE: &str = "quick://state";
pub const EV_TOAST: &str = "toast://message";

pub fn emit_build(app: &AppHandle, payload: BuildEvent) {
    if let Err(err) = app.emit(EV_BUILD_STATUS, payload) {
        tracing::warn!(error = %err, "推送构建事件失败");
    }
}

pub fn emit_deploy(app: &AppHandle, payload: DeployEvent) {
    if let Err(err) = app.emit(EV_DEPLOY_STATUS, payload) {
        tracing::warn!(error = %err, "推送部署事件失败");
    }
}

pub fn emit_quick(app: &AppHandle, payload: QuickState) {
    if let Err(err) = app.emit(EV_QUICK_STATE, payload) {
        tracing::warn!(error = %err, "推送快捷状态失败");
    }
}

pub fn toast_error(app: &AppHandle, message: impl Into<String>) {
    let payload = serde_json::json!({ "level": "error", "message": message.into() });
    let _ = app.emit(EV_TOAST, payload);
}
