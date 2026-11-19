//! 项目扫描。
//!
//! 方案 4.2：扫描阶段只读取配置文件，**绝不执行项目脚本**。
//! 需要执行的动作（安装依赖、构建）都必须由用户明确触发。

use buildship_common::{AppError, AppResult};
use buildship_domain::{PackageManager, ScanResult};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// package.json 中与本工具相关的子集
#[derive(Debug, Clone, Default)]
pub struct PackageJson {
    pub name: Option<String>,
    pub version: Option<String>,
    pub scripts: BTreeMap<String, String>,
    pub engines_node: Option<String>,
    pub dependencies: BTreeMap<String, String>,
}

fn read_json(path: &Path) -> AppResult<serde_json::Value> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| AppError::io(format!("读取 {} 失败：{e}", path.display())))?;
    // 有些项目 package.json 带 BOM
    let text = text.trim_start_matches('\u{feff}');
    serde_json::from_str(text)
        .map_err(|e| AppError::invalid(format!("{} 不是合法 JSON：{e}", path.display())))
}

fn string_map(value: Option<&serde_json::Value>) -> BTreeMap<String, String> {
    value
        .and_then(|v| v.as_object())
        .map(|map| {
            map.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

/// 读取 package.json（不执行任何脚本）
pub fn read_package_json(project_dir: &Path) -> AppResult<PackageJson> {
    let path = project_dir.join("package.json");
    if !path.is_file() {
        return Err(AppError::invalid(format!(
            "{} 不是前端项目：缺少 package.json",
            project_dir.display()
        )));
    }
    let json = read_json(&path)?;
    Ok(PackageJson {
        name: json
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        version: json
            .get("version")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        scripts: string_map(json.get("scripts")),
        engines_node: json
            .get("engines")
            .and_then(|v| v.get("node"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        dependencies: {
            let mut deps = string_map(json.get("dependencies"));
            deps.extend(string_map(json.get("devDependencies")));
            deps
        },
    })
}

/// 依据 lockfile 推断包管理器。
pub fn detect_package_manager(project_dir: &Path) -> PackageManager {
    if project_dir.join("pnpm-lock.yaml").is_file() {
        PackageManager::Pnpm
    } else if project_dir.join("yarn.lock").is_file() {
        PackageManager::Yarn
    } else if project_dir.join("package-lock.json").is_file()
        || project_dir.join("npm-shrinkwrap.json").is_file()
    {
        PackageManager::Npm
    } else if project_dir.join("pnpm-workspace.yaml").is_file() {
        PackageManager::Pnpm
    } else {
        PackageManager::Npm
    }
}

/// 推断框架
pub fn detect_framework(pkg: &PackageJson) -> Option<String> {
    let has = |name: &str| pkg.dependencies.contains_key(name);
    if has("next") {
        Some("Next.js".into())
    } else if has("nuxt") || has("nuxt3") {
        Some("Nuxt".into())
    } else if has("@angular/core") {
        Some("Angular".into())
    } else if has("svelte") || has("@sveltejs/kit") {
        Some("Svelte".into())
    } else if has("solid-js") {
        Some("Solid".into())
    } else if has("vue") {
        Some("Vue".into())
    } else if has("react") {
        Some("React".into())
    } else if has("vite") {
        Some("Vite".into())
    } else {
        None
    }
}

/// 常见产物目录（按优先级）
const COMMON_OUTPUT_DIRS: [&str; 6] = [
    "dist",
    "build",
    "out",
    "dist/spa",
    "output",
    ".output/public",
];

/// 推荐构建脚本：优先 `build`，其次常见变体
fn suggested_script(scripts: &BTreeMap<String, String>) -> Option<String> {
    for candidate in ["build", "build:prod", "build:production", "prod", "compile"] {
        if scripts.contains_key(candidate) {
            return Some(candidate.to_string());
        }
    }
    scripts
        .keys()
        .find(|name| name.starts_with("build"))
        .cloned()
}

/// 读取 .nvmrc 或 engines.node
fn node_requirement(project_dir: &Path, pkg: &PackageJson) -> Option<String> {
    let nvmrc = project_dir.join(".nvmrc");
    if let Ok(text) = std::fs::read_to_string(&nvmrc) {
        let trimmed = text.trim().trim_start_matches('v').to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    pkg.engines_node.clone()
}

/// 读取 Git commit（仅在存在 .git 时）
pub fn git_commit(project_dir: &Path) -> Option<String> {
    let git_dir = project_dir.join(".git");
    if !git_dir.exists() {
        return None;
    }
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    if let Some(reference) = head.strip_prefix("ref: ") {
        if let Ok(sha) = std::fs::read_to_string(git_dir.join(reference)) {
            return Some(sha.trim().to_string());
        }
        // 打包过的仓库：从 packed-refs 里找
        let packed = std::fs::read_to_string(git_dir.join("packed-refs")).ok()?;
        for line in packed.lines() {
            if line.ends_with(reference) {
                return line.split_whitespace().next().map(str::to_string);
            }
        }
        None
    } else {
        Some(head.to_string())
    }
}

/// 生成项目标识（slug）：只保留 ASCII 字母数字，其余折叠为 `-`
pub fn slugify(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_dash = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "app".to_string()
    } else {
        trimmed.chars().take(48).collect()
    }
}

/// 扫描项目目录，返回可直接填充表单的结果。
pub async fn scan(path: &Path) -> AppResult<ScanResult> {
    let project_dir: PathBuf = path
        .canonicalize()
        .map_err(|_| AppError::not_found(format!("目录不存在或不可访问：{}", path.display())))?;
    if !project_dir.is_dir() {
        return Err(AppError::invalid(format!(
            "{} 不是目录",
            project_dir.display()
        )));
    }

    let pkg = read_package_json(&project_dir)?;
    let mut warnings = Vec::new();

    if pkg.scripts.is_empty() {
        warnings.push("package.json 未定义任何 scripts，构建前需要补一个构建脚本".to_string());
    }

    let package_manager = detect_package_manager(&project_dir);
    let candidate_output_dirs: Vec<String> = COMMON_OUTPUT_DIRS
        .iter()
        .filter(|dir| project_dir.join(dir).is_dir())
        .map(|dir| dir.to_string())
        .collect();

    let suggested_output_dir = candidate_output_dirs
        .first()
        .cloned()
        .unwrap_or_else(|| "dist".to_string());
    if candidate_output_dirs.is_empty() {
        warnings.push("未发现常见产物目录，首次构建后请确认输出目录".to_string());
    }

    let name = pkg.name.clone().unwrap_or_else(|| {
        project_dir
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "project".into())
    });

    let node_version = crate::toolchain::node_version().await;
    let node_requirement = node_requirement(&project_dir, &pkg);
    if let (Some(actual), Some(required)) = (node_version.as_deref(), node_requirement.as_deref()) {
        warnings.push(format!("项目要求 Node {required}，本机检测到 {actual}"));
    }

    Ok(ScanResult {
        path: project_dir.to_string_lossy().to_string(),
        slug: slugify(&name),
        name,
        framework: detect_framework(&pkg),
        package_manager,
        scripts: pkg.scripts.keys().cloned().collect(),
        suggested_script: suggested_script(&pkg.scripts),
        suggested_output_dir,
        candidate_output_dirs,
        node_version,
        git_commit: git_commit(&project_dir),
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn slugify_handles_chinese_and_symbols() {
        assert_eq!(slugify("My App"), "my-app");
        assert_eq!(slugify("前端发布平台"), "app");
        assert_eq!(slugify("vue-admin@2"), "vue-admin-2");
    }

    #[test]
    fn detects_pnpm_by_lockfile() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();
        assert_eq!(detect_package_manager(dir.path()), PackageManager::Pnpm);
    }

    #[test]
    fn detects_framework_from_deps() {
        let mut pkg = PackageJson::default();
        pkg.dependencies.insert("vue".into(), "^3.5.0".into());
        assert_eq!(detect_framework(&pkg).as_deref(), Some("Vue"));
    }
}
