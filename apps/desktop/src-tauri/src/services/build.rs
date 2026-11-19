//! 构建服务：任务创建、子进程执行、日志流、取消、异常退出恢复。

use crate::events;
use crate::services::audit::{self, AuditInput};
use crate::services::quick;
use crate::state::{ActiveBuild, AppState, SharedState};
use buildship_build::process::CommandSpec;
use buildship_build::{toolchain, BuildCancel, BuildMessage, BuildRequest, BuildRunner};
use buildship_common::{text, time, AppError, AppResult, ErrorCode};
use buildship_db::repositories as repo;
use buildship_domain::{
    AuditAction, BuildEvent, BuildLogChunk, BuildLogSegment, BuildProfile, BuildQuery, BuildStatus,
    BuildTask, LogLine, LogQuery, LogStream, PackageManager, Project, RecordStatus,
};
use std::path::Path;
use tauri::AppHandle;
use tokio::sync::mpsc;

/// 组装传给包管理器的参数数组（绝不拼 shell 字符串）
fn build_args(pm: PackageManager, script: &str, extra: &[String]) -> Vec<String> {
    let mut args = vec!["run".to_string(), script.to_string()];
    if !extra.is_empty() {
        // npm / pnpm 需要 `--` 才能把参数透传给脚本
        if matches!(pm, PackageManager::Npm | PackageManager::Pnpm) {
            args.push("--".to_string());
        }
        args.extend(extra.iter().cloned());
    }
    args
}

fn empty_task(
    project_id: &str,
    profile: &BuildProfile,
    operator: &str,
    args_json: String,
) -> BuildTask {
    let now = time::now();
    BuildTask {
        id: uuid::Uuid::new_v4().to_string(),
        project_id: project_id.to_string(),
        profile_id: Some(profile.id.clone()),
        status: BuildStatus::Queued,
        command: profile.build_command.clone(),
        args_json,
        exit_code: None,
        started_at: None,
        finished_at: None,
        duration_ms: None,
        log_path: None,
        log_size: None,
        log_sha256: None,
        error_summary: None,
        triggered_by: Some(operator.to_string()),
        created_at: now,
        updated_at: now,
    }
}

/// 启动构建（异步执行，立即返回 queued 任务）
pub async fn start(app: AppHandle, state: SharedState, project_id: String) -> AppResult<BuildTask> {
    let project = {
        let db = state.db().await;
        repo::projects::require(db.pool(), &project_id).await?
    };
    if project.status != RecordStatus::Active {
        return Err(AppError::conflict("项目已归档，无法构建"));
    }

    let profile = {
        let db = state.db().await;
        repo::builds::default_profile(db.pool(), &project_id)
            .await?
            .ok_or_else(|| {
                AppError::invalid("项目缺少构建配置，请先在项目页配置构建脚本与产物目录")
            })?
    };

    // 1. 工具链检查
    let probe = toolchain::probe(project.package_manager.binary(), Path::new(&project.path)).await;
    if !probe.available {
        return Err(AppError::new(
            ErrorCode::ToolchainMissing,
            format!(
                "未检测到 {}，无法执行构建",
                project.package_manager.binary()
            ),
        )
        .with_detail(probe.hint.unwrap_or_default()));
    }

    // 2. 并发控制：同一项目只允许一个活动构建
    {
        let db = state.db().await;
        if let Some(active) = repo::builds::active_task(db.pool(), &project_id).await? {
            return Err(AppError::new(
                ErrorCode::BuildAlreadyRunning,
                format!(
                    "该项目已有进行中的构建任务（{}），请等待完成或先取消",
                    &active.id[..8.min(active.id.len())]
                ),
            ));
        }
    }

    let script = profile
        .build_script
        .clone()
        .unwrap_or_else(|| "build".to_string());
    let extra: Vec<String> = serde_json::from_str(&profile.build_args_json).unwrap_or_default();
    let args = build_args(project.package_manager, &script, &extra);
    let args_json = serde_json::to_string(&args)?;
    let task = empty_task(&project.id, &profile, &state.operator, args_json);

    // 3. 建任务记录 + 审计（同一事务）
    {
        let db = state.db().await;
        let mut tx = db.pool().begin().await?;
        repo::builds::insert_task(&mut *tx, &task).await?;
        repo::audit::append(
            &mut *tx,
            &audit::event(AuditInput {
                actor: &state.operator,
                action: AuditAction::BuildStarted,
                entity_type: "build_task",
                entity_id: &task.id,
                project_id: Some(&project.id),
                summary: format!("启动构建：{}", project.name),
                detail: Some(serde_json::json!({
                    "command": task.command,
                    "args": args,
                    "profileId": profile.id,
                })),
            }),
        )
        .await?;
        tx.commit().await?;
    }

    let cancel = BuildCancel::new();
    state.builds.register(&task.id, cancel.clone()).await;
    state
        .builds
        .set_active(Some(ActiveBuild {
            project_id: project.id.clone(),
            project_name: project.name.clone(),
            task_id: task.id.clone(),
        }))
        .await;

    let app_handle = app.clone();
    let state_clone = state.clone();
    let task_for_run = task.clone();
    let project_for_run = project.clone();
    let profile_for_run = profile.clone();
    tokio::spawn(async move {
        if let Err(err) = run_build(
            app_handle.clone(),
            state_clone.clone(),
            project_for_run,
            profile_for_run,
            task_for_run.clone(),
            cancel,
        )
        .await
        {
            tracing::error!(task = %task_for_run.id, error = %err, "构建执行异常");
            events::toast_error(&app_handle, format!("构建执行异常：{err}"));
            state_clone.builds.unregister(&task_for_run.id).await;
            state_clone.builds.set_active(None).await;
            quick::emit(&app_handle, &state_clone).await;
        }
    });

    quick::emit(&app, &state).await;
    Ok(task)
}

/// 真正执行构建的协程
async fn run_build(
    app: AppHandle,
    state: SharedState,
    project: Project,
    profile: BuildProfile,
    task: BuildTask,
    cancel: BuildCancel,
) -> AppResult<()> {
    let started_at = time::now();
    {
        let db = state.db().await;
        let mut tx = db.pool().begin().await?;
        repo::builds::mark_running(&mut *tx, &task.id, started_at).await?;
        tx.commit().await?;
    }

    events::emit_build(
        &app,
        BuildEvent {
            task_id: task.id.clone(),
            project_id: project.id.clone(),
            status: BuildStatus::Running,
            seq: 0,
            line: None,
            exit_code: None,
            message: Some(format!("开始执行：{}", task.command)),
        },
    );

    let script = profile
        .build_script
        .clone()
        .unwrap_or_else(|| "build".to_string());
    let extra: Vec<String> = serde_json::from_str(&profile.build_args_json).unwrap_or_default();
    let args = build_args(project.package_manager, &script, &extra);

    let mut spec = CommandSpec::new(project.package_manager.binary(), args, project.path.clone());
    if let Ok(env_map) =
        serde_json::from_str::<std::collections::BTreeMap<String, String>>(&profile.env_json)
    {
        spec.env = env_map.into_iter().collect();
    }

    let log_path = state
        .paths
        .build_logs_dir()
        .join(format!("{}.log", task.id));
    let request = BuildRequest {
        task_id: task.id.clone(),
        project_id: project.id.clone(),
        spec,
        log_path: log_path.clone(),
    };

    let (tx, mut rx) = mpsc::unbounded_channel::<BuildMessage>();
    let cancel_for_runner = cancel.clone();
    let runner =
        tokio::spawn(async move { BuildRunner::run(request, cancel_for_runner, tx).await });

    let mut line_count = 0i64;
    while let Some(message) = rx.recv().await {
        if let BuildMessage::Line(line) = message {
            line_count += 1;
            events::emit_build(
                &app,
                BuildEvent {
                    task_id: task.id.clone(),
                    project_id: project.id.clone(),
                    status: BuildStatus::Running,
                    seq: line.seq,
                    line: Some(line),
                    exit_code: None,
                    message: None,
                },
            );
        }
    }

    let outcome = runner
        .await
        .map_err(|err| AppError::internal(format!("构建任务异常结束：{err}")))??;

    let finished_at = time::now();
    let status = outcome.status;
    let log_path_str = log_path.to_string_lossy().to_string();

    {
        let db = state.db().await;
        let mut tx = db.pool().begin().await?;
        repo::builds::finish_task(
            &mut *tx,
            &task.id,
            status,
            outcome.exit_code.map(|code| code as i64),
            finished_at,
            outcome.duration_ms,
            outcome.message.as_deref(),
        )
        .await?;
        repo::builds::attach_log(
            &mut *tx,
            &task.id,
            &log_path_str,
            outcome.log_size,
            &outcome.log_sha256,
        )
        .await?;
        let segment: BuildLogSegment = repo::builds::new_log_segment(
            &task.id,
            0,
            &log_path_str,
            0,
            outcome.log_size,
            line_count,
        );
        repo::builds::insert_log_segment(&mut *tx, &segment).await?;
        repo::projects::set_last_build(&mut *tx, &project.id, status, finished_at).await?;

        let (action, summary) = match status {
            BuildStatus::Succeeded => (
                AuditAction::BuildSucceeded,
                format!("构建成功：{}", project.name),
            ),
            BuildStatus::Cancelled => (
                AuditAction::BuildCancelled,
                format!("构建取消：{}", project.name),
            ),
            _ => (
                AuditAction::BuildFailed,
                format!("构建失败：{}", project.name),
            ),
        };
        repo::audit::append(
            &mut *tx,
            &audit::event(AuditInput {
                actor: &state.operator,
                action,
                entity_type: "build_task",
                entity_id: &task.id,
                project_id: Some(&project.id),
                summary,
                detail: Some(serde_json::json!({
                    "exitCode": outcome.exit_code,
                    "durationMs": outcome.duration_ms,
                    "logSha256": outcome.log_sha256,
                    "logSize": outcome.log_size,
                })),
            }),
        )
        .await?;
        tx.commit().await?;
    }

    state.builds.unregister(&task.id).await;
    state.builds.set_active(None).await;
    state.set_last_build(status).await;

    events::emit_build(
        &app,
        BuildEvent {
            task_id: task.id.clone(),
            project_id: project.id.clone(),
            status,
            seq: line_count,
            line: None,
            exit_code: outcome.exit_code.map(|code| code as i64),
            message: outcome
                .message
                .clone()
                .or_else(|| Some("构建完成".to_string())),
        },
    );
    quick::emit(&app, &state).await;

    Ok(())
}

/// 取消构建
pub async fn cancel(state: &SharedState, task_id: &str) -> AppResult<()> {
    {
        let db = state.db().await;
        let task = repo::builds::require_task(db.pool(), task_id).await?;
        if task.status.is_terminal() {
            return Err(AppError::conflict("任务已结束，无需取消"));
        }
    }
    let token = state
        .builds
        .cancel_token(task_id)
        .await
        .ok_or_else(|| AppError::conflict("任务不在本进程运行中，无法取消"))?;
    token.cancel();
    Ok(())
}

pub async fn list(state: &AppState, query: BuildQuery) -> AppResult<Vec<BuildTask>> {
    let db = state.db().await;
    repo::builds::list_tasks(db.pool(), &query).await
}

pub async fn get(state: &AppState, task_id: &str) -> AppResult<BuildTask> {
    let db = state.db().await;
    repo::builds::require_task(db.pool(), task_id).await
}

/// 读取日志增量（从日志文件按行读取，跳过已消费的行）
pub async fn read_log(state: &AppState, query: LogQuery) -> AppResult<BuildLogChunk> {
    let task = {
        let db = state.db().await;
        repo::builds::require_task(db.pool(), &query.task_id).await?
    };

    let lines: Vec<LogLine> = match &task.log_path {
        Some(path) if Path::new(path).is_file() => {
            // 用字节读入再逐行解码：日志里可能混有 GBK 字节，`read_to_string`
            // 遇到非法 UTF-8 会整体失败，导致整份日志读不出来。
            let bytes = std::fs::read(path).unwrap_or_default();
            let mut raw_lines: Vec<&[u8]> = bytes.split(|b| *b == b'\n').collect();
            if raw_lines.last().is_some_and(|line| line.is_empty()) {
                raw_lines.pop();
            }
            raw_lines
                .into_iter()
                .enumerate()
                .map(|(index, raw)| {
                    let line = text::decode_line(raw);
                    let (stream, body) = if let Some(rest) = line.strip_prefix("! ") {
                        (LogStream::Stderr, rest)
                    } else if let Some(rest) = line.strip_prefix("· ") {
                        (LogStream::System, rest)
                    } else {
                        (LogStream::Stdout, line.as_str())
                    };
                    LogLine {
                        seq: index as i64 + 1,
                        stream,
                        text: body.to_string(),
                    }
                })
                .collect()
        }
        _ => Vec::new(),
    };

    let limit = query.limit.unwrap_or(2000).clamp(1, 5000) as usize;
    let (chunk, next_offset): (Vec<LogLine>, i64) = if query.tail {
        let start = lines.len().saturating_sub(limit);
        (
            lines.iter().skip(start).cloned().collect(),
            lines.len() as i64,
        )
    } else {
        let offset = query.offset.max(0) as usize;
        let chunk: Vec<LogLine> = lines.iter().skip(offset).take(limit).cloned().collect();
        let next = (offset + chunk.len()) as i64;
        (chunk, next)
    };
    let eof = task.status.is_terminal() && next_offset as usize >= lines.len();

    Ok(BuildLogChunk {
        task_id: task.id,
        lines: chunk,
        next_offset,
        eof,
    })
}

/// 应用启动时收尾上次异常退出残留的任务
pub async fn recover_interrupted(state: &AppState) -> AppResult<()> {
    let db = state.db().await;
    let affected = repo::builds::recover_interrupted(db.pool()).await?;
    if affected > 0 {
        tracing::warn!(count = affected, "已将上次异常退出的构建任务标记为失败");
    }
    Ok(())
}
