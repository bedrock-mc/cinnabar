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
mod activation;
mod construction;
mod death;
pub mod disconnect;
mod dressing_room;
mod external;
#[cfg(test)]
mod flow_tests;
mod focus;
#[cfg(test)]
mod home_promo_tests;
pub mod inbox;
mod input;
mod invite;
mod join_requests;
mod realm_membership;
pub use join_requests::open_join_requests_from_key;
#[cfg(feature = "native-networking")]
pub mod launcher_account;
#[cfg(feature = "native-networking")]
mod launcher_services;
mod minecraft_import;
#[cfg(feature = "native-networking")]
mod native_catalog;
#[cfg(feature = "native-networking")]
mod native_gatherings;
#[cfg(feature = "native-networking")]
pub mod native_local_world;
mod navigation;
mod presence_targets;
mod reconnect;
mod renderer;
pub mod screen_focus;
#[cfg(test)]
mod server_input_tests;
mod services;
mod session_intents;
mod shader_options;
#[cfg(feature = "native-networking")]
pub use services::sync_cinnabar_players;
use session_intents::SessionIntents;
pub mod settings_options;
mod settings_paths;
pub mod settings_storage;
pub mod settings_support;
mod settings_values;
mod shader_pack_status;
#[cfg(feature = "developer-control")]
mod sign_in_fixture;
pub mod video_settings;
mod view;
mod worlds_tab;
mod xbox_presence;

use launcher::menu::auth::AuthState;
#[cfg(feature = "native-networking")]
use launcher_host::auth::AuthSupervisor;
use launcher_host::{
    auth::auth_cache_path, destination::target_for, server_trust, servers, sign_in_browser,
};
use ui::RenderMode;

pub use input::clipboard::MenuClipboard;
pub(crate) use input::context::observe_keyboard_labels;
pub use input::{HOTKEY_OPTIONS, drive_menu_input, drive_window_ime};
use launcher::menu::view::MenuFeeds;

use launcher::menu::view::{
    JoinKind, LocalWorldCard, MenuFriendCard, MenuRealmCard, MenuServerCard, SavedServer,
};

pub use account::SignInStatus;
#[cfg(feature = "native-networking")]
pub use launcher_services::LauncherServices;
use servers::{ServerWriter, load_servers};
pub use video_settings::persist_video_settings;

use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use bevy::prelude::{Commands, ResMut, Resource};

use launcher::install_layout::InstallLayout;

use launcher::menu::{MAX_SERVER_ADDRESS_BYTES, MAX_SERVER_NAME_BYTES, MenuList};
/// Vanilla's port box: six number characters, prefilled with the Bedrock default.
const MAX_SERVER_PORT_BYTES: usize = 6;
use launcher::menu::DEFAULT_PORT;

use launcher::menu::split_address;

use launcher::menu::{MenuAction, MenuDialog, MenuField, MenuScreen, MenuServerTab};

#[derive(Debug, Resource)]
pub struct MenuRuntime {
    /// Execution only; the application keeps the real Runtime outside the World until workers retire.
    #[cfg(not(target_arch = "wasm32"))]
    native_io: Option<bridge::tasks::NativeExecution>,
    visible: bool,
    screen: MenuScreen,
    focused: usize,
    hovered: Option<MenuAction>,
    pressed: Option<MenuAction>,
    pointer_down: bool,
    touch_available: bool,
    touch_editor_open: bool,
    server_tab: MenuServerTab,
    profile_tab: launcher::menu::ProfileTab,
    dialog: Option<MenuDialog>,
    field: Option<MenuField>,
    /// Bumped by every field edit and caret move, restarting the caret blink.
    caret_revision: u64,
    /// Explicit editor activations request the keyboard again after platform dismissal.
    field_activation: u64,
    /// Screens opened on the way here; back returns to the one below.
    history: json_ui::ScreenNav<MenuScreen>,
    navigation_focus: navigation::NavigationFocus,
    /// The Play page beneath the current session's loading and in-game screens.
    session_origin: Option<MenuScreen>,
    retry_target: Option<reconnect::RetryTarget>,
    /// The Add/Edit Server boxes, each typed through the chat editor's caret model.
    name: ui::ChatEditor,
    address: ui::ChatEditor,
    port: ui::ChatEditor,
    skin_name: ui::ChatEditor,
    friend_query: ui::ChatEditor,
    settings_query: ui::ChatEditor,
    /// Command macro command boxes; edits write through to persisted settings.
    macro_commands: [ui::ChatEditor; launcher::menu::settings_options::COMMAND_MACRO_COUNT],
    message: Option<String>,
    gui_scale_preference: Option<u8>,
    gui_scale_offset: i8,
    gui_scale_display_offset: i8,
    gui_scale_choices: Vec<ui::DesktopGuiScaleChoice>,
    fullscreen: bool,
    fullscreen_change: Option<bool>,
    video_settings_writer: Option<launcher_host::video_settings::writer::Writer>,
    last_saved_video_settings: launcher_host::video_settings::SavedVideoSettings,
    failed_video_settings_save: Option<launcher_host::video_settings::SavedVideoSettings>,
    render_mode: RenderMode,
    render_mode_request: Option<RenderMode>,
    /// Discovered shader-pack names synced by the app; renderer choices index it.
    shader_packs: std::sync::Arc<[String]>,
    /// Revision that rejects picker actions captured before the list changed.
    shader_packs_revision: u64,
    /// The applied pack name, retaining a pending menu choice until taken.
    shader_pack: Option<String>,
    shader_pack_request: Option<String>,
    shader_pack_status: shader_pack_status::ShaderPackStatusUi,
    /// Set by [`MenuRuntime::request_shader_pack_reload`] until the app takes it.
    shader_pack_reload: bool,
    shader_pack_removing: Option<std::sync::Arc<str>>,
    shader_pack_remove_request: Option<String>,
    shader_pack_import: renderer::PackImport,
    vsync_override: Option<bool>,
    display_name: String,
    launcher: bool,
    servers: MenuList<SavedServer>,
    config_path: PathBuf,
    saves: ServerWriter,
    intents: SessionIntents,
    /// The session controller's last published state.
    session: SessionStatus,
    featured: MenuList<MenuServerCard>,
    #[cfg(feature = "native-networking")]
    native_gatherings:
        std::collections::HashMap<String, launcher_host::native_account::NativeGatheringSelection>,
    realms: MenuList<MenuRealmCard>,
    friends: MenuList<MenuFriendCard>,
    catalog_message: Option<String>,
    #[cfg(feature = "native-networking")]
    auth_process: Option<AuthSupervisor>,
    #[cfg(feature = "native-networking")]
    native_catalog: native_catalog::Controller,
    auth_attempted: bool,
    auth_restart_requested: bool,
    layout: InstallLayout,
    /// The client's own skin, cloned into every reconnection's `NetworkConfig`.
    player_skin: crate::player_skin::LocalPlayerSkin,
    dressing_room: std::sync::Arc<launcher::dressing_room::DressingRoomView>,
    dressing_room_worker: Option<dressing_room::Worker>,
    /// Fallback pixels for catalog entries created before world assets were ready.
    default_player_skin: Option<render_api::StandardSkin>,
    skin_update_pending: bool,
    skin_outbound: Option<(u64, protocol::Packet)>,
    skin_packet_pending: Option<protocol::Packet>,
    editing: Option<usize>,
    disconnect_message: Option<String>,
    /// Death screen shown for the current death; cleared once alive again.
    death_shown: bool,
    death_lifecycle: Option<(u64, u64)>,
    death_loading: bool,
    death_presentation: launcher::menu::death::DeathPresentation,
    death_retry_remaining: Option<std::time::Duration>,
    local_worlds: MenuList<LocalWorldCard>,
    local_world_requested: Option<usize>,
    local_ui: worlds_tab::LocalWorldsUi,
    /// Sign-in state reported by the core's account control, when bound.
    control_auth: Option<AuthState>,
    /// Tracks the current browser handoff and suppresses repeated automatic opens.
    sign_in_browser: sign_in_browser::SignInBrowser,
    /// Interactive prompts take focus; cached validation keeps the home status label.
    sign_in_requested: bool,
    /// Cancellation stays dismissed until the player explicitly starts sign-in again.
    sign_in_cancelled: bool,
    /// A helper-start failure stays owned by its interactive prompt; only Failed is stored.
    sign_in_failure: Option<AuthState>,
    #[cfg(feature = "developer-control")]
    sign_in_fixture: Option<developer_control::protocol::SignInFixtureState>,
    /// Developer recordings present placeholder accounts without signing in.
    presentation_accounts: bool,
    sign_out_requested: bool,
    external: Option<external::ExternalServices>,
    accounts: accounts::Manager,
    pub global_resource_actions: Vec<launcher::global_resources::Action>,
    pub global_resources: std::sync::Arc<launcher::global_resources::Snapshot>,
    /// Mods-tab work for the mod runtime; disclosure toggles apply in place.
    pub mod_actions: Vec<launcher::mods::Action>,
    pub mods: std::sync::Arc<launcher::mods::Snapshot>,
    shader_options: shader_options::ShaderOptions,
    minecraft_import: std::sync::Arc<launcher::minecraft_import::Snapshot>,
    minecraft_import_requests: Vec<launcher_host::minecraft_import::Request>,
    settings: launcher::menu::view::SettingsInteraction,
    settings_input: focus::SettingsInput,
    settings_options: std::sync::Arc<launcher::menu::settings_options::SettingsOptions>,
    storage: std::sync::Arc<launcher::menu::settings_storage::StorageView>,
    settings_dirty: bool,
    settings_writer: Option<launcher::menu::settings_options::persistence::writer::Writer>,
    /// Worker startup failures wait until this deadline before another attempt.
    settings_retry_at: Option<std::time::Instant>,
    settings_apply: bool,
    /// Stored choices beneath session-only overrides, restored before saving preferences.
    session_overrides: Vec<launcher::menu::settings_options::OptionPreference>,
    /// A developer controller is driving: hotkey toggles stay in memory.
    transient_toggles: bool,
    language_choices: std::sync::Arc<[(String, String)]>,
    language_pending: bool,
    language_asset_path: PathBuf,
    input_mode: input::MenuInputMode,
    /// The current or pending session is a local world, and whether it was live last frame.
    local_world_joined: bool,
    local_world_active: bool,
    feeds: MenuFeeds,
    /// The pause screen's invite screen and the invites it queued.
    invite: invite::InviteUi,
    realm_membership: realm_membership::RealmUi,
    /// Discord join requests waiting for the host's answer.
    join_requests: join_requests::JoinRequestUi,
}

impl MenuRuntime {
    /// Mirrors the applied mode; a pending menu toggle wins until taken.
    pub fn sync_render_mode(&mut self, applied: RenderMode) {
        if self.render_mode_request.is_none() {
            self.render_mode = applied;
        }
    }

    /// Shows the VSync toggle locked to a launch-flag override.
    #[must_use]
    pub const fn with_vsync_override(mut self, vsync: Option<bool>) -> Self {
        self.vsync_override = vsync;
        self
    }

    /// Consume the pending Video-section change.
    pub fn take_render_mode_request(&mut self) -> Option<RenderMode> {
        self.render_mode_request.take()
    }

    /// Return the extension settings file alongside the other user settings.
    pub fn graphics_file(&self) -> PathBuf {
        self.layout.graphics_file()
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Settings retains the background of the launcher or world beneath it; pause, its invite
    /// screen and death keep the world visible.
    pub fn uses_panorama(&self) -> bool {
        self.visible
            && match self.screen {
                MenuScreen::Pause | MenuScreen::Death | MenuScreen::Invite => false,
                MenuScreen::Settings | MenuScreen::DressingRoom => !self.over_world(),
                _ => true,
            }
    }

    pub fn screen(&self) -> MenuScreen {
        self.screen
    }

    /// Returns the selected settings section without building a presentation snapshot.
    pub fn settings_section(&self) -> Option<launcher::menu::settings_sections::SettingSection> {
        launcher::menu::settings_sections::SettingSection::from_index(self.settings.section)
    }

    pub fn player_skin(&self) -> &crate::player_skin::LocalPlayerSkin {
        &self.player_skin
    }

    pub fn is_launcher(&self) -> bool {
        self.launcher
    }

    /// A join is queued or in progress.
    pub fn is_connecting(&self) -> bool {
        self.session.connecting || self.intents.join.is_some()
    }

    pub fn observe_session(&mut self, status: SessionStatus) {
        self.session = status;
    }

    pub fn layout(&self) -> &InstallLayout {
        &self.layout
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub fn set_visible(&mut self, visible: bool) {
        if !visible {
            self.apply_global_resources_on_leave();
        }
        self.visible = visible;
        if !visible {
            self.field = None;
            self.settings.slider_selected = None;
            self.settings.slider_pointer = None;
            self.settings.slider_hovered = None;
            self.dialog = None;
        }
    }

    /// The local worlds the worlds tab lists (the local-worlds module feeds it).
    pub fn set_local_worlds(&mut self, worlds: Vec<LocalWorldCard>) {
        self.local_worlds.replace(worlds);
    }

    /// A local world the player chose to open, for the local-worlds module.
    pub fn take_local_world_request(&mut self) -> Option<usize> {
        self.local_world_requested.take()
    }

    /// A cancelled join drops any queued join and returns to the play screen.
    pub fn cancel_join(&mut self) {
        self.intents.join = None;
        #[cfg(feature = "native-networking")]
        {
            self.native_catalog.cancel_join();
            self.intents.gathering = None;
            self.intents.native_world = None;
        }
        self.intents.join_sign_in_deadline = None;
        self.intents.join_sign_in_check = false;
        self.intents.native_auth_required = false;
        self.retry_target = None;
        self.show_session_origin(MenuScreen::Play);
    }

    /// Shows local join errors in the world menu and remote errors on the disconnect screen.
    pub fn show_join_failure(&mut self, message: String) {
        if launcher::menu::disconnect::requires_client_update(&message) {
            launcher_host::lifecycle::update::protocol_required();
        }
        if self.feeds.join.kind == JoinKind::Local || !self.launcher {
            self.message = Some(message);
        } else {
            bevy::log::warn!(error = %message, "join failed");
            self.message = None;
            self.disconnect_message = Some(message);
            self.focused = 0;
            self.hovered = None;
        }
    }

    pub fn show_transfer(&mut self, address: &str) {
        self.reset_death();
        self.message = Some(format!("Transferring to {address}…"));
    }

    /// Resets the join progress screen for a join to `address`.
    pub fn begin_join_progress(&mut self, address: &str, local_world: bool) {
        self.feeds.join = launcher::menu::view::JoinProgress::new(
            launcher_host::destination::join_kind(address, local_world),
        );
    }

    /// Returns the session back to the launcher after a fatal session error.
    ///
    /// Returns `false` when the client was started with `--address`, which has
    /// no launcher to fall back to and must still exit the process.
    pub fn absorb_session_failure(&mut self, error: &str) -> bool {
        self.reset_death();
        if !self.launcher {
            return false;
        }
        self.show_session_origin(MenuScreen::Play);
        self.hovered = None;
        self.pressed = None;
        // The raw chain is for the log; the disconnect screen words it as vanilla does.
        bevy::log::warn!(error, "session ended");
        self.message = None;
        self.disconnect_message = Some(error.to_owned());
        // Let the account catalog repopulate now that the session is gone.
        true
    }

    pub fn take_disconnect_request(&mut self) -> bool {
        let requested = std::mem::take(&mut self.intents.disconnect);
        #[cfg(feature = "native-networking")]
        if requested {
            self.native_catalog.cancel_join();
        }
        requested
    }

    pub fn take_exit_request(&mut self) -> bool {
        let requested = std::mem::take(&mut self.intents.exit);
        #[cfg(feature = "native-networking")]
        if requested {
            self.native_catalog.cancel_join();
        }
        requested
    }

    pub fn activate_from_input(&mut self, action: MenuAction) {
        self.activate_control_input(action, false);
    }

    pub fn activate_from_navigation(&mut self, action: MenuAction) {
        self.activate_control_input(action, true);
    }

    fn activate_control_input(&mut self, action: MenuAction, navigation: bool) {
        if self.screen == MenuScreen::Settings {
            self.settings.record_activation(action, navigation);
        }
        self.activate(action);
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
        let saved = if let Some(index) = self.editing.filter(|&index| index < self.servers.len()) {
            let existing = &mut self.servers[index];
            existing.name = server.name;
            existing.address = server.address;
            index
        } else if let Some(index) = self
            .servers
            .iter()
            .position(|existing| existing.address.eq_ignore_ascii_case(&server.address))
        {
            let existing = &mut self.servers[index];
            *existing = SavedServer {
                favorite: existing.favorite,
                last_joined_unix: existing.last_joined_unix,
                ..server
            };
            index
        } else {
            self.servers.push(server);
            self.servers.len() - 1
        };
        // The server just saved replaces whichever one was selected before.
        self.feeds.select_saved(saved);
        if let Some(external) = &mut self.external {
            external.preferences_dirty = true;
            return true;
        }
        if let Err(error) = self.saves.save(&self.servers) {
            self.message = Some(format!("Could not save server: {error}"));
            return false;
        }
        true
    }

    /// Queues the list for writing; a schema refusal leaves only the log.
    fn save_servers(&mut self) {
        if let Some(external) = &mut self.external {
            external.preferences_dirty = true;
            return;
        }
        if let Err(error) = self.saves.save(&self.servers) {
            bevy::log::warn!("saved servers not written: {error:#}");
        }
    }

    /// Surfaces a saved-server write that failed on the worker.
    pub fn poll_saves(&mut self) {
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
    pub fn request_connect(&mut self, address: String) {
        #[cfg(feature = "native-networking")]
        {
            self.native_catalog.cancel_join();
            self.intents.gathering = None;
            self.intents.native_world = None;
        }
        if self.external.is_some() {
            self.request_external_connect(address);
            return;
        }
        if self.account_change_pending() {
            self.message = Some("Please wait for the account change to finish.".into());
            return;
        }
        if address.trim().is_empty() {
            self.message = Some("That server has no address.".to_owned());
            return;
        }
        bridge::join_profile::JoinProfile::begin();
        self.remember_session_origin();
        self.hold_join_for_sign_in();
        let auth_cache = self.launcher_auth_cache();
        self.remember_retry_target(&address, auth_cache.as_deref(), false);
        self.local_world_joined = false;
        self.intents.join = Some(JoinIntent {
            address,
            auth_cache,
            local_world: false,
        });
        self.disconnect_message = None;
        self.show_connecting();
    }

    /// Supplies one authorized test join's private cache without changing the selected account.
    #[cfg(feature = "developer-control")]
    pub fn use_auth_cache_for_pending_join(&mut self, auth_cache: PathBuf) {
        if let Some(join) = self.intents.join.as_mut() {
            join.auth_cache = Some(auth_cache.clone());
            let address = join.address.clone();
            self.remember_retry_target(&address, Some(&auth_cache), false);
        }
    }
}

impl Drop for MenuRuntime {
    fn drop(&mut self) {
        if let Err(error) = self.flush_user_settings() {
            bevy::log::warn!("could not save settings at shutdown: {error}");
        }
        self.stop_sign_in();
    }
}

/// Condenses a runtime error into something that fits the menu message area.
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

use client_state::{JoinIntent, SessionStatus};

#[cfg(test)]
mod reconnect_tests;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
