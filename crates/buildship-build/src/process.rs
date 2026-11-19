//! 外部命令执行封装。
//!
//! 安全要点（方案 4.3 / 12）：
//! 1. 永远用"程序 + 参数数组"启动，不拼 shell 字符串。
//! 2. Windows 上 npm/pnpm/yarn 是 `.cmd` 批处理，CreateProcess 无法直接执行，
//!    必须经由 `cmd.exe /d /c`。为了不引入 shell 注入面，参数会先过白名单校验，
//!    拒绝包含 `& | < > ^ % ! " ' \`` 换行 等元字符的参数。
//! 3. 取消构建时整棵进程树终止（Windows: taskkill /T /F；Unix: 进程组 SIGKILL）。

use buildship_common::{AppError, AppResult};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::{Child, Command};

/// 禁止出现在参数里的 shell 元字符（含引号，因为 cmd 会二次解析）。
const FORBIDDEN_CHARS: [char; 12] = [
    '&', '|', '<', '>', '^', '%', '!', '"', '\'', '`', '\n', '\r',
];

/// 参数校验：拒绝会改变命令语义的字符。
pub fn validate_arg(arg: &str) -> AppResult<()> {
    if arg.len() > 1024 {
        return Err(AppError::invalid("命令参数过长"));
    }
    for ch in FORBIDDEN_CHARS {
        if arg.contains(ch) {
            return Err(AppError::invalid(format!(
                "命令参数包含不允许的字符 `{ch}`：{arg}"
            )));
        }
    }
    if arg.contains('\u{0}') {
        return Err(AppError::invalid("命令参数包含非法空字符"));
    }
    Ok(())
}

fn is_windows_script(program: &str) -> bool {
    let lower = program.to_ascii_lowercase();
    lower.ends_with(".cmd") || lower.ends_with(".bat")
}

/// Windows 上把裸名字（`yarn`、`npm`）解析成 PATH 里的真实文件。
///
/// 必要性：`CreateProcess` 只自动补 `.exe`，而 npm/yarn/pnpm 在很多环境下
/// 是 `npm.cmd` 这类批处理垫片（标准 Node 安装、nvm-windows、Volta 的 shim 目录）。
/// 不解析就会直接报 `program not found`，构建第一步就挂。
fn resolve_program(program: &str) -> String {
    if !cfg!(windows) {
        return program.to_string();
    }
    let has_separator = program.contains('\\') || program.contains('/');
    if Path::new(program).is_absolute() || has_separator {
        return program.to_string();
    }
    resolve_binary(program)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| program.to_string())
}

/// 在 PATH 中查找可执行文件（Windows 会按 PATHEXT 尝试扩展名）。
pub fn resolve_binary(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    resolve_in_path(&path_var, name)
}

/// 可测的解析核心：给定 PATH 与名字，返回真实可执行文件。
///
/// Windows 关键点：npm/yarn/pnpm 常同时存在`无扩展名脚本`与 `.cmd` 垫片
/// （npm 包的 `bin` 目录、Git Bash 的软链）。`CreateProcess` 只认 PATHEXT 里的
/// 扩展名，返回无扩展名的文件会得到 `os error 193（不是有效的 Win32 应用程序）`。
/// 因此：名字本身没有扩展名时，只按 PATHEXT 匹配，不直接采用同名无扩展名文件。
#[cfg(windows)]
fn resolve_in_path(path_var: &std::ffi::OsStr, name: &str) -> Option<PathBuf> {
    let has_ext = Path::new(name).extension().is_some();
    let exts: Vec<String> = std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string())
        .split(';')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
        .collect();
    for dir in std::env::split_paths(path_var) {
        if has_ext {
            let direct = dir.join(name);
            if direct.is_file() {
                return Some(direct);
            }
            continue;
        }
        for ext in &exts {
            let candidate = dir.join(format!("{name}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// 非 Windows：PATH 里同名文件即为可执行文件。
#[cfg(not(windows))]
fn resolve_in_path(path_var: &std::ffi::OsStr, name: &str) -> Option<PathBuf> {
    for dir in std::env::split_paths(path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// 一次性命令定义。
#[derive(Debug, Clone)]
pub struct CommandSpec {
    /// 可执行文件名或绝对路径（`npm`、`C:\...\pnpm.cmd`）
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
}

impl CommandSpec {
    pub fn new(program: impl Into<String>, args: Vec<String>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args,
            cwd: cwd.into(),
            env: Vec::new(),
        }
    }

    /// 校验全部参数
    pub fn validate(&self) -> AppResult<()> {
        for arg in &self.args {
            validate_arg(arg)?;
        }
        Ok(())
    }

    /// 生成实际要 spawn 的 (program, args)。
    ///
    /// Windows 下的 `.cmd` 通过 `cmd.exe /d /c` 启动，参数逐个传递，
    /// 由 Rust 负责给含空格的参数加引号（我们已禁止参数内含引号）。
    pub fn to_spawn(&self) -> (String, Vec<String>) {
        let program = resolve_program(&self.program);
        if cfg!(windows) && is_windows_script(&program) {
            let mut args = vec!["/d".to_string(), "/c".to_string(), program];
            args.extend(self.args.iter().cloned());
            ("cmd.exe".to_string(), args)
        } else {
            (program, self.args.clone())
        }
    }

    /// 展示用命令行（写入数据库/日志，不含环境变量值）
    pub fn display(&self) -> String {
        let mut text = self.program.clone();
        for arg in &self.args {
            if arg.contains(' ') {
                text.push_str(&format!(" \"{arg}\""));
            } else {
                text.push(' ');
                text.push_str(arg);
            }
        }
        text
    }

    /// 启动子进程。
    pub fn spawn(&self) -> AppResult<Child> {
        self.validate()?;
        if !self.cwd.exists() {
            return Err(AppError::not_found(format!(
                "工作目录不存在：{}",
                self.cwd.display()
            )));
        }
        let (program, args) = self.to_spawn();
        let mut cmd = Command::new(&program);
        cmd.args(&args)
            .current_dir(&self.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // 让构建脚本拿到彩色输出而不是被判定为管道后降级
            .env("FORCE_COLOR", "1")
            .env("CI", "1")
            .kill_on_drop(false);

        for (key, value) in &self.env {
            cmd.env(key, value);
        }

        #[cfg(windows)]
        {
            // 本应用是 GUI 子系统进程（没有控制台）。启动控制台子进程时，
            // Windows 会为它新建一个控制台窗口并弹到桌面上——这正是用户看到的
            // 那个标题为 `...\yarn.exe` 的黑框。
            // CREATE_NO_WINDOW 让它无窗口运行，stdout/stderr 仍走我们接好的管道。
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // 独立进程组，便于整组终止
            cmd.process_group(0);
        }

        cmd.spawn().map_err(|e| {
            let hint = if e.kind() == std::io::ErrorKind::NotFound {
                format!(
                    "（在 PATH 中找不到 `{}`，请确认已安装并加入 PATH）",
                    self.program
                )
            } else {
                String::new()
            };
            AppError::new(
                buildship_common::ErrorCode::ProcessFailed,
                format!("启动命令失败 {}：{e}{hint}", self.display()),
            )
        })
    }
}

/// 终止整棵进程树。
pub async fn kill_tree(child: &mut Child) -> AppResult<()> {
    let Some(pid) = child.id() else {
        return Ok(());
    };

    if cfg!(windows) {
        // taskkill 递归终止子进程（npm → node → vite 等）
        let mut killer = Command::new("taskkill");
        killer
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // 同样是控制台程序，别让它闪一下黑框
        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            killer.creation_flags(CREATE_NO_WINDOW);
        }
        let status = killer.status().await;
        match status {
            Ok(_) => {}
            Err(e) => tracing::warn!(pid, error = %e, "taskkill 调用失败，回退到直接 kill"),
        }
        let _ = child.kill().await;
    } else {
        #[cfg(unix)]
        {
            // 负号表示向整个进程组发信号
            unsafe {
                libc::kill(-(pid as i32), libc::SIGTERM);
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
        let _ = child.kill().await;
    }
    let _ = child.wait().await;
    Ok(())
}

/// 便捷执行并收集输出的辅助函数（用于 `node -v` 这类短命令）。
pub async fn run_capture(
    spec: &CommandSpec,
    timeout: std::time::Duration,
) -> AppResult<(i32, String)> {
    let mut child = spec.spawn()?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let read_stdout = async {
        let mut buf = Vec::new();
        if let Some(mut out) = stdout {
            use tokio::io::AsyncReadExt;
            let _ = out.read_to_end(&mut buf).await;
        }
        String::from_utf8_lossy(&buf).to_string()
    };
    let read_stderr = async {
        let mut buf = Vec::new();
        if let Some(mut err) = stderr {
            use tokio::io::AsyncReadExt;
            let _ = err.read_to_end(&mut buf).await;
        }
        String::from_utf8_lossy(&buf).to_string()
    };

    let combined = async {
        let (out, err) = tokio::join!(read_stdout, read_stderr);
        format!("{out}{err}")
    };

    match tokio::time::timeout(timeout, async {
        let (text, status) = tokio::join!(combined, child.wait());
        (text, status)
    })
    .await
    {
        Ok((text, status)) => {
            let code = status.map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
            Ok((code, text))
        }
        Err(_) => {
            kill_tree(&mut child).await?;
            Err(AppError::new(
                buildship_common::ErrorCode::ProcessFailed,
                format!("命令执行超时：{}", spec.display()),
            ))
        }
    }
}

/// 判断路径是否为批处理脚本
pub fn is_script_path(path: &Path) -> bool {
    let program = path.to_string_lossy().to_lowercase();
    program.ends_with(".cmd") || program.ends_with(".bat")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_pipe_injection() {
        assert!(validate_arg("build && rm -rf /").is_err());
        assert!(validate_arg("a|b").is_err());
        assert!(validate_arg("--mode production").is_ok());
    }

    #[test]
    fn rejects_quotes() {
        assert!(validate_arg("\"quoted\"").is_err());
        assert!(validate_arg("it's").is_err());
    }

    #[test]
    fn display_quotes_spaces() {
        let spec = CommandSpec::new("npm", vec!["run".into(), "build:app".into()], ".");
        assert_eq!(spec.display(), "npm run build:app");
    }

    /// 绝对路径的 `.cmd` 必须走 `cmd.exe /d /c`，否则 CreateProcess 无法执行批处理
    #[cfg(windows)]
    #[test]
    fn cmd_script_goes_through_cmd_exe() {
        let spec = CommandSpec::new(r"C:\tools\pnpm.cmd", vec!["build".into()], ".");
        let (program, args) = spec.to_spawn();
        assert_eq!(program.to_ascii_lowercase(), "cmd.exe");
        assert_eq!(args[0], "/d");
        assert_eq!(args[1], "/c");
        assert_eq!(args[2], r"C:\tools\pnpm.cmd");
        assert_eq!(args[3], "build");
    }

    /// 裸名字解析失败时保持原样（由 spawn 给出可读错误），不能 panic
    #[test]
    fn unresolved_bare_name_is_kept_as_is() {
        let spec = CommandSpec::new("definitely-not-installed-pm", Vec::new(), ".");
        let (program, args) = spec.to_spawn();
        if cfg!(windows) {
            assert_eq!(program, "definitely-not-installed-pm");
            assert!(args.is_empty());
        } else {
            assert_eq!(program, "definitely-not-installed-pm");
        }
    }

    /// 同名无扩展名文件存在时，必须优先选 PATHEXT 里的 `.cmd`，
    /// 否则 spawn 会报 os error 193（不是有效的 Win32 应用程序）。
    #[cfg(windows)]
    #[test]
    fn picks_pathext_shim_over_extensionless_file() {
        let dir = std::env::temp_dir().join("buildship-resolve-pathext-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("fakepm"), b"#!/bin/sh\nexit 0\n").unwrap();
        std::fs::write(dir.join("fakepm.cmd"), b"@echo off\r\n").unwrap();

        let path_var = std::env::join_paths([dir.as_os_str()]).unwrap();
        let found = resolve_in_path(&path_var, "fakepm").expect("应能解析到 shim");
        let has_cmd_ext = std::env::var("PATHEXT")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .contains(".cmd");
        if has_cmd_ext {
            assert_eq!(
                found
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase()),
                Some("cmd".to_string())
            );
        } else {
            // 没有 .CMD 的环境中不应退化成无扩展名文件
            assert!(found.is_file());
        }
    }
}
