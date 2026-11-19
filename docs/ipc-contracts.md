# Tauri IPC 契约

> 对应方案 16 章"第一步"：Tauri IPC 契约。
> 渲染层与 Rust 的唯一通道：`invoke(command, args)` 调用命令，`listen(event)` 订阅事件。
> 所有命令统一返回 `Result<T, AppError>`，失败时 `invoke` 的 reject 值是 `{ code, message, detail? }`。

## 1. 命名与序列化约定

| 约定 | 说明 |
| --- | --- |
| 命令名 | `snake_case`，前缀表示领域：`system_* / project_* / build_* / artifact_* / env_* / target_* / release_* / window_* / floating_*` |
| 参数名 | Rust 侧 `snake_case`，前端 `invoke` 传 `camelCase`（Tauri 自动转换）；本工程统一用**单对象参数** `input` / `query`，避免位置参数顺序错误 |
| 返回值 | 实体与 DTO 全部 `#[serde(rename_all = "camelCase")]`，前端类型见 `src/types/ipc.ts` |
| 时间 | 字符串，UTC RFC3339 毫秒；展示层用 `useFormat.formatTime` 转本地时间 |
| 分页 | `Page<T> { items, total, hasMore }`，`build_list` / `artifact_list` / `audit_list` / `release_list` 使用 `limit` + `offset` |

## 2. 错误码

前端按 `code` 分支，不解析文案。完整枚举见 `buildship-common::error::ErrorCode`：

| code | 触发场景 | 前端建议处理 |
| --- | --- | --- |
| `INVALID_INPUT` | 参数为空、路径非法、字段越界 | 表单内联提示 |
| `NOT_FOUND` | 项目/任务/发布单不存在（可能已被删除） | 刷新列表 |
| `CONFLICT` | slug 或路径重复、重复添加 | 提示改名或复用已有项目 |
| `PERMISSION_DENIED` | 目标目录不可写 | 引导换目录或以管理员运行 |
| `TOOLCHAIN_MISSING` | 未检测到 Node.js / 包管理器 | 跳转设置页展示安装指引 |
| `BUILD_ALREADY_RUNNING` | 该项目已有 queued/running 任务 | 提示查看当前任务并允许取消 |
| `PROCESS_FAILED` | 子进程非零退出 | 展开日志定位 |
| `CANCELLED` | 用户取消 | 静默 |
| `IO_ERROR` / `DB_ERROR` | 磁盘或数据库异常 | 展示 detail，建议备份 |
| `NETWORK_ERROR` / `AGENT_UNAVAILABLE` | Agent 不可达 | 跳转环境配置，提示检测连接 |
| `CHECKSUM_MISMATCH` | 发布包哈希/manifest 不一致 | 阻断发布，要求重新打包 |
| `HEALTH_CHECK_FAILED` | 上线后健康检查失败 | 展示 Agent 回滚结果 |
| `STATE_INVALID` | 非法状态流转（如回滚一个未部署的版本） | 刷新详情 |
| `CREDENTIAL_UNAVAILABLE` | 凭据缺失或 keyring 读取失败 | 引导重新填写 Token |
| `UNIMPLEMENTED` | 明确未实现的功能 | 提示版本计划 |
| `INTERNAL` | 兜底 | 展示"请查看日志" |

## 3. 命令清单（52 个）

### 3.1 系统与设置（14）

| 命令 | 参数 | 返回 | 说明 |
| --- | --- | --- | --- |
| `system_bootstrap` | — | `BootstrapInfo` | 启动引导：数据目录、DB 版本、工具链、设置、选中项目、悬浮栏状态 |
| `system_toolchain` | — | `ToolchainReport` | 探测 node / npm / pnpm / yarn / git，附安装指引 |
| `dashboard_summary` | — | `DashboardSummary` | 首页统计：项目数、今日构建、成功率、最近发布 |
| `settings_get` | — | `AppSetting[]` | 读取全部设置项 |
| `settings_update` | `patch: SettingsPatch` | `AppSetting[]` | 只更新传入的键，其余保持不变 |
| `audit_list` | `query: AuditQuery` | `Page<AuditEvent>` | 审计查询（按动作/项目/时间范围） |
| `backup_list` | — | `BackupEntry[]` | 列出备份文件（名称、大小、时间、schema 版本） |
| `backup_create` | — | `BackupResult` | `VACUUM INTO` 生成一致性备份 |
| `backup_restore` | `path: string` | `BackupResult` | 校验 → 安全备份 → 关池 → 替换 → 重开迁移 → 完整性检查 |
| `secret_list` | — | `SecretReference[]` | 凭据引用列表（**不含明文**） |
| `secret_save` | `label: string, token: string` | `SecretReference` | 写入系统凭据管理器，库里只存引用 |
| `file_sha256` | `path: string` | `string` | 计算任意文件哈希（拖入校验用） |
| `open_data_dir` | — | `void` | 用系统文件管理器打开数据目录 |
| `runtime_info` | — | `json` | 版本、平台、DB 路径、日志路径（"关于"页用） |

### 3.2 项目（8）

| 命令 | 参数 | 返回 | 说明 |
| --- | --- | --- | --- |
| `project_list` | `status?: ProjectStatus` | `Project[]` | 默认只返回 active，归档项目需显式查询 |
| `project_scan` | `path: string` | `ScanResult` | **只读**扫描：框架、包管理器、建议脚本、git commit、产物目录，不写库 |
| `project_add` | `input: AddProjectInput` | `Project` | 事务内建项目 + 默认构建配置 + 审计；归档同名路径会复活原记录 |
| `project_update` | `input: UpdateProjectInput` | `Project` | 改名/改产物目录/改脚本 |
| `project_archive` | `id: string` | `void` | 软删除（`status=archived`），保留历史构建与审计 |
| `project_profiles` | `project_id: string` | `BuildProfile[]` | 构建配置列表（含默认标记） |
| `quick_state_get` | — | `QuickState` | 悬浮栏状态：当前项目、构建状态、最近结果 |
| `quick_set_project` | `project_id: string` | `QuickState` | 切换悬浮栏快捷构建目标 |

### 3.3 构建（6）

| 命令 | 参数 | 返回 | 说明 |
| --- | --- | --- | --- |
| `build_start` | `project_id: string` | `BuildTask` | 工具链检查 → 并发互斥 → 事务建任务 → 异步执行；立即返回任务 |
| `build_cancel` | `task_id: string` | `void` | 取消运行中任务，整进程树终止（`taskkill /T /F` / `kill(-pid)`） |
| `build_list` | `query: BuildQuery` | `Page<BuildTask>` | 支持按项目、状态、时间范围过滤 |
| `build_get` | `task_id: string` | `BuildTask` | 单任务详情（退出码、耗时、错误） |
| `build_logs` | `query: LogQuery { taskId, offset, limit?, tail? }` | `BuildLogChunk` | 从日志文件按段读取，带 `stream` 标记（stdout/stderr）；`tail: true` 直接取末尾 N 行（首次打开日志页用，避免从上万行头部开始） |
| `build_open_log_dir` | `task_id: string` | `void` | 打开该任务的日志目录 |

### 3.4 发布包（6）

| 命令 | 参数 | 返回 | 说明 |
| --- | --- | --- | --- |
| `artifact_list` | `project_id?: string, limit?: number` | `Artifact[]` | 按创建时间倒序 |
| `artifact_create` | `build_task_id: string` | `Artifact` | 校验产物目录 → 打包（ZIP + manifest + SHA-256）→ 事务写库 + 审计 |
| `artifact_get` | `id: string` | `Artifact` | 包详情（哈希、文件数、树哈希、manifest 路径） |
| `artifact_verify` | `id: string` | `ArtifactVerifyResult` | 重算哈希并与 manifest 逐文件比对 |
| `artifact_delete` | `id: string` | `void` | 被 Release 引用时**拒绝删除**；否则标记 `purged` 并删文件 |
| `artifact_open_dir` | `id: string` | `void` | 打开包所在目录 |

### 3.5 发布（11）

| 命令 | 参数 | 返回 | 说明 |
| --- | --- | --- | --- |
| `env_list` | `include_archived?: boolean` | `EnvironmentWithTargets[]` | 环境 + 其下站点一起返回，减少往返 |
| `env_save` | `input: SaveEnvironmentInput` | `DeploymentEnvironment` | 新建或更新（按 id 是否存在） |
| `env_archive` | `id: string` | `void` | 归档环境 |
| `target_save` | `input: SaveTargetInput` | `DeploymentTarget` | 站点地址、物理路径、健康检查地址、凭据引用、TLS 校验开关 |
| `target_archive` | `id: string` | `void` | 归档站点 |
| `target_check` | `id: string` | `AgentStatus` | 读取 keyring 凭据后探测 `/v1/status`，返回 Agent 概览 |
| `release_list` | `project_id?, environment_id?, limit?` | `Page<ReleaseDetail>` | 发布记录 |
| `release_detail` | `id: string` | `ReleaseDetail` | 含部署任务与事件时间线 |
| `release_create` | `input: CreateReleaseInput` | `ReleaseDetail` | 校验包 → Agent 在线 → 事务建 Release + Deployment + 事件 + 审计 → 异步上传应用 |
| `release_rollback` | `input: RollbackInput` | `ReleaseDetail` | **必须填原因**；二次确认在前端完成，原因落审计 |
| `release_healthy_candidates` | `project_id, environment_id` | `ReleaseDetail[]` | 可回滚的健康版本列表 |

### 3.6 窗口与悬浮栏（5）

| 命令 | 参数 | 返回 | 说明 |
| --- | --- | --- | --- |
| `window_show_main` | — | `void` | 显示并聚焦主面板（托盘/悬浮栏调用） |
| `window_hide_main` | — | `void` | 隐藏到托盘（不退出进程） |
| `floating_expand` | — | `void` | 展开为 372×104 胶囊并贴边修正 |
| `floating_collapse` | — | `void` | 收起为 104×104 图标 |
| `floating_set_ignore_cursor` | `ignore: boolean` | `void` | 控制鼠标穿透；由 Rust 侧 120ms 热区轮询驱动 |

### 3.7 维护（2）

| 命令 | 参数 | 返回 | 说明 |
| --- | --- | --- | --- |
| `maintenance_clean` | `backup_first: boolean` | `CleanSummary` | 停构建 → 删 keyring 凭据 → 关池 → 删除数据目录 → 重建骨架 → 自动重启 |
| `maintenance_uninstall` | `backup_first: boolean` | `CleanSummary` | 同上，再以 `/S` 启动安装目录的 `uninstall.exe`；找不到卸载器时只清数据 |

- `CleanSummary { dataDir, removed[], failed[], freedBytes, backupPath, uninstaller, message }`
- 清理前有路径形状校验：只接受 `<已存在父目录>/Buildship`，拒绝更危险的路径
- 备份导出目标为桌面（数据目录随后即被删除，备份必须挪出该目录）

## 4. 事件契约

事件是**单向推送**，只用于"通知状态变了"，不承载需要保证送达的数据；
前端收到事件后通过命令重新拉取权威数据，避免事件丢包导致状态不一致。

| 事件名 | 载荷 | 触发时机 |
| --- | --- | --- |
| `build://status` | `BuildEvent { taskId, kind: "queued"\|"running"\|"log"\|"finished"\|"failed"\|"cancelled", status?, exitCode?, message?, line?: LogLine }` | 构建状态变化与每行日志 |
| `deploy://status` | `DeployEvent { releaseId, kind: "uploading"\|"applying"\|"deployed"\|"failed"\|"rolled_back", message? }` | 发布/回滚过程 |
| `quick://state` | `QuickState` | 悬浮栏状态变化（构建开始/结束、切换项目） |
| `toast://message` | `{ level: "error"\|"warning"\|"info", message: string }` | 后台任务失败但用户不在对应页面 |
| `navigate://route` | `{ route: string }` | 悬浮栏点击"面板/发布包"等快捷入口，主面板跟随跳转 |

### 日志事件节流

`build://status` 的 `kind: "log"` 高频，前端 `builds` store 有上限保护：

- 单任务内存保留最近 **5000 行**，超出丢弃最旧；
- 界面只渲染末尾 **1200 行**（提示总行数），SGR 解析结果按 `taskId:seq` 缓存；
- 日志正文保留原始 ANSI 转义序列，由前端 `useAnsi` 转成着色 span；非 UTF-8 字节在后端按 GBK 兜底解码；
- 界面滚动到底部才自动跟随，用户上滚则暂停跟随；
- 完整日志始终可从 `build_logs` 分页回读，不依赖内存。

## 5. capabilities（最小权限）

`apps/desktop/src-tauri/capabilities/default.json` 只放真正需要的权限：

```json
"permissions": [
  "core:default",
  "core:event:allow-emit",
  "core:event:allow-emit-to",
  "core:window:allow-start-dragging", "core:window:allow-set-position",
  "core:window:allow-set-size", "core:window:allow-set-always-on-top",
  "core:window:allow-show", "core:window:allow-hide",
  "core:window:allow-set-focus", "core:window:allow-unminimize",
  "dialog:allow-open",
  "opener:allow-open-path"
]
```

**没有** `shell:*`、`fs:*`、`http:*`：前端无法直接执行系统命令、读写任意文件或发网络请求，
所有能力必须经过 Rust 侧的服务层校验。

## 6. 前端调用约定

```ts
// composables/useIpc.ts
export async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new IpcCallError({ code: "INTERNAL", message: "当前不在桌面环境中运行" });
  try {
    return await invoke<T>(command, args);
  } catch (raw) {
    throw new IpcCallError(normalizeError(raw));
  }
}
```

- 组件不直接 `invoke`，统一走 store action → `call()`；
- store 捕获 `IpcCallError` 后调用 `useMessage()` 展示，并按 `code` 决定是否刷新列表；
- 需要"尽最大努力"的操作（刷新列表、轮询状态）必须静默降级，不弹错误。
