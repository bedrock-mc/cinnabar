//! Java-inspired launcher/menu state and the small amount of input plumbing
//! needed before a Bedrock session exists.
//!
//! The game client remains the authority for rendering and input. The menu is
//! deliberately retained UI rather than a second windowing toolkit, so the
//! no-argument path is light, keyboard/controller friendly, and uses exactly
//! the same font, safe-area, and pointer coordinates as the gameplay HUD.

mod account;
pub(crate) mod auth;
pub(crate) mod core_process;
mod input;
pub(crate) mod servers;

use auth::{AuthState, AuthSupervisor};

pub(crate) use core_process::{CoreProcessGuard, spawn_core_for_address, wait_for_core};
use core_process::{auth_cache_path, core_executable};
pub(crate) use input::{
    MenuClipboard, drive_menu_connection, drive_menu_input, follow_server_transfer,
    recover_menu_session_failure,
};
use servers::{load_servers, save_servers};

use std::{
    fs,
    path::PathBuf,
    process::Child,
    time::{SystemTime, UNIX_EPOCH},
};

use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};

use crate::{
    install_layout::InstallLayout, session_cleanup::SessionDirectoryGuard,
    ui_runtime::presentation::IconRef,
};

const MAX_SERVER_NAME_BYTES: usize = 64;
const MAX_SERVER_ADDRESS_BYTES: usize = 128;

/// Bounded number of consecutive automatic transfer-follow hops.
///
/// Mirrors the Go core's pre-login transfer-follower limit so a malicious or
/// misconfigured transfer loop ends in a visible menu state instead of
/// reconnecting forever. User-initiated joins always start a fresh chain.
pub(crate) const MAX_TRANSFER_CHAIN_HOPS: u32 = 8;

/// Renders a validated transfer host and port as a dialable address.
///
/// IPv6 literals are bracketed the way the Go core's dialer expects.
pub(crate) fn format_transfer_address(host: &str, port: u16) -> String {
    if host.starts_with('[') && host.ends_with(']') {
        format!("{host}:{port}")
    } else if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

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
    text_selected: bool,
    settings_return_to_pause: bool,
    name: String,
    address: String,
    message: Option<String>,
    gui_scale: u8,
    display_name: String,
    launcher: bool,
    servers: Vec<SavedServer>,
    config_path: PathBuf,
    pending_connect: Option<PendingConnect>,
    connecting: bool,
    disconnect_requested: bool,
    exit_requested: bool,
    session_generation: u64,
    /// Automatic transfer-follow hops remaining in the current chain.
    transfer_hops_remaining: u32,
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
    layout: InstallLayout,
    /// Identity-checked owner of this session's runtime directory; bound
    /// once a connect attempt provisions it and released on disconnect,
    /// session failure, exit, or drop.
    session_directory: Option<SessionDirectoryGuard>,
}

#[derive(Debug)]
struct PendingConnect {
    address: String,
    auth_cache: Option<PathBuf>,
}

impl MenuRuntime {
    #[cfg(test)]
    pub(crate) fn new(visible: bool, gui_scale: u8, display_name: String) -> Self {
        Self::new_with_layout(
            visible,
            gui_scale,
            display_name,
            InstallLayout::discover().expect("test executable must have a development layout"),
        )
    }

    pub(crate) fn new_with_layout(
        visible: bool,
        gui_scale: u8,
        display_name: String,
        layout: InstallLayout,
    ) -> Self {
        let config_path = layout.server_file();
        let loaded = load_servers(&config_path);
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
            text_selected: false,
            settings_return_to_pause: false,
            name: String::new(),
            address: String::new(),
            message: loaded.recovery_message,
            gui_scale: gui_scale.clamp(1, 4),
            display_name,
            servers: loaded.servers,
            config_path,
            pending_connect: None,
            connecting: false,
            disconnect_requested: false,
            exit_requested: false,
            session_generation: 1,
            transfer_hops_remaining: MAX_TRANSFER_CHAIN_HOPS,
            featured: Vec::new(),
            gatherings: Vec::new(),
            realms: Vec::new(),
            friends: Vec::new(),
            catalog_message: None,
            catalog_started: false,
            catalog_path: layout.catalog_file(std::process::id()),
            catalog_process: None,
            auth_process: None,
            auth_attempted: false,
            auth_restart_requested: false,
            layout,
            session_directory: None,
        }
    }

    pub(crate) fn is_visible(&self) -> bool {
        self.visible
    }

    pub(crate) fn is_launcher(&self) -> bool {
        self.launcher
    }

    pub(crate) fn is_connecting(&self) -> bool {
        self.connecting
    }

    pub(crate) fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
        if !visible {
            self.field = None;
            self.text_selected = false;
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
        self.settings_return_to_pause = false;
        self.message = None;
        self.visible = true;
    }

    fn take_pending_connect(&mut self) -> Option<PendingConnect> {
        if self
            .auth_process
            .as_ref()
            .is_some_and(|process| !process.cleanup_complete())
        {
            return None;
        }
        self.pending_connect.take()
    }

    pub(crate) fn mark_connected(&mut self) {
        self.connecting = false;
        self.visible = false;
        self.screen = MenuScreen::Home;
        self.message = None;
        self.field = None;
        self.text_selected = false;
        self.settings_return_to_pause = false;
    }

    pub(crate) fn mark_connecting(&mut self) {
        self.connecting = true;
        self.visible = true;
        self.screen = MenuScreen::Play;
        self.settings_return_to_pause = false;
        self.message = Some("Connecting…".to_owned());
    }

    pub(crate) fn mark_disconnected(&mut self) {
        self.visible = true;
        self.screen = MenuScreen::Home;
        self.focused = 0;
        self.connecting = false;
        self.field = None;
        self.text_selected = false;
        self.settings_return_to_pause = false;
        self.dialog = None;
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
        self.text_selected = false;
        self.settings_return_to_pause = false;
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

    /// Takes ownership of the bound session directory, releasing any
    /// previous binding first so at most one session directory is live.
    pub(crate) fn bind_session_directory(&mut self, directory: SessionDirectoryGuard) {
        self.session_directory = Some(directory);
    }

    /// Releases the session runtime directory now (after the core has been
    /// stopped); a no-op when nothing is bound.
    pub(crate) fn release_session_directory(&mut self) {
        self.session_directory = None;
    }

    pub(crate) fn activate(&mut self, action: MenuAction) {
        if let Some(index) = self
            .focus_actions()
            .iter()
            .position(|candidate| *candidate == action)
        {
            self.focused = index;
        }
        match action {
            MenuAction::AddName => self.focus_field(MenuField::Name),
            MenuAction::AddAddress => self.focus_field(MenuField::Address),
            _ => {
                self.field = None;
                self.text_selected = false;
            }
        }
        self.pressed = Some(action);
        self.message = None;
        match action {
            MenuAction::Navigate(screen) => {
                self.settings_return_to_pause = false;
                self.enter(screen);
            }
            MenuAction::OpenExitDialog => {
                self.dialog = Some(MenuDialog::Exit);
                self.focused = 0;
            }
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
                self.enter(MenuScreen::AddServer);
                self.focus_field(MenuField::Name);
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
                    self.focused = 0;
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
            MenuAction::AddName | MenuAction::AddAddress => {}
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
            MenuAction::PauseSettings => {
                self.enter(MenuScreen::Settings);
                self.settings_return_to_pause = true;
            }
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
        match actions[self.focused] {
            MenuAction::AddName => self.focus_field(MenuField::Name),
            MenuAction::AddAddress => self.focus_field(MenuField::Address),
            _ => {
                self.field = None;
                self.text_selected = false;
            }
        }
    }

    pub(crate) fn activate_focused(&mut self) {
        let Some(action) = self.focus_actions().get(self.focused).copied() else {
            return;
        };
        self.activate(action);
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
        self.text_selected = false;
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
            MenuScreen::Settings if self.settings_return_to_pause => {
                self.settings_return_to_pause = false;
                self.enter(MenuScreen::Pause);
            }
            _ => {
                self.settings_return_to_pause = false;
                self.enter(if self.screen == MenuScreen::AddServer {
                    MenuScreen::Servers
                } else {
                    MenuScreen::Home
                });
            }
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
        // A user-initiated join always starts a fresh transfer chain.
        self.begin_fresh_transfer_chain();
        self.stop_catalog();
        let auth_cache = account::validated_auth_cache(
            &self.layout,
            self.auth_process.as_ref().map(AuthSupervisor::state),
        );
        self.stop_sign_in();
        self.pending_connect = Some(PendingConnect {
            address,
            auth_cache,
        });
        self.mark_connecting();
    }

    /// Starts a fresh bounded transfer-follow chain for a user join.
    pub(crate) fn begin_fresh_transfer_chain(&mut self) {
        self.transfer_hops_remaining = MAX_TRANSFER_CHAIN_HOPS;
    }

    /// Consumes one hop of the bounded automatic transfer-follow chain.
    ///
    /// Returns `false` when the chain is exhausted; the caller must surface
    /// the explicit cannot-follow state instead of reconnecting again.
    pub(crate) fn consume_transfer_chain_hop(&mut self) -> bool {
        if self.transfer_hops_remaining == 0 {
            return false;
        }
        self.transfer_hops_remaining -= 1;
        true
    }

    /// Prepares the replacement-handoff target for a server-directed
    /// transfer without staging a user connect.
    ///
    /// Well-formedness only, exactly like the protocol boundary: no host
    /// allowlist exists because vanilla servers legitimately transfer across
    /// unrelated hosts. Returns `None` for an unusable target.
    pub(crate) fn transfer_handoff_target(
        &self,
        host: &str,
        port: u16,
    ) -> Option<(String, Option<PathBuf>)> {
        let trimmed = host.trim();
        if trimmed.is_empty() {
            return None;
        }
        let address = format_transfer_address(trimmed, port);
        let auth_cache = account::validated_auth_cache(
            &self.layout,
            self.auth_process.as_ref().map(AuthSupervisor::state),
        );
        Some((address, auth_cache))
    }
}

impl Drop for MenuRuntime {
    fn drop(&mut self) {
        self.stop_sign_in();
        self.stop_catalog();
        let _ = fs::remove_file(&self.catalog_path);
    }
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

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
