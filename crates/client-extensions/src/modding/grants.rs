//! Explicit local environment grants for a selected personal component.

use super::packet_relay::PacketModSelection;
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
/// A package's vanilla inventory requests, which its manifest must also ask for.
const INVENTORY_ENV: &str = "CINNABAR_MOD_INVENTORY";
/// Comma-separated command names the selected component may request.
const COMMANDS_ENV: &str = "CINNABAR_MOD_COMMANDS";
const PACKET_DELAY_ENV: &str = "CINNABAR_MOD_PACKET_DELAY";
const PACKET_DELAY_VISUAL_ENV: &str = "CINNABAR_MOD_PACKET_DELAY_VISUAL";

/// A packet mod component or core module selected on its own; it gets the packets grant.
pub const PACKET_MOD_ENV: &str = "CINNABAR_PACKET_MOD";
/// Also links WASI into the packet mod selected on its own.
const PACKET_MOD_WASI_ENV: &str = "CINNABAR_PACKET_MOD_WASI";

/// The packet mod selected on its own through the environment.
pub fn environment_packet_mod() -> Option<PacketModSelection> {
    let path = std::env::var_os(PACKET_MOD_ENV)?;
    Some(PacketModSelection {
        path: path.into(),
        grants: ModGrants {
            packets: true,
            wasi: std::env::var(PACKET_MOD_WASI_ENV).is_ok_and(|value| value == "1"),
            ..environment_grants()
        },
    })
}

/// The grants a component or package selected on its own gets from the environment.
pub fn environment_grants() -> ModGrants {
    ModGrants {
        environment: true,
        world_draw: std::env::var("CINNABAR_MOD_WORLD_DRAW").is_ok_and(|value| value == "1"),
        render_through_walls: std::env::var("CINNABAR_MOD_RENDER_THROUGH_WALLS")
            .is_ok_and(|value| value == "1"),
        entity_observe: std::env::var("CINNABAR_MOD_ENTITY_OBSERVE")
            .is_ok_and(|value| value == "1"),
        entity_highlight: std::env::var("CINNABAR_MOD_ENTITY_HIGHLIGHT")
            .is_ok_and(|value| value == "1"),
        terrain: std::env::var("CINNABAR_MOD_TERRAIN").is_ok_and(|value| value == "1"),
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
        inventory: std::env::var(INVENTORY_ENV).is_ok_and(|value| value == "1"),
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
