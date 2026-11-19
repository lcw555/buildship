//! 凭据访问：Token / 密码只进系统凭据库，SQLite 只存引用。
//!
//! Windows 使用 Credential Manager，macOS 使用 Keychain（由 keyring crate 抽象）。

use buildship_common::{AppError, AppResult, ErrorCode};

const SERVICE: &str = "buildship.agent";

fn entry(label: &str) -> AppResult<keyring::Entry> {
    keyring::Entry::new(SERVICE, label).map_err(|err| {
        AppError::new(
            ErrorCode::CredentialUnavailable,
            format!("无法访问系统凭据库：{err}"),
        )
    })
}

/// 写入 / 覆盖凭据
pub fn save(label: &str, secret: &str) -> AppResult<()> {
    if label.trim().is_empty() {
        return Err(AppError::invalid("凭据名称不能为空"));
    }
    if secret.trim().is_empty() {
        return Err(AppError::invalid("凭据内容不能为空"));
    }
    entry(label)?.set_password(secret).map_err(|err| {
        AppError::new(
            ErrorCode::CredentialUnavailable,
            format!("写入系统凭据失败：{err}"),
        )
    })
}

/// 读取凭据
pub fn load(label: &str) -> AppResult<String> {
    entry(label)?.get_password().map_err(|err| {
        AppError::new(
            ErrorCode::CredentialUnavailable,
            format!("读取系统凭据失败：{err}"),
        )
    })
}

/// 删除凭据（不存在时忽略）
pub fn delete(label: &str) -> AppResult<()> {
    match entry(label)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(AppError::new(
            ErrorCode::CredentialUnavailable,
            format!("删除系统凭据失败：{err}"),
        )),
    }
}
