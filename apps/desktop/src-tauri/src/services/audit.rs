//! 审计事件构造与写入。
//!
//! 方案 3.1 / 5.4：关键业务写入与审计事件必须在同一个事务提交。
//! 因此这里的函数只返回 `AuditEvent`，由调用方在事务里 `repo::audit::append`。

use buildship_domain::{AuditAction, AuditEvent};
use chrono::{DateTime, Utc};

pub struct AuditInput<'a> {
    pub actor: &'a str,
    pub action: AuditAction,
    pub entity_type: &'a str,
    pub entity_id: &'a str,
    pub project_id: Option<&'a str>,
    pub summary: String,
    pub detail: Option<serde_json::Value>,
}

pub fn event(input: AuditInput<'_>) -> AuditEvent {
    audit_event_at(input, buildship_common::time::now())
}

pub fn audit_event_at(input: AuditInput<'_>, at: DateTime<Utc>) -> AuditEvent {
    AuditEvent {
        id: uuid::Uuid::new_v4().to_string(),
        actor: input.actor.to_string(),
        action: input.action,
        entity_type: input.entity_type.to_string(),
        entity_id: input.entity_id.to_string(),
        project_id: input.project_id.map(str::to_string),
        summary: input.summary,
        detail_json: input.detail.map(|value| value.to_string()),
        created_at: at,
    }
}
