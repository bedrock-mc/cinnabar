//! Screen roles refer to the manifest-generated art keys.

use super::embedded;

/// Create-world artwork resolves from the shipped art.
pub(crate) const WORLD_PREVIEW: &str = embedded::WORLD_PREVIEW.key;
pub(crate) const WORLD_CATEGORY_ICONS: [&str; 7] = [
    embedded::WORLD_GENERAL.key,
    embedded::WORLD_ADVANCED.key,
    embedded::WORLD_MULTIPLAYER.key,
    embedded::WORLD_CHEATS.key,
    embedded::WORLD_RESOURCES.key,
    embedded::WORLD_BEHAVIOR.key,
    embedded::WORLD_EXPERIMENTAL.key,
];
pub(crate) const HARDCORE_ICON: &str = embedded::WORLD_HARDCORE.key;
/// The pixelated loading animation selected by OreUI `ep`.
pub(crate) const LOADING_ANIMATION: &str = embedded::LOADING.key;
pub(crate) const CHEVRON_LEFT_IMAGE: &str = embedded::CHEVRON_LEFT.key;
pub(crate) const CHEVRON_UP_IMAGE: &str = embedded::CHEVRON_UP.key;
pub(crate) const CHEVRON_DOWN_IMAGE: &str = embedded::CHEVRON_DOWN.key;
pub(crate) const BASE_PACK_IMAGE: &str = embedded::BASE_PACK.key;
pub(crate) const MISSING_PACK_IMAGE: &str = embedded::MISSING_PACK.key;
pub(crate) const OVERWORLD_BLOCK_IMAGE: &str = embedded::OVERWORLD_BLOCK.key;
pub(crate) const SETTINGS_ICON_HIGHLIGHT_IMAGE: &str = embedded::ICON_HIGHLIGHT.key;
/// Servers status artwork: low, medium, high and pending ping.
pub(crate) const SERVER_PING_IMAGES: [&str; 4] = [
    embedded::PING_GREEN.key,
    embedded::PING_YELLOW.key,
    embedded::PING_RED.key,
    embedded::PING_PENDING.key,
];
pub(crate) const SERVER_PLAYERS_IMAGE: &str = embedded::SERVERS_PLAYERS.key;
/// Play-tab artwork in Worlds, Realms, Servers order.
pub(crate) const PLAY_TAB_ICONS: [&str; 3] = [
    embedded::PLAY_WORLDS.key,
    embedded::PLAY_REALMS.key,
    embedded::PLAY_SERVERS.key,
];
/// Standalone category images from the shipped art, in sidebar order.
pub const INBOX_ICONS: [&str; 5] = [
    embedded::INBOX_NEWS.key,
    embedded::INBOX_REALMS.key,
    embedded::INBOX_INVITES.key,
    embedded::INBOX_PASS.key,
    embedded::INBOX_FEEDBACK.key,
];
/// Empty-message illustrations, in the same order as the category icons.
pub(crate) const INBOX_EMPTY_IMAGES: [&str; 5] = [
    embedded::INBOX_EMPTY_NEWS.key,
    embedded::INBOX_EMPTY_REALMS.key,
    embedded::INBOX_EMPTY_INVITES.key,
    embedded::INBOX_EMPTY_PASS.key,
    embedded::INBOX_EMPTY_FEEDBACK.key,
];
/// Settings category art, from the embedded manifest.
pub(crate) const SETTINGS_ICONS: [&str; 14] = [
    embedded::SETTINGS_ACCESSIBILITY.key,
    embedded::SETTINGS_KEYBOARD.key,
    embedded::SETTINGS_CONTROLS.key,
    embedded::SETTINGS_TOUCH.key,
    embedded::SETTINGS_PARTY.key,
    embedded::SETTINGS_WORKBENCH.key,
    embedded::SETTINGS_PAINTING.key,
    embedded::SETTINGS_SOUND.key,
    embedded::SETTINGS_ACCOUNT.key,
    embedded::SETTINGS_SUBSCRIPTIONS.key,
    embedded::SETTINGS_CHEST.key,
    embedded::SETTINGS_STORAGE.key,
    embedded::SETTINGS_LANGUAGE.key,
    embedded::SETTINGS_COMMAND.key,
];
/// Standalone Profile assets named by the version-matched OreUI components.
pub(crate) const PROFILE_STAT_ICONS: [&str; 4] = [
    embedded::PROFILE_STAT_CLOCK.key,
    embedded::PROFILE_STAT_PICKAXE.key,
    embedded::PROFILE_STAT_SWORD.key,
    embedded::PROFILE_STAT_BOOTS.key,
];
/// The deterministic fallback banner choices used by OreUI `mZ`.
pub(crate) const PROFILE_BANNERS: [&str; assets::oreui_panorama::BANNER_COUNT] = [
    embedded::PROFILE_BANNER_LAKE.key,
    embedded::PROFILE_BANNER_CAVERN.key,
    embedded::PROFILE_BANNER_VILLAGE.key,
    embedded::PROFILE_BANNER_COAST.key,
    embedded::PROFILE_BANNER_OASIS.key,
    embedded::PROFILE_BANNER_SNOW.key,
    embedded::PROFILE_BANNER_CANYON.key,
    embedded::PROFILE_BANNER_RUINS.key,
];
/// Profile error art, from the shipped original art.
pub(crate) const PROFILE_ERRORS: [&str; 3] = [
    embedded::PROFILE_ERROR_NOTHING.key,
    embedded::PROFILE_ERROR_GENERIC.key,
    embedded::PROFILE_ERROR_CONNECTION.key,
];
/// The Overview row art selected by OreUI `g2`.
pub(crate) const PROFILE_SUMMARY_ICONS: [&str; 4] = [
    embedded::PROFILE_SUMMARY_FRIENDS.key,
    embedded::PROFILE_SUMMARY_FOLLOWERS.key,
    embedded::PROFILE_SUMMARY_GALLERY.key,
    embedded::PROFILE_SUMMARY_ACHIEVEMENTS.key,
];
/// Gamerscore art shared by Overview and achievement cards.
pub(crate) const PROFILE_GAMERSCORE: &str = embedded::PROFILE_GAMERSCORE.key;
/// Settings support links use the native external-link mask.
pub(crate) const EXTERNAL_LINK_ICON: &str = embedded::EXTERNAL_LINK.key;
/// Settings binding reset mask.
pub(crate) const RESET_ICON: &str = embedded::RESET.key;

#[cfg(test)]
const REFERENCED_KEY_GROUPS: &[&[&str]] = &[
    &[
        WORLD_PREVIEW,
        HARDCORE_ICON,
        LOADING_ANIMATION,
        CHEVRON_LEFT_IMAGE,
        CHEVRON_UP_IMAGE,
        CHEVRON_DOWN_IMAGE,
        BASE_PACK_IMAGE,
        MISSING_PACK_IMAGE,
        OVERWORLD_BLOCK_IMAGE,
        SETTINGS_ICON_HIGHLIGHT_IMAGE,
        SERVER_PLAYERS_IMAGE,
        PROFILE_GAMERSCORE,
        EXTERNAL_LINK_ICON,
        RESET_ICON,
    ],
    &WORLD_CATEGORY_ICONS,
    &SERVER_PING_IMAGES,
    &PLAY_TAB_ICONS,
    &INBOX_ICONS,
    &INBOX_EMPTY_IMAGES,
    &SETTINGS_ICONS,
    &PROFILE_STAT_ICONS,
    &PROFILE_BANNERS,
    &PROFILE_ERRORS,
    &PROFILE_SUMMARY_ICONS,
];

/// Lists the shipped artwork used by the screen renderers.
#[cfg(test)]
pub(crate) fn referenced_keys() -> impl Iterator<Item = &'static str> {
    REFERENCED_KEY_GROUPS
        .iter()
        .flat_map(|group| group.iter().copied())
}
