//! 维护操作：清理本机数据、一键卸载。
//!
//! 为什么需要这一层：
//! - 安装包卸载器默认把 `%APPDATA%\Buildship` 留下（用户会在资源管理器里看到残留），
//!   应用内提供「一键卸载」把数据清理与卸载器调用串成一次点击；
//! - 清理前必须先停构建、删系统凭据、关连接池，否则 Windows 上文件被占用删不掉。

use crate::infrastructure::credential;
use crate::services::settings;
use crate::state::AppState;
use buildship_common::fsutil::{dir_size, human_size};
use buildship_common::{AppError, AppResult};
use buildship_db::repositories as repo;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// 清理/卸载的执行结果，直接回给前端展示明细。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanSummary {
    pub data_dir: String,
    /// 已删除的顶层条目名
    pub removed: Vec<String>,
    /// 删不掉的条目（通常是仍被占用的文件）
    pub failed: Vec<String>,
    pub freed_bytes: i64,
    /// 卸载前导出的备份路径（勾选了「先备份」时才有）
    pub backup_path: Option<String>,
    /// 找到的卸载程序路径；为 None 表示当前是便携/开发运行
    pub uninstaller: Option<String>,
    pub message: String,
}

/// 清理前的防御性校验。
///
/// 这段代码会 `remove_dir_all`，一旦路径被误配置（例如把根目录当成数据目录），
/// 后果不可逆。因此只接受 `<某个已有目录>/Buildship` 这种形状的路径。
fn ensure_safe_root(root: &Path) -> AppResult<()> {
    let name = root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    // 目录名必须是 Buildship，且有父目录（排除盘符根、用户主目录被直接清掉）
    let parent_ok = root.parent().is_some_and(|parent| parent.exists());
    if name != "Buildship" || !parent_ok {
        return Err(AppError::internal(format!(
            "拒绝清理异常的数据目录：{}",
            root.display()
        )));
    }
    Ok(())
}

fn dir_size_of(path: &Path) -> i64 {
    if path.is_dir() {
        dir_size(path)
    } else {
        std::fs::metadata(path)
            .map(|meta| meta.len() as i64)
            .unwrap_or(0)
    }
}

/// 把最新备份复制到桌面：数据目录马上要被删，备份必须挪出去才有意义。
async fn export_backup(state: &AppState) -> AppResult<PathBuf> {
    let created = settings::create_backup(state).await?;
    let source = PathBuf::from(&created.entry.path);
    let target_dir = dirs::desktop_dir()
        .or_else(dirs::home_dir)
        .ok_or_else(|| AppError::io("无法定位桌面目录，备份未导出"))?;
    let target = target_dir.join(&created.entry.file_name);
    std::fs::copy(&source, &target)
        .map_err(|err| AppError::io(format!("导出备份到桌面失败：{err}")))?;
    Ok(target)
}

/// 清理本机数据（保留程序本身），返回明细。
pub async fn clean_data(state: &AppState, backup_first: bool) -> AppResult<CleanSummary> {
    let root = state.paths.root().to_path_buf();
    ensure_safe_root(&root)?;

    // 1. 先停所有构建：子进程仍持有日志/产物文件时，Windows 会拒绝删除
    let cancelled = state.builds.cancel_all().await;
    if cancelled > 0 {
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    // 2. 可选导出备份到桌面（必须在删库之前做）
    let backup_path = if backup_first {
        match export_backup(state).await {
            Ok(path) => Some(path.to_string_lossy().to_string()),
            Err(err) => {
                tracing::warn!(error = %err, "卸载前导出备份失败");
                None
            }
        }
    } else {
        None
    };

    // 3. 删除系统凭据库里的条目（正文不在数据库里，不删会残留在凭据管理器）
    let labels: Vec<String> = {
        let db = state.db().await;
        repo::settings::list_secret_refs(db.pool())
            .await
            .map(|items| items.into_iter().map(|item| item.label).collect())
            .unwrap_or_default()
    };
    for label in labels {
        if let Err(err) = credential::delete(&label) {
            tracing::warn!(label = %label, error = %err, "删除系统凭据失败");
        }
    }

    // 4. 关闭连接池，释放 db / -wal / -shm 的文件句柄
    {
        let db = state.db().await;
        db.pool().close().await;
    }

    // 5. 逐项删除（根目录保留，随后重建骨架）
    let mut removed = Vec::new();
    let mut failed = Vec::new();
    let mut freed = 0i64;
    if let Ok(entries) = std::fs::read_dir(&root) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let size = dir_size_of(&path);
            let result = if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
            match result {
                Ok(_) => {
                    freed += size;
                    removed.push(name);
                }
                Err(err) => {
                    tracing::warn!(entry = %name, error = %err, "清理数据条目失败");
                    failed.push(name);
                }
            }
        }
    }

    // 6. 重建目录，进程还能继续写日志/重启
    state.paths.ensure()?;

    let message = if failed.is_empty() {
        format!("已清理 {} 项，释放 {}", removed.len(), human_size(freed))
    } else {
        format!(
            "已清理 {} 项，{} 项被占用未能删除（退出后会自动释放）",
            removed.len(),
            failed.len()
        )
    };

    Ok(CleanSummary {
        data_dir: root.to_string_lossy().to_string(),
        removed,
        failed,
        freed_bytes: freed,
        backup_path,
        uninstaller: None,
        message,
    })
}

/// 查找安装包自带的卸载程序（NSIS 安装在 exe 同目录放 `uninstall.exe`）。
pub fn find_uninstaller() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    ["uninstall.exe", "Uninstall.exe"]
        .iter()
        .map(|name| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// 静默启动卸载程序。
///
/// `/S` 是 NSIS 的静默参数：Tauri 模板在静默模式下会直接结束仍在运行的应用，
/// 因此不需要我们先退出，卸载器自己会处理。
fn launch_uninstaller(path: &Path) -> AppResult<()> {
    std::process::Command::new(path)
        .arg("/S")
        .current_dir(path.parent().unwrap_or_else(|| Path::new(".")))
        .spawn()
        .map_err(|err| AppError::io(format!("启动卸载程序失败：{err}")))?;
    Ok(())
}

/// 一键卸载：清数据 → 启动卸载器（找不到则只清数据）。
pub async fn uninstall(state: &AppState, backup_first: bool) -> AppResult<CleanSummary> {
    let mut summary = clean_data(state, backup_first).await?;

    match find_uninstaller() {
        Some(path) => {
            launch_uninstaller(&path)?;
            summary.uninstaller = Some(path.to_string_lossy().to_string());
            summary.message = "数据已清理，卸载程序已启动".to_string();
        }
        None => {
            summary.message =
                "数据已清理。当前是便携或开发运行，未找到卸载程序，请手动删除程序目录".to_string();
        }
    }

    Ok(summary)
}
