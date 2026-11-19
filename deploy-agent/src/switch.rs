//! `current` 指针切换。
//!
//! 目标：切换过程尽量原子，且不破坏 `releases/` 里的历史版本。
//!
//! - `link` 模式：`current` 是指向 `releases/<id>` 的目录联接（Windows junction，
//!   普通用户即可创建，不需要管理员或开发者模式）/ 符号链接（Unix）。
//!   切换 = 建临时链接 → 改两名 → 删旧链接，窗口期在毫秒级。
//! - `copy` 模式：`current` 是真实目录，先完整复制到 `current.next`，再两步改名。
//!   兼容所有环境（含无权创建联接的服务器），代价是切换期间多占一份磁盘。
//!
//! 删除 `current` 时必须先尝试 `remove_dir`（只删链接本身），
//! 只有在确认是真实目录时才递归删除，避免把版本内容误删。

use crate::config::LinkStrategy;
use crate::store::Layout;
use buildship_common::{AppError, AppResult};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchMode {
    Link,
    Copy,
}

impl SwitchMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Link => "link",
            Self::Copy => "copy",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SwitchOutcome {
    pub mode: SwitchMode,
    pub current_path: PathBuf,
}

impl SwitchOutcome {
    /// 切换后的线上目录（`.../current`），供审计与响应体使用
    pub fn current_path(&self) -> &Path {
        &self.current_path
    }
}

/// 把 `current` 切到指定版本
pub fn switch(
    layout: &Layout,
    release_id: &str,
    strategy: LinkStrategy,
) -> AppResult<SwitchOutcome> {
    let release_dir = layout.release_dir(release_id);
    if !release_dir.is_dir() {
        return Err(AppError::not_found(format!(
            "版本目录不存在：{}（请先上传发布包）",
            release_dir.display()
        )));
    }
    layout.ensure()?;
    // 上一轮异常退出可能留下半成品，先清理
    let _ = remove_entry(&layout.current_next());
    let _ = remove_entry(&layout.current_prev());

    match strategy {
        LinkStrategy::Link => {
            make_link(&layout.current_next(), &release_dir).map_err(|e| {
                AppError::new(
                    buildship_common::ErrorCode::PermissionDenied,
                    format!(
                        "创建目录联接失败（linkStrategy=link）：{e}。\
                         可改为 linkStrategy = \"copy\" 或 \"auto\""
                    ),
                )
            })?;
            finalize(layout)?;
            Ok(SwitchOutcome {
                mode: SwitchMode::Link,
                current_path: layout.current(),
            })
        }
        LinkStrategy::Copy => {
            copy_tree(&release_dir, &layout.current_next())?;
            finalize(layout)?;
            Ok(SwitchOutcome {
                mode: SwitchMode::Copy,
                current_path: layout.current(),
            })
        }
        LinkStrategy::Auto => match make_link(&layout.current_next(), &release_dir) {
            Ok(()) => {
                finalize(layout)?;
                Ok(SwitchOutcome {
                    mode: SwitchMode::Link,
                    current_path: layout.current(),
                })
            }
            Err(link_error) => {
                tracing::warn!(
                    error = %link_error,
                    "创建目录联接失败，自动退回复制模式"
                );
                let _ = remove_entry(&layout.current_next());
                copy_tree(&release_dir, &layout.current_next())?;
                finalize(layout)?;
                Ok(SwitchOutcome {
                    mode: SwitchMode::Copy,
                    current_path: layout.current(),
                })
            }
        },
    }
}

/// `current.next` 就绪后，用它替换 `current`
fn finalize(layout: &Layout) -> AppResult<()> {
    let current = layout.current();
    let next = layout.current_next();
    let prev = layout.current_prev();

    if next_missing(&next) {
        return Err(AppError::internal(format!(
            "切换失败：暂存目录不存在 {}",
            next.display()
        )));
    }

    let had_current = path_present(&current);
    if had_current {
        std::fs::rename(&current, &prev).map_err(|e| {
            AppError::io(format!(
                "切换失败（旧版本改名为 current.prev）：{e}。常见原因：Web 服务器正占用目录中的文件"
            ))
        })?;
    }

    if let Err(e) = std::fs::rename(&next, &current) {
        // 回退，保证站点仍在旧版本上可用
        if had_current {
            let _ = std::fs::rename(&prev, &current);
        }
        return Err(AppError::io(format!(
            "切换失败（暂存目录改名为 current）：{e}"
        )));
    }

    if had_current {
        if let Err(e) = remove_entry(&prev) {
            tracing::warn!(error = %e, "清理 current.prev 失败，可手工删除");
        }
    }
    Ok(())
}

fn next_missing(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_err()
}

/// 路径是否存在（不跟随链接）
pub fn path_present(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// 删除目录项：优先按链接处理（只删链接本身），确认为真实目录才递归删除。
pub fn remove_entry(path: &Path) -> AppResult<()> {
    if !path_present(path) {
        return Ok(());
    }
    match std::fs::remove_dir(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => {
            // 非空真实目录才走递归；若这里是链接，remove_dir 不会失败
            std::fs::remove_dir_all(path).map_err(|e| {
                AppError::io(format!(
                    "删除目录失败 {}（remove_dir: {err}）：{e}",
                    path.display()
                ))
            })
        }
    }
}

/// 创建目录联接 / 符号链接
fn make_link(link: &Path, target: &Path) -> std::io::Result<()> {
    remove_entry(link).map_err(|e| std::io::Error::other(e.to_string()))?;
    let target_abs = std::fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf());

    #[cfg(windows)]
    {
        // junction 要求链接路径先存在且为空目录
        std::fs::create_dir(link)?;
        match win_junction::set_mount_point(link, &target_abs) {
            Ok(()) => Ok(()),
            Err(err) => {
                let _ = std::fs::remove_dir(link);
                // 退路：符号链接（需要管理员权限或开发者模式）
                std::os::windows::fs::symlink_dir(&target_abs, link).map_err(|symlink_err| {
                    std::io::Error::other(format!(
                        "junction 失败（{err}）；符号链接失败（{symlink_err}）"
                    ))
                })
            }
        }
    }

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&target_abs, link)
    }
}

/// 递归复制目录树（含空目录）
fn copy_tree(from: &Path, to: &Path) -> AppResult<()> {
    std::fs::create_dir_all(to)
        .map_err(|e| AppError::io(format!("创建暂存目录失败 {}：{e}", to.display())))?;
    for entry in walkdir::WalkDir::new(from).follow_links(false) {
        let entry = entry.map_err(|e| AppError::io(format!("遍历版本目录失败：{e}")))?;
        let relative = entry
            .path()
            .strip_prefix(from)
            .map_err(|e| AppError::internal(format!("计算相对路径失败：{e}")))?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let destination = to.join(relative);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&destination).map_err(|e| {
                AppError::io(format!("创建目录失败 {}：{e}", destination.display()))
            })?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            std::fs::copy(entry.path(), &destination).map_err(|e| {
                AppError::io(format!(
                    "复制文件失败 {} → {}：{e}",
                    entry.path().display(),
                    destination.display()
                ))
            })?;
        }
    }
    Ok(())
}

// ---------- Windows 目录联接（不用 mklink，不拼命令行） ----------

#[cfg(windows)]
mod win_junction {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    const FSCTL_SET_REPARSE_POINT: u32 = 0x0009_00A4;
    const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const OPEN_EXISTING: u32 = 3;
    const INVALID_HANDLE_VALUE: isize = -1;

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateFileW(
            file_name: *const u16,
            desired_access: u32,
            share_mode: u32,
            security_attributes: *mut core::ffi::c_void,
            creation_disposition: u32,
            flags_and_attributes: u32,
            template_file: isize,
        ) -> isize;

        fn DeviceIoControl(
            device: isize,
            io_control_code: u32,
            in_buffer: *const core::ffi::c_void,
            in_buffer_size: u32,
            out_buffer: *mut core::ffi::c_void,
            out_buffer_size: u32,
            bytes_returned: *mut u32,
            overlapped: *mut core::ffi::c_void,
        ) -> i32;

        fn CloseHandle(object: isize) -> i32;
    }

    /// 把 `link`（必须已存在的空目录）设置为指向 `target` 的目录联接
    pub fn set_mount_point(link: &Path, target: &Path) -> std::io::Result<()> {
        let substitute = format!(r"\??\{}", target.display());
        let print = target.display().to_string();
        let substitute_w: Vec<u16> = OsStr::new(&substitute).encode_wide().collect();
        let print_w: Vec<u16> = OsStr::new(&print).encode_wide().collect();

        let substitute_bytes = substitute_w.len() * 2;
        let print_bytes = print_w.len() * 2;
        const MOUNT_HEADER: usize = 12; // 4 × USHORT + ULONG Flags
        let data_len = MOUNT_HEADER + substitute_bytes + 2 + print_bytes + 2;
        let total = 8 + data_len; // + ReparseTag / ReparseDataLength / Reserved

        let mut buffer = vec![0u8; total];
        buffer[0..4].copy_from_slice(&IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
        buffer[4..6].copy_from_slice(&(data_len as u16).to_le_bytes());
        // 6..8 Reserved 保持 0
        buffer[8..10].copy_from_slice(&0u16.to_le_bytes()); // SubstituteNameOffset
        buffer[10..12].copy_from_slice(&(substitute_bytes as u16).to_le_bytes());
        buffer[12..14].copy_from_slice(&((substitute_bytes + 2) as u16).to_le_bytes());
        buffer[14..16].copy_from_slice(&(print_bytes as u16).to_le_bytes());
        // 16..20 Flags = 0（相对联接）

        let mut cursor = 20usize;
        for unit in &substitute_w {
            buffer[cursor..cursor + 2].copy_from_slice(&unit.to_le_bytes());
            cursor += 2;
        }
        cursor += 2; // 替代名 NUL 结尾
        for unit in &print_w {
            buffer[cursor..cursor + 2].copy_from_slice(&unit.to_le_bytes());
            cursor += 2;
        }

        let link_w: Vec<u16> = link
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        unsafe {
            let handle = CreateFileW(
                link_w.as_ptr(),
                GENERIC_WRITE,
                0,
                std::ptr::null_mut(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                0,
            );
            if handle == INVALID_HANDLE_VALUE {
                return Err(std::io::Error::last_os_error());
            }
            let mut returned = 0u32;
            let ok = DeviceIoControl(
                handle,
                FSCTL_SET_REPARSE_POINT,
                buffer.as_ptr() as *const core::ffi::c_void,
                buffer.len() as u32,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
            );
            let err = std::io::Error::last_os_error();
            CloseHandle(handle);
            if ok == 0 {
                return Err(err);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LinkStrategy;
    use std::fs;
    use tempfile::tempdir;

    fn prepare_release(layout: &Layout, id: &str, body: &str) {
        layout.ensure().unwrap();
        let dir = layout.release_dir(id);
        fs::create_dir_all(dir.join("assets")).unwrap();
        fs::write(dir.join("index.html"), body).unwrap();
        fs::write(dir.join("assets/app.js"), "console.log(1)").unwrap();
    }

    #[test]
    fn copy_mode_switches_and_keeps_history() {
        let temp = tempdir().unwrap();
        let layout = Layout::new(temp.path());
        prepare_release(&layout, "r1", "<html>v1</html>");
        prepare_release(&layout, "r2", "<html>v2</html>");

        let first = switch(&layout, "r1", LinkStrategy::Copy).unwrap();
        assert_eq!(first.mode, SwitchMode::Copy);
        assert_eq!(
            fs::read_to_string(layout.current().join("index.html")).unwrap(),
            "<html>v1</html>"
        );

        let second = switch(&layout, "r2", LinkStrategy::Copy).unwrap();
        assert_eq!(second.mode, SwitchMode::Copy);
        assert_eq!(
            fs::read_to_string(layout.current().join("index.html")).unwrap(),
            "<html>v2</html>"
        );
        // 历史版本仍在
        assert!(layout.release_dir("r1").join("index.html").is_file());
    }

    #[test]
    fn refuses_unknown_release() {
        let temp = tempdir().unwrap();
        let layout = Layout::new(temp.path());
        layout.ensure().unwrap();
        assert!(switch(&layout, "missing", LinkStrategy::Copy).is_err());
    }

    #[test]
    fn auto_mode_falls_back_gracefully() {
        let temp = tempdir().unwrap();
        let layout = Layout::new(temp.path());
        prepare_release(&layout, "r1", "<html>v1</html>");
        // 无论走 link 还是 copy，最终都必须能读到内容
        switch(&layout, "r1", LinkStrategy::Auto).unwrap();
        assert_eq!(
            fs::read_to_string(layout.current().join("index.html")).unwrap(),
            "<html>v1</html>"
        );
    }
}
