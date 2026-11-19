//! Buildship 数据访问层。
//!
//! - `Db`：连接池 + 迁移 + 备份恢复
//! - `repositories`：按聚合拆分的 Repository，函数签名接受任意 `Executor`，
//!   这样服务层可以在同一个事务里同时写业务表和审计表（方案 3.1 要求）。
//!
//! 约束：前端不直接访问数据库；所有 SQL 只出现在本 crate 内。

pub mod backup;
pub mod db;
pub mod repositories;

pub use db::Db;
