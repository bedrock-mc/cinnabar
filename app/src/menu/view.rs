//! The menu's presented data: saved servers, catalog cards, and the per-frame
//! view the renderers draw from.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{MenuAction, MenuDialog, MenuField, MenuScreen, MenuServerTab, auth::AuthState};
use crate::ui_runtime::presentation::IconRef;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct SavedServer {
    pub(crate) name: String,
    pub(crate) address: String,
    #[serde(default)]
    pub(crate) favorite: bool,
    #[serde(default)]
    pub(crate) last_joined_unix: u64,
}

/// One local world for the play screen's worlds tab, supplied by the local
/// worlds module through [`super::MenuRuntime::set_local_worlds`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct LocalWorldCard {
    pub(crate) name: String,
    pub(crate) game_mode: String,
    /// The owner's world type label (Normal (BDS) or Flat (Dragonfly)).
    pub(crate) world_type: String,
    pub(crate) date: String,
    pub(crate) size: String,
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
    #[serde(default)]
    pub(crate) owner: String,
    #[serde(default)]
    pub(crate) online_players: u32,
    #[serde(default)]
    pub(crate) max_players: u32,
    #[serde(default)]
    pub(crate) days_left: i32,
    #[serde(default)]
    pub(crate) expired: bool,
    /// Joined as a member rather than owned.
    #[serde(default)]
    pub(crate) member: bool,
}

/// A featured server's info-panel details; artwork is a local cached path.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ServerDetails {
    pub(crate) description: String,
    pub(crate) news_title: String,
    pub(crate) news: String,
    pub(crate) screenshots: Vec<String>,
    pub(crate) games: Vec<MenuGameCard>,
}

/// One game a featured server advertises.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct MenuGameCard {
    pub(crate) title: String,
    pub(crate) subtitle: String,
    pub(crate) description: String,
    pub(crate) image_path: String,
}

/// The signed-in profile as the start screen shows it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct MenuProfile {
    pub(crate) gamertag: String,
    pub(crate) picture_path: String,
    pub(crate) real_name: String,
    pub(crate) presence: String,
    pub(crate) gamerscore: Option<i64>,
    pub(crate) friends: Option<u32>,
    pub(crate) followers: Option<u32>,
}

/// Service feed data beyond the catalog cards: featured-server details keyed
/// by address, the profile, and the featured server the info panel shows.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct MenuFeeds {
    pub(crate) inbox_state: super::inbox::InboxState,
    pub(crate) details: HashMap<String, ServerDetails>,
    pub(crate) profile: MenuProfile,
    pub(crate) selected_featured: Option<usize>,
    /// The saved server the Servers tab's details show, instead of a featured one.
    pub(crate) selected_saved: Option<usize>,
    /// The Realm the Realms tab's details show.
    pub(crate) selected_realm: Option<usize>,
    /// RakNet pongs keyed by the address the row joins.
    pub(crate) pings: HashMap<String, PingInfo>,
    /// The info panel's description and news are expanded past "read more".
    pub(crate) description_expanded: bool,
    pub(crate) news_expanded: bool,
    pub(crate) home: MenuHome,
    /// The join the progress screen reports while connecting.
    pub(crate) join: JoinProgress,
}

/// Which kind of join is under way; picks vanilla's connect title and progress screen.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum JoinKind {
    #[default]
    External,
    Realm,
    Local,
}

/// A join's stage, as vanilla's progress handlers split it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum JoinStage {
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
pub(crate) struct JoinProgress {
    pub(crate) kind: JoinKind,
    pub(crate) stage: JoinStage,
    /// The core reported this join, so its report vanishing means the handoff.
    reported: bool,
}

impl JoinProgress {
    pub(crate) fn new(kind: JoinKind) -> Self {
        Self {
            kind,
            ..Self::default()
        }
    }

    /// Folds in the core's latest report; `None` before its first or after the handoff.
    pub(crate) fn observe(&mut self, core: Option<JoinStage>) {
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
    pub(crate) fn cancellable(&self) -> bool {
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
pub(crate) struct MenuHome {
    pub(crate) play_art: Option<ButtonArt>,
    pub(crate) store_art: Option<ButtonArt>,
    pub(crate) inbox_unread: u32,
    pub(crate) inbox_counts: std::collections::BTreeMap<usize, u32>,
    pub(crate) realm_invites: u32,
    pub(crate) live_event: Option<LiveEventCard>,
    pub(crate) persona_head: String,
    /// Inbox messages, newest first as the service lists them.
    pub(crate) inbox: Vec<InboxItem>,
}

/// One inbox message.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct InboxItem {
    pub(crate) instance_id: String,
    pub(crate) report_id: String,
    pub(crate) received: String,
    pub(crate) source: String,
    pub(crate) header: String,
    pub(crate) body: String,
    pub(crate) category: String,
    pub(crate) unread: bool,
}

/// A main button's messaging art: local image paths per layer and its banner.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ButtonArt {
    pub(crate) banner_texture: String,
    pub(crate) colors: std::collections::BTreeMap<String, [u8; 3]>,
    pub(crate) default_background: String,
    pub(crate) hover_background: String,
    pub(crate) default_foreground: String,
    pub(crate) hover_foreground: String,
    pub(crate) banner: String,
}

/// The live gathering the start screen's event button leads to.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct LiveEventCard {
    pub(crate) button_text: String,
    pub(crate) caption: String,
    pub(crate) countdown: bool,
    pub(crate) start_unix: i64,
    pub(crate) badge_path: String,
    pub(crate) address: String,
    pub(crate) route_to_servers: bool,
}

impl MenuFeeds {
    /// Show another featured server; its panel opens collapsed.
    pub(crate) fn select(&mut self, index: usize) {
        if self.selected_featured != Some(index) {
            self.description_expanded = false;
            self.news_expanded = false;
        }
        self.selected_featured = Some(index);
        self.selected_saved = None;
    }

    /// Show a saved server's details in place of the featured one.
    pub(crate) fn select_saved(&mut self, index: usize) {
        self.selected_saved = Some(index);
        self.selected_featured = None;
    }

    pub(crate) fn toggle_read_more(&mut self, section: u8) {
        match section {
            0 => self.description_expanded = !self.description_expanded,
            _ => self.news_expanded = !self.news_expanded,
        }
    }
}

/// One server's pong: `online` is false when it did not answer.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct PingInfo {
    pub(crate) online: bool,
    pub(crate) players: u32,
    pub(crate) max_players: u32,
    pub(crate) ping_ms: u32,
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
    /// The menu opened over the session's world rather than the launcher's.
    pub(crate) over_world: bool,
    pub(crate) screen: MenuScreen,
    pub(crate) focused_action: Option<MenuAction>,
    pub(crate) hovered: Option<MenuAction>,
    pub(crate) pressed: Option<MenuAction>,
    pub(crate) server_tab: MenuServerTab,
    pub(crate) dialog: Option<MenuDialog>,
    pub(crate) field: Option<MenuField>,
    /// The focused field's caret.
    pub(crate) caret: MenuCaret,
    pub(crate) name: String,
    pub(crate) address: String,
    pub(crate) port: String,
    pub(crate) message: Option<String>,
    pub(crate) gui_scale_offset: i8,
    pub(crate) gui_scale_choices: Vec<i8>,
    pub(crate) fullscreen: bool,
    pub(crate) render_mode: ui::RenderMode,
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
    pub(crate) connecting: bool,
    pub(crate) settings_section: u8,
    pub(crate) global_resources: std::sync::Arc<crate::global_resources::Snapshot>,
    /// Why the last session ended, shown until acknowledged.
    pub(crate) disconnect_message: Option<String>,
    /// The saved server the add screen is editing.
    pub(crate) editing: Option<usize>,
    pub(crate) local_worlds: Vec<LocalWorldCard>,
    /// The local-world create, edit and template screens and their modals.
    pub(crate) local: crate::local_worlds::WorldsView,
    pub(crate) settings_options: std::sync::Arc<super::settings_options::SettingsOptions>,
    pub(crate) storage: std::sync::Arc<super::settings_storage::StorageView>,
    pub(crate) settings_dropdown: Option<u16>,
    pub(crate) key_remap: Option<u16>,
    pub(crate) settings_advanced_graphics: bool,
    pub(crate) language_choices: std::sync::Arc<[(String, String)]>,
    pub(crate) feeds: MenuFeeds,
    /// The Marketplace's state while its screen is up.
    pub(crate) store: Option<std::sync::Arc<crate::store::StoreSnapshot>>,
}

/// The focused text field's caret.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct MenuCaret {
    /// Byte offset into the focused field's text.
    pub(crate) byte: usize,
    /// Selected byte range, when the focused editor has a selection.
    pub(crate) selection: Option<[usize; 2]>,
    /// Changes with every edit and caret move, restarting the blink.
    pub(crate) revision: u64,
    /// The blink phase, which the presentation sets from its clock.
    pub(crate) shown: bool,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(super) struct CatalogFile {
    #[serde(default)]
    pub(super) featured: Vec<MenuServerCard>,
    #[serde(default)]
    pub(super) gatherings: Vec<MenuServerCard>,
    #[serde(default)]
    pub(super) realms: Vec<MenuRealmCard>,
    #[serde(default)]
    pub(super) friends: Vec<CatalogFriend>,
    #[serde(default)]
    pub(super) errors: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct CatalogFriend {
    pub(super) gamertag: String,
    pub(super) world_name: String,
    pub(super) xuid: String,
    pub(super) members: i32,
    pub(super) max_members: i32,
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
