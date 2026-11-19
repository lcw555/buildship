//! Buildship 桌面端入口。
//!
//! 装配顺序：日志 → 数据目录 → SQLite（含迁移）→ 运行时状态 → 注册到 Builder
//! → 窗口加载后即可调用 IPC。残留任务收尾、托盘、悬浮窗在 `setup` 里完成。
//!
//! 关键约束：Tauri 的窗口在 `setup` 之前就已经开始加载页面，前端首帧就会调用
//! `system_bootstrap`。如果把 `manage(state)` 写在 `setup` 里，打开数据库的这段时间
//! 前端必然撞上 "state not managed"，因此状态必须在 Builder 构建之前注册完毕。

pub mod commands;
pub mod events;
pub mod infrastructure;
pub mod platform;
pub mod services;
pub mod state;

use buildship_common::{logging, AppPaths};
use buildship_db::{repositories as repo, Db};
use state::{AppState, QuickContext, SharedState};
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, WindowEvent};

fn show_main(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// 同步完成全部启动初始化，返回可直接注册给 Tauri 的运行时状态。
///
/// 失败时返回可直接展示给人看的中文说明：此时窗口还没有，`platform::fatal`
/// 会用系统对话框弹出来，同时日志里也留了同样的内容。
fn bootstrap_state() -> Result<SharedState, String> {
    let paths = AppPaths::system().map_err(|err| format!("无法定位应用数据目录：{err}"))?;
    paths
        .ensure()
        .map_err(|err| format!("初始化数据目录失败：{err}"))?;

    let _ = logging::init(Some(&paths.logs_dir()), "info");
    // GUI 子系统下 stderr 可能没有去处，panic 必须落日志，否则崩溃现场完全不可见
    std::panic::set_hook(Box::new(|info| {
        tracing::error!("未捕获的 panic：{info}");
    }));
    tracing::info!(dir = %paths.root().display(), version = env!("CARGO_PKG_VERSION"), "Buildship 启动");

    let db = tauri::async_runtime::block_on(Db::open(&paths.db_file()))
        .map_err(|err| format!("打开数据库失败：{err}"))?;
    tracing::info!(schema = db.schema_version(), "数据库就绪");

    let operator = state::detect_operator();

    // 恢复上次选中的项目，让悬浮栏重启后仍指向同一项目
    let mut quick = QuickContext::default();
    if let Ok(Some(project_id)) =
        tauri::async_runtime::block_on(repo::settings::get(db.pool(), "selected_project_id"))
    {
        if let Ok(Some(project)) =
            tauri::async_runtime::block_on(repo::projects::get(db.pool(), &project_id))
        {
            quick.project_id = Some(project.id);
            quick.project_name = Some(project.name);
        }
    }

    Ok(Arc::new(AppState {
        paths,
        db: tokio::sync::RwLock::new(db),
        operator,
        builds: Default::default(),
        quick: tokio::sync::Mutex::new(quick),
    }))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 必须在日志初始化之前：GUI 子系统下先挂回父控制台，终端启动才看得到日志
    platform::init_console();

    let shared = match bootstrap_state() {
        Ok(state) => state,
        Err(message) => {
            platform::fatal(&message);
            return;
        }
    };

    tauri::Builder::default()
        // 必须在窗口加载前注册，见文件头注释
        .manage(shared.clone())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            // 上次异常退出残留的任务收尾：放后台跑，不拖慢首帧
            let state = shared.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(err) = services::build::recover_interrupted(state.as_ref()).await {
                    tracing::warn!(error = %err, "构建任务恢复检查失败");
                }
                let db = state.db().await;
                match repo::deployments::recover_interrupted(db.pool()).await {
                    Ok(count) if count > 0 => {
                        tracing::warn!(count, "已将中断的部署任务标记为失败")
                    }
                    Ok(_) => {}
                    Err(err) => tracing::warn!(error = %err, "部署任务恢复检查失败"),
                }
            });

            // 系统托盘
            let show_item = MenuItem::with_id(app, "show", "显示主面板", true, None::<&str>)?;
            let quick_build = MenuItem::with_id(app, "build", "构建当前项目", true, None::<&str>)?;
            let reset_float =
                MenuItem::with_id(app, "reset_floating", "复位悬浮窗位置", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出 Buildship", true, None::<&str>)?;
            let menu =
                Menu::with_items(app, &[&show_item, &quick_build, &reset_float, &quit_item])?;

            let mut tray = TrayIconBuilder::with_id("main-tray")
                .tooltip("Buildship")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_main(app),
                    "reset_floating" => {
                        if let Err(err) = platform::floating::reset_position(app) {
                            tracing::warn!(error = %err, "复位悬浮窗位置失败");
                        }
                    }
                    "build" => {
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let state = app.state::<SharedState>().inner().clone();
                            let context = state.quick_snapshot().await;
                            if let Some(project_id) = context.project_id {
                                if let Err(err) =
                                    services::build::start(app.clone(), state.clone(), project_id)
                                        .await
                                {
                                    events::toast_error(&app, err.message);
                                }
                            } else {
                                show_main(&app);
                            }
                        });
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main(tray.app_handle());
                    }
                });

            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;

            platform::floating::setup(app.handle())?;
            platform::floating::watch_moves(app.handle());
            platform::floating::start_hit_test(app.handle().clone());

            Ok(())
        })
        .on_window_event(|window, event| {
            // 关闭主窗口时收进托盘，保持后台构建与悬浮栏可用
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            // 系统
            commands::system_bootstrap,
            commands::system_toolchain,
            commands::dashboard_summary,
            commands::settings_get,
            commands::settings_update,
            commands::audit_list,
            commands::backup_list,
            commands::backup_create,
            commands::backup_restore,
            commands::secret_list,
            commands::secret_save,
            commands::file_sha256,
            commands::open_data_dir,
            commands::runtime_info,
            // 项目
            commands::project_list,
            commands::project_scan,
            commands::project_add,
            commands::project_update,
            commands::project_archive,
            commands::project_profiles,
            commands::quick_state_get,
            commands::quick_set_project,
            // 构建
            commands::build_start,
            commands::build_cancel,
            commands::build_list,
            commands::build_get,
            commands::build_logs,
            commands::build_open_log_dir,
            // 发布包
            commands::artifact_list,
            commands::artifact_create,
            commands::artifact_get,
            commands::artifact_verify,
            commands::artifact_delete,
            commands::artifact_open_dir,
            // 发布
            commands::env_list,
            commands::env_save,
            commands::env_archive,
            commands::target_save,
            commands::target_archive,
            commands::target_check,
            commands::release_list,
            commands::release_detail,
            commands::release_create,
            commands::release_rollback,
            commands::release_healthy_candidates,
            // 窗口
            commands::window_show_main,
            commands::window_hide_main,
            commands::floating_expand,
            commands::floating_collapse,
            commands::floating_set_ignore_cursor,
            // 维护
            commands::maintenance_clean,
            commands::maintenance_uninstall,
        ])
        .run(tauri::generate_context!())
        .expect("Buildship 启动失败");
}
