//! Repository 层。
//!
//! 每个函数都接受任意 `Executor`，服务层可以：
//! - 直接传 `&pool`（单条语句）
//! - 传 `&mut *tx`（同一事务内写业务表 + 审计表）

pub mod artifacts;
pub mod audit;
pub mod builds;
pub mod deployments;
pub mod projects;
pub mod releases;
pub mod settings;

use buildship_common::time;
use chrono::{DateTime, Utc};
use sqlx::types::chrono::NaiveDateTime;
use uuid::Uuid;

/// 生成业务主键（UUID v4）
pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

/// 当前 UTC 时间
pub fn now() -> DateTime<Utc> {
    time::now()
}

/// 写入用时间串
pub fn now_str() -> String {
    time::to_db(time::now())
}

/// 把任意时间转成写库字符串
pub fn ts(value: DateTime<Utc>) -> String {
    time::to_db(value)
}

/// sqlx 在 SQLite 上对 TEXT 时间列可能解出 NaiveDateTime，这里统一转换。
pub fn from_naive(value: Option<NaiveDateTime>) -> Option<DateTime<Utc>> {
    value.map(|v| DateTime::<Utc>::from_naive_utc_and_offset(v, Utc))
}
