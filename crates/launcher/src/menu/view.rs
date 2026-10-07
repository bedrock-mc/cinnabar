//! The menu's presented data: saved servers, catalog cards, and the per-frame
//! view the renderers draw from.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{MenuAction, MenuDialog, MenuField, MenuScreen, MenuServerTab, auth::AuthState};
use ui::IconRef;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SavedServer {
    pub name: String,
    pub address: String,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub last_joined_unix: u64,
}

/// One local world for the play screen's worlds tab, supplied by the local
/// worlds module through [`MenuView::local_worlds`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LocalWorldCard {
    pub name: String,
    pub game_mode: String,
    /// The saved world generator label.
    pub world_type: String,
    pub date: String,
    pub size: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub struct MenuServerCard {
    pub name: String,
    pub address: String,
    pub caption: String,
    #[serde(default)]
    pub image_path: String,
    #[serde(skip)]
    pub icon: Option<IconRef>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub struct MenuRealmCard {
    pub name: String,
    pub state: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub online_players: u32,
    #[serde(default)]
    pub max_players: u32,
    #[serde(default)]
    pub days_left: i32,
    #[serde(default)]
    pub expired: bool,
    /// Joined as a member rather than owned.
    #[serde(default)]
    pub member: bool,
}

/// Marks a featured address as an experience's ID, joined when selected.
pub const EXPERIENCE_ADDRESS_PREFIX: &str = "gathering/";

/// Marks an address as the friend world hosted by this XUID.
pub const FRIEND_ADDRESS_PREFIX: &str = "friend_xuid/";

/// Whether the server at `address` can be pinged; an experience has no server until joined.
pub fn pingable(address: &str) -> bool {
    !address.starts_with(EXPERIENCE_ADDRESS_PREFIX)
}

/// A featured server's info-panel details; artwork is a local cached path.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ServerDetails {
    pub group: String,
    /// Live experience count; only positive values are shown in its details panel.
    pub player_count: Option<i64>,
    pub description: String,
    /// The details banner; empty uses the first screenshot.
    pub banner: String,
    pub news_title: String,
    pub news: String,
    pub screenshots: Vec<String>,
    pub games: Vec<MenuGameCard>,
    /// The server's remote HTTPS logo, which Discord Rich Presence shows by URL.
    pub logo_url: String,
}

/// One game a featured server advertises.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MenuGameCard {
    pub title: String,
    pub subtitle: String,
    pub description: String,
    pub image_path: String,
}

/// The signed-in profile as the start screen shows it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MenuProfile {
    pub loaded: bool,
    pub unavailable: bool,
    pub xuid: String,
    pub statistics_loaded: bool,
    pub statistics_error: bool,
    pub achievements_loaded: bool,
    pub achievements_error: bool,
    pub achievements: Option<protocol::launcher_control::ProfileAchievements>,
    pub gamertag: String,
    pub picture_path: String,
    pub avatar_path: String,
    pub avatar_loaded: bool,
    pub avatar_error: bool,
    pub featured_screenshot_path: String,
    pub featured_screenshot_loaded: bool,
    pub featured_screenshot_error: bool,
    pub real_name: String,
    pub presence: String,
    pub gamerscore: Option<i64>,
    pub friends: Option<u32>,
    pub followers: Option<u32>,
    pub statistics: Option<protocol::launcher_control::ProfileStatistics>,
}

impl MenuProfile {
    /// Finishes every loading input when the account feed cannot provide a profile.
    pub fn unavailable() -> Self {
        Self {
            loaded: true,
            unavailable: true,
            avatar_loaded: true,
            avatar_error: true,
            featured_screenshot_loaded: true,
            featured_screenshot_error: true,
            statistics_loaded: true,
            statistics_error: true,
            achievements_loaded: true,
            achievements_error: true,
            ..Self::default()
        }
    }
}

/// Service feed data beyond the catalog cards: featured-server details keyed
/// by address, the profile, and the featured server the info panel shows.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MenuFeeds {
    pub accounts: Vec<crate::accounts::AccountProfile>,
    pub account_active_id: Option<String>,
    pub account_error: Option<String>,
    pub account_adding: bool,
    pub inbox_state: super::inbox::InboxState,
    pub details: HashMap<String, ServerDetails>,
    pub profile: MenuProfile,
    pub profile_refresh_requested: bool,
    pub selected_featured: Option<usize>,
    /// The saved server the Servers tab's details show, instead of a featured one.
    pub selected_saved: Option<usize>,
    /// The Realm the Realms tab's details show.
    pub selected_realm: Option<usize>,
    /// RakNet pongs keyed by the address the row joins.
    pub pings: HashMap<String, PingInfo>,
    /// The info panel's description and news are expanded past "read more".
    pub description_expanded: bool,
    pub news_expanded: bool,
    pub home: MenuHome,
    /// The join the progress screen reports while connecting.
    pub join: JoinProgress,
    /// The join's pending question whether to trust a NetherNet server.
    pub server_trust: Option<ServerTrustPrompt>,
    /// The player's answer to that question, until it is sent to the core that asked.
    pub server_trust_answer: Option<(ServerTrustPrompt, bool)>,
}

/// The core asks whether to trust the NetherNet server at `url` before the join goes on.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ServerTrustPrompt {
    pub id: u64,
    pub url: String,
    /// Asked by a per-session core rather than the launcher core; ids are per core.
    pub from_session_core: bool,
}

/// Which kind of join is under way; picks vanilla's connect title and progress screen.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum JoinKind {
    #[default]
    External,
    Realm,
    Local,
}

/// A join's stage, as vanilla's progress handlers split it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum JoinStage {
    /// The Realm lookup.
    Realm,
    /// Transport connect and login.
    #[default]
    Connecting,
    /// Pack acquisition; the byte total stays zero until a download begins.
    Packs {
        done: u32,
        total: u32,
        received_bytes: u64,
        total_bytes: u64,
    },
    /// The core handed the session over and the world is not ready yet.
    Generating,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct JoinProgress {
    pub kind: JoinKind,
    pub stage: JoinStage,
    /// The core reported this join, so its report vanishing means the handoff.
    reported: bool,
}

impl JoinProgress {
    pub fn new(kind: JoinKind) -> Self {
        Self {
            kind,
            ..Self::default()
        }
    }

    /// Folds in the core's latest report; `None` before its first or after the handoff.
    pub fn observe(&mut self, core: Option<JoinStage>) {
        match core {
            Some(stage) => {
                self.stage = stage;
                self.reported = true;
            }
            None if self.reported => self.stage = JoinStage::Generating,
            None => {}
        }
    }

    /// Whether vanilla's handler for this stage lets the player cancel.
    pub fn cancellable(&self) -> bool {
        match self.stage {
            JoinStage::Realm => false,
            JoinStage::Connecting => self.kind == JoinKind::External,
            JoinStage::Packs { total_bytes, .. } => total_bytes > 0,
            JoinStage::Generating => true,
        }
    }
}

/// The start screen's service data: messaging tile art, inbox and invite
/// counts, the live event button and the rendered persona head.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MenuHome {
    pub play_art: Option<ButtonArt>,
    pub store_art: Option<ButtonArt>,
    pub inbox_unread: u32,
    pub inbox_counts: std::collections::BTreeMap<usize, u32>,
    pub realm_invites: u32,
    pub live_event: Option<LiveEventCard>,
    pub persona_head: String,
    /// Inbox messages, newest first as the service lists them.
    pub inbox: Vec<InboxItem>,
}

/// One inbox message.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InboxItem {
    pub instance_id: String,
    pub report_id: String,
    pub received: String,
    pub source: String,
    pub header: String,
    pub body: String,
    pub category: String,
    pub unread: bool,
}

/// A main button's messaging art: local image paths per layer and its banner.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ButtonArt {
    pub banner_texture: String,
    pub colors: std::collections::BTreeMap<String, [u8; 3]>,
    pub default_background: String,
    pub hover_background: String,
    pub default_foreground: String,
    pub hover_foreground: String,
    pub banner: String,
}

/// The live gathering the start screen's event button leads to.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LiveEventCard {
    pub button_text: String,
    pub caption: String,
    pub countdown: bool,
    pub start_unix: i64,
    pub badge_path: String,
    pub address: String,
    pub route_to_servers: bool,
}

impl MenuFeeds {
    /// Show another featured server; its panel opens collapsed.
    pub fn select(&mut self, index: usize) {
        if self.selected_featured != Some(index) {
            self.description_expanded = false;
            self.news_expanded = false;
        }
        self.selected_featured = Some(index);
        self.selected_saved = None;
    }

    /// Show a saved server's details in place of the featured one.
    pub fn select_saved(&mut self, index: usize) {
        self.selected_saved = Some(index);
        self.selected_featured = None;
    }

    pub fn toggle_read_more(&mut self, section: u8) {
        match section {
            0 => self.description_expanded = !self.description_expanded,
            _ => self.news_expanded = !self.news_expanded,
        }
    }
}

/// One server's pong: `online` is false when it did not answer.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PingInfo {
    pub motd: String,
    pub online: bool,
    pub players: u32,
    pub max_players: u32,
    pub ping_ms: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MenuFriendCard {
    pub gamertag: String,
    pub world_name: String,
    pub members: String,
    pub xuid: String,
    /// The world's player limit; zero when the host did not publish one.
    pub max_members: u32,
}

#[derive(Clone, Debug)]
pub struct MenuView {
    pub visible: bool,
    /// The menu opened over the session's world rather than the launcher's.
    pub over_world: bool,
    pub screen: MenuScreen,
    pub focused_action: Option<MenuAction>,
    pub hovered: Option<MenuAction>,
    pub pressed: Option<MenuAction>,
    /// Keyboard and gamepad focus draws an outline; pointer focus still navigates.
    pub navigation_focus_visible: bool,
    pub gamepad_input: bool,
    pub server_tab: MenuServerTab,
    pub profile_tab: super::ProfileTab,
    pub dialog: Option<MenuDialog>,
    pub field: Option<MenuField>,
    /// The focused field's caret.
    pub caret: MenuCaret,
    pub name: String,
    pub address: String,
    pub port: String,
    pub message: Option<String>,
    pub gui_scale_offset: i8,
    pub gui_scale_choices: Vec<ui::DesktopGuiScaleChoice>,
    pub fullscreen: bool,
    pub render_mode: ui::RenderMode,
    /// Session VSync forced by a launch flag; the saved toggle is shown locked to it.
    pub vsync_override: Option<bool>,
    pub display_name: String,
    pub servers: Vec<SavedServer>,
    pub featured: Vec<MenuServerCard>,
    pub realms: Vec<MenuRealmCard>,
    pub friends: Vec<MenuFriendCard>,
    pub featured_icon: Option<IconRef>,
    pub realm_icon: Option<IconRef>,
    pub friend_icon: Option<IconRef>,
    pub saved_icon: Option<IconRef>,
    pub profile_icon: Option<IconRef>,
    pub catalog_loading: bool,
    pub catalog_message: Option<String>,
    pub auth_state: AuthState,
    pub connecting: bool,
    pub settings_section: u8,
    pub dressing_room: std::sync::Arc<crate::dressing_room::DressingRoomView>,
    pub player_skin: Option<protocol::StandardSkin>,
    pub player_skin_model: crate::dressing_room::SkinModel,
    pub global_resources: std::sync::Arc<crate::global_resources::Snapshot>,
    /// Why the last session ended, shown until acknowledged.
    pub disconnect_message: Option<String>,
    /// The saved server the add screen is editing.
    pub editing: Option<usize>,
    pub local_worlds: Vec<LocalWorldCard>,
    /// The local-world create, edit and template screens and their modals.
    pub local: crate::local_worlds::WorldsView,
    pub settings_options: std::sync::Arc<super::settings_options::SettingsOptions>,
    pub storage: std::sync::Arc<super::settings_storage::StorageView>,
    pub settings_dropdown: Option<u16>,
    pub settings_scale_picker: bool,
    /// Last interactive settings activation and its monotonic input revision.
    pub settings_control_activation: Option<(MenuAction, u64)>,
    pub settings_control_activation_navigation: bool,
    /// Continuous pointer position remains independent of the persisted slider step.
    pub settings_slider_pointer: Option<SettingsSliderPointer>,
    pub settings_slider_hovered: Option<u16>,
    pub settings_slider_selected: Option<u16>,
    pub key_remap: Option<u16>,
    pub settings_advanced_graphics: bool,
    pub language_choices: std::sync::Arc<[(String, String)]>,
    pub feeds: MenuFeeds,
    /// The Marketplace's state while its screen is up.
    pub store: Option<std::sync::Arc<crate::store::StoreSnapshot>>,
    /// The open local world is hosted for Xbox friends, so the pause screen offers invites.
    pub hosting: bool,
    /// The invite screen's friends and picks while it is up.
    pub invite: Option<std::sync::Arc<super::invite::InviteState>>,
    /// Who sent the oldest open Discord join request.
    pub join_request: Option<String>,
}

/// An active settings slider retains the unrounded pointer fraction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SettingsSliderPointer {
    pub option: u16,
    pub fraction: f32,
    pub mouse_input: bool,
}

/// A control's complete layout bounds, retained even outside a scroll viewport.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SettingsFocusTarget {
    pub action: MenuAction,
    pub bounds: ui::UiRect,
    pub landmark: Option<u16>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsFocusAxis {
    Horizontal,
    Vertical,
}

/// Directional navigation enters a landmark through its remembered or delegated control.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SettingsFocusLandmark {
    pub id: u16,
    pub parent: Option<u16>,
    pub bounds: ui::UiRect,
    pub scroll_axis: Option<SettingsFocusAxis>,
    pub delegate: Option<MenuAction>,
    pub delegate_landmark: Option<u16>,
    pub remember: bool,
    pub trap: bool,
    pub focus_control_disabled: bool,
}

/// The focused text field's caret.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MenuCaret {
    /// Byte offset into the focused field's text.
    pub byte: usize,
    /// Selected byte range, when the focused editor has a selection.
    pub selection: Option<[usize; 2]>,
    /// Changes with every edit and caret move, restarting the blink.
    pub revision: u64,
    /// The blink phase, which the presentation sets from its clock.
    pub shown: bool,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct CatalogFile {
    #[serde(default)]
    pub featured: Vec<MenuServerCard>,
    #[serde(default)]
    pub realms: Vec<MenuRealmCard>,
    #[serde(default)]
    pub friends: Vec<CatalogFriend>,
    #[serde(default)]
    pub errors: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CatalogFriend {
    pub gamertag: String,
    pub world_name: String,
    pub xuid: String,
    pub members: i32,
    pub max_members: i32,
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
            max_members: u32::try_from(friend.max_members).unwrap_or(0),
        }
    }
}

impl MenuView {
    /// The join's pending trust question, which draws as a popup over the join screen.
    pub fn server_trust_prompt(&self) -> Option<&ServerTrustPrompt> {
        self.feeds.server_trust.as_ref().filter(|_| self.connecting)
    }

    /// The oldest Discord join request's sender, asked in a popup once no other popup is up.
    pub fn join_request_prompt(&self) -> Option<&str> {
        self.join_request
            .as_deref()
            .filter(|_| self.dialog.is_none() && self.server_trust_prompt().is_none())
    }

    /// Whether a popup draws over the screen and takes its input.
    pub fn popup_open(&self) -> bool {
        self.dialog.is_some()
            || self.server_trust_prompt().is_some()
            || self.join_request.is_some()
            || self.dressing_room.editor.is_some()
    }

    /// Whether the launcher is waiting for the player to complete device-code sign-in.
    pub fn auth_state_awaiting_code(&self) -> bool {
        matches!(self.auth_state, AuthState::AwaitingCode { .. })
    }

    /// Creates the empty home view used before host settings and service feeds are loaded.
    pub fn new(visible: bool, display_name: String) -> Self {
        Self {
            visible,
            over_world: !visible,
            screen: MenuScreen::Home,
            focused_action: Some(MenuAction::Navigate(MenuScreen::Home)),
            hovered: None,
            pressed: None,
            navigation_focus_visible: true,
            gamepad_input: false,
            server_tab: MenuServerTab::Featured,
            profile_tab: super::ProfileTab::default(),
            dialog: None,
            field: None,
            caret: MenuCaret {
                shown: true,
                ..MenuCaret::default()
            },
            name: String::new(),
            address: String::new(),
            port: String::new(),
            message: None,
            gui_scale_offset: 0,
            gui_scale_choices: ui::DesktopGuiScale::for_window([1, 1]).choices().collect(),
            fullscreen: false,
            render_mode: ui::RenderMode::Vanilla,
            vsync_override: None,
            display_name,
            servers: Vec::new(),
            featured: Vec::new(),
            realms: Vec::new(),
            friends: Vec::new(),
            featured_icon: None,
            realm_icon: None,
            friend_icon: None,
            saved_icon: None,
            profile_icon: None,
            catalog_loading: false,
            catalog_message: None,
            auth_state: AuthState::SignedOut,
            connecting: false,
            settings_section: 0,
            dressing_room: Default::default(),
            player_skin: None,
            player_skin_model: Default::default(),
            global_resources: Default::default(),
            disconnect_message: None,
            editing: None,
            local_worlds: Vec::new(),
            local: Default::default(),
            settings_options: Default::default(),
            storage: Default::default(),
            settings_dropdown: None,
            settings_scale_picker: false,
            settings_control_activation: None,
            settings_control_activation_navigation: false,
            settings_slider_pointer: None,
            settings_slider_hovered: None,
            settings_slider_selected: None,
            key_remap: None,
            settings_advanced_graphics: false,
            language_choices: Default::default(),
            feeds: Default::default(),
            store: None,
            hosting: false,
            invite: None,
            join_request: None,
        }
    }
}
