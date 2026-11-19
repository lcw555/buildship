//! 子进程输出的文本解码。
//!
//! 构建工具链（npm / yarn / vite / vue-cli）在中文 Windows 上的输出编码并不统一：
//! 一部分工具写 UTF-8，一部分按控制台代码页写 GBK。若一律 `from_utf8_lossy`，
//! GBK 行会变成满屏 `����`。
//!
//! 策略：整行严格按 UTF-8 解码；失败时按 GBK 解码（GBK 能覆盖绝大多数中文
//! Windows 控制台输出）；两者都失败才退化为带替换符的 UTF-8。

use std::borrow::Cow;

/// 解码一行原始字节，去掉行尾换行与 NUL。
pub fn decode_line(bytes: &[u8]) -> String {
    let trimmed = trim_line_ending(bytes);
    if trimmed.is_empty() {
        return String::new();
    }
    decode_bytes(trimmed).into_owned()
}

/// 解码任意字节片段（不裁剪换行），供日志回读等场景复用。
pub fn decode_bytes(bytes: &[u8]) -> Cow<'_, str> {
    match std::str::from_utf8(bytes) {
        Ok(text) => Cow::Borrowed(text),
        Err(_) => {
            let (decoded, _, had_errors) = encoding_rs::GBK.decode(bytes);
            if had_errors {
                // GBK 也解不了：保留替换符，至少不丢行结构
                Cow::Owned(String::from_utf8_lossy(bytes).into_owned())
            } else {
                decoded
            }
        }
    }
}

fn trim_line_ending(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    while end > 0 && matches!(bytes[end - 1], b'\n' | b'\r' | 0) {
        end -= 1;
    }
    let mut start = 0;
    while start < end && bytes[start] == 0 {
        start += 1;
    }
    &bytes[start..end]
}

/// 判断一行是否已经含 ANSI 转义序列（前端据此决定是否走解析分支）。
pub fn has_ansi(text: &str) -> bool {
    text.as_bytes().contains(&0x1b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf8_with_chinese() {
        assert_eq!(decode_line("构建完成\r\n".as_bytes()), "构建完成");
    }

    #[test]
    fn decodes_gbk_with_chinese() {
        // "构建失败" 的 GBK 字节
        let gbk = [0xB9, 0xB9, 0xBD, 0xA8, 0xCA, 0xA7, 0xB0, 0xDC];
        assert_eq!(decode_line(&gbk), "构建失败");
    }

    #[test]
    fn keeps_ansi_sequences() {
        let raw = b"\x1b[42m\x1b[30m DONE \x1b[39m\x1b[49m Build complete.";
        let text = decode_line(raw);
        assert!(has_ansi(&text));
        assert!(text.ends_with("Build complete."));
    }

    #[test]
    fn strips_embedded_nul() {
        assert_eq!(decode_line(b"\0\0abc\0"), "abc");
    }

    #[test]
    fn empty_input_is_empty() {
        assert_eq!(decode_line(b"\r\n"), "");
    }
}
