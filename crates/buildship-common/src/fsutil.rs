//! 文件系统小工具：目录体积统计等。
//!
//! 放在公共库而不是桌面端，是为了让清理/备份这类逻辑能在无 Tauri 依赖的
//! 环境下直接单测（桌面端 crate 链接了 WebView2Loader，测试二进制启动成本高）。

use std::path::Path;

/// 递归统计目录占用字节数；遇到无权限或不存在的路径返回已统计部分。
pub fn dir_size(path: &Path) -> i64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    let mut total = 0i64;
    for entry in entries.flatten() {
        let child = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => total += dir_size(&child),
            Ok(_) => total += entry.metadata().map(|meta| meta.len() as i64).unwrap_or(0),
            Err(_) => {}
        }
    }
    total
}

/// 人类可读的体积文本（1024 进制，保留一位小数）。
pub fn human_size(bytes: i64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes.max(0) as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_nested_files() {
        let dir = tempfile::tempdir().expect("临时目录");
        let sub = dir.path().join("a/b");
        std::fs::create_dir_all(&sub).expect("建目录");
        std::fs::write(dir.path().join("one.txt"), b"12345").expect("写文件");
        std::fs::write(sub.join("two.txt"), b"123").expect("写文件");
        assert_eq!(dir_size(dir.path()), 8);
    }

    #[test]
    fn missing_dir_is_zero() {
        assert_eq!(dir_size(Path::new("C:/definitely/not/here")), 0);
    }

    #[test]
    fn formats_sizes() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2.0 KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MB");
    }
}
