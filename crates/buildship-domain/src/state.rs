//! 状态机。
//!
//! 所有状态变更都应先调用这里的校验函数，再去写库。把规则集中在领域层，
//! 可以避免"某个 service 忘了判断"导致 release 状态乱跳。

use crate::enums::{BuildStatus, DeploymentStatus, ReleaseStatus};
use buildship_common::{AppError, AppResult, ErrorCode};

fn deny(from: &str, to: &str) -> AppError {
    AppError::new(
        ErrorCode::StateInvalid,
        format!("非法状态流转：{from} → {to}"),
    )
}

/// 构建任务状态机：`queued → running → succeeded / failed / cancelled`
pub fn ensure_build_transition(from: BuildStatus, to: BuildStatus) -> AppResult<()> {
    use BuildStatus::*;
    let ok = matches!(
        (from, to),
        (Queued, Running)
            | (Queued, Cancelled)
            | (Running, Succeeded)
            | (Running, Failed)
            | (Running, Cancelled)
    );
    if ok {
        return Ok(());
    }
    Err(deny(&format!("{from:?}"), &format!("{to:?}")))
}

/// Release 状态机：`draft → uploading → deploying → health_check → healthy`
///
/// 任何阶段失败都进入 `failed`；成功后若被回滚则进入 `rolled_back`。
pub fn ensure_release_transition(from: ReleaseStatus, to: ReleaseStatus) -> AppResult<()> {
    use ReleaseStatus::*;
    if from == to {
        return Ok(());
    }
    let ok = matches!(
        (from, to),
        (Draft, Uploading)
            | (Draft, Failed)
            | (Draft, Cancelled)
            | (Uploading, Deploying)
            | (Uploading, Failed)
            | (Uploading, Cancelled)
            | (Deploying, HealthCheck)
            | (Deploying, Failed)
            | (Deploying, Cancelled)
            | (HealthCheck, Healthy)
            | (HealthCheck, Failed)
            | (HealthCheck, RolledBack)
            | (Healthy, RolledBack)
            | (Failed, RolledBack)
    );
    if ok {
        return Ok(());
    }
    Err(deny(&format!("{from:?}"), &format!("{to:?}")))
}

/// 部署任务状态机，与 Release 阶段保持同步推进。
pub fn ensure_deployment_transition(from: DeploymentStatus, to: DeploymentStatus) -> AppResult<()> {
    use DeploymentStatus::*;
    if from == to {
        return Ok(());
    }
    let ok = matches!(
        (from, to),
        (Queued, Uploading)
            | (Queued, Failed)
            | (Queued, Cancelled)
            | (Uploading, Deploying)
            | (Uploading, Failed)
            | (Uploading, Cancelled)
            | (Deploying, HealthCheck)
            | (Deploying, Succeeded)
            | (Deploying, Failed)
            | (Deploying, Cancelled)
            | (HealthCheck, Succeeded)
            | (HealthCheck, Failed)
            | (HealthCheck, Cancelled)
    );
    if ok {
        return Ok(());
    }
    Err(deny(&format!("{from:?}"), &format!("{to:?}")))
}

/// 校验回滚目标是否合法：只能是健康的历史版本，且不是当前版本。
pub fn ensure_rollback_target(
    target_status: ReleaseStatus,
    target_is_current: bool,
    target_id: &str,
    current_id: Option<&str>,
) -> AppResult<()> {
    if target_is_current {
        return Err(AppError::conflict("目标版本已是当前线上版本，无需回滚"));
    }
    if Some(target_id) == current_id {
        return Err(AppError::conflict("目标版本已是当前线上版本，无需回滚"));
    }
    if !target_status.is_rollback_candidate() {
        return Err(AppError::conflict("只能回滚到状态为 healthy 的历史版本"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_happy_path_is_allowed() {
        assert!(ensure_build_transition(BuildStatus::Queued, BuildStatus::Running).is_ok());
        assert!(ensure_build_transition(BuildStatus::Running, BuildStatus::Succeeded).is_ok());
    }

    #[test]
    fn build_cannot_skip_to_success() {
        assert!(ensure_build_transition(BuildStatus::Queued, BuildStatus::Succeeded).is_err());
    }

    #[test]
    fn release_happy_path_is_allowed() {
        for (from, to) in [
            (ReleaseStatus::Draft, ReleaseStatus::Uploading),
            (ReleaseStatus::Uploading, ReleaseStatus::Deploying),
            (ReleaseStatus::Deploying, ReleaseStatus::HealthCheck),
            (ReleaseStatus::HealthCheck, ReleaseStatus::Healthy),
        ] {
            assert!(ensure_release_transition(from, to).is_ok());
        }
    }

    #[test]
    fn healthy_release_cannot_go_back_to_draft() {
        assert!(ensure_release_transition(ReleaseStatus::Healthy, ReleaseStatus::Draft).is_err());
    }

    #[test]
    fn rollback_target_must_be_healthy() {
        assert!(ensure_rollback_target(ReleaseStatus::Failed, false, "r2", Some("r1")).is_err());
        assert!(ensure_rollback_target(ReleaseStatus::Healthy, true, "r1", Some("r1")).is_err());
        assert!(ensure_rollback_target(ReleaseStatus::Healthy, false, "r0", Some("r1")).is_ok());
    }
}
