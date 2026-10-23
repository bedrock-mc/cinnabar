//! Shared settings selectors, captions and reset routes.

use super::settings_options::SettingsGroup;

/// One settings section and the presentation metadata used by both menu hosts.
#[derive(Clone, Copy, Debug)]
pub struct SectionDefinition {
    pub id: SettingSection,
    pub name: &'static str,
    pub index: u8,
    pub title: Option<&'static str>,
    pub tab_title: Option<&'static str>,
    pub reset_group: Option<SettingsGroup>,
}

macro_rules! sections {
    ($($id:ident => $name:literal, $index:literal, $title:expr, $tab:expr, $reset:expr;)+) => {
        /// A selector in the JSON-UI settings contract.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum SettingSection { $($id,)+ }

        impl SettingSection {
            /// Reads a section selector from the JSON-UI contract.
            pub fn from_name(name: &str) -> Option<Self> {
                match name { $($name => Some(Self::$id),)+ _ => None }
            }

            /// Reads a selected section index from the menu state.
            pub fn from_index(index: u8) -> Option<Self> {
                match index { $($index => Some(Self::$id),)+ _ => None }
            }

            /// Returns the JSON-UI caption, preserving the fallback for unsupported panels.
            pub const fn title(self) -> &'static str {
                match self.definition().title {
                    Some(title) => title,
                    None => match Self::Video.definition().title { Some(title) => title, None => panic!("video section needs a caption") },
                }
            }

            /// Returns this section's shared presentation and reset metadata.
            pub const fn definition(self) -> SectionDefinition { SECTION_DEFINITIONS[self as usize] }
        }

        pub const SECTION_DEFINITIONS: &[SectionDefinition] = &[$(
            SectionDefinition { id: SettingSection::$id, name: $name, index: $index,
                title: $title, tab_title: $tab, reset_group: $reset },
        )+];

        /// Selector variables exposed to JSON-UI, derived from the section definitions.
        pub const SETTINGS_SECTIONS: &[(&str, u8)] = &[$(($name, $index),)+];
    }
}

sections! {
    Accessibility => "accessibility_forced_index", 2, Some("options.accessibility.title"), Some("menu.accessibility.tab.title"), Some(SettingsGroup::Accessibility);
    Updates => "cinnabar_updates_index", 45, Some("Updates"), Some("Updates"), None;
    Game => "game_forced_index", 4, Some("createWorldScreen.game.settings"), Some("options.category.game"), None;
    KeyboardAndMouse => "keyboard_and_mouse_forced_index", 14, Some("options.keyboardAndMouseSettings"), Some("menu.keyboardAndMouse.tab.title"), None;
    ControllerAndSwitch => "controller_and_switch_forced_index", 15, Some("options.controllerSettings"), Some("menu.controller.tab.title"), None;
    Touch => "touch_forced_index", 16, None, Some("menu.touch.tab.title"), None;
    Party => "party_forced_index", 17, None, None, None;
    Chat => "cinnabar_chat_index", 47, Some("Chat"), Some("Chat"), Some(SettingsGroup::Chat);
    Discord => "cinnabar_discord_index", 48, Some("Discord"), Some("Discord"), Some(SettingsGroup::Discord);
    General => "general_forced_index", 18, Some("options.generalTitle"), Some("menu.general.tab.title"), None;
    Video => "video_forced_index", 21, Some("options.videoTitle"), Some("menu.video.tab.title"), Some(SettingsGroup::Video);
    Sound => "sound_forced_index", 23, Some("options.sounds.title"), Some("menu.audio.tab.title"), Some(SettingsGroup::Audio);
    Account => "account_forced_index", 19, Some("options.accountTitle"), Some("menu.account.tab.title"), None;
    GlobalTexturePack => "global_texture_pack_forced_index", 24, Some("menu.globalpacks"), Some("menu.globalpacks"), None;
    Shaders => "cinnabar_shaders_index", 46, Some("Shaders"), if super::settings_options::SHADER_PACK_BINDINGS { Some("Shaders") } else { None }, None;
    Mods => "cinnabar_mods_index", 44, Some("Mods"), Some("Mods"), None;
    StorageManagement => "storage_management_forced_index", 25, Some("menu.storageManagement"), Some("menu.storage.tab.title"), None;
    Language => "language_forced_index", 27, Some("options.language"), Some("menu.language.tab.title"), None;
    Creator => "creator_forced_index", 20, Some("options.creatorTitle"), Some("menu.creator.tab.title"), None;
    Server => "server_forced_index", 1, Some("Server Settings"), Some("Server Settings"), None;
    HowToPlay => "how_to_play_index", 3, None, None, None;
    Classroom => "classroom_forced_index", 5, None, None, None;
    EduCloudLevel => "edu_cloud_level_forced_index", 6, None, None, None;
    Multiplayer => "multiplayer_forced_index", 7, None, None, None;
    World => "world_forced_index", 8, None, None, None;
    Members => "members_forced_index", 9, None, None, None;
    RealmsSaves => "realms_saves_forced_index", 10, None, None, None;
    Subscription => "subscription_forced_index", 11, None, None, None;
    Backup => "backup_forced_index", 12, None, None, None;
    DevOptions => "dev_options_forced_index", 13, None, None, None;
    EduCloudStorage => "edu_cloud_storage_forced_index", 26, None, None, None;
    Preview => "preview_forced_index", 28, None, None, None;
    Debug => "debug_forced_index", 29, None, None, None;
    DiscoveryDebug => "discovery_debug_forced_index", 30, None, None, None;
    UiDebug => "ui_debug_forced_index", 31, None, None, None;
    EduDebug => "edu_debug_forced_index", 32, None, None, None;
    FlightingDebug => "flighting_debug_forced_index", 34, None, None, None;
    RealmsDebug => "realms_debug_forced_index", 35, None, None, None;
    Automation => "automation_forced_index", 36, None, None, None;
    LevelTexturePack => "level_texture_pack_index", 37, None, None, None;
    Broadcast => "broadcast_forced_index", 38, None, None, None;
    Addon => "addon_index", 39, None, None, None;
    InviteLinks => "invite_links_forced_index", 40, None, None, None;
    GeneralInviteLink => "general_invite_link_forced_index", 41, None, None, None;
    AdvancedInviteLink => "advanced_invite_link_forced_index", 42, None, None, None;
    RealmsAdvanced => "realms_advanced_forced_index", 43, None, None, None;
}

/// Counts visible tabs without repeating their membership.
const fn tab_count() -> usize {
    let mut count = 0;
    let mut index = 0;
    while index < SECTION_DEFINITIONS.len() {
        if SECTION_DEFINITIONS[index].tab_title.is_some() {
            count += 1;
        }
        index += 1;
    }
    count
}

/// Collects sidebar selectors and captions in their registry order.
const fn tabs() -> [(&'static str, &'static str); tab_count()] {
    let mut tabs = [("", ""); tab_count()];
    let mut row = 0;
    let mut index = 0;
    while index < SECTION_DEFINITIONS.len() {
        let section = SECTION_DEFINITIONS[index];
        if let Some(title) = section.tab_title {
            tabs[row] = (section.name, title);
            row += 1;
        }
        index += 1;
    }
    tabs
}

pub const SETTINGS_TABS: [(&str, &str); tab_count()] = tabs();
