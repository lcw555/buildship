# 部署协议（桌面端 ↔ Deploy Agent）

> 协议结构定义在 `crates/buildship-release/src/protocol.rs`，**两端共用同一份代码**，
> 避免文档与实现漂移。客户端实现在 `crates/buildship-release/src/client.rs`，
> 服务端实现在 `deploy-agent/src/routes.rs`。

## 1. 传输与认证

| 项 | 值 |
| --- | --- |
| 传输 | HTTP/1.1（TLS 由前置 nginx / IIS 终止，Agent 自身不处理证书） |
| 认证 | 请求头 `x-buildship-token`，固定时间比较 |
| 协议版本 | 请求头 `x-buildship-api-version: 1`，缺失视为 v1，不匹配直接 400 |
| 请求追踪 | 请求头 `x-buildship-request-id`（UUID），服务端原样回传给 `AgentErrorBody.requestId` |
| 超时 | 连接 10s；常规请求 120s；上传 30min；apply/rollback 300s |

`x-buildship-token` 在桌面端**不入库**：`deployment_targets.credential_ref` 指向
`secret_references`，真值存 Windows 凭据管理器 / macOS 钥匙串，调用时临时读取。

## 2. 端点

### 2.1 `GET /v1/status`

```json
{
  "apiVersion": "1",
  "agentVersion": "0.2.5",
  "hostname": "WEB-PROD-01",
  "os": "windows x86_64",
  "uptimeSeconds": 86400,
  "serverTime": "2026-11-18T10:00:00.000Z",
  "sites": [
    {
      "site": "demo",
      "physicalPath": "C:\\sites\\demo",
      "currentRelease": "20261118-101500-a1b2c3",
      "previousRelease": "20261117-090000-9f8e7d",
      "releaseCount": 6,
      "updatedAt": "2026-11-18T10:15:00.000Z"
    }
  ]
}
```

用途：桌面端"检测连接"按钮、发布前在线校验、站点版本概览。

### 2.2 `GET /v1/releases?site=<name>`

返回该站点已部署版本（按上传时间倒序），`isCurrent` 标记线上版本。

### 2.3 `POST /v1/upload`

| 请求头 | 必填 | 说明 |
| --- | --- | --- |
| `x-buildship-release-id` | 是 | 版本 ID，仅允许 `[A-Za-z0-9._-]{1,128}` |
| `x-buildship-sha256` | 是 | 整包 SHA-256（64 位十六进制） |
| `x-buildship-site` | 多站点时必填 | 目标站点名；单站点可省略 |
| `Content-Type` | 是 | `application/zip` |
| `Content-Length` | 是 | 字节数 |

请求体是**原始 ZIP 字节流**（非 multipart），服务端边收边算摘要。

```json
{
  "releaseId": "20261118-101500-a1b2c3",
  "storedPath": "C:\\sites\\demo\\releases\\20261118-101500-a1b2c3",
  "sizeBytes": 1048576,
  "sha256": "…",
  "fileCount": 128,
  "treeSha256": "…",
  "alreadyExisted": false
}
```

校验链：长度上限 → SHA-256 → `manifest.json` 结构 → 逐文件内容 → **zip-slip 路径过滤** → 解压落盘 → 计算 `treeSha256`。

### 2.4 `POST /v1/apply`

```json
{
  "releaseId": "20261118-101500-a1b2c3",
  "site": "demo",
  "physicalPath": "C:\\sites\\demo",
  "healthCheckUrl": "https://demo.example.com/healthz",
  "treeSha256": "…",
  "requestId": "5b0f…"
}
```

服务端步骤：

1. 校验站点名 + `physicalPath` 与配置**逐段相等**（防越权写任意目录）；
2. 校验版本目录存在、元信息存在、`treeSha256` 与记录一致；
3. 站点级互斥锁（同站点不允许并发切换）；
4. 建临时联接 / 复制到 `current.next` → 两步改名替换 `current`；
5. 更新 `state.json`，给版本元信息补 `appliedAt`；
6. 健康检查（`healthCheckUrl` 或站点默认值，10s 超时）；
7. 健康检查失败 **且有上一版本** → 自动回滚，返回 `rolledBack: true`；
8. 写审计；成功则按 `keepReleases` 清理旧版本（保护当前与上一版本）。

```json
{
  "releaseId": "20261118-101500-a1b2c3",
  "previousReleaseId": "20261117-090000-9f8e7d",
  "currentPath": "C:\\sites\\demo\\current",
  "healthStatus": "passed",
  "healthMessage": "HTTP 200，耗时 37 ms",
  "appliedAt": "2026-11-18T10:16:02.000Z",
  "rolledBack": false
}
```

> **重要语义**：健康检查失败但已成功回滚时，Agent 返回 **HTTP 200 + `healthStatus: "failed"` + `rolledBack: true`**。
> 这样桌面端能拿到"已自动恢复"的事实并写进发布事件，而不是只看到一个失败。
> 健康检查失败且**无历史版本可回滚**时，返回 `502 HEALTH_CHECK_FAILED`。

### 2.5 `POST /v1/rollback`

```json
{
  "toReleaseId": "20261117-090000-9f8e7d",
  "site": "demo",
  "physicalPath": "C:\\sites\\demo",
  "healthCheckUrl": "https://demo.example.com/healthz",
  "reason": "线上首页白屏，回退到上一验证版本",
  "requestId": "9c2e…"
}
```

`reason` 必填，落审计。回滚后健康检查仍失败时，Agent 会**再切回原版本**并返回 `rolledBack: true`，
避免把站点留在更差的状态。

### 2.6 `GET /v1/health?url=<encoded>`

由 Agent 代客户端探测（只有服务器才有内网可达视角），返回 `HealthReport { ok, statusCode, message, checkedAt }`。

### 2.7 `GET /healthz`

无鉴权，纯文本 `ok`。给负载均衡 / 容器探活用，**不暴露业务信息**。

## 3. 结构化错误

```json
{ "code": "CHECKSUM_MISMATCH", "message": "发布包 SHA-256 不一致：声明 …，实际 …", "detail": null, "requestId": "5b0f…" }
```

| code | HTTP | 桌面端映射 |
| --- | --- | --- |
| `INVALID_INPUT` | 400 | `INVALID_INPUT` |
| `NOT_FOUND` | 404 | `NOT_FOUND` |
| `CONFLICT` | 409 | `CONFLICT` |
| `PERMISSION_DENIED` / `CREDENTIAL_UNAVAILABLE` | 403 | `PERMISSION_DENIED` / `CREDENTIAL_UNAVAILABLE` |
| `CHECKSUM_MISMATCH` | 422 | `CHECKSUM_MISMATCH` |
| `HEALTH_CHECK_FAILED` | 502 | `HEALTH_CHECK_FAILED` |
| `AGENT_UNAVAILABLE` | 503 | `AGENT_UNAVAILABLE` |
| 其余 | 500 | `AGENT_UNAVAILABLE` |

## 4. 幂等

- `apply` / `rollback` 携带 `requestId`，Agent 在 `.buildship/idempotency/<requestId>.json`
  缓存上一次响应（保留最近 200 条），重试同一请求直接返回原结果，不会重复切换。
- `upload` 以 `releaseId + sha256` 判幂等：同 ID 同内容返回 `alreadyExisted: true`，不重复解压；
  同 ID 但内容不同返回 `409 CONFLICT`，要求换版本号（防止覆盖已部署版本）。

## 5. 服务器目录布局

```text
<physicalPath>/
├── current/                            ← Web 服务器站点根目录指向这里
├── releases/
│   ├── 20261117-090000-9f8e7d/         ← 不可变历史版本
│   └── 20261118-101500-a1b2c3/
└── .buildship/
    ├── state.json                      ← { current, previous, appliedAt }
    ├── releases/<id>.json               ← 版本元信息（哈希/文件数/树哈希）
    ├── archives/<id>.zip                ← 原始包，回滚取证用
    ├── audit.jsonl                      ← 追加式审计
    ├── idempotency/<requestId>.json
    └── tmp/                             ← 上传与解压暂存
```

### 切换方式

| `linkStrategy` | 行为 | 适用 |
| --- | --- | --- |
| `auto`（默认） | 先尝试目录联接，失败自动退回复制 | 绝大多数场景 |
| `link` | 强制联接；不可用时报错并给出改配置建议 | 明确要求零拷贝 |
| `copy` | 完整复制到 `current.next` 再改名 | 无权创建联接、或需跨卷 |

Windows 目录联接通过 `DeviceIoControl(FSCTL_SET_REPARSE_POINT)` 直接创建，
**不需要管理员权限**，也不拼接 `mklink` 命令行。

> 删除 `current` 时先尝试 `remove_dir`（只摘除链接本身），
> 只有确认是真实目录才递归删除 —— 防止把版本内容误删。

## 6. 配置示例

```toml
# deploy-agent.toml
bind = "0.0.0.0:7801"
tokenFile = "C:\\ProgramData\\Buildship\\agent-token.txt"
maxUploadMb = 2048
healthTimeoutSecs = 10
linkStrategy = "auto"

[[sites]]
name = "demo"
physicalPath = "C:\\sites\\demo"
healthCheckUrl = "https://demo.example.com/healthz"
keepReleases = 5
```

启动与自检：

```text
deploy-agent --config deploy-agent.toml --check   # 只校验配置与目录权限
deploy-agent --config deploy-agent.toml
```

## 7. 运维要点

1. **Token 长度至少 16 字符**，建议 32 字节随机值；文件权限限制为仅服务账号可读。
2. Agent 监听 `HTTP`，生产环境必须由反向代理终止 TLS，并限制来源 IP。
3. 站点根目录交给 `current`，不要让人手工往 `current/` 里放文件（下次发布会丢）。
4. 首次发布前先用 `--check` 确认目录可写；Agent 启动时也会做写探针。
5. 回滚历史依赖 `releases/`，`keepReleases` 太小会导致"想回滚但版本已被清理"。
