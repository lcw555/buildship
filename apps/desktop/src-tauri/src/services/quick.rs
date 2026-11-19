//! 快捷栏 / 工作台共享的"当前上下文"。

use crate::events;
use crate::state::AppState;
use buildship_domain::QuickState;
use tauri::AppHandle;

/// 计算当前快捷状态
pub async fn snapshot(state: &AppState) -> QuickState {
    let context = state.quick_snapshot().await;
    let active = state.builds.active().await;
    QuickState {
        has_project: context.project_id.is_some(),
        active_build: active.is_some(),
        active_task_id: active.as_ref().map(|item| item.task_id.clone()),
        project_name: context.project_name.clone(),
        project_id: context.project_id.clone(),
        last_build_status: context.last_build_status,
    }
}

/// 推送给所有窗口
pub async fn emit(app: &AppHandle, state: &AppState) {
    events::emit_quick(app, snapshot(state).await);
}
