//! 构建执行器的进程收尾行为。
//!
//! 这里盯的是一个真实踩过的坑：子进程先关闭 stdout/stderr、随后才退出时，
//! 事件循环若在「管道读完」就 break，就拿不到退出码，成功的构建会被
//! 误判为「构建失败，进程异常终止」。

use buildship_build::process::CommandSpec;
use buildship_build::runner::{BuildCancel, BuildMessage, BuildOutcome, BuildRequest, BuildRunner};
use buildship_domain::BuildStatus;
use std::path::PathBuf;

fn temp_spec(marker: &str) -> CommandSpec {
    if cfg!(windows) {
        CommandSpec::new(
            "cmd",
            vec![
                "/d".to_string(),
                "/c".to_string(),
                "echo".to_string(),
                marker.to_string(),
            ],
            std::env::temp_dir(),
        )
    } else {
        CommandSpec::new(
            "sh",
            vec!["-c".to_string(), format!("echo {marker}")],
            std::env::temp_dir(),
        )
    }
}

async fn run_case(marker: &str) -> (BuildOutcome, String) {
    let dir = tempfile::tempdir().expect("创建临时目录");
    let log_path: PathBuf = dir.path().join("build.log");
    let request = BuildRequest {
        task_id: "task-test".to_string(),
        project_id: "project-test".to_string(),
        spec: temp_spec(marker),
        log_path: log_path.clone(),
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<BuildMessage>();

    let outcome = BuildRunner::run(request, BuildCancel::new(), tx)
        .await
        .expect("构建执行不应报错");
    // 事件流里的 Finished 必须和返回值一致
    let mut finished = None;
    while let Ok(message) = rx.try_recv() {
        if let BuildMessage::Finished(payload) = message {
            finished = Some(payload);
        }
    }
    let log = std::fs::read_to_string(&log_path).unwrap_or_default();
    assert!(
        finished.is_some_and(|payload| payload.status == outcome.status),
        "事件流应发出同状态的 Finished"
    );
    (outcome, log)
}

#[tokio::test]
async fn captures_exit_code_after_pipes_close() {
    let (outcome, log) = run_case("buildship-ok").await;
    assert_eq!(outcome.exit_code, Some(0), "应拿到真实退出码 0");
    assert_eq!(outcome.status, BuildStatus::Succeeded);
    assert!(outcome.message.is_none(), "成功构建不应带错误摘要");
    assert!(log.contains("buildship-ok"), "日志应包含子进程输出");
}

#[tokio::test]
async fn failed_exit_code_is_reported() {
    let dir = tempfile::tempdir().expect("创建临时目录");
    let log_path = dir.path().join("fail.log");
    let spec = if cfg!(windows) {
        // exit /b 3 会立即以非 0 退出
        CommandSpec::new(
            "cmd",
            vec![
                "/d".to_string(),
                "/c".to_string(),
                "exit".to_string(),
                "/b".to_string(),
                "3".to_string(),
            ],
            std::env::temp_dir(),
        )
    } else {
        CommandSpec::new(
            "sh",
            vec!["-c".to_string(), "exit 3".to_string()],
            std::env::temp_dir(),
        )
    };

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<BuildMessage>();
    let outcome = BuildRunner::run(
        BuildRequest {
            task_id: "task-fail".to_string(),
            project_id: "project-fail".to_string(),
            spec,
            log_path,
        },
        BuildCancel::new(),
        tx,
    )
    .await
    .expect("构建执行不应报错");

    assert_eq!(outcome.exit_code, Some(3));
    assert_eq!(outcome.status, BuildStatus::Failed);
    assert!(
        outcome
            .message
            .as_deref()
            .is_some_and(|text| text.contains('3')),
        "错误摘要应带退出码"
    );
}
