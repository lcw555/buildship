//! 桌面端构建脚本。
//!
//! 除了 Tauri 自身的资源生成，这里额外处理一个 Windows GNU 工具链的平台细节：
//! `webview2-com-sys` 在 GNU 目标下链接的是 `WebView2Loader.dll`（MSVC 目标才会
//! 静态链接 `.lib`），因此可执行文件旁边必须有这个 DLL，否则启动即
//! `STATUS_DLL_NOT_FOUND (0xC0000135)`。
//!
//! 做法：把仓库内 `vendor/WebView2Loader.dll` 复制到产物目录：
//! - `OUT_DIR`：`cargo test` 的测试可执行文件所在目录
//! - `target/<profile>/`：`cargo build` / `tauri dev` 的主程序目录
//! - `target/<profile>/deps/`：集成测试与依赖产物目录
//!
//! 安装包侧由 `tauri.conf.json` 的 `bundle.resources` 负责放到安装根目录。

use std::path::{Path, PathBuf};

fn main() {
    tauri_build::build();

    #[cfg(target_os = "windows")]
    stage_webview2_loader();
}

#[cfg(target_os = "windows")]
fn stage_webview2_loader() {
    const DLL: &str = "WebView2Loader.dll";
    let manifest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let source = manifest_dir.join("vendor").join(DLL);
    // DLL 缺失不阻断编译（例如 CI 只跑纯逻辑测试），但会在运行时暴露为
    // 0xC0000135，因此这里明确打印提示。
    println!("cargo:rerun-if-changed=vendor/{DLL}");
    if !source.is_file() {
        println!("cargo:warning=未找到 vendor/{DLL}，Windows GNU 目标运行时可能报 0xC0000135");
        return;
    }

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let mut targets: Vec<PathBuf> = vec![out_dir.clone()];

    // OUT_DIR = target/<profile>/build/<pkg>-<hash>/out，
    // 向上数四级才是 profile 目录（out → <hash> → <pkg> → build → <profile>）
    if let Some(profile_dir) = out_dir.ancestors().nth(4) {
        targets.push(profile_dir.to_path_buf());
        targets.push(profile_dir.join("deps"));
    }

    for dir in targets {
        if !dir.is_dir() {
            continue;
        }
        let destination = dir.join(DLL);
        if same_file(&source, &destination) {
            continue;
        }
        if let Err(err) = std::fs::copy(&source, &destination) {
            println!("cargo:warning=复制 {DLL} 到 {} 失败：{err}", dir.display());
        }
    }
}

/// 避免无意义的重复拷贝（大小与修改时间都一致时跳过）
#[cfg(target_os = "windows")]
fn same_file(source: &Path, destination: &Path) -> bool {
    let (Ok(a), Ok(b)) = (std::fs::metadata(source), std::fs::metadata(destination)) else {
        return false;
    };
    a.len() == b.len() && a.modified().ok() == b.modified().ok()
}
