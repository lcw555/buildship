//! 哈希计算。

use buildship_common::{AppError, AppResult};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

/// 文件 SHA-256（流式读取，避免大包占内存）
pub fn sha256_file(path: &Path) -> AppResult<String> {
    let mut file = std::fs::File::open(path)
        .map_err(|e| AppError::io(format!("打开文件失败 {}：{e}", path.display())))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 128 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|e| AppError::io(format!("读取文件失败 {}：{e}", path.display())))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// 字节 SHA-256
pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// 目录树一致性哈希：对排序后的 `path\0size\0sha256` 逐行做 SHA-256。
///
/// 用于"manifest 与实际内容是否一致"的快速比对，以及发布包去重。
pub fn tree_hash(entries: &[(String, i64, String)]) -> String {
    let mut sorted: Vec<&(String, i64, String)> = entries.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut hasher = Sha256::new();
    for (path, size, digest) in sorted {
        hasher.update(path.as_bytes());
        hasher.update([0u8]);
        hasher.update(size.to_string().as_bytes());
        hasher.update([0u8]);
        hasher.update(digest.as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_hash_is_order_independent() {
        let a = vec![
            ("b.js".to_string(), 2, "x".to_string()),
            ("a.js".to_string(), 1, "y".to_string()),
        ];
        let b = vec![
            ("a.js".to_string(), 1, "y".to_string()),
            ("b.js".to_string(), 2, "x".to_string()),
        ];
        assert_eq!(tree_hash(&a), tree_hash(&b));
    }

    #[test]
    fn tree_hash_changes_on_content() {
        let a = vec![("a.js".to_string(), 1, "y".to_string())];
        let b = vec![("a.js".to_string(), 1, "z".to_string())];
        assert_ne!(tree_hash(&a), tree_hash(&b));
    }
}
