//! 数据库约束与触发器集成测试。
//!
//! 这些不变量**必须由数据库兜底**，不能只靠 service 层判断：
//! 任何绕过 service 的写入（脚本、手工修数据、未来的新代码路径）都应被拒绝。

use buildship_common::AppResult;
use buildship_db::Db;

const TS: &str = "2026-01-01T00:00:00.000Z";

/// 内存库 + 全量迁移
async fn setup() -> Db {
    Db::open_memory().await.expect("内存库初始化失败")
}

async fn seed_project(db: &Db, id: &str) {
    let sql = format!(
        "INSERT INTO projects (id, name, slug, path, package_manager, created_at, updated_at)
         VALUES ('{id}', '项目{id}', 'slug-{id}', 'C:/work/{id}', 'npm', '{TS}', '{TS}')"
    );
    db.execute(&sql).await.expect("插入项目失败");
}

async fn insert_build(db: &Db, id: &str, project_id: &str, status: &str) -> AppResult<()> {
    let sql = format!(
        "INSERT INTO build_tasks (id, project_id, status, command, created_at, updated_at)
         VALUES ('{id}', '{project_id}', '{status}', 'npm run build', '{TS}', '{TS}')"
    );
    db.execute(&sql).await
}

async fn seed_task(db: &Db, project_id: &str, task_id: &str) {
    insert_build(db, task_id, project_id, "succeeded")
        .await
        .expect("插入构建任务失败");
}

async fn seed_artifact(db: &Db, id: &str, project_id: &str, task_id: &str) {
    // sha256 列有 length = 64 的 CHECK
    let sha = "a".repeat(64);
    let sql = format!(
        "INSERT INTO artifacts
            (id, project_id, build_task_id, file_name, file_path, size_bytes, sha256,
             file_count, manifest_path, created_at, updated_at)
         VALUES ('{id}', '{project_id}', '{task_id}', 'a.zip', 'C:/out/a.zip', 10, '{sha}',
                 1, 'C:/out/a.manifest.json', '{TS}', '{TS}')"
    );
    db.execute(&sql).await.expect("插入发布包失败");
}

async fn seed_env_and_target(db: &Db) {
    db.execute(&format!(
        "INSERT INTO deployment_environments (id, name, kind, created_at, updated_at)
         VALUES ('env-1', 'production', 'prod', '{TS}', '{TS}')"
    ))
    .await
    .expect("插入环境失败");
    db.execute(&format!(
        "INSERT INTO deployment_targets
            (id, environment_id, name, agent_base_url, iis_site, physical_path, created_at, updated_at)
         VALUES ('tgt-1', 'env-1', 'main', 'http://127.0.0.1:7801', 'demo', 'C:/sites/demo', '{TS}', '{TS}')"
    ))
    .await
    .expect("插入站点失败");
}

async fn insert_release(db: &Db, id: &str, version: &str, is_current: i32) -> AppResult<()> {
    let sql = format!(
        "INSERT INTO releases
            (id, project_id, artifact_id, environment_id, target_id, version, status,
             is_current, created_at, updated_at)
         VALUES ('{id}', 'prj-1', 'art-1', 'env-1', 'tgt-1', '{version}', 'healthy',
                 {is_current}, '{TS}', '{TS}')"
    );
    db.execute(&sql).await
}

#[tokio::test]
async fn migration_creates_all_fourteen_tables() {
    let db = setup().await;
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master
         WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE '\\_sqlx%' ESCAPE '\\'",
    )
    .fetch_one(db.pool())
    .await
    .expect("统计表数量失败");

    // 14 张业务表（含 schema_migrations 自身）
    assert_eq!(count, 14, "初始迁移必须一次建齐 14 张核心表");
    assert!(db.schema_version() >= 1);
}

#[tokio::test]
async fn only_one_active_build_per_project() {
    let db = setup().await;
    seed_project(&db, "prj-1").await;

    insert_build(&db, "bt-1", "prj-1", "running")
        .await
        .expect("第一个活动构建应成功");
    let second = insert_build(&db, "bt-2", "prj-1", "queued").await;
    assert!(
        second.is_err(),
        "同一项目不允许两个 queued/running 构建（idx_build_tasks_active）"
    );

    // 第一个任务结束后即可再建
    db.execute("UPDATE build_tasks SET status = 'succeeded' WHERE id = 'bt-1'")
        .await
        .expect("更新状态失败");
    insert_build(&db, "bt-3", "prj-1", "queued")
        .await
        .expect("前一个任务结束后应允许新构建");
}

#[tokio::test]
async fn only_one_default_profile_per_project() {
    let db = setup().await;
    seed_project(&db, "prj-1").await;

    let insert = |id: &str, is_default: i32| {
        format!(
            "INSERT INTO project_build_profiles
                (id, project_id, name, build_command, output_dir, is_default, created_at, updated_at)
             VALUES ('{id}', 'prj-1', 'profile-{id}', 'npm run build', 'dist', {is_default}, '{TS}', '{TS}')"
        )
    };
    db.execute(&insert("p1", 1))
        .await
        .expect("首个默认配置应成功");
    let err = db.execute(&insert("p2", 1)).await;
    assert!(
        err.is_err(),
        "同一项目只允许一个 is_default = 1（idx_profiles_default）"
    );

    db.execute(&insert("p3", 0))
        .await
        .expect("非默认配置可以存在多个");
}

#[tokio::test]
async fn only_one_current_release_per_project_and_env() {
    let db = setup().await;
    seed_project(&db, "prj-1").await;
    seed_task(&db, "prj-1", "bt-1").await;
    seed_artifact(&db, "art-1", "prj-1", "bt-1").await;
    seed_env_and_target(&db).await;

    insert_release(&db, "rel-1", "v1", 1)
        .await
        .expect("第一个当前版本应成功");
    let err = insert_release(&db, "rel-2", "v2", 1).await;
    assert!(
        err.is_err(),
        "同一项目+环境只允许一个 is_current = 1（idx_releases_current）"
    );

    insert_release(&db, "rel-3", "v3", 0)
        .await
        .expect("非当前版本可以有多条");

    // 版本号在项目+环境内唯一
    let dup = insert_release(&db, "rel-4", "v3", 0).await;
    assert!(dup.is_err(), "重复版本号应被 idx_releases_version 拒绝");
}

#[tokio::test]
async fn audit_events_are_append_only() {
    let db = setup().await;
    db.execute(&format!(
        "INSERT INTO audit_events (id, actor, action, entity_type, entity_id, summary, created_at)
         VALUES ('au-1', 'tester', 'project.create', 'project', 'prj-1', '新增项目', '{TS}')"
    ))
    .await
    .expect("写入审计失败");

    let update = db
        .execute("UPDATE audit_events SET summary = '被篡改' WHERE id = 'au-1'")
        .await;
    assert!(update.is_err(), "audit_events 不允许 UPDATE");

    let delete = db
        .execute("DELETE FROM audit_events WHERE id = 'au-1'")
        .await;
    assert!(delete.is_err(), "audit_events 不允许 DELETE");

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_events")
        .fetch_one(db.pool())
        .await
        .expect("统计审计失败");
    assert_eq!(count, 1, "被拒绝的修改不应影响原记录");
}

#[tokio::test]
async fn foreign_keys_are_enforced() {
    let db = setup().await;
    // 外键指向不存在的项目
    let err = insert_build(&db, "bt-x", "prj-missing", "queued").await;
    assert!(
        err.is_err(),
        "外键必须生效：build_tasks.project_id → projects.id"
    );
}

#[tokio::test]
async fn checks_reject_invalid_enum_and_hash() {
    let db = setup().await;
    seed_project(&db, "prj-1").await;

    // status 不在 CHECK 列表内
    let bad_status = insert_build(&db, "bt-1", "prj-1", "exploding").await;
    assert!(bad_status.is_err(), "非法构建状态应被 CHECK 拒绝");

    seed_task(&db, "prj-1", "bt-ok").await;
    // sha256 长度必须为 64
    let bad_hash = db
        .execute(&format!(
            "INSERT INTO artifacts
                (id, project_id, build_task_id, file_name, file_path, size_bytes, sha256,
                 file_count, manifest_path, created_at, updated_at)
             VALUES ('art-bad', 'prj-1', 'bt-ok', 'a.zip', 'C:/out/a.zip', 1, 'short',
                     1, 'C:/out/a.manifest.json', '{TS}', '{TS}')"
        ))
        .await;
    assert!(bad_hash.is_err(), "sha256 长度不足 64 应被 CHECK 拒绝");
}

#[tokio::test]
async fn integrity_check_passes_after_migration() {
    let db = setup().await;
    let (ok, message) = db.integrity_check().await.expect("完整性检查失败");
    assert!(ok, "{message}");
}
