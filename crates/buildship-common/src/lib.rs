//! Buildship 公共基础库：错误模型、时间、路径、日志与敏感信息脱敏。
//!
//! 该 crate 不依赖 Tauri、不依赖数据库，可被桌面端与 deploy-agent 共同使用。

pub mod error;
pub mod fsutil;
pub mod logging;
pub mod paths;
pub mod redact;
pub mod text;
pub mod time;

pub use error::{AppError, AppResult, ErrorCode};
pub use paths::AppPaths;
