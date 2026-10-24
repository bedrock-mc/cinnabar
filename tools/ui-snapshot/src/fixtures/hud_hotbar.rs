//! The survival HUD with the Bedrock Hotbar Position setting off and on.
use super::*;

/// The hotbar flush with the bottom edge, then at vanilla Bedrock's height.
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
];
