//! 时间工具：全库统一 UTC 存储。

use chrono::{DateTime, SecondsFormat, Utc};

/// 当前 UTC 时间。
pub fn now() -> DateTime<Utc> {
    Utc::now()
}

/// 以毫秒精度序列化为 RFC3339 字符串，用于 SQLite TEXT 列写入。
pub fn to_db(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// 生成发布版本目录名用的紧凑时间戳：`20261118-120001`。
pub fn compact_stamp(value: DateTime<Utc>) -> String {
    value.format("%Y%m%d-%H%M%S").to_string()
}
