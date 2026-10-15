//! Formatting performed only when a retained HUD component changes.

use json_ui::{BossBar, Sidebar};

use super::super::super::super::{
    HudFrame, bounded_visible_text, hud_layout,
    retained_hud::{PresentedScoreValue, PresentedScoreboard},
};
use crate::ui_runtime::UiRuntime;

/// Cheap visible identities for the translated position and days lines.
#[derive(Clone, Copy, Default, PartialEq)]
pub(super) struct WorldTextInput {
    position: Option<[i32; 3]>,
    days: Option<f64>,
}

/// Projects visibility and day boundaries without formatting any text.
pub(super) fn world_text_input(runtime: &UiRuntime, frame: &HudFrame) -> WorldTextInput {
    let alive = runtime.last_known_local_player_alive.unwrap_or(true);
    let rules = runtime.gameplay_hud();
    WorldTextInput {
        position: frame
            .player_block
            .filter(|_| alive && (rules.show_coordinates() || frame.holding_filled_map)),
        days: frame
            .world_time
            .filter(|_| alive && rules.show_days_played())
            .map(|time| (time / f64::from(world::TICKS_PER_DAY)).floor()),
    }
}

/// Formats visible world lines using the current language tables.
pub(super) fn world_text_lines(
    runtime: &UiRuntime,
    input: WorldTextInput,
) -> (Option<String>, Option<String>) {
    let translate = |key: &str, fallback: &str, arguments: &[String]| {
        let template = runtime
            .translation(key)
            .map_or_else(|| fallback.to_owned(), |text| text.to_string());
        protocol::format_translation(&template, arguments)
    };
    let position = input.position.map(|block| {
        translate(
            "map.position",
            "Position: %s, %s, %s",
            &block.map(|axis| axis.to_string()),
        )
    });
    let days = input.days.map(|days| {
        if days < 0.0 {
            translate("hudScreen.daysPlayed.overflow", "Too many to count!", &[])
        } else {
            translate(
                "hudScreen.daysPlayed",
                "Days played: %s",
                &[format!("{days:.0}")],
            )
        }
    });
    (position, days)
}

/// Copies the existing ordered scoreboard projection into bounded template values.
pub(super) fn sidebar_model(scoreboard: &PresentedScoreboard) -> Sidebar {
    Sidebar {
        title: bounded_visible_text(&scoreboard.title).to_owned(),
        rows: scoreboard
            .rows
            .iter()
            .map(|row| {
                let score = match &row.value {
                    PresentedScoreValue::Text(text) => bounded_visible_text(text).to_owned(),
                    PresentedScoreValue::Hearts {
                        full_hearts,
                        half_heart,
                    } => (u32::from(*full_hearts) * 2 + u32::from(*half_heart)).to_string(),
                };
                (bounded_visible_text(&row.label).to_owned(), score)
            })
            .collect(),
        background_opacity: super::super::SIDEBAR_OPACITY,
        title_background_opacity: super::super::SIDEBAR_TITLE_OPACITY,
    }
}

/// Formats one retained boss bar without changing its stable store ordering.
pub(super) fn boss_model(bar: &ui::BossBarView) -> BossBar {
    let [r, g, b, _] = hud_layout::BOSS_TINTS
        .iter()
        .find(|(color, _)| *color == bar.style.color)
        .map_or([255; 4], |(_, rgba)| *rgba);
    BossBar {
        name: bounded_visible_text(&bar.title).to_owned(),
        progress: f64::from(bar.health),
        color: format!("#{r:02x}{g:02x}{b:02x}"),
        notches: match bar.style.overlay {
            ui::BossOverlay::Progress => 0,
            ui::BossOverlay::Notched6 => 6,
            ui::BossOverlay::Notched10 => 10,
            ui::BossOverlay::Notched12 => 12,
            ui::BossOverlay::Notched20 => 20,
        },
    }
}
