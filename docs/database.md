# 数据库设计（SQLite）

> 对应方案 5 章与 16 章"第一步"：14 张核心表在初始 migration 中一次性建立，不推迟。
> 迁移文件：`crates/buildship-db/migrations/0001_init.sql`

## 1. 连接与运行参数

| 参数 | 值 | 原因 |
| --- | --- | --- |
| `journal_mode` | `WAL` | 读写并发，构建写日志时界面仍可查询 |
| `foreign_keys` | `ON` | 每个连接都要开，否则约束不生效 |
| `busy_timeout` | `5000ms` | 构建线程与界面并发写入时自动等待 |
| `synchronous` | `NORMAL` | WAL 下的性能/安全折中 |
| 迁移 | `sqlx::migrate!` 编译期内嵌 | 安装包不依赖外部 SQL 文件 |

`Db::open` 完成后立即执行 `integrity_check`（含 `foreign_key_check`），
打开即校验，避免带着损坏的库继续跑。

## 2. ER 图

```mermaid
erDiagram
    projects ||--o{ project_build_profiles : "拥有"
    projects ||--o{ build_tasks : "产生"
    projects ||--o{ artifacts : "产出"
    projects ||--o{ releases : "发布"
    build_tasks ||--o{ build_logs : "分段日志"
    build_tasks ||--o{ artifacts : "构建来源"
    artifacts ||--o{ releases : "被打包发布"
    deployment_environments ||--o{ deployment_targets : "包含站点"
    deployment_environments ||--o{ releases : "目标环境"
    deployment_targets ||--o{ releases : "发布目标"
    releases ||--o{ deployment_tasks : "部署任务"
    releases ||--o{ release_events : "事件流"

    projects {
        TEXT id PK
        TEXT name
        TEXT slug UK
        TEXT path UK
        TEXT framework
        TEXT package_manager
        TEXT output_dir
        TEXT status
        TEXT last_build_status
        TEXT last_build_at
        TEXT created_at
        TEXT updated_at
    }

    project_build_profiles {
        TEXT id PK
        TEXT project_id FK
        TEXT name
        TEXT command
        TEXT build_args_json
        TEXT output_dir
        INTEGER is_default
    }

    build_tasks {
        TEXT id PK
        TEXT project_id FK
        TEXT profile_id FK
        TEXT status
        TEXT trigger_kind
        INTEGER exit_code
        TEXT started_at
        TEXT finished_at
        INTEGER duration_ms
        TEXT error_message
        TEXT created_at
    }

    build_logs {
        TEXT id PK
        TEXT task_id FK
        INTEGER segment_index
        TEXT file_path
        INTEGER line_count
        INTEGER byte_size
        TEXT created_at
    }

    artifacts {
        TEXT id PK
        TEXT project_id FK
        TEXT build_task_id FK
        TEXT file_name
        TEXT file_path
        TEXT size_bytes
        TEXT sha256
        INTEGER file_count
        TEXT tree_sha256
        TEXT manifest_path
        TEXT status
        TEXT created_at
    }

    deployment_environments {
        TEXT id PK
        TEXT name UK
        TEXT kind
        TEXT description
        TEXT status
    }

    deployment_targets {
        TEXT id PK
        TEXT environment_id FK
        TEXT name
        TEXT base_url
        TEXT physical_path
        TEXT health_check_url
        TEXT credential_ref FK
        INTEGER tls_verify
        TEXT status
    }

    releases {
        TEXT id PK
        TEXT project_id FK
        TEXT artifact_id FK
        TEXT environment_id FK
        TEXT target_id FK
        TEXT version
        TEXT status
        INTEGER is_current
        TEXT release_notes
        TEXT deployed_at
        TEXT rolled_back_at
    }

    deployment_tasks {
        TEXT id PK
        TEXT release_id FK
        TEXT kind
        TEXT status
        TEXT idempotency_key UK
        TEXT remote_release_id
        TEXT request_payload_json
        TEXT response_payload_json
        TEXT started_at
        TEXT finished_at
        TEXT error_message
    }

    release_events {
        TEXT id PK
        TEXT release_id FK
        INTEGER seq
        TEXT level
        TEXT action
        TEXT message
        TEXT created_at
    }

    audit_events {
        TEXT id PK
        TEXT actor
        TEXT action
        TEXT entity_type
        TEXT entity_id
        TEXT project_id
        TEXT detail_json
        TEXT created_at
    }

    app_settings {
        TEXT key PK
        TEXT value
        TEXT updated_at
    }

    secret_references {
        TEXT id PK
        TEXT label UK
        TEXT provider
        TEXT account
        TEXT created_at
    }

    schema_migrations {
        INTEGER version PK
        TEXT description
        TEXT applied_at
    }
```

## 3. 业务约束如何落到数据库

单靠应用层校验不够，关键不变量用**部分唯一索引**锁死，任何绕过 service 的写入都会被拒绝。

| 不变量 | 实现 |
| --- | --- |
| 同一项目同时只能有一个排队/运行中的构建 | `idx_build_tasks_active ON build_tasks(project_id) WHERE status IN ('queued','running')` |
| 同一项目+环境只能有一个"当前版本" | `idx_releases_current ON releases(project_id, environment_id) WHERE is_current = 1` |
| 同一项目只能有一个默认构建配置 | `idx_profiles_default ON project_build_profiles(project_id) WHERE is_default = 1` |
| 项目 slug / 路径唯一 | `idx_projects_slug`、`idx_projects_path` |
| 发布版本号在项目+环境内唯一 | `idx_releases_version` |
| 部署任务幂等 | `idx_deploy_tasks_idempotency ON deployment_tasks(idempotency_key)` |
| 事件流序号连续且唯一 | `idx_release_events_seq ON release_events(release_id, seq)` |

事件序号由单条 SQL 原子生成，避免并发下的读-改-写竞态：

```sql
INSERT INTO release_events (id, release_id, seq, level, action, message, created_at)
SELECT ?1, ?2, COALESCE(MAX(seq), 0) + 1, ?3, ?4, ?5, ?6
FROM release_events WHERE release_id = ?2;
```

## 4. 审计不可篡改

```sql
CREATE TRIGGER trg_audit_events_no_update
BEFORE UPDATE ON audit_events
BEGIN
    SELECT RAISE(ABORT, 'audit_events 只允许追加，不允许修改');
END;

CREATE TRIGGER trg_audit_events_no_delete
BEFORE DELETE ON audit_events
BEGIN
    SELECT RAISE(ABORT, 'audit_events 只允许追加，不允许删除');
END;
```

删除项目时审计记录必须保留，因此 `audit_events.project_id` 不使用外键级联。

## 5. 时间与序列化约定

- 所有时间列存 **UTC RFC3339 毫秒**字符串（`buildship_common::time::to_db`），
  展示层负责转本地时区，避免"存在库里的时间随机器时区漂移"。
- Rust 实体用 `#[serde(rename_all = "camelCase")]`，与 TS 类型一一对应。
- 枚举以 `snake_case` 字符串落库，与 `sqlx::Type` 派生保持一致；
  升级只增不改，历史值始终可读。

## 6. 备份与恢复

```mermaid
flowchart LR
    A["backup_create"] --> B["VACUUM INTO backups/xxx.db"]
    B --> C["记录 size + sha256"]
    C --> D{"需要恢复？"}
    D -->|是| E["backup_validate<br/>只读打开 + integrity_check"]
    E --> F["先做一次安全备份"]
    F --> G["关闭连接池"]
    G --> H["删除 -wal/-shm"]
    H --> I["替换主库文件"]
    I --> J["重新 open + migrate"]
    J --> K["integrity_check 通过才算成功"]
    K --> L["写审计 audit_events"]
```

用 `VACUUM INTO` 而不是直接复制文件：WAL 模式下直接拷贝可能拿到未合并的中间态。

## 7. 迁移策略

- 迁移编号连续：`0001_init.sql`、`0002_xxx.sql`……
- 只允许**新增**表/列/索引；破坏性变更必须新表 + 数据搬迁 + 改名，且单独评审。
- `schema_migrations` 是 `_sqlx_migrations` 的可读镜像（`version / description / applied_at`），供运维与"关于"页展示。
- 迁移在事务中执行，失败即回滚并让启动阶段直接报错，不带病运行。
