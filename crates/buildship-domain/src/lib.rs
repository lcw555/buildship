//! Buildship 领域层：实体、枚举与状态机。
//!
//! 该 crate 只描述"业务是什么"，不接触数据库连接、不接触 Tauri、不接触文件系统。
//! 状态机的合法性校验集中在此，任何写入都要先过 `state` 模块。

pub mod dto;
pub mod enums;
pub mod models;
pub mod state;

pub use dto::*;
pub use enums::*;
pub use models::*;
