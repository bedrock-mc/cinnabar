//! Graphics presets reuse the existing raised segmented choices.

use super::{CAPTION, Content, TEXT_DIMMER, widgets};
use launcher::menu::{MenuAction, settings_options::video_presets::VideoPreset};
use ui_engine::engine::screen::PaintError;

/// Draws the derived selection and all preset actions above the Video controls.
pub(super) fn draw(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    let selected = content.view.settings_options.video_preset();
    let title = format!(
        "Video preset: {}",
        selected.map_or("Custom", VideoPreset::label)
    );
    let description = "Choose a graphics preset. Individual changes show Custom.";
    let [left, right] = content.inset();
    let width = (right - left).max(1.0);
    let cell_width = width / VideoPreset::ALL.len() as f32;
    let control_height = VideoPreset::ALL
        .into_iter()
        .try_fold(0.0_f32, |height, preset| {
            widgets::choice_height(content.canvas, preset.label(), cell_width)
                .map(|choice| height.max(choice))
        })?;
    let description_height = content
        .canvas
        .measure_height(description, width, CAPTION)?
        .max(content.canvas.r(CAPTION.line));
    let extra = control_height + description_height + content.canvas.r(1.2);
    let bounds = content.row(&title, "", width, None, extra)?;
    content.texts(bounds, &title, "", width, extra)?;
    let top = bounds[3] - content.canvas.r(1.2) - extra;
    for focus in [false, true] {
        for (index, preset) in VideoPreset::ALL.into_iter().enumerate() {
            let bounds = [
                left + index as f32 * cell_width - content.canvas.r(0.2),
                top,
                left + (index + 1) as f32 * cell_width,
                top + control_height,
            ];
            let on = selected == Some(preset);
            let action = MenuAction::SettingsVideoPreset(preset);
            if focus {
                widgets::choice_focus(content.canvas, content.view, bounds, on, action)?;
            } else {
                widgets::choice(
                    content.canvas,
                    content.view,
                    bounds,
                    preset.label(),
                    on,
                    action,
                )?;
            }
        }
    }
    content.canvas.text(
        description,
        [left, top + control_height + content.canvas.r(0.8)],
        width,
        CAPTION,
        TEXT_DIMMER,
        false,
    )?;
    super::services::action_row(
        content,
        "Automatic graphics settings",
        "Tune untouched settings for this device. Your edits are kept.",
        "Detect best settings",
        Some(MenuAction::SettingsDetectGraphics),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
