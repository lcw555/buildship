//! Agent 配置加载与校验。
//!
//! 配置来源优先级：命令行 `--config` > 环境变量 `BUILDSHIP_AGENT_CONFIG` > 默认路径。
//! Token 支持直接写在配置文件里（便于单机部署），也支持 `tokenFile` 引用，
//! 后者更适合托管在服务器上并限制文件权限。

use buildship_common::{AppError, AppResult};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// `current` 的切换方式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkStrategy {
    /// 先尝试目录联接（Windows junction，无需管理员），失败再退回复制
    Auto,
    /// 强制目录联接 / 符号链接
    Link,
    /// 强制复制整棵目录树（最兼容，切换不是严格原子）
    Copy,
}

impl LinkStrategy {
    fn parse(raw: &str) -> AppResult<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "auto" => Ok(Self::Auto),
            "link" | "junction" | "symlink" => Ok(Self::Link),
            "copy" => Ok(Self::Copy),
            other => Err(AppError::invalid(format!(
                "linkStrategy 只能是 auto / link / copy，当前为 {other}"
            ))),
        }
    }
}

/// 单个站点的配置
#[derive(Debug, Clone)]
pub struct Site {
    pub name: String,
    /// 站点根目录（绝对路径）。Agent 只允许操作这个目录内的内容。
    pub root: PathBuf,
    pub health_check_url: Option<String>,
    /// 保留的版本数量，超出后清理最旧的（0 表示不清理）
    pub keep_releases: u32,
}

/// 运行期配置
#[derive(Debug, Clone)]
pub struct Config {
    pub bind: SocketAddr,
    pub token: String,
    pub max_upload_bytes: u64,
    pub health_timeout: Duration,
    pub link_strategy: LinkStrategy,
    pub sites: BTreeMap<String, Site>,
    pub config_path: PathBuf,
}

// ---------- 反序列化用的原始结构 ----------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawConfig {
    bind: Option<String>,
    token: Option<String>,
    token_file: Option<PathBuf>,
    max_upload_mb: Option<u64>,
    health_timeout_secs: Option<u64>,
    link_strategy: Option<String>,
    #[serde(default)]
    sites: Vec<RawSite>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSite {
    name: String,
    physical_path: PathBuf,
    health_check_url: Option<String>,
    keep_releases: Option<u32>,
}

impl Config {
    /// 从配置文件加载。`overrides` 用于命令行覆盖 bind / token。
    pub fn load(
        path: &Path,
        bind_override: Option<String>,
        token_override: Option<String>,
    ) -> AppResult<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| AppError::io(format!("读取配置文件失败 {}：{e}", path.display())))?;
        let raw: RawConfig = toml::from_str(&text).map_err(|e| {
            AppError::invalid(format!("配置文件格式不合法 {}：{e}", path.display()))
        })?;

        let bind_raw = bind_override
            .or(raw.bind)
            .unwrap_or_else(|| "0.0.0.0:7801".to_string());
        let bind: SocketAddr = bind_raw
            .parse()
            .map_err(|e| AppError::invalid(format!("bind 地址不合法（{bind_raw}）：{e}")))?;

        // Token：命令行 > 环境变量 > 配置文件 > tokenFile
        let token = match token_override {
            Some(value) if !value.trim().is_empty() => value,
            _ => match std::env::var("BUILDSHIP_AGENT_TOKEN") {
                Ok(value) if !value.trim().is_empty() => value,
                _ => match raw.token {
                    Some(value) if !value.trim().is_empty() => value,
                    _ => match &raw.token_file {
                        Some(file) => read_token_file(file)?,
                        None => {
                            return Err(AppError::new(
                                buildship_common::ErrorCode::CredentialUnavailable,
                                "未配置访问 Token：请在配置文件中设置 token 或 tokenFile，\
                                 或通过环境变量 BUILDSHIP_AGENT_TOKEN 传入",
                            ))
                        }
                    },
                },
            },
        };
        if token.len() < 16 {
            return Err(AppError::invalid(
                "Token 长度至少 16 个字符，请使用随机生成的强凭据",
            ));
        }

        if raw.sites.is_empty() {
            return Err(AppError::invalid("至少需要配置一个站点（sites）"));
        }

        let mut sites = BTreeMap::new();
        for item in raw.sites {
            if item.name.trim().is_empty() {
                return Err(AppError::invalid("站点 name 不能为空"));
            }
            if !is_valid_site_name(&item.name) {
                return Err(AppError::invalid(format!(
                    "站点 name 只能包含小写字母、数字、点、下划线与短横线：{}",
                    item.name
                )));
            }
            let root = normalize_root(&item.physical_path, &item.name)?;
            if sites.contains_key(&item.name) {
                return Err(AppError::conflict(format!("站点重复定义：{}", item.name)));
            }
            sites.insert(
                item.name.clone(),
                Site {
                    name: item.name,
                    root,
                    health_check_url: item.health_check_url.filter(|u| !u.trim().is_empty()),
                    keep_releases: item.keep_releases.unwrap_or(5),
                },
            );
        }

        let max_mb = raw.max_upload_mb.unwrap_or(2048);
        if max_mb == 0 {
            return Err(AppError::invalid("maxUploadMb 必须大于 0"));
        }

        Ok(Self {
            bind,
            token,
            max_upload_bytes: max_mb.saturating_mul(1024 * 1024),
            health_timeout: Duration::from_secs(raw.health_timeout_secs.unwrap_or(10).max(1)),
            link_strategy: LinkStrategy::parse(raw.link_strategy.as_deref().unwrap_or("auto"))?,
            sites,
            config_path: path.to_path_buf(),
        })
    }

    pub fn site(&self, name: &str) -> AppResult<&Site> {
        self.sites
            .get(name)
            .ok_or_else(|| AppError::not_found(format!("站点未在 Agent 配置中登记：{name}")))
    }

    /// 校验客户端传入的物理路径确实属于该站点，防止越权写入任意目录。
    pub fn authorize_path(&self, site: &str, physical_path: &str) -> AppResult<&Site> {
        let target = self.site(site)?;
        let given = PathBuf::from(physical_path.trim());
        if physical_path.trim().is_empty() {
            return Err(AppError::invalid("physicalPath 不能为空"));
        }
        if !given.is_absolute() {
            return Err(AppError::invalid(format!(
                "physicalPath 必须是绝对路径：{}",
                given.display()
            )));
        }
        if !paths_equal(
            &normalize_lexically(&given),
            &normalize_lexically(&target.root),
        ) {
            return Err(AppError::new(
                buildship_common::ErrorCode::PermissionDenied,
                format!(
                    "physicalPath 与 Agent 登记的站点目录不一致：请求 {}，实际 {}",
                    given.display(),
                    target.root.display()
                ),
            ));
        }
        Ok(target)
    }
}

/// Windows 路径大小写不敏感，比较前统一折叠；其它平台保持原样。
pub fn paths_equal(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        left.to_string_lossy().to_lowercase() == right.to_string_lossy().to_lowercase()
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn read_token_file(path: &Path) -> AppResult<String> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        AppError::new(
            buildship_common::ErrorCode::CredentialUnavailable,
            format!("读取 tokenFile 失败 {}：{e}", path.display()),
        )
    })?;
    Ok(text.trim().to_string())
}

fn is_valid_site_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() || first.is_ascii_digit() => {}
        _ => return false,
    }
    name.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
}

/// 站点根目录必须是绝对路径；允许目录尚不存在（首次发布时创建）。
fn normalize_root(input: &Path, site_name: &str) -> AppResult<PathBuf> {
    if !input.is_absolute() {
        return Err(AppError::invalid(format!(
            "站点 {site_name} 的 physicalPath 必须是绝对路径：{}",
            input.display()
        )));
    }
    let normalized = normalize_lexically(input);
    std::fs::create_dir_all(&normalized)
        .map_err(|e| AppError::io(format!("创建站点根目录失败 {}：{e}", normalized.display())))?;
    // 真实路径优先（处理 Windows 8.3 短名与符号链接），
    // 但必须去掉 `\\?\` 前缀，否则与客户端传入的普通路径比较会不相等。
    Ok(std::fs::canonicalize(&normalized)
        .map(|path| strip_verbatim(&path))
        .unwrap_or(normalized))
}

/// 去掉 Windows 扩展长度路径前缀：`\\?\C:\x` → `C:\x`，`\\?\UNC\srv\s` → `\\srv\s`
pub fn strip_verbatim(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        return PathBuf::from(rest.to_string());
    }
    path.to_path_buf()
}

/// 词法层面规范化：去掉 `.`、折叠重复分隔符、消解 `..`，不做 IO。
pub fn normalize_lexically(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// 默认配置文件查找顺序。
pub fn default_config_path() -> PathBuf {
    if let Ok(value) = std::env::var("BUILDSHIP_AGENT_CONFIG") {
        if !value.trim().is_empty() {
            return PathBuf::from(value);
        }
    }
    let local = PathBuf::from("deploy-agent.toml");
    if local.exists() {
        return local;
    }
    #[cfg(windows)]
    {
        let program_data =
            std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".to_string());
        PathBuf::from(program_data)
            .join("Buildship")
            .join("deploy-agent.toml")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/etc/buildship/deploy-agent.toml")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexical_normalization_folds_dots() {
        let normalized = normalize_lexically(Path::new(r"C:\sites\demo\.\a\..\current"));
        assert_eq!(normalized, PathBuf::from(r"C:\sites\demo\current"));
    }

    #[test]
    fn site_name_validation() {
        assert!(is_valid_site_name("demo"));
        assert!(is_valid_site_name("demo-web.prod_1"));
        assert!(!is_valid_site_name("Demo"));
        assert!(!is_valid_site_name("-demo"));
        assert!(!is_valid_site_name("demo/web"));
    }

    #[test]
    fn strategy_parsing() {
        assert_eq!(LinkStrategy::parse("auto").unwrap(), LinkStrategy::Auto);
        assert!(LinkStrategy::parse("weird").is_err());
    }

    #[test]
    fn strips_verbatim_prefix() {
        // canonicalize 会返回扩展长度前缀，必须剥掉才能与客户端路径比较
        assert_eq!(
            strip_verbatim(Path::new(r"\\?\C:\sites\demo")),
            PathBuf::from(r"C:\sites\demo")
        );
        assert_eq!(
            strip_verbatim(Path::new(r"\\?\UNC\server\share\demo")),
            PathBuf::from(r"\\server\share\demo")
        );
        assert_eq!(
            strip_verbatim(Path::new(r"C:\sites\demo")),
            PathBuf::from(r"C:\sites\demo")
        );
    }

    #[test]
    fn path_compare_is_case_insensitive_on_windows() {
        let same = paths_equal(Path::new(r"C:\Sites\Demo"), Path::new(r"c:\sites\demo"));
        #[cfg(windows)]
        assert!(same);
        #[cfg(not(windows))]
        assert!(!same);
    }
}
