//! The survival HUD with the Bedrock Hotbar Position setting off and on, and its Video row.
use super::*;

/// The hotbar flush with the bottom edge, then at vanilla Bedrock's height, then the setting.
pub(super) const STATES: &[State] = &[
    #[cfg(feature = "test-support")]
    State {
        name: "hud-hotbar-java",
        size: [640, 360],
        gui_scale: 2,
        fixture: || Fixture::BedrockHotbar { raised: false },
    },
    #[cfg(feature = "test-support")]
    State {
        name: "hud-hotbar-bedrock",
        size: [640, 360],
        gui_scale: 2,
        fixture: || Fixture::BedrockHotbar { raised: true },
    },
    State {
        name: "settings-video-bedrock-hotbar",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: settings_video_bedrock_hotbar,
    },
];

/// Focuses the Bedrock Hotbar Position toggle, saved on, so the Video page scrolls to its row.
fn settings_video_bedrock_hotbar() -> Fixture {
    let id = SettingId::CinnabarBedrockHotbar;
    let mut view = settings("video_forced_index");
    Arc::make_mut(&mut view.settings_options).set(id.index(), 1);
    view.focused_action = Some(MenuAction::SettingsOption(id.index() as u16, 0));
    Fixture::Menu(Box::new(view))
}
