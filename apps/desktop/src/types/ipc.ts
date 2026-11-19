/**
 * 与 Rust 侧 DTO 一一对应的类型定义。
 *
 * 约定：Rust 结构体统一 `#[serde(rename_all = "camelCase")]`，
 * 枚举统一 snake_case 字符串。新增字段时两边同时改，避免"前端多一个字段就崩"。
 */

export type PackageManager = "npm" | "pnpm" | "yarn";
export type RecordStatus = "active" | "archived";
export type BuildStatus = "queued" | "running" | "succeeded" | "failed" | "cancelled";
export type LogStream = "stdout" | "stderr" | "system";
export type ArtifactStatus = "ready" | "corrupt" | "missing" | "purged";
export type EnvironmentKind = "dev" | "test" | "staging" | "prod";
export type ReleaseStatus =
  | "draft"
  | "uploading"
  | "deploying"
  | "health_check"
  | "healthy"
  | "failed"
  | "rolled_back"
  | "cancelled";
export type DeploymentKind = "deploy" | "rollback";
export type DeploymentStatus =
  | "queued"
  | "uploading"
  | "deploying"
  | "health_check"
  | "succeeded"
  | "failed"
  | "cancelled";
export type HealthStatus = "unknown" | "passed" | "failed" | "skipped";
export type EventLevel = "info" | "warn" | "error";

export interface Project {
  id: string;
  name: string;
  slug: string;
  path: string;
  packageManager: PackageManager;
  packageManagerVersion: string | null;
  nodeVersion: string | null;
  framework: string | null;
  status: RecordStatus;
  lastBuildStatus: BuildStatus | null;
  lastBuildAt: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface BuildProfile {
  id: string;
  projectId: string;
  name: string;
  buildScript: string | null;
  buildCommand: string;
  outputDir: string;
  envJson: string;
  isDefault: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface BuildTask {
  id: string;
  projectId: string;
  profileId: string | null;
  status: BuildStatus;
  command: string;
  argsJson: string;
  exitCode: number | null;
  startedAt: string | null;
  finishedAt: string | null;
  durationMs: number | null;
  logPath: string | null;
  logSize: number | null;
  logSha256: string | null;
  errorSummary: string | null;
  triggeredBy: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface Artifact {
  id: string;
  projectId: string;
  buildTaskId: string;
  fileName: string;
  filePath: string;
  sizeBytes: number;
  sha256: string;
  fileCount: number;
  manifestPath: string;
  manifestSha256: string | null;
  status: ArtifactStatus;
  gitCommit: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface DeploymentEnvironment {
  id: string;
  name: string;
  kind: EnvironmentKind;
  description: string | null;
  status: RecordStatus;
  createdAt: string;
  updatedAt: string;
}

export interface DeploymentTarget {
  id: string;
  environmentId: string;
  name: string;
  serverHost: string | null;
  agentBaseUrl: string;
  iisSite: string;
  physicalPath: string;
  healthCheckUrl: string | null;
  credentialRef: string | null;
  tlsVerify: boolean;
  status: RecordStatus;
  lastCheckedAt: string | null;
  lastCheckOk: boolean | null;
  createdAt: string;
  updatedAt: string;
}

export interface DeploymentTask {
  id: string;
  releaseId: string;
  targetId: string;
  projectId: string;
  kind: DeploymentKind;
  status: DeploymentStatus;
  stage: string | null;
  progress: number;
  requestId: string;
  idempotencyKey: string | null;
  errorCode: string | null;
  errorMessage: string | null;
  startedAt: string | null;
  finishedAt: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface Release {
  id: string;
  projectId: string;
  artifactId: string;
  environmentId: string;
  targetId: string;
  version: string;
  deployPath: string | null;
  status: ReleaseStatus;
  previousReleaseId: string | null;
  rolledBackFrom: string | null;
  rollbackReason: string | null;
  healthStatus: HealthStatus;
  healthMessage: string | null;
  isCurrent: boolean;
  operator: string | null;
  startedAt: string | null;
  finishedAt: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface ReleaseEvent {
  id: string;
  releaseId: string;
  seq: number;
  phase: string;
  level: EventLevel;
  message: string;
  detailJson: string | null;
  createdAt: string;
}

export interface AuditEvent {
  id: string;
  actor: string;
  action: string;
  entityType: string;
  entityId: string;
  projectId: string | null;
  summary: string;
  detailJson: string | null;
  createdAt: string;
}

export interface AppSetting {
  key: string;
  value: string;
  updatedAt: string;
}

export interface ToolStatus {
  name: string;
  available: boolean;
  version: string | null;
  path: string | null;
  hint: string | null;
}

export interface ToolchainReport {
  node: ToolStatus;
  packageManagers: ToolStatus[];
  checkedAt: string;
}

export interface ScanResult {
  path: string;
  name: string;
  slug: string;
  framework: string | null;
  packageManager: PackageManager;
  scripts: string[];
  suggestedScript: string | null;
  suggestedOutputDir: string;
  candidateOutputDirs: string[];
  nodeVersion: string | null;
  gitCommit: string | null;
  warnings: string[];
}

export interface LogLine {
  seq: number;
  stream: LogStream;
  text: string;
}

export interface BuildLogChunk {
  taskId: string;
  lines: LogLine[];
  nextOffset: number;
  eof: boolean;
}

export interface BootstrapInfo {
  appVersion: string;
  platform: string;
  dataDir: string;
  dbPath: string;
  schemaVersion: number;
  toolchain: ToolchainReport;
  settings: AppSetting[];
  operator: string;
}

export interface DashboardSummary {
  projectCount: number;
  activeBuildCount: number;
  artifactCount: number;
  healthyReleaseCount: number;
  totalArtifactBytes: number;
  recentProjects: Project[];
  recentBuilds: BuildTask[];
  recentReleases: Release[];
}

export interface ReleaseDetail {
  release: Release;
  events: ReleaseEvent[];
  artifact: Artifact | null;
  environment: DeploymentEnvironment | null;
  target: DeploymentTarget | null;
  deploymentTask: DeploymentTask | null;
  previous: Release | null;
}

export interface EnvironmentWithTargets {
  environment: DeploymentEnvironment;
  targets: DeploymentTarget[];
}

export interface ArtifactVerifyResult {
  artifactId: string;
  ok: boolean;
  expectedSha256: string;
  actualSha256: string | null;
  message: string;
  checkedAt: string;
}

export interface BackupEntry {
  fileName: string;
  path: string;
  sizeBytes: number;
  sha256: string;
  createdAt: string;
}

export interface BackupResult {
  entry: BackupEntry;
  message: string;
}

/** 凭据存储位置（对应 Rust `SecretProvider`，snake_case 序列化） */
export type SecretProvider = "windows_credential_manager" | "macos_keychain" | "memory";

/** 凭据引用：只含元数据，**不含明文** */
export interface SecretReference {
  id: string;
  label: string;
  provider: SecretProvider;
  account: string;
  createdAt: string;
  lastUsedAt: string | null;
}

export interface QuickState {
  hasProject: boolean;
  activeBuild: boolean;
  activeTaskId: string | null;
  projectName: string | null;
  projectId: string | null;
  lastBuildStatus: BuildStatus | null;
}

export interface BuildEvent {
  taskId: string;
  projectId: string;
  status: BuildStatus;
  seq: number;
  line: LogLine | null;
  exitCode: number | null;
  message: string | null;
}

export interface DeployEvent {
  releaseId: string;
  deploymentTaskId: string;
  status: DeploymentStatus;
  releaseStatus: ReleaseStatus;
  stage: string | null;
  progress: number;
  message: string | null;
  errorCode: string | null;
}

export interface IpcError {
  code: string;
  message: string;
  detail?: string;
}

export interface AddProjectInput {
  path: string;
  name?: string | null;
  packageManager?: PackageManager | null;
  outputDir?: string | null;
}

export interface UpdateProjectInput {
  id: string;
  name: string;
  packageManager: PackageManager;
  packageManagerVersion?: string | null;
  buildScript?: string | null;
  buildArgs: string[];
  outputDir: string;
  env: Record<string, string>;
}

export interface SaveEnvironmentInput {
  id?: string | null;
  name: string;
  kind: EnvironmentKind;
  description?: string | null;
}

export interface SaveTargetInput {
  id?: string | null;
  environmentId: string;
  name: string;
  serverHost?: string | null;
  agentBaseUrl: string;
  iisSite: string;
  physicalPath: string;
  healthCheckUrl?: string | null;
  credentialRef?: string | null;
  tlsVerify: boolean;
}

/** 清理 / 卸载的执行结果 */
export interface CleanSummary {
  dataDir: string;
  removed: string[];
  failed: string[];
  freedBytes: number;
  backupPath: string | null;
  uninstaller: string | null;
  message: string;
}
