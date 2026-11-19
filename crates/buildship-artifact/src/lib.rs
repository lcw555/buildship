//! 发布包域：文件哈希、manifest 生成与校验、ZIP 归档。

pub mod archive;
pub mod hashing;
pub mod manifest;

pub use archive::{package, verify_package, PackageOptions, PackageResult, VerifyReport};
pub use hashing::{sha256_bytes, sha256_file, tree_hash};
pub use manifest::{
    Manifest, ManifestArtifact, ManifestFile, ManifestSummary, MANIFEST_SCHEMA_VERSION,
};
