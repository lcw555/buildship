//! 构建域：工具链探测、项目扫描、构建子进程执行。

pub mod process;
pub mod runner;
pub mod scanner;
pub mod toolchain;

pub use runner::{BuildCancel, BuildMessage, BuildOutcome, BuildRequest, BuildRunner};
