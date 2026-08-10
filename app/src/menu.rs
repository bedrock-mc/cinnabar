//! Java-inspired launcher/menu state and the small amount of input plumbing
//! needed before a Bedrock session exists.
//!
//! The game client remains the authority for rendering and input. The menu is
//! deliberately retained UI rather than a second windowing toolkit, so the
//! no-argument path is light, keyboard/controller friendly, and uses exactly
//! the same font, safe-area, and pointer coordinates as the gameplay HUD.

mod account;
pub(crate) mod auth;
mod input;

use auth::{AuthState, AuthSupervisor};

pub(crate) use input::{drive_menu_connection, drive_menu_input, recover_menu_session_failure};

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};

use crate::{
    runtime::endpoint::{bridge_endpoint_exists, bridge_endpoint_path},
    ui_runtime::presentation::IconRef,
};

const MAX_SERVER_NAME_BYTES: usize = 64;
const MAX_SERVER_ADDRESS_BYTES: usize = 128;
const DEFAULT_SERVER_FILE: &str = ".local/cinnabar/servers.json";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MenuScreen {
    Home,
    Play,
    Social,
    Servers,
    Profile,
    Settings,
    AddServer,
    Pause,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MenuServerTab {
    Featured,
    Favorites,
    Recent,
    Saved,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MenuDialog {
    Exit,
    RemoveSaved(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MenuField {
    Name,
    Address,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MenuAction {
    Navigate(MenuScreen),
    OpenExitDialog,
    ConfirmExit,
    DismissDialog,
    SelectServerTab(MenuServerTab),
    RefreshCatalog,
    StartSignIn,
    CancelSignIn,
    PlayAddServer,
    PlaySaved(usize),
    PlayFeatured(usize),
    PlayGathering(usize),
    PlayRealm(usize),
    PlayFriend(usize),
    ToggleFavorite(usize),
    RemoveSavedDialog(usize),
    ConfirmRemoveSaved(usize),
    AddName,
    AddAddress,
    AddSave,
    AddSaveConnect,
    AddBack,
    SettingsScale(u8),
    PauseResume,
    PauseDisconnect,
    PauseSettings,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct SavedServer {
    pub(crate) name: String,
    pub(crate) address: String,
    #[serde(default)]
    pub(crate) favorite: bool,
    #[serde(default)]
    pub(crate) last_joined_unix: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub(crate) struct MenuServerCard {
    pub(crate) name: String,
    pub(crate) address: String,
    pub(crate) caption: String,
    #[serde(default)]
    pub(crate) image_path: String,
    #[serde(skip)]
    pub(crate) icon: Option<IconRef>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub(crate) struct MenuRealmCard {
    pub(crate) name: String,
    pub(crate) state: String,
    #[serde(default)]
    pub(crate) target: String,
    #[serde(default)]
    pub(crate) address: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MenuFriendCard {
    pub(crate) gamertag: String,
    pub(crate) world_name: String,
    pub(crate) members: String,
    pub(crate) xuid: String,
}

#[derive(Clone, Debug)]
pub(crate) struct MenuView {
    pub(crate) visible: bool,
    pub(crate) screen: MenuScreen,
    pub(crate) focused_action: Option<MenuAction>,
    pub(crate) hovered: Option<MenuAction>,
    pub(crate) pressed: Option<MenuAction>,
    pub(crate) server_tab: MenuServerTab,
    pub(crate) dialog: Option<MenuDialog>,
    pub(crate) field: Option<MenuField>,
    pub(crate) name: String,
    pub(crate) address: String,
    pub(crate) message: Option<String>,
    pub(crate) gui_scale: u8,
    pub(crate) display_name: String,
    pub(crate) servers: Vec<SavedServer>,
    pub(crate) featured: Vec<MenuServerCard>,
    pub(crate) gatherings: Vec<MenuServerCard>,
    pub(crate) realms: Vec<MenuRealmCard>,
    pub(crate) friends: Vec<MenuFriendCard>,
    pub(crate) featured_icon: Option<IconRef>,
    pub(crate) gathering_icon: Option<IconRef>,
    pub(crate) realm_icon: Option<IconRef>,
    pub(crate) friend_icon: Option<IconRef>,
    pub(crate) saved_icon: Option<IconRef>,
    pub(crate) profile_icon: Option<IconRef>,
    pub(crate) catalog_loading: bool,
    pub(crate) catalog_message: Option<String>,
    pub(crate) auth_state: AuthState,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct CatalogFile {
    #[serde(default)]
    featured: Vec<MenuServerCard>,
    #[serde(default)]
    gatherings: Vec<MenuServerCard>,
    #[serde(default)]
    realms: Vec<MenuRealmCard>,
    #[serde(default)]
    friends: Vec<CatalogFriend>,
    #[serde(default)]
    errors: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct CatalogFriend {
    gamertag: String,
    world_name: String,
    xuid: String,
    members: i32,
    max_members: i32,
}

impl From<CatalogFriend> for MenuFriendCard {
    fn from(friend: CatalogFriend) -> Self {
        let members = if friend.max_members > 0 {
            format!("{}/{} players", friend.members, friend.max_members)
        } else {
            format!("{} players", friend.members)
        };
        Self {
            gamertag: friend.gamertag,
            world_name: friend.world_name,
            members,
            xuid: friend.xuid,
        }
    }
}

#[derive(Debug, Resource)]
pub(crate) struct MenuRuntime {
    visible: bool,
    screen: MenuScreen,
    focused: usize,
    hovered: Option<MenuAction>,
    pressed: Option<MenuAction>,
    pointer_down: bool,
    server_tab: MenuServerTab,
    dialog: Option<MenuDialog>,
    field: Option<MenuField>,
    name: String,
    address: String,
    message: Option<String>,
    gui_scale: u8,
    display_name: String,
    launcher: bool,
    servers: Vec<SavedServer>,
    config_path: PathBuf,
    pending_connect: Option<String>,
    connecting: bool,
    disconnect_requested: bool,
    exit_requested: bool,
    session_generation: u64,
    featured: Vec<MenuServerCard>,
    gatherings: Vec<MenuServerCard>,
    realms: Vec<MenuRealmCard>,
    friends: Vec<MenuFriendCard>,
    catalog_message: Option<String>,
    catalog_started: bool,
    catalog_path: PathBuf,
    catalog_process: Option<Child>,
    auth_process: Option<AuthSupervisor>,
    auth_attempted: bool,
    auth_restart_requested: bool,
}

impl MenuRuntime {
    pub(crate) fn new(visible: bool, gui_scale: u8, display_name: String) -> Self {
        let config_path = PathBuf::from(DEFAULT_SERVER_FILE);
        let servers = load_servers(&config_path);
        Self {
            // The launcher owns the session lifecycle only when the client
            // started on the menu. `--address` keeps the historical behaviour
            // of exiting the process when its one session fails.
            launcher: visible,
            visible,
            screen: MenuScreen::Home,
            focused: 0,
            hovered: None,
            pressed: None,
            pointer_down: false,
            server_tab: MenuServerTab::Featured,
            dialog: None,
            field: None,
            name: String::new(),
            address: String::new(),
            message: None,
            gui_scale: gui_scale.clamp(1, 4),
            display_name,
            servers,
            config_path,
            pending_connect: None,
            connecting: false,
            disconnect_requested: false,
            exit_requested: false,
            session_generation: 1,
            featured: Vec::new(),
            gatherings: Vec::new(),
            realms: Vec::new(),
            friends: Vec::new(),
            catalog_message: None,
            catalog_started: false,
            catalog_path: PathBuf::from(format!(
                ".local/cinnabar/catalog-{}.json",
                std::process::id()
            )),
            catalog_process: None,
            auth_process: None,
            auth_attempted: false,
            auth_restart_requested: false,
        }
    }

    pub(crate) fn is_visible(&self) -> bool {
        self.visible
    }

    pub(crate) fn is_connecting(&self) -> bool {
        self.connecting
    }

    pub(crate) fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
        if !visible {
            self.field = None;
            self.dialog = None;
        }
    }

    pub(crate) fn view(&self) -> MenuView {
        let auth_state = self
            .auth_process
            .as_ref()
            .map_or(AuthState::SignedOut, |process| process.state().clone());
        let catalog_loading = matches!(
            &auth_state,
            AuthState::Checking | AuthState::AwaitingCode { .. }
        ) || (auth_state == AuthState::Authenticated
            && (!self.catalog_started || self.catalog_process.is_some()));
        MenuView {
            visible: self.visible,
            screen: self.screen,
            focused_action: self.focus_actions().get(self.focused).copied(),
            hovered: self.hovered,
            pressed: self.pressed,
            server_tab: self.server_tab,
            dialog: self.dialog,
            field: self.field,
            name: self.name.clone(),
            address: self.address.clone(),
            message: self.message.clone(),
            gui_scale: self.gui_scale,
            display_name: self.display_name.clone(),
            servers: self.servers.clone(),
            featured: self.featured.clone(),
            gatherings: self.gatherings.clone(),
            realms: self.realms.clone(),
            friends: self.friends.clone(),
            featured_icon: None,
            gathering_icon: None,
            realm_icon: None,
            friend_icon: None,
            saved_icon: None,
            profile_icon: None,
            catalog_loading,
            catalog_message: self.catalog_message.clone(),
            auth_state,
        }
    }

    pub(crate) fn open_pause(&mut self) {
        if self.visible || self.connecting {
            return;
        }
        self.screen = MenuScreen::Pause;
        self.focused = 0;
        self.message = None;
        self.visible = true;
    }

    pub(crate) fn take_pending_connect(&mut self) -> Option<String> {
        self.pending_connect.take()
    }

    pub(crate) fn mark_connected(&mut self) {
        self.connecting = false;
        self.visible = false;
        self.screen = MenuScreen::Home;
        self.message = None;
        self.field = None;
    }

    pub(crate) fn mark_connecting(&mut self) {
        self.connecting = true;
        self.visible = true;
        self.screen = MenuScreen::Play;
        self.message = Some("Connecting…".to_owned());
    }

    /// Returns the session back to the launcher after a fatal session error.
    ///
    /// Returns `false` when the client was started with `--address`, which has
    /// no launcher to fall back to and must still exit the process.
    pub(crate) fn absorb_session_failure(&mut self, error: &str) -> bool {
        if !self.launcher {
            return false;
        }
        self.connecting = false;
        self.visible = true;
        self.screen = MenuScreen::Play;
        self.dialog = None;
        self.field = None;
        self.message = Some(session_failure_message(error));
        // Let the account catalog repopulate now that the session is gone.
        self.catalog_started = false;
        true
    }

    pub(crate) fn take_disconnect_request(&mut self) -> bool {
        std::mem::take(&mut self.disconnect_requested)
    }

    pub(crate) fn take_exit_request(&mut self) -> bool {
        std::mem::take(&mut self.exit_requested)
    }

    pub(crate) fn next_session_generation(&mut self) -> u64 {
        self.session_generation = self.session_generation.saturating_add(1).max(1);
        self.session_generation
    }

    pub(crate) fn activate(&mut self, action: MenuAction) {
        self.pressed = Some(action);
        self.message = None;
        match action {
            MenuAction::Navigate(screen) => self.enter(screen),
            MenuAction::OpenExitDialog => self.dialog = Some(MenuDialog::Exit),
            MenuAction::ConfirmExit => {
                self.dialog = None;
                self.exit_requested = true;
            }
            MenuAction::DismissDialog => self.dialog = None,
            MenuAction::SelectServerTab(tab) => {
                self.server_tab = tab;
                self.focused = 0;
            }
            MenuAction::RefreshCatalog => {
                self.stop_catalog();
                self.catalog_started = false;
                self.catalog_message = None;
            }
            MenuAction::StartSignIn => self.start_sign_in(),
            MenuAction::CancelSignIn => self.stop_sign_in(),
            MenuAction::PlayAddServer => {
                self.name.clear();
                self.address.clear();
                self.field = Some(MenuField::Name);
                self.enter(MenuScreen::AddServer);
            }
            MenuAction::PlaySaved(index) => {
                if index < self.servers.len() {
                    self.servers[index].last_joined_unix = now_unix();
                    let address = self.servers[index].address.clone();
                    let _ = save_servers(&self.config_path, &self.servers);
                    self.request_connect(address);
                }
            }
            MenuAction::PlayFeatured(index) => {
                if let Some(server) = self.featured.get(index) {
                    self.request_connect(server.address.clone());
                }
            }
            MenuAction::PlayGathering(index) => {
                if let Some(server) = self.gatherings.get(index) {
                    self.request_connect(server.address.clone());
                }
            }
            MenuAction::PlayRealm(index) => {
                if let Some(realm) = self.realms.get(index) {
                    let target = if realm.target.is_empty() {
                        realm.address.clone()
                    } else {
                        realm.target.clone()
                    };
                    if target.is_empty() {
                        self.message = Some(format!(
                            "{} is {} and cannot be joined right now.",
                            realm.name, realm.state
                        ));
                    } else {
                        self.request_connect(target);
                    }
                }
            }
            MenuAction::ToggleFavorite(index) => {
                if let Some(server) = self.servers.get_mut(index) {
                    server.favorite = !server.favorite;
                    self.message = Some(if server.favorite {
                        format!("{} added to Favorites.", server.name)
                    } else {
                        format!("{} removed from Favorites.", server.name)
                    });
                    let _ = save_servers(&self.config_path, &self.servers);
                }
            }
            MenuAction::RemoveSavedDialog(index) => {
                if index < self.servers.len() {
                    self.dialog = Some(MenuDialog::RemoveSaved(index));
                }
            }
            MenuAction::ConfirmRemoveSaved(index) => {
                if index < self.servers.len() {
                    let removed = self.servers.remove(index);
                    let _ = save_servers(&self.config_path, &self.servers);
                    self.message = Some(format!("Removed {}.", removed.name));
                }
                self.dialog = None;
            }
            MenuAction::PlayFriend(index) => {
                if let Some(friend) = self.friends.get(index) {
                    if friend.xuid.is_empty() {
                        self.message =
                            Some("That friend world has no stable Xbox identity.".to_owned());
                    } else {
                        self.request_connect(format!("friend_xuid/{}", friend.xuid));
                    }
                }
            }
            MenuAction::AddName => self.field = Some(MenuField::Name),
            MenuAction::AddAddress => self.field = Some(MenuField::Address),
            MenuAction::AddSave => {
                if self.save_draft() {
                    self.enter(MenuScreen::Play);
                }
            }
            MenuAction::AddSaveConnect => {
                if self.save_draft() {
                    self.request_connect(self.address.clone());
                }
            }
            MenuAction::AddBack => self.go_back(),
            MenuAction::SettingsScale(scale) => self.gui_scale = scale.clamp(1, 4),
            MenuAction::PauseResume => self.set_visible(false),
            MenuAction::PauseDisconnect => {
                self.disconnect_requested = true;
                self.set_visible(false);
            }
            MenuAction::PauseSettings => self.enter(MenuScreen::Settings),
        }
    }

    pub(crate) fn move_focus(&mut self, direction: i32) {
        let actions = self.focus_actions();
        if actions.is_empty() {
            self.focused = 0;
            return;
        }
        let length = actions.len() as i32;
        self.focused = (self.focused as i32 + direction).rem_euclid(length) as usize;
    }

    pub(crate) fn activate_focused(&mut self) {
        let Some(action) = self.focus_actions().get(self.focused).copied() else {
            return;
        };
        self.activate(action);
    }

    pub(crate) fn edit_text(&mut self, text: &str) {
        let Some(field) = self.field else {
            return;
        };
        let target = match field {
            MenuField::Name => &mut self.name,
            MenuField::Address => &mut self.address,
        };
        let maximum = match field {
            MenuField::Name => MAX_SERVER_NAME_BYTES,
            MenuField::Address => MAX_SERVER_ADDRESS_BYTES,
        };
        for character in text.chars().filter(|character| !character.is_control()) {
            if target.len().saturating_add(character.len_utf8()) > maximum {
                break;
            }
            target.push(character);
        }
    }

    pub(crate) fn backspace_text(&mut self) {
        let Some(field) = self.field else {
            return;
        };
        let target = match field {
            MenuField::Name => &mut self.name,
            MenuField::Address => &mut self.address,
        };
        let _ = target.pop();
    }

    fn focus_actions(&self) -> Vec<MenuAction> {
        if let Some(dialog) = self.dialog {
            return match dialog {
                MenuDialog::Exit => vec![MenuAction::ConfirmExit, MenuAction::DismissDialog],
                MenuDialog::RemoveSaved(index) => vec![
                    MenuAction::ConfirmRemoveSaved(index),
                    MenuAction::DismissDialog,
                ],
            };
        }
        let nav = || {
            vec![
                MenuAction::Navigate(MenuScreen::Home),
                MenuAction::Navigate(MenuScreen::Play),
                MenuAction::Navigate(MenuScreen::Social),
                MenuAction::Navigate(MenuScreen::Servers),
                MenuAction::Navigate(MenuScreen::Profile),
                MenuAction::Navigate(MenuScreen::Settings),
                MenuAction::OpenExitDialog,
            ]
        };
        match self.screen {
            MenuScreen::Home => {
                let mut actions = nav();
                actions.extend((0..self.friends.len().min(1)).map(MenuAction::PlayFriend));
                actions.extend((0..self.realms.len().min(1)).map(MenuAction::PlayRealm));
                actions.extend((0..self.featured.len().min(2)).map(MenuAction::PlayFeatured));
                actions
            }
            MenuScreen::Play => {
                let mut actions = nav();
                actions.extend((0..self.friends.len()).map(MenuAction::PlayFriend));
                actions.extend((0..self.realms.len()).map(MenuAction::PlayRealm));
                actions.extend(
                    self.servers
                        .iter()
                        .enumerate()
                        .filter(|(_, server)| server.last_joined_unix > 0)
                        .map(|(index, _)| MenuAction::PlaySaved(index)),
                );
                actions
            }
            MenuScreen::Social => {
                let mut actions = nav();
                actions.push(MenuAction::RefreshCatalog);
                actions.extend((0..self.friends.len()).map(MenuAction::PlayFriend));
                actions
            }
            MenuScreen::Servers => {
                let mut actions = nav();
                actions.extend([
                    MenuAction::SelectServerTab(MenuServerTab::Featured),
                    MenuAction::SelectServerTab(MenuServerTab::Favorites),
                    MenuAction::SelectServerTab(MenuServerTab::Recent),
                    MenuAction::SelectServerTab(MenuServerTab::Saved),
                    MenuAction::PlayAddServer,
                ]);
                match self.server_tab {
                    MenuServerTab::Featured => {
                        actions.extend((0..self.featured.len()).map(MenuAction::PlayFeatured));
                        actions.extend((0..self.gatherings.len()).map(MenuAction::PlayGathering));
                    }
                    MenuServerTab::Favorites => actions.extend(
                        self.servers
                            .iter()
                            .enumerate()
                            .filter(|(_, server)| server.favorite)
                            .map(|(index, _)| MenuAction::PlaySaved(index)),
                    ),
                    MenuServerTab::Recent => actions.extend(
                        self.servers
                            .iter()
                            .enumerate()
                            .filter(|(_, server)| server.last_joined_unix > 0)
                            .map(|(index, _)| MenuAction::PlaySaved(index)),
                    ),
                    MenuServerTab::Saved => {
                        actions.extend((0..self.servers.len()).map(MenuAction::PlaySaved));
                    }
                }
                actions
            }
            MenuScreen::Profile => {
                let mut actions = nav();
                actions.push(
                    if matches!(
                        self.auth_process.as_ref().map(AuthSupervisor::state),
                        Some(AuthState::Checking | AuthState::AwaitingCode { .. })
                    ) {
                        MenuAction::CancelSignIn
                    } else {
                        MenuAction::StartSignIn
                    },
                );
                actions
            }
            MenuScreen::Settings => {
                let mut actions = nav();
                actions.extend([
                    MenuAction::SettingsScale(1),
                    MenuAction::SettingsScale(2),
                    MenuAction::SettingsScale(3),
                    MenuAction::SettingsScale(4),
                ]);
                actions
            }
            MenuScreen::AddServer => vec![
                MenuAction::AddName,
                MenuAction::AddAddress,
                MenuAction::AddSave,
                MenuAction::AddSaveConnect,
                MenuAction::AddBack,
            ],
            MenuScreen::Pause => vec![
                MenuAction::PauseResume,
                MenuAction::PauseSettings,
                MenuAction::PauseDisconnect,
            ],
        }
    }

    fn enter(&mut self, screen: MenuScreen) {
        self.screen = screen;
        self.focused = 0;
        self.hovered = None;
        self.field = None;
        self.dialog = None;
        self.message = None;
        self.visible = true;
    }

    fn go_back(&mut self) {
        if self.dialog.take().is_some() {
            return;
        }
        match self.screen {
            MenuScreen::Home => {}
            MenuScreen::Pause => self.set_visible(false),
            _ => self.enter(if self.screen == MenuScreen::AddServer {
                MenuScreen::Servers
            } else {
                MenuScreen::Home
            }),
        }
    }

    fn save_draft(&mut self) -> bool {
        let name = self.name.trim();
        let address = self.address.trim();
        if name.is_empty() || address.is_empty() {
            self.message = Some("Enter a server name and address.".to_owned());
            return false;
        }
        let server = SavedServer {
            name: name.to_owned(),
            address: address.to_owned(),
            favorite: false,
            last_joined_unix: 0,
        };
        if let Some(existing) = self
            .servers
            .iter_mut()
            .find(|existing| existing.address.eq_ignore_ascii_case(&server.address))
        {
            let favorite = existing.favorite;
            let last_joined_unix = existing.last_joined_unix;
            *existing = SavedServer {
                favorite,
                last_joined_unix,
                ..server
            };
        } else {
            self.servers.push(server);
        }
        if let Err(error) = save_servers(&self.config_path, &self.servers) {
            self.message = Some(format!("Could not save server: {error}"));
            return false;
        }
        true
    }

    fn request_connect(&mut self, address: String) {
        if address.trim().is_empty() {
            self.message = Some("That server has no address.".to_owned());
            return;
        }
        self.stop_catalog();
        self.pending_connect = Some(address);
        self.mark_connecting();
    }
}

impl Drop for MenuRuntime {
    fn drop(&mut self) {
        self.stop_sign_in();
        self.stop_catalog();
        let _ = fs::remove_file(&self.catalog_path);
    }
}

#[derive(Debug, Resource, Default)]
pub(crate) struct CoreProcessGuard {
    child: Option<Child>,
}

impl CoreProcessGuard {
    pub(crate) fn replace(&mut self, child: Child) {
        self.stop();
        self.child = Some(child);
    }

    pub(crate) fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for CoreProcessGuard {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(crate) fn spawn_core_for_address(socket_dir: &Path, address: &str) -> Result<Child> {
    let executable = core_executable().ok_or_else(|| {
        anyhow::anyhow!(
            "bedrock-core executable was not found beside the client or in target/debug/target/release"
        )
    })?;
    clear_stale_bridge_endpoint(socket_dir)?;
    let mut command = Command::new(&executable);
    command
        .arg("-socket-dir")
        .arg(socket_dir)
        .arg("-upstream")
        .arg(address)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let auth_cache = auth_cache_path();
    if let Some(auth_cache) = auth_cache {
        command.arg("-auth-cache").arg(auth_cache);
    }
    let child = command
        .spawn()
        .with_context(|| format!("spawn {} for {address}", executable.display()))?;
    Ok(child)
}

/// Drops any endpoint publication left behind by an earlier core.
///
/// [`wait_for_core`] can only observe that the endpoint exists, so a stale
/// publication would satisfy it immediately and the client would dial a socket
/// nothing is listening on. Clearing it first means the wait observes the newly
/// spawned core's own bind.
pub(crate) fn clear_stale_bridge_endpoint(socket_dir: &Path) -> Result<()> {
    let endpoint = bridge_endpoint_path(socket_dir);
    match fs::remove_file(&endpoint) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error)
            .with_context(|| format!("remove stale bridge endpoint {}", endpoint.display())),
    }
}

pub(crate) fn wait_for_core(socket_dir: &Path) -> Result<()> {
    for _ in 0..100 {
        if bridge_endpoint_exists(socket_dir) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    bail!(
        "bedrock-core did not publish its endpoint at {}",
        socket_dir.display()
    )
}

fn core_executable() -> Option<PathBuf> {
    let filename = if cfg!(windows) {
        "bedrock-core.exe"
    } else {
        "bedrock-core"
    };
    let current = std::env::current_exe().ok();
    let working = std::env::current_dir().ok();
    let candidates = [
        current
            .as_ref()
            .and_then(|path| path.parent())
            .map(|path| path.join(filename)),
        working.as_ref().map(|path| path.join(filename)),
        working
            .as_ref()
            .map(|path| path.join("target/debug").join(filename)),
        working
            .as_ref()
            .map(|path| path.join("target/release").join(filename)),
    ];
    candidates.into_iter().flatten().find(|path| path.is_file())
}

fn auth_cache_path() -> Option<PathBuf> {
    Some(configured_auth_cache_path()).filter(|path| path.is_file())
}

fn configured_auth_cache_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".local/auth/microsoft-token.json")
}

/// Condenses a runtime error into something that fits the menu message area.
fn session_failure_message(error: &str) -> String {
    const MAX_DETAIL_CHARS: usize = 120;
    let detail = error.trim();
    if detail.is_empty() {
        return "Disconnected from the server.".to_owned();
    }
    let mut condensed: String = detail.chars().take(MAX_DETAIL_CHARS).collect();
    if detail.chars().count() > MAX_DETAIL_CHARS {
        condensed.push('…');
    }
    format!("Disconnected: {condensed}")
}

fn load_servers(path: &Path) -> Vec<SavedServer> {
    let Ok(bytes) = fs::read(path) else {
        return Vec::new();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

fn save_servers(path: &Path, servers: &[SavedServer]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(servers).context("encode saved servers")?;
    fs::write(path, bytes).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
