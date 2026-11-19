//! 应用运行时状态。
//!
//! 只有这里持有"全局可变"的东西：数据库连接、进行中的构建、当前选中项目。
//! 其它模块都通过函数参数拿引用，避免到处都是全局单例。

use buildship_build::BuildCancel;
use buildship_common::AppPaths;
use buildship_db::Db;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

/// 正在进行的构建
#[derive(Debug, Clone)]
pub struct ActiveBuild {
    pub project_id: String,
    pub project_name: String,
    pub task_id: String,
}

/// 当前选中项目 + 活动构建（供悬浮栏与工作台使用）
#[derive(Debug, Default, Clone)]
pub struct QuickContext {
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub last_build_status: Option<buildship_domain::BuildStatus>,
    pub active: Option<ActiveBuild>,
}

/// 构建并发控制：同一项目只允许一个活动构建
#[derive(Default)]
pub struct BuildManager {
    cancels: Mutex<HashMap<String, BuildCancel>>,
    active: Mutex<Option<ActiveBuild>>,
}

impl BuildManager {
    pub async fn register(&self, task_id: &str, cancel: BuildCancel) {
        self.cancels
            .lock()
            .await
            .insert(task_id.to_string(), cancel);
    }

    pub async fn cancel_token(&self, task_id: &str) -> Option<BuildCancel> {
        self.cancels.lock().await.get(task_id).cloned()
    }

    pub async fn unregister(&self, task_id: &str) {
        self.cancels.lock().await.remove(task_id);
    }

    /// 取消全部进行中的构建（维护操作前必须先停子进程，否则日志文件删不掉）。
    /// 返回发出的取消指令数量。
    pub async fn cancel_all(&self) -> usize {
        let tokens = {
            let cancels = self.cancels.lock().await;
            cancels
                .iter()
                .map(|(task_id, token)| (task_id.clone(), token.clone()))
                .collect::<Vec<_>>()
        };
        for (_, token) in &tokens {
            token.cancel();
        }
        tokens.len()
    }

    pub async fn set_active(&self, active: Option<ActiveBuild>) {
        *self.active.lock().await = active;
    }

    pub async fn active(&self) -> Option<ActiveBuild> {
        self.active.lock().await.clone()
    }
}

/// 应用全局状态
pub struct AppState {
    pub paths: AppPaths,
    pub db: RwLock<Db>,
    /// 本机用户标识（写入审计的 actor）
    pub operator: String,
    pub builds: BuildManager,
    pub quick: Mutex<QuickContext>,
}

pub type SharedState = Arc<AppState>;

impl AppState {
    pub async fn db(&self) -> tokio::sync::RwLockReadGuard<'_, Db> {
        self.db.read().await
    }

    /// 备份恢复时需要替换整个连接
    pub async fn replace_db(&self, db: Db) {
        *self.db.write().await = db;
    }

    pub async fn quick_snapshot(&self) -> QuickContext {
        self.quick.lock().await.clone()
    }

    pub async fn set_selected_project(
        &self,
        project_id: Option<String>,
        project_name: Option<String>,
    ) {
        let mut quick = self.quick.lock().await;
        quick.project_id = project_id;
        quick.project_name = project_name;
    }

    pub async fn set_last_build(&self, status: buildship_domain::BuildStatus) {
        let mut quick = self.quick.lock().await;
        quick.last_build_status = Some(status);
    }
}

/// 解析本机用户标识：优先系统用户名，保证审计可追溯到人。
pub fn detect_operator() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "local-user".to_string())
}
