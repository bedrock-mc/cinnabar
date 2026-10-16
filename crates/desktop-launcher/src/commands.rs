//! The commands and events the web UI calls; their TypeScript side is `ui/src/api/tauri.ts`.

use crate::{
    featured::{self, FeaturedServer},
    game::{self, GameState, LaunchOptions},
    install::{self, InstallState},
    news::{self, News},
    surface::MAIN_WINDOW,
};
use launcher::{desktop::GameExit, install_layout::InstallLayout};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use update_manifest::{build::Build, channel::Channel};
use update_service::{Handoff, ManualPackage, Service, UpdateState};

/// Everything the commands share.
pub struct Launcher {
    pub layout: InstallLayout,
    pub current: Build,
    pub game_executable: PathBuf,
    pub updates: Service,
    pub game: Arc<game::Supervisor>,
    /// An opened manual installer stays leased until this process exits.
    pub package: Mutex<Option<ManualPackage>>,
}

pub type Shared = Arc<Launcher>;

macro_rules! commands {
    ($($name:ident),+ $(,)?) => {
        /// Every command name the UI may invoke.
        #[cfg(test)]
        pub const COMMANDS: &[&str] = &[$(stringify!($name)),+];

        pub fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
            tauri::generate_handler![$($name),+]
        }
    };
}

commands!(
    get_install_state,
    get_update_state,
    check_for_updates,
    install_update,
    set_channel,
    launch_game,
    get_game_state,
    get_news,
    get_featured_servers,
    open_external,
    open_logs,
    get_recent_servers,
    get_changelog,
);

/// The edge clock as RFC 3339 text.
pub fn now_text() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}

/// Checks the selected channel off the UI thread.
pub fn spawn_check(launcher: &Shared) {
    let launcher = Arc::clone(launcher);
    std::thread::spawn(move || launcher.updates.check(OffsetDateTime::now_utc()));
}

#[tauri::command]
fn get_install_state(launcher: State<'_, Shared>) -> InstallState {
    install::state(&launcher.current, launcher.updates.channel())
}

#[tauri::command]
fn get_update_state(launcher: State<'_, Shared>) -> UpdateState {
    launcher.updates.state()
}

#[tauri::command]
fn check_for_updates(launcher: State<'_, Shared>) {
    spawn_check(&launcher);
}

/// Installs only on this explicit request, and never while the game runs.
#[tauri::command]
fn install_update(app: AppHandle, launcher: State<'_, Shared>) -> Result<(), String> {
    if launcher.game.running() {
        return Err("Close Cinnabar before updating.".into());
    }
    let launcher = Arc::clone(&launcher);
    std::thread::spawn(
        move || match launcher.updates.install(OffsetDateTime::now_utc) {
            // The helper waits for this process to exit before it installs.
            Ok(Handoff::Exit) => app.exit(0),
            Ok(Handoff::Package(package)) => {
                match app
                    .opener()
                    .open_path(package.path.to_string_lossy(), None::<&str>)
                {
                    Ok(()) => {
                        let _ = package.opened();
                        *launcher
                            .package
                            .lock()
                            .unwrap_or_else(|poison| poison.into_inner()) = Some(package);
                        // The platform installer replaces this launcher, so it must not stay open.
                        app.exit(0);
                    }
                    Err(error) => tracing::warn!("could not open the update installer: {error}"),
                }
            }
            Err(_) => {}
        },
    );
    Ok(())
}

#[tauri::command]
fn set_channel(channel: Channel, launcher: State<'_, Shared>) -> Result<(), String> {
    launcher
        .updates
        .set_channel(channel)
        .map_err(|error| error.to_string())?;
    spawn_check(&launcher);
    Ok(())
}

#[tauri::command]
fn launch_game(
    opts: Option<LaunchOptions>,
    app: AppHandle,
    launcher: State<'_, Shared>,
) -> Result<(), String> {
    start_game(&app, &launcher, opts.unwrap_or_default()).map_err(|error| format!("{error:#}"))
}

#[tauri::command]
fn get_game_state(launcher: State<'_, Shared>) -> GameState {
    launcher.game.state()
}

/// Fetches fresh news when online, otherwise the cached feed marked stale.
#[tauri::command]
async fn get_news(launcher: State<'_, Shared>) -> Result<News, String> {
    let path = news_cache(&launcher.layout);
    let fetched = news::fetch().await;
    let now = OffsetDateTime::now_utc();
    Ok(match fetched {
        Ok(items) => {
            let saved = news::save(&path, items, now);
            if let Err(error) = &saved {
                tracing::debug!("could not cache news: {error:#}");
            }
            news::present(saved.ok().or_else(|| news::load(&path)), true, now)
        }
        Err(error) => {
            tracing::debug!("news unavailable: {error:#}");
            news::present(news::load(&path), false, now)
        }
    })
}

/// The API's curated servers (cached offline) followed by the game's own featured list.
#[tauri::command]
async fn get_featured_servers(launcher: State<'_, Shared>) -> Result<Vec<FeaturedServer>, String> {
    let cache = launcher
        .layout
        .user_data_root
        .join("launcher-featured.json");
    let curated = match featured::fetch().await {
        Ok(servers) => {
            if let Err(error) = featured::save_cache(&cache, &servers) {
                tracing::debug!("could not cache featured servers: {error:#}");
            }
            servers
        }
        Err(error) => {
            tracing::debug!("featured servers unavailable: {error:#}");
            featured::load_cache(&cache).unwrap_or_default()
        }
    };
    Ok(featured::merge(
        curated,
        featured::from_game(&launcher.layout.featured_servers_file()),
    ))
}

/// Opens only HTTPS links, in the player's browser.
#[tauri::command]
fn open_external(url: String, app: AppHandle) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("only HTTPS links open from the launcher".into());
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_recent_servers(launcher: State<'_, Shared>) -> Vec<featured::RecentServer> {
    featured::recent(
        &launcher.layout.server_file(),
        &launcher.layout.featured_servers_file(),
    )
}

/// The installed build's highlights; `unseen` only on this version's first read.
#[tauri::command]
fn get_changelog(launcher: State<'_, Shared>) -> Option<crate::changelog::Changelog> {
    crate::changelog::installed(
        &launcher.current.version,
        &launcher
            .layout
            .user_data_root
            .join("launcher-changelog-seen.json"),
    )
}

/// Opens the folder holding the launcher's and the game's logs.
#[tauri::command]
fn open_logs(app: AppHandle, launcher: State<'_, Shared>) -> Result<(), String> {
    app.opener()
        .open_path(launcher.layout.log_dir().to_string_lossy(), None::<&str>)
        .map_err(|error| error.to_string())
}

/// Starts the game unless an update owns the installation, hiding the launcher while it runs.
pub fn start_game(
    app: &AppHandle,
    launcher: &Shared,
    options: LaunchOptions,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !matches!(
            launcher.updates.state(),
            UpdateState::Downloading { .. }
                | UpdateState::Verifying { .. }
                | UpdateState::Installing { .. }
        ),
        "wait for the update to finish"
    );
    let command = game::command(&launcher.game_executable, options.server.as_ref());
    let exited = {
        let app = app.clone();
        let launcher = Arc::clone(launcher);
        move |_, reason| {
            show_window(&app);
            if reason == Some(GameExit::ReturnForUpdate) {
                launcher.updates.require();
                spawn_check(&launcher);
            }
        }
    };
    launcher.game.launch(command, now_text(), exited)?;
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.hide();
    }
    Ok(())
}

/// Brings the launcher window back to the front.
pub fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Forwards a state change to the UI.
pub fn emit<T: serde::Serialize + Clone>(app: &AppHandle, event: &str, payload: &T) {
    if let Err(error) = app.emit(event, payload.clone()) {
        tracing::debug!("could not send {event}: {error}");
    }
}

fn news_cache(layout: &InstallLayout) -> PathBuf {
    layout.user_data_root.join("launcher-news.json")
}
