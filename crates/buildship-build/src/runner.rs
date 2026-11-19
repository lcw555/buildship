//! 构建执行器：启动子进程、流式收集日志、支持取消。
//!
//! 设计要点：
//! - 日志按行推送（Tauri 事件），同时落盘到独立日志文件（方案 5.4：正文不塞数据库）。
//! - 日志写入前统一脱敏（`buildship_common::redact`）。
//! - 取消时终止整棵进程树，状态标记为 cancelled 而不是 failed。

use crate::process::{kill_tree, CommandSpec};
use buildship_common::{redact, text, AppError, AppResult};
use buildship_domain::{BuildStatus, LogLine, LogStream};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{mpsc, Notify};

/// 取消令牌：可在命令线程里安全触发。
#[derive(Clone, Default)]
pub struct BuildCancel {
    flag: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl BuildCancel {
    pub fn new() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
        }
    }

    /// 触发取消（幂等）
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

/// 一次构建的输入
#[derive(Debug, Clone)]
pub struct BuildRequest {
    pub task_id: String,
    pub project_id: String,
    pub spec: CommandSpec,
    pub log_path: PathBuf,
}

/// 一次构建的结果
#[derive(Debug, Clone)]
pub struct BuildOutcome {
    pub status: BuildStatus,
    pub exit_code: Option<i32>,
    pub duration_ms: i64,
    pub log_path: PathBuf,
    pub log_size: i64,
    pub log_sha256: String,
    /// 尾部若干行，用于失败摘要
    pub tail: Vec<String>,
    pub message: Option<String>,
}

/// 推送给上层的消息
#[derive(Debug, Clone)]
pub enum BuildMessage {
    Line(LogLine),
    Finished(BuildOutcome),
}

type LineMsg = (LogStream, String);

/// 构建执行器（无状态，所有上下文通过参数传入）
pub struct BuildRunner;

impl BuildRunner {
    /// 执行构建。
    ///
    /// `tx` 用于流式推送日志；函数返回时也会通过它发出 `Finished`。
    pub async fn run(
        req: BuildRequest,
        cancel: BuildCancel,
        tx: mpsc::UnboundedSender<BuildMessage>,
    ) -> AppResult<BuildOutcome> {
        if let Some(parent) = req.log_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AppError::io(format!("创建日志目录失败：{e}")))?;
        }
        let mut log_file = std::fs::File::create(&req.log_path)
            .map_err(|e| AppError::io(format!("创建构建日志文件失败：{e}")))?;

        let started = Instant::now();
        let mut child = req.spec.spawn()?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let (line_tx, mut line_rx) = mpsc::unbounded_channel::<LineMsg>();
        if let Some(stdout) = stdout {
            let sender = line_tx.clone();
            tokio::spawn(async move {
                pump(stdout, LogStream::Stdout, sender).await;
            });
        }
        if let Some(stderr) = stderr {
            let sender = line_tx.clone();
            tokio::spawn(async move {
                pump(stderr, LogStream::Stderr, sender).await;
            });
        }
        drop(line_tx);

        let seq = AtomicI64::new(0);
        let mut tail: VecDeque<String> = VecDeque::with_capacity(32);
        let mut exit_status: Option<std::process::ExitStatus> = None;
        let mut cancelled = false;
        // 所有管道读完（子进程关闭了 stdout/stderr）后仍需等待进程退出，
        // 否则会漏掉退出码，把成功的构建误判成「进程异常终止」。
        let mut lines_open = true;

        loop {
            tokio::select! {
                biased;

                maybe = line_rx.recv(), if lines_open => {
                    match maybe {
                        Some((stream, text)) => {
                            emit(&mut log_file, &seq, &tx, stream, &text, &mut tail);
                        }
                        None => lines_open = false,
                    }
                }
                status = child.wait(), if exit_status.is_none() && !cancelled => {
                    match status {
                        Ok(status) => exit_status = Some(status),
                        Err(err) => {
                            tracing::warn!(task = %req.task_id, error = %err, "等待构建进程失败");
                            break;
                        }
                    }
                }
                // 输出已关闭但进程迟迟不退出（遗留子进程持有句柄），兜底强杀
                _ = tokio::time::sleep(Duration::from_secs(30)), if !lines_open && exit_status.is_none() => {
                    tracing::warn!(task = %req.task_id, "构建输出已关闭但进程未退出，强制终止");
                    emit(&mut log_file, &seq, &tx, LogStream::System, "构建输出已关闭但进程未退出，已强制终止", &mut tail);
                    kill_tree(&mut child).await?;
                    break;
                }
                _ = async {
                    if !cancel.is_cancelled() {
                        cancel.notify.notified().await;
                    }
                }, if !cancelled => {
                    cancelled = true;
                    emit(&mut log_file, &seq, &tx, LogStream::System, "收到取消指令，正在终止构建进程…", &mut tail);
                    kill_tree(&mut child).await?;
                    break;
                }
            }

            if !lines_open && exit_status.is_some() {
                break;
            }
        }

        // 进程退出后仍可能有缓冲行待读取；给 2 秒收尾时间
        if !cancelled {
            let _ = tokio::time::timeout(Duration::from_secs(2), async {
                while let Some((stream, text)) = line_rx.recv().await {
                    emit(&mut log_file, &seq, &tx, stream, &text, &mut tail);
                }
            })
            .await;
        }

        let duration_ms = started.elapsed().as_millis() as i64;
        let exit_code = exit_status.and_then(|status| status.code());

        let status = if cancelled {
            BuildStatus::Cancelled
        } else if exit_code == Some(0) {
            BuildStatus::Succeeded
        } else {
            BuildStatus::Failed
        };

        let message = match status {
            BuildStatus::Succeeded => None,
            BuildStatus::Cancelled => Some("构建已取消".to_string()),
            _ => Some(match exit_code {
                Some(code) => format!("构建失败，退出码 {code}"),
                None => "构建失败，进程异常终止".to_string(),
            }),
        };

        if let Some(text) = &message {
            emit(&mut log_file, &seq, &tx, LogStream::System, text, &mut tail);
        }
        let _ = log_file.flush();
        drop(log_file);

        let (log_size, log_sha256) = hash_file(&req.log_path)?;

        let outcome = BuildOutcome {
            status,
            exit_code,
            duration_ms,
            log_path: req.log_path.clone(),
            log_size,
            log_sha256,
            tail: tail.into_iter().collect(),
            message,
        };
        let _ = tx.send(BuildMessage::Finished(outcome.clone()));
        Ok(outcome)
    }
}

/// 逐行读取子进程输出。
///
/// 不用 `lines()`：Windows 上构建脚本可能输出 GBK 等非 UTF-8 字节，
/// `lines()` 遇到非法 UTF-8 会直接返回 Err 并中断读取，导致日志被截断。
/// 解码统一走 [`buildship_common::text::decode_line`]，先按 UTF-8、失败再按 GBK。
async fn pump<R>(reader: R, stream: LogStream, sender: mpsc::UnboundedSender<LineMsg>)
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut buffer = Vec::with_capacity(4096);
    loop {
        buffer.clear();
        match reader.read_until(b'\n', &mut buffer).await {
            Ok(0) => break,
            Ok(_) => {
                let text = text::decode_line(&buffer);
                if sender.send((stream, text)).is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}

/// 写一行：脱敏 → 落盘 → 推送事件
fn emit(
    file: &mut std::fs::File,
    seq: &AtomicI64,
    tx: &mpsc::UnboundedSender<BuildMessage>,
    stream: LogStream,
    text: &str,
    tail: &mut VecDeque<String>,
) {
    let clean = redact::scrub_line(text);
    let index = seq.fetch_add(1, Ordering::SeqCst) + 1;

    let prefix = match stream {
        LogStream::Stderr => "! ",
        LogStream::System => "· ",
        LogStream::Stdout => "",
    };
    let _ = writeln!(file, "{prefix}{clean}");
    if tail.len() == 32 {
        tail.pop_front();
    }
    tail.push_back(clean.clone());

    let _ = tx.send(BuildMessage::Line(LogLine {
        seq: index,
        stream,
        text: clean,
    }));
}

fn hash_file(path: &std::path::Path) -> AppResult<(i64, String)> {
    use std::io::Read;
    let mut file =
        std::fs::File::open(path).map_err(|e| AppError::io(format!("读取日志文件失败：{e}")))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut size = 0i64;
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|e| AppError::io(format!("读取日志文件失败：{e}")))?;
        if read == 0 {
            break;
        }
        size += read as i64;
        hasher.update(&buffer[..read]);
    }
    Ok((size, hex::encode(hasher.finalize())))
}
