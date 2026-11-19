//! 站点磁盘布局与状态文件。
//!
//! ```text
//! <site root>/
//!   current/                      线上版本（目录联接或真实目录）
//!   releases/<release_id>/        不可变版本目录
//!   .buildship/
//!     state.json                  当前 / 上一版本指针
//!     releases/<release_id>.json  版本元信息（哈希、文件数、上传时间）
//!     archives/<release_id>.zip   原始发布包（回滚与审计取证）
//!     audit.jsonl                 追加式审计日志
//!     idempotency/<request_id>.json  幂等重放响应
//!     tmp/                        上传与解压暂存
//! ```

use buildship_common::{time, AppError, AppResult};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 站点目录布局
#[derive(Debug, Clone)]
pub struct Layout {
    root: PathBuf,
}

impl Layout {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn current(&self) -> PathBuf {
        self.root.join("current")
    }
    pub fn current_next(&self) -> PathBuf {
        self.root.join("current.next")
    }
    pub fn current_prev(&self) -> PathBuf {
        self.root.join("current.prev")
    }
    pub fn releases_dir(&self) -> PathBuf {
        self.root.join("releases")
    }
    pub fn release_dir(&self, release_id: &str) -> PathBuf {
        self.releases_dir().join(release_id)
    }
    pub fn meta_dir(&self) -> PathBuf {
        self.root.join(".buildship")
    }
    pub fn state_file(&self) -> PathBuf {
        self.meta_dir().join("state.json")
    }
    pub fn audit_file(&self) -> PathBuf {
        self.meta_dir().join("audit.jsonl")
    }
    pub fn release_meta_dir(&self) -> PathBuf {
        self.meta_dir().join("releases")
    }
    pub fn release_meta_file(&self, release_id: &str) -> PathBuf {
        self.release_meta_dir().join(format!("{release_id}.json"))
    }
    pub fn archives_dir(&self) -> PathBuf {
        self.meta_dir().join("archives")
    }
    pub fn archive_file(&self, release_id: &str) -> PathBuf {
        self.archives_dir().join(format!("{release_id}.zip"))
    }
    pub fn idem_dir(&self) -> PathBuf {
        self.meta_dir().join("idempotency")
    }
    pub fn idem_file(&self, request_id: &str) -> PathBuf {
        self.idem_dir().join(format!("{request_id}.json"))
    }
    pub fn tmp_dir(&self) -> PathBuf {
        self.meta_dir().join("tmp")
    }

    /// 建立必需的目录结构
    pub fn ensure(&self) -> AppResult<()> {
        for dir in [
            self.releases_dir(),
            self.meta_dir(),
            self.release_meta_dir(),
            self.archives_dir(),
            self.idem_dir(),
            self.tmp_dir(),
        ] {
            std::fs::create_dir_all(&dir)
                .map_err(|e| AppError::io(format!("创建目录失败 {}：{e}", dir.display())))?;
        }
        Ok(())
    }
}

/// 版本指针状态
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteState {
    pub current: Option<String>,
    pub previous: Option<String>,
    pub applied_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl SiteState {
    pub fn load(layout: &Layout) -> AppResult<Self> {
        let path = layout.state_file();
        if !path.is_file() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| AppError::io(format!("读取 state.json 失败：{e}")))?;
        if text.trim().is_empty() {
            return Ok(Self::default());
        }
        serde_json::from_str(&text)
            .map_err(|e| AppError::internal(format!("state.json 解析失败：{e}")))
    }

    /// 先写临时文件再原子替换，避免断电留下半个 JSON。
    pub fn save(&self, layout: &Layout) -> AppResult<()> {
        layout.ensure()?;
        let path = layout.state_file();
        let tmp = path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| AppError::internal(format!("state.json 序列化失败：{e}")))?;
        std::fs::write(&tmp, text.as_bytes())
            .map_err(|e| AppError::io(format!("写入 state.json 失败：{e}")))?;
        replace_file(&tmp, &path)?;
        Ok(())
    }
}

/// 单个版本的元信息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseMeta {
    pub release_id: String,
    pub sha256: String,
    pub size_bytes: i64,
    pub file_count: i64,
    pub tree_sha256: String,
    pub uploaded_at: DateTime<Utc>,
    #[serde(default)]
    pub applied_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub source_request_id: Option<String>,
}

impl ReleaseMeta {
    pub fn load(layout: &Layout, release_id: &str) -> AppResult<Option<Self>> {
        let path = layout.release_meta_file(release_id);
        if !path.is_file() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| AppError::io(format!("读取版本元信息失败：{e}")))?;
        serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| AppError::internal(format!("版本元信息解析失败：{e}")))
    }

    pub fn save(&self, layout: &Layout) -> AppResult<()> {
        layout.ensure()?;
        let path = layout.release_meta_file(&self.release_id);
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| AppError::internal(format!("版本元信息序列化失败：{e}")))?;
        std::fs::write(&path, text.as_bytes())
            .map_err(|e| AppError::io(format!("写入版本元信息失败：{e}")))?;
        Ok(())
    }
}

/// 列出站点内所有版本（按上传时间倒序）
pub fn list_metas(layout: &Layout) -> AppResult<Vec<ReleaseMeta>> {
    let dir = layout.release_meta_dir();
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut items = Vec::new();
    for entry in
        std::fs::read_dir(&dir).map_err(|e| AppError::io(format!("读取版本目录失败：{e}")))?
    {
        let entry = entry.map_err(|e| AppError::io(format!("读取版本条目录失败：{e}")))?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(_) => continue,
        };
        if let Ok(meta) = serde_json::from_str::<ReleaseMeta>(&text) {
            items.push(meta);
        }
    }
    items.sort_by_key(|item| std::cmp::Reverse(item.uploaded_at));
    Ok(items)
}

/// 版本 ID 只允许安全字符，防止路径穿越
pub fn validate_release_id(release_id: &str) -> AppResult<()> {
    let trimmed = release_id.trim();
    if trimmed.is_empty() {
        return Err(AppError::invalid("releaseId 不能为空"));
    }
    if trimmed.len() > 128 {
        return Err(AppError::invalid("releaseId 过长（上限 128 字符）"));
    }
    let ok = trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if !ok || trimmed == "." || trimmed == ".." {
        return Err(AppError::invalid(format!(
            "releaseId 含非法字符（仅允许字母、数字、- _ .）：{trimmed}"
        )));
    }
    Ok(())
}

/// 跨平台原子替换文件
pub fn replace_file(from: &Path, to: &Path) -> AppResult<()> {
    #[cfg(windows)]
    {
        if to.exists() {
            // Windows 上 rename 不能覆盖已存在文件，先删再改名。
            // 失败场景下旧文件仍在，调用方会拿到明确错误。
            std::fs::remove_file(to).map_err(|e| {
                AppError::io(format!("替换文件失败（删除旧文件 {}）：{e}", to.display()))
            })?;
        }
    }
    std::fs::rename(from, to).map_err(|e| {
        AppError::io(format!(
            "替换文件失败 {} → {}：{e}",
            from.display(),
            to.display()
        ))
    })?;
    Ok(())
}

/// 统计目录内文件数量与总字节数
pub fn count_tree(dir: &Path) -> AppResult<(i64, i64)> {
    let mut files = 0i64;
    let mut bytes = 0i64;
    for entry in walkdir::WalkDir::new(dir).follow_links(false) {
        let entry = entry.map_err(|e| AppError::io(format!("遍历版本目录失败：{e}")))?;
        if entry.file_type().is_file() {
            files += 1;
            bytes += entry.metadata().map(|m| m.len() as i64).unwrap_or(0);
        }
    }
    Ok((files, bytes))
}

/// 计算目录树哈希（与桌面端 `tree_hash` 算法保持一致）
pub fn tree_hash_of(dir: &Path) -> AppResult<String> {
    let mut entries = Vec::new();
    for entry in walkdir::WalkDir::new(dir).follow_links(false) {
        let entry = entry.map_err(|e| AppError::io(format!("遍历版本目录失败：{e}")))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(dir)
            .map_err(|e| AppError::internal(format!("计算相对路径失败：{e}")))?
            .to_string_lossy()
            .replace('\\', "/");
        let size = entry.metadata().map(|m| m.len() as i64).unwrap_or(0);
        let digest = buildship_artifact::hashing::sha256_file(entry.path())?;
        entries.push((relative, size, digest));
    }
    Ok(buildship_artifact::hashing::tree_hash(&entries))
}

/// 记录一次标记时间
pub fn now() -> DateTime<Utc> {
    time::now()
}
