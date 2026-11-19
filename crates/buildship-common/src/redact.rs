//! 日志脱敏。
//!
//! 构建日志来自用户的构建脚本，可能打印 `.env` 内容、Token、数据库连接串。
//! 方案 5.5 / 12 要求日志不得输出敏感信息，因此在写入日志文件与推送到 UI 之前
//! 统一过一遍脱敏规则。

const SENSITIVE_KEYS: [&str; 12] = [
    "password",
    "passwd",
    "pwd",
    "secret",
    "token",
    "authorization",
    "apikey",
    "api_key",
    "access_key",
    "private_key",
    "credential",
    "cookie",
];

/// HTTP 认证方案词：`Authorization: Bearer xxx` 里的 `Bearer` 不是密钥本体，
/// 但它后面的段一定是，需要连带屏蔽。
const AUTH_SCHEMES: [&str; 5] = ["bearer", "basic", "token", "digest", "jwt"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    /// 无需额外处理
    None,
    /// 下一段整体脱敏（`--token abc` 中的 `abc`）
    MaskOne,
    /// 下一段若为认证方案词，则再连带屏蔽其后的值
    MaskScheme,
}

enum Segment {
    /// 普通文本，原样保留
    Plain(String),
    /// 形如 `PASSWORD=123`，值与键在同一段，已就地脱敏
    Masked(String),
    /// 形如 `--token`，其值在下一段
    BareKey(String),
    /// 形如 `Authorization:`，值在后续段
    KeyOnly(String),
}

fn contains_key(segment: &str) -> Option<usize> {
    let lower = segment.to_ascii_lowercase();
    SENSITIVE_KEYS
        .iter()
        .filter_map(|key| lower.find(key))
        .min()
}

fn classify(segment: &str) -> Segment {
    let Some(key_at) = contains_key(segment) else {
        return Segment::Plain(segment.to_string());
    };
    let sep = segment[key_at..]
        .char_indices()
        .find(|(_, c)| *c == ':' || *c == '=')
        .map(|(idx, _)| key_at + idx);
    match sep {
        // 分隔符就在末尾：真正的值在后面的段里
        Some(pos) if pos + 1 == segment.len() => Segment::KeyOnly(segment.to_string()),
        Some(pos) => Segment::Masked(format!("{}***", &segment[..=pos])),
        None => Segment::BareKey(segment.to_string()),
    }
}

fn is_auth_scheme(segment: &str) -> bool {
    AUTH_SCHEMES.contains(&segment.trim().to_ascii_lowercase().as_str())
}

fn handle(segment: &str, out: &mut String, pending: &mut Pending) {
    if segment.is_empty() {
        return;
    }
    match *pending {
        Pending::MaskOne | Pending::MaskScheme => {
            out.push_str("***");
            // `Authorization: Bearer xxx` → Bearer 之后继续屏蔽一段
            *pending = if *pending == Pending::MaskScheme && is_auth_scheme(segment) {
                Pending::MaskOne
            } else {
                Pending::None
            };
        }
        Pending::None => match classify(segment) {
            Segment::Plain(text) | Segment::Masked(text) => out.push_str(&text),
            Segment::BareKey(text) => {
                out.push_str(&text);
                *pending = Pending::MaskOne;
            }
            Segment::KeyOnly(text) => {
                out.push_str(&text);
                *pending = Pending::MaskScheme;
            }
        },
    }
}

/// 对单行日志做脱敏，返回可安全写入磁盘 / 推送到界面的文本。
///
/// 规则：按空白与 `& , ;` 切段；
/// - 段内命中敏感键名且带 `:` / `=` 时就地脱敏其值；
/// - 命中键名但值在末尾（`Authorization:`）→ 屏蔽下一段，若下一段是
///   `Bearer`/`Basic` 等方案词则再连带屏蔽一段；
/// - 命中键名但没有分隔符（`--token`）→ 下一段整体替换。
pub fn scrub_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut pending = Pending::None;
    let mut segment = String::new();

    for ch in line.chars() {
        if matches!(ch, ' ' | '\t' | '&' | ',' | ';') {
            handle(&segment, &mut out, &mut pending);
            segment.clear();
            out.push(ch);
        } else {
            segment.push(ch);
        }
    }
    handle(&segment, &mut out, &mut pending);
    out
}

/// 给结构化字段使用：把已知为凭据的值整体替换。
pub fn scrub_secret(value: &str) -> String {
    if value.is_empty() {
        String::new()
    } else {
        "***".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_env_style_assignment() {
        assert_eq!(scrub_line("PASSWORD=123456"), "PASSWORD=***");
    }

    #[test]
    fn keeps_plain_lines() {
        let line = "vite v5.4.0 building for production...";
        assert_eq!(scrub_line(line), line);
    }

    #[test]
    fn masks_authorization_header() {
        let masked = scrub_line("Authorization: Bearer abc.def");
        assert!(!masked.contains("abc.def"), "get {masked}");
        assert!(masked.contains("***"));
    }

    #[test]
    fn masks_basic_auth_without_leaking_payload() {
        let masked = scrub_line("authorization: Basic dXNlcjpwYXNz");
        assert!(!masked.contains("dXNlcjpwYXNz"), "get {masked}");
    }

    #[test]
    fn masks_middle_segment() {
        let masked = scrub_line("connect --token abc123 --verbose");
        assert_eq!(masked, "connect --token *** --verbose");
    }
}
