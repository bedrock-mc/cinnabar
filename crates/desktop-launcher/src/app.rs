//! The Tauri window, its lifecycle and the background work it starts.

use crate::{
    commands::{self, Launcher},
    game,
    surface::{GAME_EVENT, MAIN_WINDOW, UPDATE_EVENT},
};
use anyhow::{Context, Result};
use launcher::install_layout::InstallLayout;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use update_installer::startup;
use update_manifest::build::Build;

/// Smallest window that still fits the launchpad's play action and news.
const MIN_SIZE: (f64, f64) = (900.0, 560.0);
const DEFAULT_SIZE: (f64, f64) = (1180.0, 720.0);

pub fn run() -> Result<()> {
    let layout = InstallLayout::discover().context("resolve install layout")?;
    install_logging(&layout);
    let directory = std::env::current_exe()?
        .parent()
        .context("missing launcher directory")?
        .to_owned();
    // The update helper starts the launcher with a receipt only to continue into the game.
    let continue_into_game = std::env::var_os(startup::STARTUP_ENV).is_some();
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            commands::show_window(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(commands::handler())
        .on_window_event(|window, event| {
            // Closing while the game runs keeps the supervisor alive without a visible window.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event
                && window
                    .app_handle()
                    .try_state::<commands::Shared>()
                    .is_some_and(|launcher| launcher.game.running())
            {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(move |app| {
            let launcher = launcher(app.handle(), layout, &directory)?;
            app.manage(Arc::clone(&launcher));
            window(app)?;
            start(app.handle(), &launcher, continue_into_game);
            Ok(())
        })
        .run(tauri::generate_context!())
        .context("run the launcher window")
}

fn launcher(
    app: &tauri::AppHandle,
    layout: InstallLayout,
    directory: &std::path::Path,
) -> Result<commands::Shared> {
    let settings = layout
        .user_config_root
        .join(launcher::menu::settings_options::SETTINGS_FILE);
    let target = layout
        .is_installed()
        .then(|| {
            update_installer::target::resolve(
                std::env::consts::OS,
                &std::env::current_exe().ok()?,
                std::env::var_os(startup::APPIMAGE)
                    .map(PathBuf::from)
                    .as_deref(),
            )
            .ok()
        })
        .flatten();
    let update_events = app.clone();
    let updates = update_service::Service::new(
        update_service::Config {
            update_dir: layout.update_dir(),
            current: Build::current(),
            target,
            helper: directory.join(update_installer::helper_filename()),
        },
        || launcher::menu::settings_options::legacy_update_channel(&settings),
        move |state| commands::emit(&update_events, UPDATE_EVENT, state),
    );
    let game_events = app.clone();
    Ok(Arc::new(Launcher {
        game_executable: startup::executable(directory, startup::GAME),
        layout,
        current: Build::current(),
        updates,
        game: game::Supervisor::new(move |state| commands::emit(&game_events, GAME_EVENT, state)),
        package: Mutex::new(None),
    }))
}

/// Opens the launchpad, the one place the web UI is loaded, with a custom title bar; macOS
/// keeps its native window buttons.
fn window(app: &tauri::App) -> Result<()> {
    let builder = WebviewWindowBuilder::new(app, MAIN_WINDOW, WebviewUrl::App("index.html".into()))
        .title(launcher::PRODUCT_NAME)
        .inner_size(DEFAULT_SIZE.0, DEFAULT_SIZE.1)
        .min_inner_size(MIN_SIZE.0, MIN_SIZE.1)
        .center();
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    #[cfg(not(target_os = "macos"))]
    let builder = builder.decorations(false);
    builder.build()?;
    Ok(())
}

/// Checks for updates in the background and continues an update the player started.
fn start(app: &tauri::AppHandle, launcher: &commands::Shared, continue_into_game: bool) {
    #[cfg(debug_assertions)]
    if let Some(fixture) = std::env::var_os("CINNABAR_LAUNCHER_FIXTURE") {
        fixture::apply(launcher, &fixture.to_string_lossy());
        return;
    }
    let reported_failure = matches!(
        launcher.updates.state(),
        update_service::UpdateState::Failed { .. }
    );
    let checks = std::env::var("CINNABAR_UPDATE_CHECK").map_or(true, |value| value != "0");
    if checks && !reported_failure {
        commands::spawn_check(launcher);
    }
    if continue_into_game
        && let Err(error) = commands::start_game(app, launcher, game::LaunchOptions::default())
    {
        tracing::warn!("could not continue into the updated game: {error:#}");
    }
}

/// Writes this process's log beside the game's logs.
fn install_logging(layout: &InstallLayout) {
    let directory = layout.log_dir();
    let file = std::fs::create_dir_all(&directory)
        .and_then(|()| std::fs::File::create(directory.join("launcher.log")));
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false);
    match file {
        Ok(file) => builder.with_writer(Mutex::new(file)).init(),
        Err(_) => builder.init(),
    }
}

/// Development-only states for screenshots and UI work without a published release.
#[cfg(debug_assertions)]
mod fixture {
    use update_service::UpdateState;

    pub fn apply(launcher: &super::commands::Shared, name: &str) {
        let state = match name {
            "update-available" => UpdateState::Available {
                version: "0.2.0".into(),
                channel: launcher.updates.channel(),
                notes_url: Some("https://github.com/bedrock-mc/cinnabar/releases".into()),
                size_bytes: Some(148 * 1024 * 1024),
                required: false,
                downgrade: false,
            },
            "update-required" => UpdateState::Available {
                version: "0.2.0".into(),
                channel: launcher.updates.channel(),
                notes_url: None,
                size_bytes: Some(148 * 1024 * 1024),
                required: true,
                downgrade: false,
            },
            "downloading" => UpdateState::Downloading {
                version: "0.2.0".into(),
                received_bytes: 61 * 1024 * 1024,
                total_bytes: Some(148 * 1024 * 1024),
            },
            _ => return,
        };
        launcher.updates.show_fixture(state);
    }
}
