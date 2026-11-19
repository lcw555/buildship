-- Buildship 初始数据结构（V1 全量）
-- 约定：
--   1. 主键统一 UUID 文本；时间统一 UTC RFC3339 文本（毫秒精度）。
--   2. 所有状态列带 CHECK 约束，取值与 Rust 枚举严格一致。
--   3. 业务历史（release / audit / 已完成任务）只增不删；删除一律走归档。
--   4. 审计表通过触发器禁止 UPDATE / DELETE。

CREATE TABLE schema_migrations (
    version     INTEGER PRIMARY KEY,
    description TEXT    NOT NULL,
    applied_at  TEXT    NOT NULL
);

-- ---------------------------------------------------------------------------
-- 设置与凭据引用（先于 deployment_targets 建表，避免外键前向引用）
-- ---------------------------------------------------------------------------
CREATE TABLE app_settings (
    key        TEXT PRIMARY KEY NOT NULL,
    value      TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE secret_references (
    id           TEXT PRIMARY KEY NOT NULL,
    label        TEXT NOT NULL,
    provider     TEXT NOT NULL CHECK (provider IN ('windows_credential_manager', 'macos_keychain', 'memory')),
    account      TEXT NOT NULL,
    created_at   TEXT NOT NULL,
    last_used_at TEXT
);
CREATE UNIQUE INDEX idx_secret_label ON secret_references (label);

-- ---------------------------------------------------------------------------
-- 项目
-- ---------------------------------------------------------------------------
CREATE TABLE projects (
    id                      TEXT PRIMARY KEY NOT NULL,
    name                    TEXT NOT NULL,
    slug                    TEXT NOT NULL,
    path                    TEXT NOT NULL,
    package_manager         TEXT NOT NULL CHECK (package_manager IN ('npm', 'pnpm', 'yarn')),
    package_manager_version TEXT,
    node_version            TEXT,
    framework               TEXT,
    status                  TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'archived')),
    last_build_status       TEXT CHECK (last_build_status IS NULL OR last_build_status IN
                                ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
    last_build_at           TEXT,
    created_at              TEXT NOT NULL,
    updated_at              TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_projects_slug ON projects (slug);
CREATE UNIQUE INDEX idx_projects_path ON projects (path);
CREATE INDEX idx_projects_status ON projects (status, updated_at DESC);

CREATE TABLE project_build_profiles (
    id           TEXT PRIMARY KEY NOT NULL,
    project_id   TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    name         TEXT NOT NULL,
    build_script TEXT,
    build_command TEXT NOT NULL,
    build_args_json TEXT NOT NULL DEFAULT '[]',
    output_dir   TEXT NOT NULL,
    env_json     TEXT NOT NULL DEFAULT '{}',
    is_default   INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_profiles_project_name ON project_build_profiles (project_id, name);
CREATE UNIQUE INDEX idx_profiles_default ON project_build_profiles (project_id) WHERE is_default = 1;

-- ---------------------------------------------------------------------------
-- 构建
-- ---------------------------------------------------------------------------
CREATE TABLE build_tasks (
    id            TEXT PRIMARY KEY NOT NULL,
    project_id    TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    profile_id    TEXT REFERENCES project_build_profiles (id) ON DELETE SET NULL,
    status        TEXT NOT NULL CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
    command       TEXT NOT NULL,
    args_json     TEXT NOT NULL DEFAULT '[]',
    exit_code     INTEGER,
    started_at    TEXT,
    finished_at   TEXT,
    duration_ms   INTEGER,
    log_path      TEXT,
    log_size      INTEGER,
    log_sha256    TEXT,
    error_summary TEXT,
    triggered_by  TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);
CREATE INDEX idx_build_tasks_project ON build_tasks (project_id, created_at DESC);
CREATE INDEX idx_build_tasks_status ON build_tasks (status, created_at DESC);
-- 同一项目同一时刻只允许一个活动构建（DB 层兜底，服务层先查再插）
CREATE UNIQUE INDEX idx_build_tasks_active ON build_tasks (project_id)
    WHERE status IN ('queued', 'running');

CREATE TABLE build_logs (
    id            TEXT PRIMARY KEY NOT NULL,
    task_id       TEXT NOT NULL REFERENCES build_tasks (id) ON DELETE CASCADE,
    segment_index INTEGER NOT NULL,
    file_path     TEXT NOT NULL,
    start_offset  INTEGER NOT NULL DEFAULT 0,
    end_offset    INTEGER NOT NULL DEFAULT 0,
    line_count    INTEGER NOT NULL DEFAULT 0,
    created_at    TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_build_logs_task_segment ON build_logs (task_id, segment_index);

-- ---------------------------------------------------------------------------
-- 发布包
-- ---------------------------------------------------------------------------
CREATE TABLE artifacts (
    id              TEXT PRIMARY KEY NOT NULL,
    project_id      TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    build_task_id   TEXT NOT NULL REFERENCES build_tasks (id) ON DELETE RESTRICT,
    file_name       TEXT NOT NULL,
    file_path       TEXT NOT NULL,
    size_bytes      INTEGER NOT NULL CHECK (size_bytes >= 0),
    sha256          TEXT NOT NULL CHECK (length(sha256) = 64),
    file_count      INTEGER NOT NULL CHECK (file_count >= 0),
    manifest_path   TEXT NOT NULL,
    manifest_sha256 TEXT,
    status          TEXT NOT NULL DEFAULT 'ready' CHECK (status IN ('ready', 'corrupt', 'missing', 'purged')),
    git_commit      TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);
CREATE INDEX idx_artifacts_project ON artifacts (project_id, created_at DESC);
CREATE INDEX idx_artifacts_sha ON artifacts (sha256);

-- ---------------------------------------------------------------------------
-- 发布环境与目标
-- ---------------------------------------------------------------------------
CREATE TABLE deployment_environments (
    id          TEXT PRIMARY KEY NOT NULL,
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('dev', 'test', 'staging', 'prod')),
    description TEXT,
    status      TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'archived')),
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_env_name ON deployment_environments (name);

CREATE TABLE deployment_targets (
    id              TEXT PRIMARY KEY NOT NULL,
    environment_id  TEXT NOT NULL REFERENCES deployment_environments (id) ON DELETE RESTRICT,
    name            TEXT NOT NULL,
    server_host     TEXT,
    agent_base_url  TEXT NOT NULL,
    iis_site        TEXT NOT NULL,
    physical_path   TEXT NOT NULL,
    health_check_url TEXT,
    credential_ref  TEXT REFERENCES secret_references (id) ON DELETE SET NULL,
    tls_verify      INTEGER NOT NULL DEFAULT 1 CHECK (tls_verify IN (0, 1)),
    status          TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'archived')),
    last_checked_at TEXT,
    last_check_ok   INTEGER CHECK (last_check_ok IS NULL OR last_check_ok IN (0, 1)),
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_targets_env_name ON deployment_targets (environment_id, name);

-- ---------------------------------------------------------------------------
-- 发布版本与部署任务
-- ---------------------------------------------------------------------------
CREATE TABLE releases (
    id                 TEXT PRIMARY KEY NOT NULL,
    project_id         TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    artifact_id        TEXT NOT NULL REFERENCES artifacts (id) ON DELETE RESTRICT,
    environment_id     TEXT NOT NULL REFERENCES deployment_environments (id) ON DELETE RESTRICT,
    target_id          TEXT NOT NULL REFERENCES deployment_targets (id) ON DELETE RESTRICT,
    version            TEXT NOT NULL,
    deploy_path        TEXT,
    status             TEXT NOT NULL CHECK (status IN
                            ('draft', 'uploading', 'deploying', 'health_check', 'healthy',
                             'failed', 'rolled_back', 'cancelled')),
    previous_release_id TEXT REFERENCES releases (id) ON DELETE SET NULL,
    rolled_back_from   TEXT,
    rollback_reason    TEXT,
    health_status      TEXT NOT NULL DEFAULT 'unknown'
                            CHECK (health_status IN ('unknown', 'passed', 'failed', 'skipped')),
    health_message     TEXT,
    is_current         INTEGER NOT NULL DEFAULT 0 CHECK (is_current IN (0, 1)),
    operator           TEXT,
    started_at         TEXT,
    finished_at        TEXT,
    created_at         TEXT NOT NULL,
    updated_at         TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_releases_version ON releases (project_id, environment_id, version);
-- 每个项目+环境只允许一个"当前线上版本"
CREATE UNIQUE INDEX idx_releases_current ON releases (project_id, environment_id) WHERE is_current = 1;
CREATE INDEX idx_releases_project ON releases (project_id, created_at DESC);

CREATE TABLE deployment_tasks (
    id              TEXT PRIMARY KEY NOT NULL,
    release_id      TEXT NOT NULL REFERENCES releases (id) ON DELETE CASCADE,
    target_id       TEXT NOT NULL REFERENCES deployment_targets (id) ON DELETE RESTRICT,
    project_id      TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    kind            TEXT NOT NULL CHECK (kind IN ('deploy', 'rollback')),
    status          TEXT NOT NULL CHECK (status IN
                        ('queued', 'uploading', 'deploying', 'health_check', 'succeeded', 'failed', 'cancelled')),
    stage           TEXT,
    progress        INTEGER NOT NULL DEFAULT 0 CHECK (progress BETWEEN 0 AND 100),
    request_id      TEXT NOT NULL,
    idempotency_key TEXT,
    error_code      TEXT,
    error_message   TEXT,
    started_at      TEXT,
    finished_at     TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_deploy_tasks_idempotency ON deployment_tasks (idempotency_key);
CREATE INDEX idx_deploy_tasks_release ON deployment_tasks (release_id, created_at DESC);

CREATE TABLE release_events (
    id          TEXT PRIMARY KEY NOT NULL,
    release_id  TEXT NOT NULL REFERENCES releases (id) ON DELETE CASCADE,
    seq         INTEGER NOT NULL,
    phase       TEXT NOT NULL,
    level       TEXT NOT NULL CHECK (level IN ('info', 'warn', 'error')),
    message     TEXT NOT NULL,
    detail_json TEXT,
    created_at  TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_release_events_seq ON release_events (release_id, seq);

-- ---------------------------------------------------------------------------
-- 审计（只增不改）
-- ---------------------------------------------------------------------------
CREATE TABLE audit_events (
    id          TEXT PRIMARY KEY NOT NULL,
    actor       TEXT NOT NULL,
    action      TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    entity_id   TEXT NOT NULL,
    project_id  TEXT REFERENCES projects (id) ON DELETE SET NULL,
    summary     TEXT NOT NULL,
    detail_json TEXT,
    created_at  TEXT NOT NULL
);
CREATE INDEX idx_audit_created ON audit_events (created_at DESC);
CREATE INDEX idx_audit_project ON audit_events (project_id, created_at DESC);
CREATE INDEX idx_audit_action ON audit_events (action, created_at DESC);

CREATE TRIGGER trg_audit_events_no_update
BEFORE UPDATE ON audit_events
BEGIN
    SELECT RAISE(ABORT, 'audit_events 为不可变审计表，禁止 UPDATE');
END;

CREATE TRIGGER trg_audit_events_no_delete
BEFORE DELETE ON audit_events
BEGIN
    SELECT RAISE(ABORT, 'audit_events 为不可变审计表，禁止 DELETE');
END;
