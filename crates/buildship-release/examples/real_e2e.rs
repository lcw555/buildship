//! 真实链路验证：构建 → 打包 → 发布 → 回滚。
//!
//! 与 `scripts/e2e-agent-smoke.sh` 的区别：那边用构造的 ZIP 直接打 Agent，
//! 这里跑的是**真实前端工程**，并且走的是桌面端同一套库：
//!
//! ```text
//! BuildRunner（真实 yarn install / build）
//!   → buildship_artifact::package（ZIP + manifest + 树哈希）
//!   → buildship_artifact::verify_package（发布前完整性校验）
//!   → DeployClient::upload / apply（真实 HTTP 到 deploy-agent）
//!   → 校验站点 current 内容
//!   → DeployClient::rollback（回滚 + 审计）
//! ```
//!
//! 用法：
//! ```text
//! cargo run -p buildship-release --example real_e2e -- \
//!   --project <工程目录> --site <站点物理根> --agent http://127.0.0.1:7802 \
//!   --token <token> --health http://127.0.0.1:8099/
//! ```

use buildship_artifact::{package, verify_package, PackageOptions};
use buildship_build::process::CommandSpec;
use buildship_build::runner::{BuildCancel, BuildMessage, BuildRequest, BuildRunner};
use buildship_common::time;
use buildship_release::protocol::{ApplyRequest, RollbackRequest};
use buildship_release::DeployClient;
use std::path::{Path, PathBuf};

struct Args {
    project: PathBuf,
    site: PathBuf,
    agent: String,
    token: String,
    health: String,
}

fn parse_args() -> Args {
    let mut project = None;
    let mut site = None;
    let mut agent = None;
    let mut token = None;
    let mut health = None;
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut take = || it.next().expect("参数缺少取值");
        match flag.as_str() {
            "--project" => project = Some(PathBuf::from(take())),
            "--site" => site = Some(PathBuf::from(take())),
            "--agent" => agent = Some(take()),
            "--token" => token = Some(take()),
            "--health" => health = Some(take()),
            other => panic!("未知参数：{other}"),
        }
    }
    Args {
        project: project
            .expect("缺少 --project")
            .canonicalize()
            .expect("工程目录不存在"),
        site: site
            .expect("缺少 --site")
            .canonicalize()
            .expect("站点目录不存在（请先创建）"),
        agent: agent.expect("缺少 --agent"),
        token: token.expect("缺少 --token"),
        health: health.expect("缺少 --health"),
    }
}

fn step(index: u32, title: &str) {
    println!("\n==> {index}. {title}");
}

/// 用真实的 BuildRunner 跑一条命令，日志边跑边打。
async fn run_step(label: &str, program: &str, args: Vec<String>, cwd: &Path, seq: u32) -> bool {
    let task_id = format!("e2e-{seq}");
    let log_path = cwd.join(".e2e-logs").join(format!("{label}.log"));
    let spec = CommandSpec::new(program, args, cwd.to_path_buf());
    println!("    $ {}", spec.display());
    println!("      日志：{}", log_path.display());

    let req = BuildRequest {
        task_id: task_id.clone(),
        project_id: "e2e".into(),
        spec,
        log_path: log_path.clone(),
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let cancel = BuildCancel::new();
    let runner = tokio::spawn(async move { BuildRunner::run(req, cancel, tx).await });

    while let Some(msg) = rx.recv().await {
        if let BuildMessage::Line(line) = msg {
            println!("      | {}", line.text);
        }
    }
    match runner.await {
        Ok(Ok(outcome)) => {
            println!(
                "      → {:?}（{} ms，日志 {} 字节）",
                outcome.status, outcome.duration_ms, outcome.log_size
            );
            matches!(
                outcome.status,
                buildship_domain::enums::BuildStatus::Succeeded
            )
        }
        Ok(Err(err)) => {
            println!("      → 执行失败：{}", err.message);
            false
        }
        Err(err) => {
            println!("      → 任务异常：{err}");
            false
        }
    }
}

fn package_dist(
    args: &Args,
    release_id: &str,
    artifacts: &Path,
) -> buildship_artifact::PackageResult {
    let options = PackageOptions {
        artifact_id: release_id.to_string(),
        project_id: "e2e".into(),
        project_name: "Portfolio".into(),
        project_slug: "portfolio".into(),
        build_task_id: "e2e-build".into(),
        build_command: "yarn build".into(),
        output_dir: "dist".into(),
        source_dir: args.project.join("dist"),
        dest_dir: artifacts.to_path_buf(),
        file_name: Some(release_id.to_string()),
        git_commit: None,
        packaged_by: "real_e2e".into(),
    };
    package(&options).expect("打包失败")
}

fn read_current(site: &Path) -> String {
    std::fs::read_to_string(site.join("current").join("index.html")).unwrap_or_default()
}

/// 替换 HTML 里的 `<title>` 内容（不引入正则依赖）
fn set_title(html: &str, title: &str) -> String {
    let Some(start) = html.find("<title>") else {
        return html.to_string();
    };
    let Some(end_rel) = html[start..].find("</title>") else {
        return html.to_string();
    };
    let end = start + end_rel;
    let mut out = String::with_capacity(html.len() + title.len());
    out.push_str(&html[..start]);
    out.push_str("<title>");
    out.push_str(title);
    out.push_str(&html[end..]);
    out
}

#[tokio::main]
async fn main() {
    let args = parse_args();
    let artifacts = args.project.join(".e2e-artifacts");
    std::fs::create_dir_all(&artifacts).expect("创建产物目录失败");
    let stamp = time::compact_stamp(time::now());
    let mut failures = 0;

    step(1, "真实构建：yarn install");
    if !run_step(
        "install",
        "yarn",
        vec!["install".into(), "--non-interactive".into()],
        &args.project,
        1,
    )
    .await
    {
        failures += 1;
    }

    step(2, "真实构建：yarn build");
    if !run_step("build", "yarn", vec!["build".into()], &args.project, 2).await {
        failures += 1;
    }
    let dist = args.project.join("dist");
    println!("      dist 存在：{}", dist.is_dir());

    step(3, "打包 + 完整性校验");
    let v1_id = format!("v1-{stamp}");
    let pkg1 = package_dist(&args, &v1_id, &artifacts);
    println!(
        "      {}（{} 文件 / {} 字节 / sha256 {}…）",
        pkg1.file_name,
        pkg1.file_count,
        pkg1.size_bytes,
        &pkg1.sha256[..16]
    );
    let report = verify_package(&pkg1.file_path, &pkg1.sha256).expect("校验执行失败");
    println!(
        "      verify: ok={} files={} 说明={}",
        report.ok, report.file_count, report.message
    );
    if !report.ok {
        failures += 1;
    }

    step(4, "Agent 在线检查");
    let client = DeployClient::new(&args.agent, &args.token, true).expect("客户端构造失败");
    match client.status().await {
        Ok(status) => println!(
            "      agent {} @ {}，站点 {} 个，当前版本 {:?}",
            status.agent_version,
            status.hostname,
            status.sites.len(),
            status.sites.first().and_then(|s| s.current_release.clone())
        ),
        Err(err) => {
            println!("      ✗ Agent 不可用：{}", err.message);
            std::process::exit(2);
        }
    }

    let site_name = "demo".to_string();
    let physical = args.site.to_string_lossy().to_string();

    step(5, "上传 v1");
    match client.upload(&v1_id, &pkg1.file_path, &pkg1.sha256).await {
        Ok(res) => println!(
            "      uploaded releaseId={} files={} 复用={}",
            res.release_id, res.file_count, res.already_existed
        ),
        Err(err) => {
            println!("      ✗ 上传失败：{}", err.message);
            std::process::exit(3);
        }
    }

    step(6, "应用 v1（原子切换 + 健康检查）");
    let apply1 = ApplyRequest {
        release_id: v1_id.clone(),
        site: site_name.clone(),
        physical_path: physical.clone(),
        health_check_url: Some(args.health.clone()),
        tree_sha256: Some(pkg1.tree_sha256.clone()),
        request_id: format!("req-{stamp}-a1"),
    };
    match client.apply(&apply1).await {
        Ok(res) => {
            println!(
                "      applied={} health={}（{}）rolledBack={}",
                res.release_id,
                res.health_status,
                res.health_message.unwrap_or_default(),
                res.rolled_back
            );
            if res.health_status != "passed" {
                failures += 1;
            }
        }
        Err(err) => {
            println!("      ✗ 应用失败：{}", err.message);
            std::process::exit(4);
        }
    }
    let served_v1 = read_current(&args.site);
    println!("      current/index.html {} 字节", served_v1.len());

    step(7, "构造 v2（改标题后重新构建打包）");
    // 改 index.html 的 <title>：vite 会原样带进产物，最适合用来判断线上到底换了哪一版
    let index_html = args.project.join("index.html");
    let source = std::fs::read_to_string(&index_html).unwrap_or_default();
    let patched = set_title(&source, &format!("作品集 v2 · {stamp}"));
    std::fs::write(&index_html, &patched).expect("改写 index.html 失败");
    let built = run_step("build-v2", "yarn", vec!["build".into()], &args.project, 3).await;
    if !built {
        failures += 1;
    }
    let v2_id = format!("v2-{stamp}");
    let pkg2 = package_dist(&args, &v2_id, &artifacts);
    println!("      {}（{} 文件）", pkg2.file_name, pkg2.file_count);
    client
        .upload(&v2_id, &pkg2.file_path, &pkg2.sha256)
        .await
        .expect("上传 v2 失败");

    step(8, "应用 v2");
    let apply2 = ApplyRequest {
        release_id: v2_id.clone(),
        site: site_name.clone(),
        physical_path: physical.clone(),
        health_check_url: Some(args.health.clone()),
        tree_sha256: Some(pkg2.tree_sha256.clone()),
        request_id: format!("req-{stamp}-a2"),
    };
    match client.apply(&apply2).await {
        Ok(res) => println!(
            "      applied={} previous={:?} health={}",
            res.release_id, res.previous_release_id, res.health_status
        ),
        Err(err) => {
            println!("      ✗ 应用 v2 失败：{}", err.message);
            std::process::exit(5);
        }
    }
    let served_v2 = read_current(&args.site);
    // SPA 的文案被打进 JS bundle，index.html 里看不到；
    // 但 vite 的资源文件名带内容哈希，内容一变文件名就变，用它判断最可靠。
    let v2_ok = served_v2 != served_v1 && !served_v2.is_empty();
    println!("      current 已切换到 v2 的产物：{v2_ok}");
    if !v2_ok {
        failures += 1;
    }

    step(9, "回滚到 v1");
    let rb = RollbackRequest {
        to_release_id: v1_id.clone(),
        site: site_name.clone(),
        physical_path: physical.clone(),
        health_check_url: Some(args.health.clone()),
        reason: "真实链路验证：确认回滚可用".into(),
        request_id: format!("req-{stamp}-r1"),
    };
    match client.rollback(&rb).await {
        Ok(res) => println!(
            "      rolled back to={} health={} rolledBack={}",
            res.release_id, res.health_status, res.rolled_back
        ),
        Err(err) => {
            println!("      ✗ 回滚失败：{}", err.message);
            std::process::exit(6);
        }
    }
    let served_rb = read_current(&args.site);
    let rb_ok = served_rb == served_v1;
    println!("      current 已回到 v1 的产物：{rb_ok}");
    if !rb_ok {
        failures += 1;
    }

    step(10, "版本列表");
    match client.list_releases(&site_name).await {
        Ok(list) => {
            for item in &list {
                println!(
                    "      {} current={} applied={}",
                    item.release_id,
                    item.is_current,
                    item.applied_at
                        .map(|t| t.to_rfc3339())
                        .unwrap_or_else(|| "-".into())
                );
            }
        }
        Err(err) => println!("      获取失败：{}", err.message),
    }

    println!("\n==== 真实链路：{} 项异常 ====", failures);
    if failures > 0 {
        std::process::exit(1);
    }
}
