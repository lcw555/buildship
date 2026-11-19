//! 服务层与 IPC 之间的传输对象（DTO）。
//!
//! 入参结构由前端构造，全部字段显式校验；出参结构直接序列化成 camelCase JSON。
//! 这些结构同时被 Tauri 命令层与事件负载复用，避免出现两套"接口模型"。

use serde::{Deserialize, Serialize};

use crate::enums::*;
use crate::models::*;

// ---------------------------------------------------------------------------
// 入参
// ---------------------------------------------------------------------------

/// 添加项目：仅给出目录，其余字段由扫描结果填充，可再手动修正。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddProjectInput {
    pub path: String,
    /// 自定义显示名；为空则取 package.json 的 name 或目录名
    pub name: Option<String>,
    /// 手动指定包管理器；为空则按 lockfile 推断
    pub package_manager: Option<PackageManager>,
    /// 手动指定产物目录；为空则按常见目录推断
    pub output_dir: Option<String>,
}

/// 编辑项目与其默认构建配置
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProjectInput {
    pub id: String,
    pub name: String,
    pub package_manager: PackageManager,
    pub package_manager_version: Option<String>,
    pub build_script: Option<String>,
    #[serde(default)]
    pub build_args: Vec<String>,
    pub output_dir: String,
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
}

/// 保存发布环境
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveEnvironmentInput {
    pub id: Option<String>,
    pub name: String,
    pub kind: EnvironmentKind,
    pub description: Option<String>,
}

/// 保存部署目标（IIS 站点 + Agent）
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveTargetInput {
    pub id: Option<String>,
    pub environment_id: String,
    pub name: String,
    pub server_host: Option<String>,
    pub agent_base_url: String,
    pub iis_site: String,
    pub physical_path: String,
    pub health_check_url: Option<String>,
    /// 凭据引用 ID；Token 正文存系统凭据库
    pub credential_ref: Option<String>,
    #[serde(default = "default_true")]
    pub tls_verify: bool,
}

fn default_true() -> bool {
    true
}

/// 生成发布包
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateArtifactInput {
    pub build_task_id: String,
}

/// 创建发布
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateReleaseInput {
    pub artifact_id: String,
    pub target_id: String,
    /// 版本号；为空时按时间戳生成
    pub version: Option<String>,
}

/// 回滚
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RollbackInput {
    pub target_release_id: String,
    pub reason: String,
}

/// 查询构建任务
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildQuery {
    pub project_id: Option<String>,
    pub status: Option<BuildStatus>,
    pub limit: Option<i64>,
}

/// 拉取日志增量
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogQuery {
    pub task_id: String,
    /// 已消费的行号，从 0 开始
    pub offset: i64,
    pub limit: Option<i64>,
    /// 只取末尾若干行（首次打开日志页时用，避免从上万行头部开始读）
    #[serde(default)]
    pub tail: bool,
}

/// 查询审计
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditQuery {
    pub project_id: Option<String>,
    pub action: Option<AuditAction>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// 设置项批量写入
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    pub entries: std::collections::BTreeMap<String, String>,
}

// ---------------------------------------------------------------------------
// 出参
// ---------------------------------------------------------------------------

/// 单个工具的探测结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub name: String,
    pub available: bool,
    pub version: Option<String>,
    pub path: Option<String>,
    /// 不可用时的安装指引
    pub hint: Option<String>,
}

/// 工具链体检报告
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolchainReport {
    pub node: ToolStatus,
    pub package_managers: Vec<ToolStatus>,
    pub checked_at: chrono::DateTime<chrono::Utc>,
}

/// 项目扫描结果（纯读取，不执行任何项目脚本）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub path: String,
    pub name: String,
    pub slug: String,
    pub framework: Option<String>,
    pub package_manager: PackageManager,
    pub scripts: Vec<String>,
    pub suggested_script: Option<String>,
    pub suggested_output_dir: String,
    /// 该目录下真实存在的候选产物目录
    pub candidate_output_dirs: Vec<String>,
    pub node_version: Option<String>,
    pub git_commit: Option<String>,
    pub warnings: Vec<String>,
}

/// 日志行
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub seq: i64,
    pub stream: LogStream,
    pub text: String,
}

/// 日志增量
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildLogChunk {
    pub task_id: String,
    pub lines: Vec<LogLine>,
    pub next_offset: i64,
    /// 任务已结束且没有更多日志
    pub eof: bool,
}

/// 启动引导信息：桌面端启动后前端第一件事就是拿它
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapInfo {
    pub app_version: String,
    pub platform: String,
    pub data_dir: String,
    pub db_path: String,
    pub schema_version: i64,
    pub toolchain: ToolchainReport,
    pub settings: Vec<AppSetting>,
    pub operator: String,
}

/// 工作台汇总
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardSummary {
    pub project_count: i64,
    pub active_build_count: i64,
    pub artifact_count: i64,
    pub healthy_release_count: i64,
    pub total_artifact_bytes: i64,
    pub recent_projects: Vec<Project>,
    pub recent_builds: Vec<BuildTask>,
    pub recent_releases: Vec<Release>,
}

/// 发布详情：时间线 + 关联对象
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseDetail {
    pub release: Release,
    pub events: Vec<ReleaseEvent>,
    pub artifact: Option<Artifact>,
    pub environment: Option<DeploymentEnvironment>,
    pub target: Option<DeploymentTarget>,
    pub deployment_task: Option<DeploymentTask>,
    pub previous: Option<Release>,
}

/// 环境 + 目标聚合（前端环境页一次拿全）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentWithTargets {
    pub environment: DeploymentEnvironment,
    pub targets: Vec<DeploymentTarget>,
}

/// 发布包完整性校验结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactVerifyResult {
    pub artifact_id: String,
    pub ok: bool,
    pub expected_sha256: String,
    pub actual_sha256: Option<String>,
    pub message: String,
    pub checked_at: chrono::DateTime<chrono::Utc>,
}

/// 备份文件条目
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    pub file_name: String,
    pub path: String,
    pub size_bytes: i64,
    pub sha256: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// 备份/恢复结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupResult {
    pub entry: BackupEntry,
    pub message: String,
}

/// 分页结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: i64,
}

// ---------------------------------------------------------------------------
// 事件负载
// ---------------------------------------------------------------------------

/// 构建事件：`build://status` / `build://log`
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildEvent {
    pub task_id: String,
    pub project_id: String,
    pub status: BuildStatus,
    pub seq: i64,
    pub line: Option<LogLine>,
    pub exit_code: Option<i64>,
    pub message: Option<String>,
}

/// 发布/部署事件：`deploy://status`
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeployEvent {
    pub release_id: String,
    pub deployment_task_id: String,
    pub status: DeploymentStatus,
    pub release_status: ReleaseStatus,
    pub stage: Option<String>,
    pub progress: i64,
    pub message: Option<String>,
    pub error_code: Option<String>,
}

/// 快捷栏状态：`quick://state`
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickState {
    pub has_project: bool,
    pub active_build: bool,
    pub active_task_id: Option<String>,
    pub project_name: Option<String>,
    pub project_id: Option<String>,
    pub last_build_status: Option<BuildStatus>,
}
