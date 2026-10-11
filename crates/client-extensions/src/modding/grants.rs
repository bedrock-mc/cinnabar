//! Explicit local environment grants for a selected personal component.

use mod_host::ModGrants;

const PLAYERS_ENV: &str = "CINNABAR_MOD_PLAYERS";
const PLAYER_STATE_ENV: &str = "CINNABAR_MOD_PLAYER_STATE";
const ITEM_USE_ENV: &str = "CINNABAR_MOD_ITEM_USE";
const CAMERA_ENV: &str = "CINNABAR_MOD_CAMERA";
const CONTROLS_ENV: &str = "CINNABAR_MOD_CONTROLS";
const HUD_ENV: &str = "CINNABAR_MOD_HUD";
const MOVEMENT_ENV: &str = "CINNABAR_MOD_MOVEMENT";
const INTERACTION_ENV: &str = "CINNABAR_MOD_INTERACTION";
const SETTINGS_ENV: &str = "CINNABAR_MOD_SETTINGS";
const RENDER_ENV: &str = "CINNABAR_MOD_RENDER";
const BLOCK_HIGHLIGHTS_ENV: &str = "CINNABAR_MOD_BLOCK_HIGHLIGHTS";
const FULLBRIGHT_ENV: &str = "CINNABAR_MOD_FULLBRIGHT";
const RENDER_DEPTH_ENV: &str = "CINNABAR_MOD_RENDER_DEPTH";
const ENTITIES_ENV: &str = "CINNABAR_MOD_ENTITIES";
/// Comma-separated command names the selected component may request.
const COMMANDS_ENV: &str = "CINNABAR_MOD_COMMANDS";
const PACKET_DELAY_ENV: &str = "CINNABAR_MOD_PACKET_DELAY";
const PACKET_DELAY_VISUAL_ENV: &str = "CINNABAR_MOD_PACKET_DELAY_VISUAL";

/// The grants a component or package selected on its own gets from the environment.
pub fn environment_grants() -> ModGrants {
    ModGrants {
        environment: true,
        players: std::env::var(PLAYERS_ENV).is_ok_and(|value| value == "1"),
        player_state: std::env::var(PLAYER_STATE_ENV).is_ok_and(|value| value == "1"),
        item_use: std::env::var(ITEM_USE_ENV).is_ok_and(|value| value == "1"),
        camera: std::env::var(CAMERA_ENV).is_ok_and(|value| value == "1"),
        controls: std::env::var(CONTROLS_ENV).is_ok_and(|value| value == "1"),
        hud: std::env::var(HUD_ENV).is_ok_and(|value| value == "1"),
        movement: std::env::var(MOVEMENT_ENV).is_ok_and(|value| value == "1"),
        interaction: std::env::var(INTERACTION_ENV).is_ok_and(|value| value == "1"),
        settings: std::env::var(SETTINGS_ENV).is_ok_and(|value| value == "1"),
        render: std::env::var(RENDER_ENV).is_ok_and(|value| value == "1"),
        block_highlights: std::env::var(BLOCK_HIGHLIGHTS_ENV).is_ok_and(|value| value == "1"),
        fullbright: std::env::var(FULLBRIGHT_ENV).is_ok_and(|value| value == "1"),
        render_depth: std::env::var(RENDER_DEPTH_ENV).is_ok_and(|value| value == "1"),
        entities: std::env::var(ENTITIES_ENV).is_ok_and(|value| value == "1"),
        commands: std::env::var(COMMANDS_ENV)
            .map(|names| {
                names
                    .split(',')
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        packet_delay: std::env::var(PACKET_DELAY_ENV).is_ok_and(|value| value == "1")
            || std::env::var(PACKET_DELAY_VISUAL_ENV).is_ok_and(|value| value == "1"),
        ..ModGrants::default()
    }
}
