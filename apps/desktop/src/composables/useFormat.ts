import type {
  BuildStatus,
  DeploymentStatus,
  EnvironmentKind,
  HealthStatus,
  ReleaseStatus,
} from "@/types/ipc";

const STATUS_TEXT: Record<string, string> = {
  queued: "排队中",
  running: "构建中",
  succeeded: "成功",
  failed: "失败",
  cancelled: "已取消",
  draft: "草稿",
  uploading: "上传中",
  deploying: "部署中",
  health_check: "健康检查",
  healthy: "正常",
  rolled_back: "已回滚",
  unknown: "未知",
  passed: "通过",
  skipped: "跳过",
};

export function statusText(status: string | null | undefined): string {
  if (!status) return "未构建";
  return STATUS_TEXT[status] ?? status;
}

/** 状态 → 语义色（成功绿、失败红、进行中蓝） */
export function statusTone(
  status: string | null | undefined,
): "success" | "danger" | "warning" | "info" | "neutral" {
  switch (status) {
    case "succeeded":
    case "healthy":
    case "passed":
      return "success";
    case "failed":
      return "danger";
    case "cancelled":
    case "rolled_back":
      return "warning";
    case "running":
    case "building":
    case "uploading":
    case "deploying":
    case "health_check":
    case "queued":
      return "info";
    default:
      return "neutral";
  }
}

export function formatBytes(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined || Number.isNaN(bytes)) return "-";
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value >= 100 ? 0 : 1)} ${units[unit]}`;
}

export function formatDuration(ms: number | null | undefined): string {
  if (ms === null || ms === undefined) return "-";
  if (ms < 1000) return `${ms} ms`;
  const seconds = ms / 1000;
  if (seconds < 60) return `${seconds.toFixed(1)} s`;
  const minutes = Math.floor(seconds / 60);
  const rest = Math.round(seconds % 60);
  return `${minutes} 分 ${rest} 秒`;
}

export function formatTime(value: string | null | undefined): string {
  if (!value) return "-";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(
    date.getHours(),
  )}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

export function formatRelative(value: string | null | undefined): string {
  if (!value) return "-";
  const date = new Date(value).getTime();
  if (Number.isNaN(date)) return "-";
  const diff = Date.now() - date;
  if (diff < 60_000) return "刚刚";
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)} 分钟前`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)} 小时前`;
  return `${Math.floor(diff / 86_400_000)} 天前`;
}

export function shortSha(sha: string | null | undefined): string {
  if (!sha) return "-";
  return sha.slice(0, 12);
}

export const ENV_KIND_TEXT: Record<EnvironmentKind, string> = {
  dev: "开发",
  test: "测试",
  staging: "预发布",
  prod: "生产",
};

export const ACTION_TEXT: Record<string, string> = {
  project_added: "添加项目",
  project_updated: "修改项目",
  project_archived: "归档项目",
  project_rescanned: "重新扫描",
  build_started: "启动构建",
  build_succeeded: "构建成功",
  build_failed: "构建失败",
  build_cancelled: "取消构建",
  artifact_created: "生成发布包",
  artifact_verified: "校验发布包",
  artifact_deleted: "删除发布包",
  environment_saved: "保存环境",
  environment_archived: "归档环境",
  release_created: "创建发布",
  release_succeeded: "发布成功",
  release_failed: "发布失败",
  release_rolled_back: "版本回滚",
  rollback_failed: "回滚失败",
  settings_updated: "修改设置",
  backup_created: "创建备份",
  backup_restored: "恢复备份",
  toolchain_checked: "工具链体检",
};

export function actionText(action: string): string {
  return ACTION_TEXT[action] ?? action;
}

export function buildStatusOptions(): { label: string; value: BuildStatus }[] {
  return [
    { label: "排队中", value: "queued" },
    { label: "构建中", value: "running" },
    { label: "成功", value: "succeeded" },
    { label: "失败", value: "failed" },
    { label: "已取消", value: "cancelled" },
  ];
}

export function releaseStatusText(status: ReleaseStatus): string {
  return statusText(status);
}

export function deploymentStatusText(status: DeploymentStatus): string {
  return statusText(status);
}

export function healthText(status: HealthStatus): string {
  return statusText(status);
}
