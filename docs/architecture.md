# Buildship 技术架构

> 版本：V1.0（对应开发方案 16 章"第一步"交付物之一）
> 技术栈：Tauri 2 + Rust + Vue 3 + TypeScript + SQLite

## 1. 总体结构

Buildship 是单机桌面应用，它把"本机构建"与"远程发布"两件事串成一条可审计的流水线。
渲染层只有视图与交互，所有系统能力（子进程、文件、数据库、凭据、网络）都在 Rust 侧。

```mermaid
flowchart TB
    subgraph Desktop["桌面应用进程（单进程）"]
        direction TB
        subgraph WebView["WebView2 / WKWebView"]
            UI1["主面板（index.html）<br/>Dashboard / Projects / Builds<br/>Artifacts / Releases / Audit / Settings"]
            UI2["悬浮栏（floating.html）<br/>Aurora Loop 图标 + 快捷操作"]
        end

        subgraph IPC["Tauri IPC 边界"]
            CMD["52 个 #[tauri::command]"]
            EVT["事件通道<br/>build://status · deploy://status<br/>quick://state · toast://message · navigate://route"]
        end

        subgraph Core["Rust 核心层"]
            SVC["services/*<br/>project · build · artifact · release · settings · quick"]
            PLAT["platform/*<br/>浮动窗 · 光标命中测试"]
            INFRA["infrastructure/*<br/>keyring 凭据"]
            DB["buildship-db<br/>SQLx + SQLite"]
            STATE["state.rs<br/>AppState / BuildManager / QuickContext"]
        end

        subgraph Crates["可复用 crate"]
            COMMON["buildship-common<br/>错误 · 路径 · 时间 · 脱敏 · 日志"]
            DOMAIN["buildship-domain<br/>枚举 · 实体 · 状态机 · DTO"]
            BUILD["buildship-build<br/>工具链 · 扫描 · 子进程 · 构建运行器"]
            ART["buildship-artifact<br/>哈希 · manifest · ZIP 打包校验"]
            REL["buildship-release<br/>协议结构 · HTTP 客户端"]
        end
    end

    subgraph External["进程外"]
        NODE["Node.js / npm / pnpm<br/>仅作为构建工具链"]
        FS["文件系统<br/>产物 · 日志 · 备份"]
        DBF[("buildship.db<br/>SQLite WAL")]
        CRED["Windows 凭据管理器<br/>macOS 钥匙串"]
        AGENT["Deploy Agent<br/>目标服务器 HTTP 服务"]
    end

    UI1 <-->|invoke / listen| IPC
    UI2 <-->|invoke / listen| IPC
    IPC --> SVC
    IPC --> PLAT
    SVC --> Crates
    SVC --> DB
    SVC --> INFRA
    STATE --> SVC
    DB --> DBF
    INFRA --> CRED
    BUILD -->|"CommandSpec 参数数组<br/>不拼 shell 字符串"| NODE
    ART --> FS
    SVC -->|"HTTPS + Token"| AGENT
```

### 关键约束（不可绕过）

| 约束 | 落地方式 |
| --- | --- |
| 前端不直连数据库、不执行命令 | 渲染层只能 `invoke` 白名单命令；capabilities 里没有 shell/fs 插件权限 |
| 构建进程不使用 shell 拼接 | `CommandSpec` 只接受参数数组，`validate_arg` 拒绝 `& \| < > ^ % ! " ' \`` 与换行 |
| 凭据不入库 | 库里只存 `secret_references`（provider + key），真值在 keyring |
| 审计不可篡改 | `audit_events` 上有 `BEFORE UPDATE/DELETE` 触发器，直接 `RAISE(ABORT)` |
| 状态流转受控 | 所有状态变更走 `buildship-domain::state` 校验，非法流转返回 `STATE_INVALID` |

## 2. 分层与依赖方向

依赖是单向的，`domain` 不认识 `db`，`db` 不认识 `services`。

```mermaid
flowchart LR
    L1["apps/desktop（Tauri 外壳）<br/>commands · services · platform"]
    L2["buildship-db<br/>repositories · backup"]
    L3["buildship-build<br/>process · scanner · runner · toolchain"]
    L4["buildship-artifact<br/>hashing · manifest · archive"]
    L5["buildship-release<br/>protocol · client"]
    L6["buildship-domain<br/>enums · models · state · dto"]
    L7["buildship-common<br/>error · paths · time · redact · logging"]

    L1 --> L2 --> L6
    L1 --> L3 --> L6
    L1 --> L4 --> L6
    L1 --> L5 --> L6
    L6 --> L7
    L2 --> L7
    L3 --> L7
    L4 --> L7
    L5 --> L7
```

`deploy-agent` 是独立二进制，只复用 `common + artifact + release`，不依赖 `db`，
因为服务器端不需要也不应该有桌面端数据库。

## 3. 模块边界

| 模块 | 职责 | 明确的"不做" |
| --- | --- | --- |
| `commands/*` | 参数反序列化、权限/状态前置校验、调用 service、返回 DTO | 不写业务规则、不直接拼 SQL |
| `services/project` | 只读扫描、slug 唯一化、增删改、构建配置 | 不启动构建 |
| `services/build` | 工具链前置检查、并发互斥、事务建任务、spawn 执行、日志落盘与事件推送 | 不打包 |
| `services/artifact` | 校验产物目录、调用打包、事务写库、删除保护（被发布引用则拒绝） | 不发版 |
| `services/release` | 环境/目标 CRUD、连通性检测、建 Release + Deployment、推进状态机、回滚 | 不打包 |
| `services/settings` | 设置读写、备份/恢复、凭据元数据 | 不存明文密钥 |
| `buildship-build::process` | 参数白名单、PATHEXT 解析、整树终止 | 不做业务编排 |
| `buildship-build::runner` | 流式读取、GBK 容错、脱敏、双通道输出 | 不判断成功语义（由退出码决定） |
| `buildship-artifact::archive` | 排除规则、ZIP + manifest、SHA-256、校验 | 不做上传 |
| `buildship-release::client` | 协议头、流式上传、错误码映射 | 不含重试策略（由 service 决定） |
| `deploy-agent` | 接包、校验、解压、原子切换、健康检查、审计 | 不构建、不接触桌面数据库 |

## 4. 仓库目录设计

```text
buildship/
├── Cargo.toml                     # workspace 根，统一依赖版本
├── crates/
│   ├── buildship-common/          # 错误码、路径、时间、脱敏、日志
│   ├── buildship-domain/          # 枚举、实体、状态机、DTO
│   ├── buildship-db/              # SQLx、migrations、repositories、备份
│   ├── buildship-build/           # 工具链探测、项目扫描、进程执行
│   ├── buildship-artifact/        # 哈希、manifest、ZIP 打包与校验
│   └── buildship-release/         # Agent 协议结构 + HTTP 客户端
├── apps/desktop/
│   ├── src-tauri/                 # 桌面端 Rust 外壳（commands/services/platform）
│   ├── src/                       # Vue 3 渲染层
│   │   ├── views/                 # 7 个页面
│   │   ├── stores/                # Pinia（setup store）
│   │   ├── floating/              # 悬浮栏独立入口
│   │   ├── components/            # AppShell / AuroraMark / StatusPill …
│   │   ├── composables/           # useIpc / useFormat
│   │   └── types/ipc.ts           # 与 Rust DTO 一一对应的类型
│   ├── index.html                 # 主面板入口
│   └── floating.html              # 悬浮栏入口
├── deploy-agent/                  # 目标服务器发布代理（独立二进制）
├── docs/                          # 本目录
├── scripts/                       # 图标生成等辅助脚本
└── .github/workflows/             # CI（Windows + macOS 构建与测试）
```

## 5. 关键数据流

### 5.1 构建

```mermaid
sequenceDiagram
    participant V as BuildsView
    participant C as commands::build_start
    participant S as services::build
    participant D as SQLite
    participant P as 子进程（npm run build）
    participant F as 日志文件

    V->>C: invoke("build_start", {projectId})
    C->>S: start()
    S->>S: 工具链探测 + 并发互斥（idx_build_tasks_active）
    S->>D: BEGIN → insert build_tasks(queued) + audit_events → COMMIT
    S-->>V: 任务 DTO（立即返回）
    S->>D: mark_running
    S->>P: CommandSpec.spawn()（参数数组）
    loop 每行输出
        P-->>S: stdout/stderr 字节流（read_until + lossy）
        S->>S: redact::scrub_line
        S->>F: 追加落盘
        S-->>V: emit("build://status", {kind:"log", line})
    end
    P-->>S: 退出码
    S->>D: BEGIN → finish_task + insert_log_segment + set_last_build + audit → COMMIT
    S-->>V: emit("build://status", {kind:"finished"})
```

### 5.2 发布

```mermaid
sequenceDiagram
    participant U as ArtifactsView
    participant S as services::release
    participant G as Deploy Agent
    participant D as SQLite

    U->>S: create_release(artifactId, targetId)
    S->>S: 校验发布包存在 + SHA-256 重算
    S->>G: GET /v1/status（在线/版本探测）
    S->>D: BEGIN → releases + deployment_tasks + release_events + audit → COMMIT
    S-->>U: 发布单（status=queued）
    S->>G: POST /v1/upload（流式 + x-buildship-sha256）
    G-->>S: {releaseId, storedPath, sha256, alreadyExisted}
    S->>G: POST /v1/apply（treeSha256 二次确认）
    G-->>S: {healthStatus, rolledBack, appliedAt}
    alt healthStatus = passed
        S->>D: release=deployed, is_current=1（部分唯一索引保证单当前）
    else healthStatus = failed
        S->>D: release=failed + release_events 记录原因
    end
    S-->>U: emit("deploy://status", {...})
```

### 5.3 悬浮栏

```mermaid
flowchart LR
    A["120ms 轮询光标位置"] --> B{"在 104×104 区域内？"}
    B -->|否| C["set_ignore_cursor_events(true)<br/>完全鼠标穿透"]
    B -->|是| D["set_ignore_cursor_events(false)<br/>展开为 372×104 玻璃胶囊"]
    D --> E["320ms 无悬停 → 收起"]
    E --> C
```

## 6. 窗口与运行形态

| 窗口 | label | 尺寸 | 特性 |
| --- | --- | --- | --- |
| 主面板 | `main` | 1320×860（最小 1080×720） | 关闭时收进托盘，不退出进程 |
| 悬浮栏 | `floating` | 104×104 ↔ 372×104 | 无边框、透明、置顶、不进任务栏、可拖动 |

托盘菜单：显示主面板 / 构建当前项目 / 退出。

## 7. 未完成与已知限制

| 项 | 状态 | 说明 |
| --- | --- | --- |
| macOS 实机验收 | 未完成 | 代码已按 `cfg(target_os)` 分支实现（WKWebView、apple-native keyring、点击穿透降级为不穿透），但本机无 Mac，**不得标记为已验收** |
| 多站点并行发布 | 已限制 | 同一目标同时只允许一个部署任务（服务端站点锁 + 桌面端状态机） |
| Agent HTTPS | 由部署方负责 | Agent 本身只监听 HTTP，TLS 终止建议交给前置 nginx/IIS |
| 增量/断点上传 | 未实现 | 当前整包上传，靠 Agent 侧 `alreadyExisted` 幂等复用 |
