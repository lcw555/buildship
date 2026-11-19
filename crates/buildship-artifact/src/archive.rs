//! ZIP 归档与发布包校验。
//!
//! 打包规则（方案 4.4）：
//! - 只打包配置的产物目录；
//! - 默认排除 `.env*`、密钥文件、`node_modules`、各类缓存与 OS 元数据；
//! - 支持中文文件名（ZIP 内使用 UTF-8 名字 + 统一 `/` 分隔）；
//! - 打包后不可原地修改，需要变更就重新生成新包。

use crate::hashing::{sha256_file, tree_hash};
use crate::manifest::{
    Manifest, ManifestArtifact, ManifestFile, ManifestSource, ManifestSummary,
    MANIFEST_SCHEMA_VERSION,
};
use buildship_common::{time, AppError, AppResult, ErrorCode};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

/// 打包输入
#[derive(Debug, Clone)]
pub struct PackageOptions {
    pub artifact_id: String,
    pub project_id: String,
    pub project_name: String,
    pub project_slug: String,
    pub build_task_id: String,
    pub build_command: String,
    /// 相对项目根目录的产物目录（写进 manifest，便于回溯）
    pub output_dir: String,
    /// 实际要打包的绝对目录
    pub source_dir: PathBuf,
    /// ZIP 落盘目录
    pub dest_dir: PathBuf,
    /// 自定义包名（不含扩展名）；为空按 `<slug>-<stamp>` 生成
    pub file_name: Option<String>,
    pub git_commit: Option<String>,
    pub packaged_by: String,
}

/// 打包结果
#[derive(Debug, Clone)]
pub struct PackageResult {
    pub artifact_id: String,
    pub file_name: String,
    pub file_path: PathBuf,
    pub size_bytes: i64,
    pub sha256: String,
    pub file_count: i64,
    pub total_bytes: i64,
    pub manifest_path: PathBuf,
    pub manifest_sha256: String,
    pub tree_sha256: String,
    pub excluded_rules: Vec<String>,
}

/// 校验结果
#[derive(Debug, Clone)]
pub struct VerifyReport {
    pub ok: bool,
    pub message: String,
    pub file_count: i64,
    pub actual_sha256: Option<String>,
    pub mismatched: Vec<String>,
}

const EXCLUDED_DIRS: [&str; 8] = [
    "node_modules",
    ".git",
    ".svn",
    ".hg",
    ".cache",
    ".vite",
    ".turbo",
    ".parcel-cache",
];

const EXCLUDED_FILES: [&str; 5] = [
    ".DS_Store",
    "Thumbs.db",
    "desktop.ini",
    ".gitignore",
    ".npmrc",
];

const EXCLUDED_EXTS: [&str; 7] = ["pem", "key", "p12", "pfx", "keystore", "jks", "crt"];

/// 判断相对路径是否应被排除，返回命中的规则说明。
fn exclusion_rule(relative: &str) -> Option<String> {
    let normalized = relative.replace('\\', "/");
    for part in normalized.split('/') {
        if EXCLUDED_DIRS.contains(&part) {
            return Some(format!("目录 {part}"));
        }
    }
    let name = normalized.rsplit('/').next().unwrap_or(&normalized);
    if name.starts_with(".env") {
        return Some(".env 文件".to_string());
    }
    if EXCLUDED_FILES.contains(&name) {
        return Some(format!("系统/工具文件 {name}"));
    }
    if let Some(ext) = name.rsplit('.').next() {
        if EXCLUDED_EXTS.contains(&ext.to_ascii_lowercase().as_str()) {
            return Some(format!("密钥类文件 *.{ext}"));
        }
    }
    None
}

/// 收集结果：`(相对路径, 绝对路径)` 列表 + 各排除规则命中次数
type CollectedFiles = (Vec<(String, PathBuf)>, BTreeMap<String, usize>);

/// 递归收集产物文件（相对路径 + 绝对路径）。
fn collect_files(source_dir: &Path) -> AppResult<CollectedFiles> {
    if !source_dir.is_dir() {
        return Err(AppError::not_found(format!(
            "构建产物目录不存在：{}（请检查构建配置的 outputDir）",
            source_dir.display()
        )));
    }
    let mut files = Vec::new();
    let mut excluded: BTreeMap<String, usize> = BTreeMap::new();

    for entry in WalkDir::new(source_dir).follow_links(false) {
        let entry = entry.map_err(|e| AppError::io(format!("遍历产物目录失败：{e}")))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let relative = path
            .strip_prefix(source_dir)
            .map_err(|e| AppError::internal(format!("计算相对路径失败：{e}")))?
            .to_string_lossy()
            .replace('\\', "/");
        if let Some(rule) = exclusion_rule(&relative) {
            *excluded.entry(rule).or_insert(0) += 1;
            continue;
        }
        files.push((relative, path.to_path_buf()));
    }

    if files.is_empty() {
        return Err(AppError::invalid("产物目录为空，无法生成发布包"));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok((files, excluded))
}

/// 打包：生成 ZIP + manifest + SHA-256。
pub fn package(options: &PackageOptions) -> AppResult<PackageResult> {
    let (files, excluded) = collect_files(&options.source_dir)?;

    // 第一遍：计算每个文件的摘要
    let mut manifest_files = Vec::with_capacity(files.len());
    let mut entries = Vec::with_capacity(files.len());
    let mut total_bytes = 0i64;
    for (relative, absolute) in &files {
        let size = std::fs::metadata(absolute)
            .map_err(|e| AppError::io(format!("读取文件信息失败 {}：{e}", absolute.display())))?
            .len() as i64;
        let digest = sha256_file(absolute)?;
        total_bytes += size;
        entries.push((relative.clone(), size, digest.clone()));
        manifest_files.push(ManifestFile {
            path: relative.clone(),
            size,
            sha256: digest,
        });
    }

    let tree = tree_hash(&entries);
    let manifest = Manifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        artifact: ManifestArtifact {
            id: options.artifact_id.clone(),
            project_id: options.project_id.clone(),
            project_name: options.project_name.clone(),
            project_slug: options.project_slug.clone(),
            build_task_id: options.build_task_id.clone(),
            git_commit: options.git_commit.clone(),
            created_at: time::now(),
        },
        source: ManifestSource {
            build_command: options.build_command.clone(),
            output_dir: options.output_dir.clone(),
            packaged_at: time::now(),
            packaged_by: options.packaged_by.clone(),
        },
        summary: ManifestSummary {
            file_count: manifest_files.len() as i64,
            total_bytes,
            tree_sha256: tree.clone(),
            excluded_rules: excluded.keys().cloned().collect(),
        },
        files: manifest_files,
    };
    let manifest_json = manifest.to_json();
    let manifest_bytes = manifest_json.as_bytes().to_vec();
    let manifest_sha256 = crate::hashing::sha256_bytes(&manifest_bytes);

    std::fs::create_dir_all(&options.dest_dir)
        .map_err(|e| AppError::io(format!("创建发布包目录失败：{e}")))?;

    let stem = options.file_name.clone().unwrap_or_else(|| {
        format!(
            "{}-{}",
            options.project_slug,
            time::compact_stamp(time::now())
        )
    });
    let file_name = format!("{stem}.zip");
    let zip_path = options.dest_dir.join(&file_name);
    if zip_path.exists() {
        return Err(AppError::conflict(format!(
            "发布包已存在：{}",
            zip_path.display()
        )));
    }

    // 第二遍：写 ZIP
    {
        let file = std::fs::File::create(&zip_path)
            .map_err(|e| AppError::io(format!("创建 ZIP 失败：{e}")))?;
        let mut writer = ZipWriter::new(file);

        let manifest_options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(0o644)
            .large_file(false);
        writer
            .start_file("manifest.json", manifest_options)
            .map_err(|e| AppError::io(format!("写入 manifest 失败：{e}")))?;
        writer
            .write_all(&manifest_bytes)
            .map_err(|e| AppError::io(format!("写入 manifest 失败：{e}")))?;

        for (relative, absolute) in &files {
            let size = std::fs::metadata(absolute).map(|m| m.len()).unwrap_or(0);
            let options = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .unix_permissions(0o644)
                .large_file(size > 4 * 1024 * 1024 * 1024);
            writer
                .start_file(relative.as_str(), options)
                .map_err(|e| AppError::io(format!("写入 {relative} 失败：{e}")))?;
            let mut source = std::fs::File::open(absolute)
                .map_err(|e| AppError::io(format!("打开 {relative} 失败：{e}")))?;
            std::io::copy(&mut source, &mut writer)
                .map_err(|e| AppError::io(format!("写入 {relative} 失败：{e}")))?;
        }
        writer
            .finish()
            .map_err(|e| AppError::io(format!("完成 ZIP 失败：{e}")))?;
    }

    // 包外再存一份 manifest，便于查看与审计
    let manifest_path = options.dest_dir.join(format!("{stem}.manifest.json"));
    std::fs::write(&manifest_path, &manifest_bytes)
        .map_err(|e| AppError::io(format!("写入 manifest 文件失败：{e}")))?;

    let size_bytes = std::fs::metadata(&zip_path)
        .map_err(|e| AppError::io(format!("读取 ZIP 大小失败：{e}")))?
        .len() as i64;
    let sha256 = sha256_file(&zip_path)?;

    tracing::info!(
        artifact = %options.artifact_id,
        files = manifest.summary.file_count,
        bytes = size_bytes,
        "发布包生成完成"
    );

    Ok(PackageResult {
        artifact_id: options.artifact_id.clone(),
        file_name,
        file_path: zip_path,
        size_bytes,
        sha256,
        file_count: manifest.summary.file_count,
        total_bytes,
        manifest_path,
        manifest_sha256,
        tree_sha256: tree,
        excluded_rules: excluded.keys().cloned().collect(),
    })
}

/// 校验发布包：哈希 + manifest 与内容一致性。
pub fn verify_package(zip_path: &Path, expected_sha256: &str) -> AppResult<VerifyReport> {
    if !zip_path.is_file() {
        return Ok(VerifyReport {
            ok: false,
            message: format!("发布包文件不存在：{}", zip_path.display()),
            file_count: 0,
            actual_sha256: None,
            mismatched: vec![],
        });
    }

    let actual = sha256_file(zip_path)?;
    let mut mismatched = Vec::new();
    if !actual.eq_ignore_ascii_case(expected_sha256) {
        mismatched.push(format!(
            "ZIP 哈希不一致：期望 {expected_sha256}，实际 {actual}"
        ));
    }

    let file =
        std::fs::File::open(zip_path).map_err(|e| AppError::io(format!("打开 ZIP 失败：{e}")))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|e| AppError::new(ErrorCode::ChecksumMismatch, format!("ZIP 无法解析：{e}")))?;

    let manifest_text = {
        let mut entry = archive
            .by_name("manifest.json")
            .map_err(|_| AppError::new(ErrorCode::ChecksumMismatch, "ZIP 缺少 manifest.json"))?;
        let mut text = String::new();
        entry
            .read_to_string(&mut text)
            .map_err(|e| AppError::io(format!("读取 manifest 失败：{e}")))?;
        text
    };
    let manifest = Manifest::parse(&manifest_text).map_err(|e| {
        AppError::new(
            ErrorCode::ChecksumMismatch,
            format!("manifest 解析失败：{e}"),
        )
    })?;

    let expected: BTreeMap<&str, &ManifestFile> = manifest
        .files
        .iter()
        .map(|f| (f.path.as_str(), f))
        .collect();

    let mut verified = 0i64;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| AppError::io(format!("读取 ZIP 条目失败：{e}")))?;
        let name = entry.name().to_string();
        if name == "manifest.json" || entry.is_dir() {
            continue;
        }
        match expected.get(name.as_str()) {
            None => mismatched.push(format!("多余文件：{name}")),
            Some(item) => {
                let mut hasher = Sha256::new();
                let mut buffer = vec![0u8; 64 * 1024];
                let mut size = 0i64;
                loop {
                    let read = entry
                        .read(&mut buffer)
                        .map_err(|e| AppError::io(format!("读取 {name} 失败：{e}")))?;
                    if read == 0 {
                        break;
                    }
                    size += read as i64;
                    hasher.update(&buffer[..read]);
                }
                let digest = hex::encode(hasher.finalize());
                if digest != item.sha256 || size != item.size {
                    mismatched.push(format!("内容不一致：{name}"));
                } else {
                    verified += 1;
                }
            }
        }
    }

    if verified != manifest.summary.file_count {
        mismatched.push(format!(
            "文件数量不一致：manifest 记录 {}，ZIP 校验通过 {verified}",
            manifest.summary.file_count
        ));
    }

    let ok = mismatched.is_empty();
    Ok(VerifyReport {
        ok,
        message: if ok {
            format!("校验通过，共 {verified} 个文件，SHA-256 一致")
        } else {
            format!("校验失败：{}", mismatched.join("；"))
        },
        file_count: verified,
        actual_sha256: Some(actual),
        mismatched,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn sample_project() -> (tempfile::TempDir, PackageOptions) {
        let dir = tempdir().unwrap();
        let dist = dir.path().join("dist");
        fs::create_dir_all(dist.join("assets")).unwrap();
        fs::write(dist.join("index.html"), b"<html>hello</html>").unwrap();
        fs::write(dist.join("assets/app.9f2c.js"), b"console.log(1)").unwrap();
        fs::write(dist.join(".env.production"), b"SECRET=1").unwrap();
        fs::create_dir_all(dist.join("node_modules")).unwrap();
        fs::write(dist.join("node_modules/x.js"), b"nope").unwrap();
        fs::write(dist.join("站点.txt"), "中文文件名".as_bytes()).unwrap();

        let options = PackageOptions {
            artifact_id: "art-1".into(),
            project_id: "prj-1".into(),
            project_name: "测试项目".into(),
            project_slug: "demo".into(),
            build_task_id: "task-1".into(),
            build_command: "npm run build".into(),
            output_dir: "dist".into(),
            source_dir: dist,
            dest_dir: dir.path().join("out"),
            file_name: Some("demo".into()),
            git_commit: None,
            packaged_by: "tester".into(),
        };
        (dir, options)
    }

    #[test]
    fn packages_and_verifies() {
        let (_guard, options) = sample_project();
        let result = package(&options).unwrap();
        assert_eq!(result.file_count, 3, "node_modules 与 .env 应被排除");
        assert_eq!(result.sha256.len(), 64);

        let report = verify_package(&result.file_path, &result.sha256).unwrap();
        assert!(report.ok, "{}", report.message);
        assert_eq!(report.file_count, 3);
    }

    #[test]
    fn detects_tampered_hash() {
        let (_guard, options) = sample_project();
        let result = package(&options).unwrap();
        let report = verify_package(&result.file_path, &"0".repeat(64)).unwrap();
        assert!(!report.ok);
    }

    #[test]
    fn refuses_empty_output_dir() {
        let dir = tempdir().unwrap();
        let empty = dir.path().join("dist");
        fs::create_dir_all(&empty).unwrap();
        let options = PackageOptions {
            artifact_id: "a".into(),
            project_id: "p".into(),
            project_name: "n".into(),
            project_slug: "s".into(),
            build_task_id: "t".into(),
            build_command: "npm run build".into(),
            output_dir: "dist".into(),
            source_dir: empty,
            dest_dir: dir.path().join("out"),
            file_name: None,
            git_commit: None,
            packaged_by: "t".into(),
        };
        assert!(package(&options).is_err());
    }
}
