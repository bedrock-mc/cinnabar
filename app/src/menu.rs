//! Java-inspired launcher/menu state and the small amount of input plumbing
//! needed before a Bedrock session exists.
//!
//! The game client remains the authority for rendering and input. The menu is
//! deliberately retained UI rather than a second windowing toolkit, so the
//! no-argument path is light, keyboard/controller friendly, and uses exactly
//! the same font, safe-area, and pointer coordinates as the gameplay HUD.

mod account;
mod account_control;
mod accounts;
pub(crate) mod auth;
mod construction;
pub(crate) mod core_process;
pub(crate) mod disconnect;
mod dressing_room;
#[cfg(test)]
mod flow_tests;
mod focus;
pub(crate) mod inbox;
mod input;
pub(crate) mod launcher_account;
mod launcher_core;
pub(crate) use launcher_core::target_for;
mod navigation;
#[cfg(test)]
mod server_input_tests;
pub(crate) mod server_trust;
pub(crate) mod servers;
#[cfg(test)]
mod session_teardown_tests;
pub(crate) mod settings_options;
mod settings_paths;
pub(crate) mod settings_storage;
pub(crate) mod settings_support;
mod settings_values;
mod sign_in_popup;
#[cfg(test)]
mod transfer_follow_tests;
mod video_settings;
mod worlds_tab;

use auth::{AuthState, AuthSupervisor};
use ui::RenderMode;

pub(crate) use core_process::{CoreProcessGuard, spawn_core_for_address, wait_for_core};
use core_process::{auth_cache_path, core_executable};
pub(crate) use input::{MenuClipboard, drive_menu_input};
use launcher::menu::view::{CatalogFile, MenuFeeds};
#[cfg(test)]
pub(crate) use launcher::menu::view::{InboxItem, JoinKind, JoinProgress, JoinStage, MenuHome};
pub(crate) use launcher::menu::view::{
    LocalWorldCard, MenuFriendCard, MenuRealmCard, MenuServerCard, MenuView, SavedServer,
};
pub(crate) use launcher_core::LauncherCoreSlot;
use servers::{ServerWriter, load_servers};
pub(crate) use video_settings::persist_video_settings;
pub(crate) use worlds_tab::LocalWorldAction;

use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use bevy::prelude::{Commands, Res, ResMut, Resource};

use crate::{
    install_layout::InstallLayout,
    runtime::world::ClientWorld,
    session::{JoinIntent, SessionStatus},
};
use client_ui::ui_runtime::UiRuntime;

const MAX_SERVER_NAME_BYTES: usize = 64;
const MAX_SERVER_ADDRESS_BYTES: usize = 128;
/// Vanilla's port box: six number characters, prefilled with the Bedrock default.
const MAX_SERVER_PORT_BYTES: usize = 6;
use launcher::menu::DEFAULT_PORT;

pub(crate) use launcher::menu::split_address;

pub(crate) use launcher::menu::{MenuAction, MenuDialog, MenuField, MenuScreen, MenuServerTab};

#[derive(Debug, Resource)]
pub(crate) struct MenuRuntime {
    visible: bool,
    screen: MenuScreen,
    focused: usize,
    hovered: Option<MenuAction>,
    pressed: Option<MenuAction>,
    pointer_down: bool,
    server_tab: MenuServerTab,
    profile_tab: launcher::menu::ProfileTab,
    dialog: Option<MenuDialog>,
    field: Option<MenuField>,
    /// Bumped by every field edit and caret move, restarting the caret blink.
    caret_revision: u64,
    /// Screens opened on the way here; back returns to the one below.
    history: json_ui::ScreenNav<MenuScreen>,
    /// The Add/Edit Server boxes, each typed through the chat editor's caret model.
    name: ui::ChatEditor,
    address: ui::ChatEditor,
    port: ui::ChatEditor,
    skin_name: ui::ChatEditor,
    message: Option<String>,
    gui_scale_preference: Option<u8>,
    gui_scale_offset: i8,
    gui_scale_display_offset: i8,
    gui_scale_choices: Vec<ui::DesktopGuiScaleChoice>,
    fullscreen: bool,
    fullscreen_change: Option<bool>,
    video_settings_writer: Option<video_settings::writer::Writer>,
    settings_focus: Vec<MenuAction>,
    settings_focus_geometry: focus::SettingsFocusGeometry,
    last_saved_video_settings: video_settings::SavedVideoSettings,
    failed_video_settings_save: Option<video_settings::SavedVideoSettings>,
    render_mode: RenderMode,
    render_mode_request: Option<RenderMode>,
    vsync_override: Option<bool>,
    display_name: String,
    launcher: bool,
    servers: Vec<SavedServer>,
    config_path: PathBuf,
    saves: ServerWriter,
    intents: SessionIntents,
    /// The session controller's last published state.
    session: SessionStatus,
    featured: Vec<MenuServerCard>,
    realms: Vec<MenuRealmCard>,
    friends: Vec<MenuFriendCard>,
    catalog_message: Option<String>,
    catalog_started: bool,
    catalog_path: PathBuf,
    catalog_process: Option<crate::lifecycle::children::Spawned>,
    auth_process: Option<AuthSupervisor>,
    auth_attempted: bool,
    auth_restart_requested: bool,
    layout: InstallLayout,
    /// The client's own skin, cloned into every reconnection's `NetworkConfig`.
    player_skin: crate::player_skin::LocalPlayerSkin,
    dressing_room: std::sync::Arc<launcher::dressing_room::DressingRoomView>,
    dressing_room_worker: Option<dressing_room::Worker>,
    skin_update_pending: bool,
    skin_outbound: Option<(u64, protocol::Packet)>,
    skin_packet_pending: Option<protocol::Packet>,
    editing: Option<usize>,
    settings_section: u8,
    disconnect_message: Option<String>,
    /// Death screen shown for the current death; cleared once alive again.
    death_shown: bool,
    local_worlds: Vec<LocalWorldCard>,
    local_world_requested: Option<usize>,
    local_ui: worlds_tab::LocalWorldsUi,
    /// Sign-in state reported by the core's account control, when bound.
    control_auth: Option<AuthState>,
    /// Developer recordings present placeholder accounts; nothing signs in or touches the store.
    presentation_accounts: bool,
    /// The device code whose sign-in page was last opened, so each code opens once.
    sign_in_page_code: Option<String>,
    sign_out_requested: bool,
    accounts: accounts::Manager,
    /// Marketplace actions waiting for the store driver.
    store_actions: Vec<crate::store::StoreAction>,
    pub(crate) global_resource_actions: Vec<crate::global_resources::Action>,
    pub(crate) global_resources: std::sync::Arc<crate::global_resources::Snapshot>,
    /// The Marketplace's presented state while its screen is up.
    store_snapshot: Option<std::sync::Arc<launcher::store::snapshot::StoreSnapshot>>,
    settings_options: std::sync::Arc<settings_options::SettingsOptions>,
    storage: std::sync::Arc<settings_storage::StorageView>,
    settings_dropdown: Option<u16>,
    settings_scale_picker: bool,
    settings_dirty: bool,
    /// Failed writes wait until this deadline while retaining the newest edits.
    settings_retry_at: Option<std::time::Instant>,
    settings_apply: bool,
    /// In-memory option overrides (index, persisted value) that saves never write.
    session_overrides: Vec<(usize, i32)>,
    /// A developer controller is driving: hotkey toggles stay in memory.
    transient_toggles: bool,
    language_choices: std::sync::Arc<[(String, String)]>,
    language_pending: bool,
    language_asset_path: PathBuf,
    settings_slider_drag: Option<u16>,
    settings_slider_pointer: Option<launcher::menu::view::SettingsSliderPointer>,
    settings_slider_hovered: Option<u16>,
    settings_slider_selected: Option<u16>,
    settings_control_activation: Option<(MenuAction, u64)>,
    settings_control_activation_navigation: bool,
    settings_input_revision: u64,
    input_mode: input::MenuInputMode,
    key_remap: Option<u16>,
    settings_advanced_graphics: bool,
    /// The current or pending session is a local world, and whether it was live last frame.
    local_world_joined: bool,
    local_world_active: bool,
    feeds: MenuFeeds,
}

/// Session requests raised by menu actions, for the session controller to take.
#[derive(Debug, Default)]
struct SessionIntents {
    join: Option<JoinIntent>,
    disconnect: bool,
    respawn: bool,
    exit: bool,
}

impl MenuRuntime {
    /// Mirrors the applied mode; a pending menu toggle wins until taken.
    pub(crate) fn sync_render_mode(&mut self, applied: RenderMode) {
        if self.render_mode_request.is_none() {
            self.render_mode = applied;
        }
    }

    /// Shows the VSync toggle locked to a launch-flag override.
    #[must_use]
    pub(crate) const fn with_vsync_override(mut self, vsync: Option<bool>) -> Self {
        self.vsync_override = vsync;
        self
    }

    /// Consume the pending Video-section change.
    pub(crate) fn take_render_mode_request(&mut self) -> Option<RenderMode> {
        self.render_mode_request.take()
    }

    /// Return the extension settings file alongside the other user settings.
    pub(crate) fn graphics_file(&self) -> PathBuf {
        self.layout.graphics_file()
    }

    pub(crate) fn is_visible(&self) -> bool {
        self.visible
    }

    /// Settings retains the background of the launcher or world beneath it.
    pub(crate) fn uses_panorama(&self) -> bool {
        self.visible
            && match self.screen {
                MenuScreen::Pause | MenuScreen::Death => false,
                MenuScreen::Settings | MenuScreen::DressingRoom => !self.over_world(),
                _ => true,
            }
    }

    pub(crate) fn screen(&self) -> MenuScreen {
        self.screen
    }

    pub(crate) fn player_skin(&self) -> &crate::player_skin::LocalPlayerSkin {
        &self.player_skin
    }

    pub(crate) fn is_launcher(&self) -> bool {
        self.launcher
    }

    /// A join is queued or in progress.
    pub(crate) fn is_connecting(&self) -> bool {
        self.session.connecting || self.intents.join.is_some()
    }

    pub(crate) fn observe_session(&mut self, status: SessionStatus) {
        self.session = status;
    }

    pub(crate) fn layout(&self) -> &InstallLayout {
        &self.layout
    }

    pub(crate) fn display_name(&self) -> &str {
        &self.display_name
    }

    pub(crate) fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
        if !visible {
            self.field = None;
            self.settings_slider_selected = None;
            self.settings_slider_pointer = None;
            self.settings_slider_hovered = None;
            self.dialog = None;
        }
    }

    pub(crate) fn view(&self) -> MenuView {
        // A sign-in in flight outranks the core's report, which outranks a finished helper.
        let supervisor = self
            .auth_process
            .as_ref()
            .map(|process| process.state().clone());
        let auth_state = match (supervisor, self.control_auth.clone()) {
            (Some(state @ (AuthState::Checking | AuthState::AwaitingCode { .. })), _) => state,
            (_, Some(control)) => control,
            (supervisor, None) => supervisor.unwrap_or(AuthState::SignedOut),
        };
        let catalog_loading = matches!(
            &auth_state,
            AuthState::Checking | AuthState::AwaitingCode { .. }
        ) || (auth_state == AuthState::Authenticated
            && (!self.catalog_started || self.catalog_process.is_some()));
        let auth_state = if self.presentation_accounts {
            AuthState::Authenticated
        } else {
            auth_state
        };
        MenuView {
            visible: self.visible,
            over_world: self.over_world(),
            screen: self.screen,
            focused_action: self.focus_actions().get(self.focused).copied(),
            hovered: self.hovered,
            pressed: self.pressed,
            navigation_focus_visible: self.input_mode.navigation(),
            gamepad_input: self.input_mode.gamepad(),
            server_tab: self.server_tab,
            profile_tab: self.profile_tab,
            dialog: self.dialog,
            field: self.field,
            caret: self.caret(),
            name: self.name.as_str().to_owned(),
            address: self.address.as_str().to_owned(),
            port: self.port.as_str().to_owned(),
            message: self.message.clone(),
            gui_scale_offset: self.gui_scale_display_offset,
            gui_scale_choices: self.gui_scale_choices.clone(),
            fullscreen: self.fullscreen,
            render_mode: self.render_mode,
            vsync_override: self.vsync_override,
            display_name: self.presented_display_name(),
            servers: self.servers.clone(),
            featured: self.featured.clone(),
            realms: self.realms.clone(),
            friends: self.friends.clone(),
            featured_icon: None,
            realm_icon: None,
            friend_icon: None,
            saved_icon: None,
            profile_icon: None,
            catalog_loading,
            catalog_message: self.catalog_message.clone(),
            auth_state,
            connecting: self.is_connecting(),
            settings_section: self.settings_section,
            dressing_room: self.dressing_room.clone(),
            player_skin: Some(self.player_skin.standard_skin()),
            player_skin_model: self.player_skin.model(),
            disconnect_message: self.disconnect_message.clone(),
            editing: self.editing,
            local_worlds: self.local_worlds.clone(),
            local: self.local_view(),
            settings_options: std::sync::Arc::clone(&self.settings_options),
            storage: std::sync::Arc::clone(&self.storage),
            settings_dropdown: self.settings_dropdown,
            settings_scale_picker: self.settings_scale_picker,
            settings_control_activation: self.settings_control_activation,
            settings_control_activation_navigation: self.settings_control_activation_navigation,
            settings_slider_pointer: self.settings_slider_pointer,
            settings_slider_hovered: self.settings_slider_hovered,
            settings_slider_selected: self.settings_slider_selected,
            language_choices: std::sync::Arc::clone(&self.language_choices),
            key_remap: self.key_remap,
            settings_advanced_graphics: self.settings_advanced_graphics,
            feeds: self.presented_feeds(),
            store: self.store_snapshot.clone(),
            global_resources: self.global_resources.clone(),
        }
    }

    /// Marketplace actions queued since the last call, for the store driver.
    pub(crate) fn take_store_actions(&mut self) -> Vec<crate::store::StoreAction> {
        std::mem::take(&mut self.store_actions)
    }

    /// Publish (or clear) the Marketplace's presented state.
    pub(crate) fn set_store_snapshot(
        &mut self,
        snapshot: Option<std::sync::Arc<launcher::store::snapshot::StoreSnapshot>>,
    ) {
        self.store_snapshot = snapshot;
    }

    pub(crate) fn in_store(&self) -> bool {
        self.screen == MenuScreen::Store
    }

    /// Leave the Marketplace for the start screen.
    pub(crate) fn leave_store(&mut self) {
        if self.in_store() {
            self.enter(MenuScreen::Home);
        }
    }

    /// The local worlds the worlds tab lists (the local-worlds module feeds it).
    pub(crate) fn set_local_worlds(&mut self, worlds: Vec<LocalWorldCard>) {
        self.local_worlds = worlds;
    }

    /// A local world the player chose to open, for the local-worlds module.
    pub(crate) fn take_local_world_request(&mut self) -> Option<usize> {
        self.local_world_requested.take()
    }

    /// Show the death screen once per death (health reached zero in play).
    pub(crate) fn open_death(&mut self) {
        if self.visible || self.is_connecting() || self.death_shown {
            return;
        }
        self.death_shown = true;
        self.history.reset(MenuScreen::Death);
        self.show_top();
    }

    /// Health came back above zero: a later death shows the screen again.
    pub(crate) fn note_player_alive(&mut self) {
        self.death_shown = false;
        if self.screen == MenuScreen::Death && self.visible {
            self.set_visible(false);
            self.history.reset(MenuScreen::Home);
            self.screen = MenuScreen::Home;
        }
    }

    /// The death screen's respawn press, for the session to send once.
    pub(crate) fn take_respawn_request(&mut self) -> bool {
        std::mem::take(&mut self.intents.respawn)
    }

    pub(crate) fn open_pause(&mut self) {
        if self.visible || self.is_connecting() {
            return;
        }
        self.history.reset(MenuScreen::Pause);
        self.screen = MenuScreen::Pause;
        self.focused = 0;
        self.message = None;
        self.visible = true;
    }

    /// The queued join, once any sign-in helper has finished cleaning up.
    pub(crate) fn take_join_intent(&mut self) -> Option<JoinIntent> {
        if self
            .auth_process
            .as_ref()
            .is_some_and(|process| !process.cleanup_complete())
        {
            return None;
        }
        self.intents.join.take()
    }

    /// The session is live: the menu gives way to the world.
    pub(crate) fn show_world(&mut self) {
        self.visible = false;
        self.history.reset(MenuScreen::Home);
        self.screen = MenuScreen::Home;
        self.message = None;
        self.field = None;
    }

    pub(crate) fn show_connecting(&mut self) {
        self.visible = true;
        self.history.reset(MenuScreen::Home);
        self.history.push(MenuScreen::Play);
        self.screen = MenuScreen::Play;
        self.message = Some("Connecting…".to_owned());
    }

    pub(crate) fn show_home(&mut self) {
        self.visible = true;
        self.history.reset(MenuScreen::Home);
        self.screen = MenuScreen::Home;
        self.focused = 0;
        self.field = None;
        self.dialog = None;
    }

    /// A cancelled join drops any queued join and returns to the play screen.
    pub(crate) fn cancel_join(&mut self) {
        self.intents.join = None;
        self.enter(MenuScreen::Play);
    }

    pub(crate) fn show_join_failure(&mut self, message: String) {
        self.message = Some(message);
    }

    pub(crate) fn show_transfer(&mut self, address: &str) {
        self.message = Some(format!("Transferring to {address}…"));
    }

    /// Resets the join progress screen for a join to `address`.
    pub(crate) fn begin_join_progress(&mut self, address: &str, local_world: bool) {
        self.feeds.join =
            launcher::menu::view::JoinProgress::new(launcher_core::join_kind(address, local_world));
    }

    /// Returns the session back to the launcher after a fatal session error.
    ///
    /// Returns `false` when the client was started with `--address`, which has
    /// no launcher to fall back to and must still exit the process.
    pub(crate) fn absorb_session_failure(&mut self, error: &str) -> bool {
        if !self.launcher {
            return false;
        }
        self.visible = true;
        self.history.reset(MenuScreen::Home);
        self.history.push(MenuScreen::Play);
        self.screen = MenuScreen::Play;
        self.dialog = None;
        self.field = None;
        // The raw chain is for the log; the disconnect screen words it as vanilla does.
        bevy::log::warn!(error, "session ended");
        self.message = None;
        self.disconnect_message = Some(error.to_owned());
        // Let the account catalog repopulate now that the session is gone.
        self.catalog_started = false;
        true
    }

    pub(crate) fn take_disconnect_request(&mut self) -> bool {
        std::mem::take(&mut self.intents.disconnect)
    }

    pub(crate) fn take_exit_request(&mut self) -> bool {
        std::mem::take(&mut self.intents.exit)
    }

    pub(crate) fn activate_from_input(&mut self, action: MenuAction) {
        self.activate_control_input(action, false);
    }

    pub(crate) fn activate_from_navigation(&mut self, action: MenuAction) {
        self.activate_control_input(action, true);
    }

    fn activate_control_input(&mut self, action: MenuAction, navigation: bool) {
        if self.screen == MenuScreen::Settings {
            self.settings_input_revision = self.settings_input_revision.wrapping_add(1);
            self.settings_control_activation = Some((action, self.settings_input_revision));
            self.settings_control_activation_navigation = navigation;
        }
        self.activate(action);
    }

    pub(crate) fn activate(&mut self, action: MenuAction) {
        if self.skin_editor_blocks(action) || self.presentation_blocks(action) {
            return;
        }
        if self.account_change_pending()
            && matches!(
                action,
                MenuAction::PlaySaved(_)
                    | MenuAction::PlayFeatured(_)
                    | MenuAction::PlayRealm(_)
                    | MenuAction::PlayFriend(_)
                    | MenuAction::PlayLocalWorld(_)
                    | MenuAction::OpenLiveEvent
                    | MenuAction::LocalWorld(_)
            )
        {
            self.message = Some("Please wait for the account change to finish.".into());
            return;
        }
        if let Some(index) = self
            .focus_actions()
            .iter()
            .position(|candidate| *candidate == action)
        {
            self.focused = index;
        }
        match action.text_field() {
            Some(field) => self.focus_field(field),
            None => self.field = None,
        }
        self.pressed = Some(action);
        self.message = None;
        self.disconnect_message = None;
        match action {
            MenuAction::SelectProfileTab(tab) => self.profile_tab = tab,
            MenuAction::RefreshProfile => {
                self.feeds.profile.loaded = false;
                self.feeds.profile.unavailable = false;
                self.feeds.profile_refresh_requested = true;
            }
            MenuAction::Inbox(action) => self.activate_inbox(action),
            MenuAction::Navigate(screen) => {
                if screen == MenuScreen::DressingRoom {
                    self.ensure_dressing_room();
                }
                self.enter(screen);
            }
            MenuAction::OpenExitDialog => {
                self.dialog = Some(MenuDialog::Exit);
                self.focused = 0;
            }
            MenuAction::ConfirmExit => {
                self.dialog = None;
                self.intents.exit = true;
            }
            MenuAction::DismissDialog => self.dismiss_accounts(),
            MenuAction::OpenAccounts => self.open_accounts(),
            MenuAction::AddAccount => self.add_account(),
            MenuAction::SwitchAccount(index) => self.switch_account(index),
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
            MenuAction::CancelSignIn => {
                if self.feeds.account_adding {
                    self.cancel_add_account();
                } else {
                    self.stop_sign_in();
                }
            }
            MenuAction::PlayAddServer => {
                self.editing = None;
                self.name.clear();
                self.address.clear();
                self.port.set_text(DEFAULT_PORT);
                self.enter(MenuScreen::AddServer);
                self.focus_field(MenuField::Name);
            }
            MenuAction::PlaySaved(index) => {
                if index < self.servers.len() {
                    self.servers[index].last_joined_unix = now_unix();
                    let address = self.servers[index].address.clone();
                    self.save_servers();
                    self.request_connect(address);
                }
            }
            MenuAction::PlayFeatured(index) => {
                if let Some(server) = self.featured.get(index) {
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
                    self.save_servers();
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
                    self.feeds.selected_saved = None;
                    self.save_servers();
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
            MenuAction::AddName
            | MenuAction::AddAddress
            | MenuAction::AddPort
            | MenuAction::EditSkinName => {}
            MenuAction::AddSave => {
                // Saving pops the form back to the tab that opened it.
                if self.save_draft() {
                    self.go_back();
                }
            }
            MenuAction::AddSaveConnect => {
                if self.save_draft() {
                    self.request_connect(self.draft_endpoint());
                }
            }
            MenuAction::AddBack => self.go_back(),
            MenuAction::ToggleRenderMode => {
                if render_model::ENHANCED_RENDERING_ENABLED {
                    self.render_mode = self.render_mode.toggled();
                    self.render_mode_request = Some(self.render_mode);
                }
            }
            // The game menu opened from the death screen returns to it.
            MenuAction::PauseResume if self.death_shown => {
                self.history.reset(MenuScreen::Death);
                self.show_top();
            }
            MenuAction::PauseResume => self.set_visible(false),
            MenuAction::PauseDisconnect => {
                self.intents.disconnect = true;
                self.set_visible(false);
            }
            MenuAction::PauseSettings => {
                self.enter(MenuScreen::Settings);
            }
            MenuAction::EditSaved(index) => {
                if let Some(server) = self.servers.get(index) {
                    self.name.set_text(&server.name);
                    let (address, port) = split_address(&server.address);
                    self.address.set_text(&address);
                    self.port.set_text(&port);
                    self.enter(MenuScreen::AddServer);
                    self.editing = Some(index);
                    self.focus_field(MenuField::Name);
                }
            }
            MenuAction::SettingsStorage(action) => self.activate_storage(action),
            MenuAction::SettingsSupport(action) => self.activate_support(action),
            action @ (MenuAction::SettingsScale(_)
            | MenuAction::SettingsScalePicker
            | MenuAction::SettingsFullscreen(_)
            | MenuAction::SettingsSection(_)
            | MenuAction::SettingsOption(..)
            | MenuAction::SettingsLanguage(_)
            | MenuAction::SettingsDropdown(_)
            | MenuAction::SettingsResetBindings(_)
            | MenuAction::SettingsResetGroup(_)
            | MenuAction::SettingsConfirmResetGroup(_)
            | MenuAction::SettingsConfirmResetBindings(_)
            | MenuAction::SettingsKey(_)
            | MenuAction::SettingsResetKey(_)
            | MenuAction::SettingsResetChat
            | MenuAction::SettingsAdvancedGraphics) => self.activate_settings(action),
            MenuAction::Respawn => {
                self.intents.respawn = true;
                self.set_visible(false);
            }
            MenuAction::SignOut => self.sign_out_requested = true,
            MenuAction::SelectFeatured(index) => self.feeds.select(index),
            MenuAction::SelectSaved(index) => self.feeds.select_saved(index),
            MenuAction::ServerList(action) => {
                self.settings_dirty |=
                    std::sync::Arc::make_mut(&mut self.settings_options).apply_server_list(action);
            }
            MenuAction::SelectRealm(index) => self.feeds.selected_realm = Some(index),
            MenuAction::ToggleReadMore(section) => self.feeds.toggle_read_more(section),
            MenuAction::OpenLiveEvent => self.open_live_event(),
            MenuAction::GlobalResources(action) => self.global_resource_actions.push(action),
            MenuAction::DressingRoom(action) => self.activate_dressing_room(action),
            MenuAction::Store(action) => {
                if action == crate::store::StoreAction::Open {
                    self.enter(MenuScreen::Store);
                }
                self.store_actions.push(action);
            }
            MenuAction::PlayLocalWorld(index) => {
                if index < self.local_worlds.len() {
                    self.local_world_requested = Some(index);
                }
            }
            MenuAction::LocalWorld(action) => self.queue_local_action(action),
            MenuAction::ServerTrust(trusted) => self.answer_server_trust(trusted),
        }
    }

    /// The draft's `host:port`; a host typed with its own port keeps it.
    fn draft_endpoint(&self) -> String {
        let host = self.address.as_str().trim();
        let port = self.port.as_str().trim();
        let own_port = host.contains("]:")
            || host.split_once(':').is_some_and(|(_, rest)| {
                !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
            });
        match (
            own_port || host.is_empty(),
            port.is_empty(),
            host.contains(':'),
        ) {
            (true, _, _) | (_, true, _) => host.to_owned(),
            (false, false, true) => format!("[{host}]:{port}"),
            (false, false, false) => format!("{host}:{port}"),
        }
    }

    fn save_draft(&mut self) -> bool {
        let name = self.name.as_str().trim();
        let endpoint = self.draft_endpoint();
        let address = endpoint.trim();
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
        if let Some(existing) = self.editing.and_then(|index| self.servers.get_mut(index)) {
            existing.name = server.name;
            existing.address = server.address;
        } else if let Some(existing) = self
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
        if let Err(error) = self.saves.save(&self.servers) {
            self.message = Some(format!("Could not save server: {error}"));
            return false;
        }
        true
    }

    /// Queues the list for writing; a schema refusal leaves only the log.
    fn save_servers(&mut self) {
        if let Err(error) = self.saves.save(&self.servers) {
            bevy::log::warn!("saved servers not written: {error:#}");
        }
    }

    /// Surfaces a saved-server write that failed on the worker.
    pub(crate) fn poll_saves(&mut self) {
        if let Some(error) = self.saves.take_error() {
            self.message = Some(format!("Could not save servers: {error}"));
        }
    }

    /// Joins the live event's venue, or opens the Servers tab when it routes there.
    fn open_live_event(&mut self) {
        let Some(event) = self.feeds.home.live_event.clone() else {
            return;
        };
        if event.route_to_servers || event.address.is_empty() {
            self.enter(MenuScreen::Servers);
        } else {
            self.request_connect(event.address);
        }
    }

    /// Queues a join to `address` for the session controller.
    pub(crate) fn request_connect(&mut self, address: String) {
        if self.account_change_pending() {
            self.message = Some("Please wait for the account change to finish.".into());
            return;
        }
        if address.trim().is_empty() {
            self.message = Some("That server has no address.".to_owned());
            return;
        }
        self.stop_catalog();
        let auth_cache = self.launcher_auth_cache();
        self.stop_sign_in();
        self.local_world_joined = false;
        self.intents.join = Some(JoinIntent {
            address,
            auth_cache,
            local_world: false,
        });
        self.show_connecting();
    }

    /// The featured server `address` joins, as Discord's corner art, when it has a logo URL.
    pub(crate) fn featured_badge(&self, address: &str) -> Option<rich_presence::Badge> {
        let target = target_for(address);
        let server = self
            .featured
            .iter()
            .find(|server| target_for(&server.address) == target)?;
        let image_url = &self.feeds.details.get(&server.address)?.logo_url;
        (!image_url.is_empty()).then(|| rich_presence::Badge {
            image_url: image_url.clone(),
            name: server.name.clone(),
        })
    }
}

/// Drives the launcher's own services: catalog, saves, settings, the account core and local worlds.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_menu_services(
    mut commands: Commands,
    mut menu: ResMut<MenuRuntime>,
    client_blob_cache: Res<crate::app::ClientBlobCacheOwner>,
    mut client_world: ResMut<ClientWorld>,
    mut runtime: ResMut<UiRuntime>,
    launcher: Option<ResMut<LauncherCoreSlot>>,
    launcher_account: Option<ResMut<launcher_account::LauncherAccount>>,
    mut local_worlds: Option<ResMut<crate::local_worlds::LocalWorlds>>,
    audio_settings: Option<ResMut<crate::audio::AudioSettings>>,
    settings: Option<ResMut<crate::settings_runtime::RuntimeSettings>>,
    mut local_skin: Option<ResMut<crate::player_skin::LocalPlayerSkin>>,
    network: Option<Res<crate::runtime::network::NetworkHandle>>,
) {
    menu.poll_dressing_room(
        local_skin.as_deref_mut(),
        &mut client_world,
        network.as_deref(),
        runtime.session_id(),
    );
    menu.poll_catalog(launcher_account.is_some());
    menu.poll_saves();
    menu.poll_accounts();
    menu.sync_audio_settings(audio_settings);
    menu.sync_user_settings(settings);
    menu.sync_language(&mut runtime);
    let in_session = client_world.stream.is_some();
    if let Some(mut slot) = launcher {
        // Remote direct sessions have a separate game core. Local worlds use
        // the account core, so sign-in must not restart it during local play.
        let idle = launcher_core::account_core_idle(
            menu.is_launcher(),
            menu.is_connecting(),
            in_session,
            menu.local_world_joined,
        );
        slot.drive(
            &mut commands,
            &mut menu,
            idle,
            client_blob_cache.enables_upstream_client_cache(),
            local_worlds.as_deref_mut(),
        );
    }
    if std::mem::take(&mut menu.accounts.skip_control) {
        menu.forget_launcher_trust();
        return;
    }
    match launcher_account {
        Some(mut account) => menu.sync_account_control(&mut *account),
        None => {
            menu.forget_launcher_trust();
            menu.sign_out_locally();
        }
    }
    if let Some(worlds) = local_worlds.as_deref_mut() {
        menu.sync_local_worlds(worlds, in_session);
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
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
