//! 接收发布包：流式落盘 + 哈希校验 + 安全解压。
//!
//! 安全要点：
//! - 边收边算 SHA-256，不把整个包读进内存；
//! - 超过大小上限立即中断，避免磁盘被写满；
//! - 解压前用 `enclosed_name` 过滤 `..` 与绝对路径，杜绝 zip-slip；
//! - 解压先落到 `tmp/`，成功后再改名到 `releases/<id>`，失败不留下半成品。

use crate::store::{self, Layout, ReleaseMeta};
use axum::body::Body;
use buildship_common::{AppError, AppResult, ErrorCode};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

/// 单文件解压上限（防止 zip 炸弹）
const MAX_ENTRY_BYTES: u64 = 512 * 1024 * 1024;
/// 单版本文件数量上限
const MAX_ENTRIES: usize = 200_000;

#[derive(Debug, Clone)]
pub struct UploadOutcome {
    pub release_id: String,
    pub stored_path: PathBuf,
    pub size_bytes: i64,
    pub sha256: String,
    pub file_count: i64,
    pub tree_sha256: String,
    pub already_existed: bool,
}

/// 接收并落盘发布包
pub async fn receive(
    layout: &Layout,
    release_id: &str,
    expected_sha256: &str,
    body: Body,
    max_bytes: u64,
    request_id: &str,
) -> AppResult<UploadOutcome> {
    store::validate_release_id(release_id)?;
    let expected = expected_sha256.trim().to_ascii_lowercase();
    if expected.is_empty() {
        return Err(AppError::invalid(
            "缺少发布包 SHA-256（x-buildship-sha256）",
        ));
    }
    if expected.len() != 64 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(AppError::invalid(
            "SHA-256 格式不合法（应为 64 位十六进制）",
        ));
    }
    layout.ensure()?;

    let part_path = layout.tmp_dir().join(format!("{release_id}.part"));
    let _ = tokio::fs::remove_file(&part_path).await;

    let mut file = tokio::fs::File::create(&part_path)
        .await
        .map_err(|e| AppError::io(format!("创建临时文件失败：{e}")))?;
    let mut hasher = Sha256::new();
    let mut total: u64 = 0;

    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| AppError::io(format!("读取上传数据失败：{e}")))?;
        total += chunk.len() as u64;
        if total > max_bytes {
            drop(file);
            let _ = tokio::fs::remove_file(&part_path).await;
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                format!(
                    "发布包超过 Agent 上限（{} MB），请调整 maxUploadMb",
                    max_bytes / 1024 / 1024
                ),
            ));
        }
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|e| AppError::io(format!("写入临时文件失败：{e}")))?;
    }
    file.flush()
        .await
        .map_err(|e| AppError::io(format!("刷新临时文件失败：{e}")))?;
    file.sync_all()
        .await
        .map_err(|e| AppError::io(format!("同步临时文件失败：{e}")))?;
    drop(file);

    if total == 0 {
        let _ = tokio::fs::remove_file(&part_path).await;
        return Err(AppError::invalid("发布包为空"));
    }

    let actual = hex::encode(hasher.finalize());
    if actual != expected {
        let _ = tokio::fs::remove_file(&part_path).await;
        return Err(AppError::new(
            ErrorCode::ChecksumMismatch,
            format!("发布包 SHA-256 不一致：声明 {expected}，实际 {actual}"),
        ));
    }

    // 幂等：同一版本、同一内容重复上传直接复用
    if let Some(meta) = ReleaseMeta::load(layout, release_id)? {
        if meta.sha256 == actual && layout.release_dir(release_id).is_dir() {
            let _ = tokio::fs::remove_file(&part_path).await;
            tracing::info!(release_id, %actual, "发布包已存在，按幂等直接复用");
            return Ok(UploadOutcome {
                release_id: release_id.to_string(),
                stored_path: layout.release_dir(release_id),
                size_bytes: meta.size_bytes,
                sha256: meta.sha256,
                file_count: meta.file_count,
                tree_sha256: meta.tree_sha256,
                already_existed: true,
            });
        }
    }
    // 同 id 但内容不同：不允许覆盖已部署版本，要求换 releaseId
    if layout.release_dir(release_id).is_dir() {
        let _ = tokio::fs::remove_file(&part_path).await;
        return Err(AppError::conflict(format!(
            "版本 {release_id} 已存在且内容不同，请使用新的 releaseId"
        )));
    }

    let archive = layout.archive_file(release_id);
    if let Some(parent) = archive.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| AppError::io(format!("创建归档目录失败：{e}")))?;
    }
    store::replace_file(&part_path, &archive)?;

    let extract_root = layout.tmp_dir().join(format!("{release_id}.extract"));
    let release_dir = layout.release_dir(release_id);
    let archive_clone = archive.clone();
    let release_clone = release_dir.clone();
    let extract_clone = extract_root.clone();

    let unpacked = tokio::task::spawn_blocking(move || {
        unpack(&archive_clone, &extract_clone)?;
        let count = store::count_tree(&extract_clone)?;
        let tree = store::tree_hash_of(&extract_clone)?;
        // 覆盖安装：先移除旧目录（正常情况下上面已拦截）
        if release_clone.is_dir() {
            crate::switch::remove_entry(&release_clone)?;
        }
        if let Some(parent) = release_clone.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AppError::io(format!("创建版本目录失败：{e}")))?;
        }
        std::fs::rename(&extract_clone, &release_clone).map_err(|e| {
            AppError::io(format!(
                "移动解压结果失败 {} → {}：{e}",
                extract_clone.display(),
                release_clone.display()
            ))
        })?;
        Ok::<(i64, i64, String), AppError>((count.0, count.1, tree))
    })
    .await
    .map_err(|e| AppError::internal(format!("解压任务异常：{e}")))??;

    let (file_count, unpacked_bytes, tree_sha256) = unpacked;
    let meta = ReleaseMeta {
        release_id: release_id.to_string(),
        sha256: actual.clone(),
        size_bytes: total as i64,
        file_count,
        tree_sha256: tree_sha256.clone(),
        uploaded_at: store::now(),
        applied_at: None,
        source_request_id: Some(request_id.to_string()),
    };
    meta.save(layout)?;

    tracing::info!(
        release_id,
        bytes = total,
        files = file_count,
        unpacked_bytes,
        "发布包已接收并解压"
    );

    Ok(UploadOutcome {
        release_id: release_id.to_string(),
        stored_path: release_dir,
        size_bytes: total as i64,
        sha256: actual,
        file_count,
        tree_sha256,
        already_existed: false,
    })
}

/// 解压 ZIP 到指定目录（调用方保证目录不存在或为空）
fn unpack(archive_path: &Path, destination: &Path) -> AppResult<()> {
    if destination.exists() {
        std::fs::remove_dir_all(destination).map_err(|e| {
            AppError::io(format!(
                "清理解压暂存目录失败 {}：{e}",
                destination.display()
            ))
        })?;
    }
    std::fs::create_dir_all(destination)
        .map_err(|e| AppError::io(format!("创建解压目录失败：{e}")))?;

    let file = std::fs::File::open(archive_path)
        .map_err(|e| AppError::io(format!("打开发布包失败：{e}")))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| AppError::new(ErrorCode::ChecksumMismatch, format!("ZIP 无法解析：{e}")))?;

    if zip.len() > MAX_ENTRIES {
        return Err(AppError::invalid(format!(
            "发布包条目数 {} 超过上限 {MAX_ENTRIES}",
            zip.len()
        )));
    }

    let mut extracted_bytes: u64 = 0;
    for index in 0..zip.len() {
        let mut entry = zip
            .by_index(index)
            .map_err(|e| AppError::io(format!("读取 ZIP 条目失败：{e}")))?;

        // enclosed_name 会拒绝绝对路径与 `..` 穿越
        let relative = match entry.enclosed_name() {
            Some(path) => path.to_path_buf(),
            None => {
                return Err(AppError::new(
                    ErrorCode::ChecksumMismatch,
                    format!("发布包含非法路径条目：{}", entry.name()),
                ))
            }
        };
        if relative.as_os_str().is_empty() {
            continue;
        }
        // manifest.json 只是元数据，不落在站点目录里
        if relative == Path::new("manifest.json") {
            continue;
        }

        let target = destination.join(&relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&target)
                .map_err(|e| AppError::io(format!("创建目录失败 {}：{e}", target.display())))?;
            continue;
        }

        if entry.size() > MAX_ENTRY_BYTES {
            return Err(AppError::invalid(format!(
                "条目过大：{}（{} 字节）",
                relative.display(),
                entry.size()
            )));
        }
        extracted_bytes += entry.size();
        if extracted_bytes > MAX_ENTRY_BYTES * 16 {
            return Err(AppError::invalid("解压后体积超过安全上限，疑似压缩炸弹"));
        }

        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AppError::io(format!("创建目录失败 {}：{e}", parent.display())))?;
        }
        let mut output = std::fs::File::create(&target)
            .map_err(|e| AppError::io(format!("写入文件失败 {}：{e}", target.display())))?;
        std::io::copy(&mut entry, &mut output)
            .map_err(|e| AppError::io(format!("写入文件失败 {}：{e}", target.display())))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn build_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        for (name, body) in entries {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(body).unwrap();
        }
        writer.finish().unwrap();
    }

    #[test]
    fn unpacks_and_skips_manifest() {
        let temp = tempdir().unwrap();
        let zip_path = temp.path().join("a.zip");
        build_zip(
            &zip_path,
            &[
                ("manifest.json", b"{\"schemaVersion\":1}" as &[u8]),
                ("index.html", b"<html></html>"),
                ("assets/app.js", b"console.log(1)"),
            ],
        );
        let out = temp.path().join("out");
        unpack(&zip_path, &out).unwrap();
        assert!(out.join("index.html").is_file());
        assert!(out.join("assets/app.js").is_file());
        assert!(!out.join("manifest.json").exists());
    }

    #[test]
    fn rejects_zip_slip() {
        let temp = tempdir().unwrap();
        let zip_path = temp.path().join("evil.zip");
        build_zip(&zip_path, &[("../escape.txt", b"boom")]);
        let out = temp.path().join("out");
        assert!(unpack(&zip_path, &out).is_err());
    }
}
