//! 发布域：与 Windows Server Deploy Agent 的通信协议与客户端。
//!
//! 协议设计（方案 8.2）：
//! - 版本号：`x-buildship-api-version`
//! - 请求 ID：`x-buildship-request-id`（用于幂等与日志串联）
//! - 认证：`x-buildship-token`（Bearer 风格共享密钥，Agent 侧最小权限）
//! - 结构化错误码：与桌面端 `ErrorCode` 对齐

pub mod client;
pub mod protocol;

pub use client::DeployClient;
pub use protocol::*;
