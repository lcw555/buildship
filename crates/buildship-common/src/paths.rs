//! 应用数据目录规划。
//!
//! 约定（方案 5.1）：数据库与日志、发布包都放在系统应用数据目录，
//! 绝不写入安装目录或用户项目目录。

use crate::error::{AppError, AppResult};
use std::path::{Path, PathBuf};

/// 应用数据目录布局。
#[derive(Debug, Clone)]
pub struct AppPaths {
    root: PathBuf,
}

impl AppPaths {
    /// 使用系统应用数据目录（Windows: `%APPDATA%`，macOS: `~/Library/Application Support`）。
    pub fn system() -> AppResult<Self> {
        let base = dirs::data_dir()
            .ok_or_else(|| AppError::internal("无法定位系统应用数据目录"))?
            .join("Buildship");
        Ok(Self { root: base })
    }

    /// 指定根目录（测试与便携模式使用）。
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn db_file(&self) -> PathBuf {
        self.root.join("buildship.db")
    }

    pub fn db_dir(&self) -> PathBuf {
        self.root.clone()
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// 构建任务的工作目录（临时产物、日志分段）
    pub fn work_dir(&self) -> PathBuf {
        self.root.join("work")
    }

    /// 发布包（ZIP + manifest）存放目录
    pub fn artifacts_dir(&self) -> PathBuf {
        self.root.join("artifacts")
    }

    /// 数据库备份目录
    pub fn backups_dir(&self) -> PathBuf {
        self.root.join("backups")
    }

    /// 构建日志目录
    pub fn build_logs_dir(&self) -> PathBuf {
        self.work_dir().join("logs")
    }

    /// 创建全部目录。
    pub fn ensure(&self) -> AppResult<()> {
        for dir in [
            self.root.clone(),
            self.logs_dir(),
            self.work_dir(),
            self.build_logs_dir(),
            self.artifacts_dir(),
            self.backups_dir(),
        ] {
            std::fs::create_dir_all(&dir)
                .map_err(|e| AppError::io(format!("创建目录 {} 失败：{e}", dir.display())))?;
        }
        Ok(())
    }
}
