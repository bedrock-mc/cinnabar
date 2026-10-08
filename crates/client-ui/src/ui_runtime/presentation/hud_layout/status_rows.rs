//! Captures authoritative client values for the shared native HUD painter.

use super::{HudFrame, HudTexturePages, UiRuntime};
use assets::HudTextureRole;
use ui::native_hud::{HudPaint, SheetSprite, StatusPaintInput, capture_status_hud};

pub(in super::super) fn capture(
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
    frame: &HudFrame,
    sheet: Option<&HudTexturePages>,
    options: &crate::menu::settings_options::SettingsOptions,
) -> HudPaint {
    use crate::menu::settings_options::{INVERT_CROSSHAIR_OPTION, THIRD_PERSON_CROSSHAIR_OPTION};
    let now_tick = runtime.estimated_server_tick(frame.now_millis);
    let gameplay = runtime.gameplay_hud();
    let input = StatusPaintInput {
        health: runtime.hud().health(),
        absorption: runtime.hud().absorption(),
        armor: runtime.hud().armor(),
        hunger: runtime.hud().hunger(),
        air: runtime.hud().air(),
        heart_variant: gameplay.heart_variant(now_tick),
        regenerating: gameplay.regeneration_active(now_tick),
        hardcore: gameplay.hardcore(),
        hunger_effect: gameplay.hunger_effect_active(now_tick),
        saturation_empty: gameplay.saturation_empty(),
        effects: gameplay.effects(),
        now_tick,
        now_millis: frame.now_millis,
        last_health_drop_millis: runtime.last_health_drop_millis(),
        first_person: frame.first_person || options.value(THIRD_PERSON_CROSSHAIR_OPTION.name) != 0,
        hotbar_allowed: player_runtime
            .facts
            .player_game_mode()
            .is_none_or(|mode| mode.shows_hotbar()),
        survival_stats_visible: player_runtime.facts.survival_stats_visible(),
        crosshair_blend: if options.value(INVERT_CROSSHAIR_OPTION.name) != 0 {
            ui::UiBlendMode::Invert
        } else {
            ui::UiBlendMode::Alpha
        },
        mount_health: frame.mount_health,
        mount_jump: frame.mount_jump,
    };
    let sprite = |role: HudTextureRole| {
        let sheet = sheet.expect("sprite requested only with an installed HUD sheet");
        SheetSprite {
            page: sheet.page,
            uv: sheet.sprite(role).uv,
        }
    };
    capture_status_hud(
        &input,
        sheet.map(|_| &sprite as &dyn Fn(HudTextureRole) -> SheetSprite),
    )
}

#[cfg(test)]
mod absorption_tests;
