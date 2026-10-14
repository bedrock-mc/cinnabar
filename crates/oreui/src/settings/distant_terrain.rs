//! The Video section's opt-in distant terrain group, and the units its sliders show.

use launcher::menu::{
    MenuAction,
    settings_options::{
        DISTANT_TERRAIN_DISTANCE_OPTION, DISTANT_TERRAIN_OPTION, DISTANT_TERRAIN_QUALITY_OPTION,
        DISTANT_TERRAIN_STORAGE_OPTION, distant_terrain_slider_label,
    },
};
use ui_engine::engine::screen::PaintError;

use super::Content;

/// The opt-in distant terrain extension: its options and the cache clear action.
pub(super) fn draw(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    content.heading(
        DISTANT_TERRAIN_OPTION.label,
        "Draws terrain you have already seen beyond the server's view distance. A Cinnabar extension; it changes nothing on the server.",
    )?;
    super::sections::options(
        content,
        &[
            DISTANT_TERRAIN_OPTION.name,
            DISTANT_TERRAIN_DISTANCE_OPTION.name,
            DISTANT_TERRAIN_QUALITY_OPTION.name,
            DISTANT_TERRAIN_STORAGE_OPTION.name,
        ],
    )?;
    super::services::action_row(
        content,
        "Clear distant terrain cache",
        "Removes the stored distant terrain of every server not in use.",
        "Clear",
        Some(MenuAction::SettingsClearDistantTerrainCache),
    )
}

impl Content<'_, '_> {
    /// The text of slider `name` at `value`: the distant terrain sliders' own units, or a
    /// percentage for every other slider without a format of its own.
    pub(in crate::settings) fn slider_text(&self, name: &str, value: i32) -> String {
        distant_terrain_slider_label(name, value, &|key: &str| self.word(key))
            .unwrap_or_else(|| format!("{value}%"))
    }
}
