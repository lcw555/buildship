//! Buildship 发布代理（Deploy Agent）
//!
//! 跑在目标 Web 服务器上，负责接包、校验、原子切换与回滚。
//! 与桌面端通过 `buildship-release::protocol` 定义的 HTTP 契约通信。
//!
//! 启动：
//! ```text
//! deploy-agent --config deploy-agent.toml
//! deploy-agent --config deploy-agent.toml --check   # 只校验配置
//! ```
//!
//! 目录约定见 `store` 模块文档；`current` 指向线上版本，Web 服务器应把站点
//! 根目录配置为 `<physicalPath>/current`。

mod audit;
mod auth;
mod config;
mod error;
mod health;
mod routes;
mod store;
mod switch;
mod upload;

use clap::Parser;
use config::Config;
use std::net::SocketAddr;
use std::path::PathBuf;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    name = "deploy-agent",
    version,
    about = "Buildship 发布代理：接收发布包并原子切换线上版本"
)]
struct Cli {
    /// 配置文件路径（默认查找 deploy-agent.toml / ProgramData\Buildship）
    #[arg(long, short, env = "BUILDSHIP_AGENT_CONFIG")]
    config: Option<PathBuf>,

    /// 覆盖监听地址，如 0.0.0.0:7801
    #[arg(long)]
    bind: Option<String>,

    /// 覆盖访问 Token（不建议写在命令行历史里，优先用环境变量或 tokenFile）
    #[arg(long, env = "BUILDSHIP_AGENT_TOKEN")]
    token: Option<String>,

    /// 只校验配置与目录权限，不启动服务
    #[arg(long)]
    check: bool,
}

#[tokio::main]
async fn main() {
    init_tracing();
    if let Err(err) = run().await {
        tracing::error!(error = %err, "deploy-agent 退出");
        eprintln!("deploy-agent 启动失败：{err}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let config_path = cli.config.unwrap_or_else(config::default_config_path);

    let config = Config::load(&config_path, cli.bind, cli.token)?;
    tracing::info!(
        config = %config.config_path.display(),
        bind = %config.bind,
        sites = config.sites.len(),
        link_strategy = ?config.link_strategy,
        max_upload_mb = config.max_upload_bytes / 1024 / 1024,
        "配置加载完成"
    );
    for site in config.sites.values() {
        tracing::info!(
            site = %site.name,
            root = %site.root.display(),
            keep_releases = site.keep_releases,
            "站点已登记"
        );
        // 启动即校验目录可写，避免发布时才发现权限问题
        let layout = store::Layout::new(&site.root);
        layout.ensure()?;
        let probe = layout.tmp_dir().join(".write-probe");
        std::fs::write(&probe, b"ok")?;
        std::fs::remove_file(&probe)?;
    }

    if cli.check {
        println!(
            "配置校验通过：{} 个站点，监听 {}",
            config.sites.len(),
            config.bind
        );
        return Ok(());
    }

    let bind = config.bind;
    let ctx = routes::AppCtx::new(config);
    let app = routes::router(ctx).layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(%bind, "deploy-agent 已启动");
    println!("deploy-agent 正在监听 http://{bind}");

    // 带上 ConnectInfo，审计日志才能记录来源 IP
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    tracing::info!("deploy-agent 已停止");
    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("deploy_agent=info,tower_http=warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .init();
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("收到退出信号，准备优雅停机");
}
