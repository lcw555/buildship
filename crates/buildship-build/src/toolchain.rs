//! Node.js 与包管理器探测。
//!
//! 方案 2.3 / 14：启动时检测版本、路径与可用性，缺失时给出明确安装指引，不静默失败。

use crate::process::{resolve_binary, run_capture, CommandSpec};
use buildship_domain::{PackageManager, ToolStatus, ToolchainReport};
use std::time::Duration;

const PROBE_TIMEOUT: Duration = Duration::from_secs(8);

fn hint_for(name: &str) -> String {
    match name {
        "node" => {
            "未检测到 Node.js。请安装 Node.js 18 LTS 或更高版本：https://nodejs.org/zh-cn/download"
                .to_string()
        }
        "npm" => {
            "npm 通常随 Node.js 一起安装；若缺失，请重新安装 Node.js 或执行 `npm install -g npm`"
                .to_string()
        }
        "pnpm" => {
            "未检测到 pnpm。安装命令：`npm install -g pnpm` 或 `corepack enable pnpm`".to_string()
        }
        "yarn" => {
            "未检测到 yarn。安装命令：`npm install -g yarn` 或 `corepack enable yarn`".to_string()
        }
        _ => format!("未检测到 {name}"),
    }
}

/// 探测单个工具：解析路径 + 执行 `-v`。
pub async fn probe(name: &str, cwd: &std::path::Path) -> ToolStatus {
    let Some(path) = resolve_binary(name) else {
        return ToolStatus {
            name: name.to_string(),
            available: false,
            version: None,
            path: None,
            hint: Some(hint_for(name)),
        };
    };

    let spec = CommandSpec::new(
        path.to_string_lossy().to_string(),
        vec!["-v".to_string()],
        cwd,
    );
    match run_capture(&spec, PROBE_TIMEOUT).await {
        Ok((0, output)) => {
            let version = normalize_version(&output);
            ToolStatus {
                name: name.to_string(),
                available: true,
                version,
                path: Some(path.to_string_lossy().to_string()),
                hint: None,
            }
        }
        Ok((code, output)) => ToolStatus {
            name: name.to_string(),
            available: false,
            version: normalize_version(&output),
            path: Some(path.to_string_lossy().to_string()),
            hint: Some(format!("执行 `{name} -v` 退出码 {code}，请检查安装完整性")),
        },
        Err(err) => ToolStatus {
            name: name.to_string(),
            available: false,
            version: None,
            path: Some(path.to_string_lossy().to_string()),
            hint: Some(err.message),
        },
    }
}

/// 从 `-v` 输出里提取版本号（去掉 v 前缀、取首行）。
fn normalize_version(output: &str) -> Option<String> {
    output
        .lines()
        .map(|line| line.trim())
        .find(|line| !line.is_empty())
        .map(|line| line.trim_start_matches('v').to_string())
}

/// 完整工具链体检。
pub async fn detect() -> ToolchainReport {
    let cwd = std::env::temp_dir();
    let node = probe("node", &cwd).await;
    let mut package_managers = Vec::new();
    for pm in [
        PackageManager::Npm,
        PackageManager::Pnpm,
        PackageManager::Yarn,
    ] {
        package_managers.push(probe(pm.binary(), &cwd).await);
    }
    ToolchainReport {
        node,
        package_managers,
        checked_at: buildship_common::time::now(),
    }
}

/// 只取 Node 版本号（项目扫描时展示用）。
pub async fn node_version() -> Option<String> {
    let status = probe("node", &std::env::temp_dir()).await;
    status.version
}

/// 取指定包管理器的版本号。
pub async fn package_manager_version(pm: PackageManager) -> Option<String> {
    let status = probe(pm.binary(), &std::env::temp_dir()).await;
    status.version
}

/// 判断某包管理器是否可用
pub fn is_available(name: &str) -> bool {
    resolve_binary(name).is_some()
}
