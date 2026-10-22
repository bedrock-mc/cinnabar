//! Video renderer choices and the shader-pack library use separate settings tabs.

use super::super::widgets;
use super::Content;
use launcher::menu::{
    MenuAction, RendererChoice,
    settings_storage::{StorageAction, StorageFolder},
    shader_pack_status::pack_label,
};
use launcher::shader_options::Action;
use ui_engine::engine::screen::PaintError;

#[cfg(test)]
mod tests;

/// Keeps the renderer switch in Video and links to the selected shader pack.
pub(super) fn draw(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    if render_model::ENHANCED_RENDERING_ENABLED {
        let description = if content.view.render_mode == ui::RenderMode::ShaderPack {
            format!(
                "Shader pack active: {}. Select Vanilla or Enhanced to turn shaders off.",
                content
                    .view
                    .shader_pack
                    .as_deref()
                    .map_or("Unknown", pack_label)
            )
        } else {
            "Enhanced improves lighting, shadows, sky and reflections.".to_owned()
        };
        picker(
            content,
            "Renderer",
            &description,
            [
                (RendererChoice::Vanilla, "Vanilla"),
                (RendererChoice::Enhanced, "Enhanced"),
            ]
            .into_iter(),
            content.view.renderer,
        )?;
    }
    if render_model::SHADER_PACKS_ENABLED {
        let selected = (content.view.render_mode == ui::RenderMode::ShaderPack)
            .then_some(content.view.shader_pack.as_deref())
            .flatten()
            .map_or("Off", pack_label);
        super::services::action_row(
            content,
            &format!("Shaders: {selected}"),
            "",
            "Open",
            Some(MenuAction::SettingsShaders),
        )?;
    }
    Ok(())
}

/// Draws a vertical pack library with import guidance and controls on the selected pack.
pub(super) fn shaders(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    use super::super::{theme, widgets::Variant};
    use super::resources::{actions, caption, empty, note};

    if !render_model::SHADER_PACKS_ENABLED {
        return Ok(());
    }
    let view = content.view;
    content.heading(
        "Shaders",
        "Change your world's lighting with Java shader packs.",
    )?;
    if view.storage.opens_folders {
        actions(
            content,
            &[(
                "Open shader packs folder",
                Variant::Secondary,
                Some(MenuAction::SettingsStorage(StorageAction::OpenFolder(
                    StorageFolder::ShaderPacks,
                ))),
            )],
        )?;
        content.y += content.canvas.r(1.2);
    }
    caption(
        content,
        "Drop a shader pack .zip or folder onto this page to add it. Selecting a pack applies it immediately.",
    )?;
    if let Some(failure) = view.renderer_failure.as_deref() {
        note(content, failure, theme::NEUTRAL80.fill)?;
    }
    let selected = view.render_mode == ui::RenderMode::ShaderPack;
    pack_row(
        content,
        "Off",
        "Use Vanilla rendering without a shader pack.",
        !selected,
        Some(MenuAction::SelectRenderer(RendererChoice::Vanilla)),
    )?;
    if selected && view.renderer.is_none() {
        let name = view.shader_pack.as_deref().unwrap_or("Selected pack");
        pack_row(
            content,
            pack_label(name),
            "Selected pack is outside this list or is no longer available.",
            true,
            None,
        )?;
        selected_controls(content)?;
    }
    content.heading(
        &format!("My shader packs ({})", view.shader_packs.len()),
        "",
    )?;
    if view.shader_packs.is_empty() {
        empty(
            content,
            "No shader packs added",
            "Add a Java shader pack .zip or folder to get started.",
        )?;
    }
    for (index, name) in view.shader_packs.iter().enumerate() {
        let Ok(index) = u16::try_from(index) else {
            continue;
        };
        let choice = RendererChoice::ShaderPack(index, view.shader_packs_revision);
        let active = selected && view.renderer == Some(choice);
        let subtitle = if active {
            format!("Selected · {name}")
        } else {
            name.to_owned()
        };
        pack_row(
            content,
            pack_label(name),
            &subtitle,
            active,
            Some(MenuAction::SelectRenderer(choice)),
        )?;
        if active {
            selected_controls(content)?;
        }
    }
    caption(
        content,
        "Shader packs are experimental. If a pack cannot load, the game uses Vanilla rendering.",
    )
}

/// Draws a full-width, wrapping library row with a visible and keyboard-accessible selection.
fn pack_row(
    content: &mut Content<'_, '_>,
    title: &str,
    subtitle: &str,
    selected: bool,
    action: Option<MenuAction>,
) -> Result<(), PaintError> {
    use super::super::theme::{BODY, CAPTION, TEXT, TEXT_DIMMER};
    let [left, right] = content.inset();
    let pad = content.canvas.r(1.2);
    let text_left = left + content.canvas.r(5.2);
    let width = (right - text_left - pad).max(1.0);
    let title_height = content.canvas.measure_height(title, width, BODY)?;
    let subtitle_height = content.canvas.measure_height(subtitle, width, CAPTION)?;
    let height = (title_height + subtitle_height + 2.0 * pad).max(content.canvas.r(7.2));
    let bounds = [left, content.y, right, content.y + height];
    widgets::row(content.canvas, content.view, bounds, selected, action)?;
    let state = content.canvas.interaction(content.view, action);
    super::super::radio::draw(
        content.canvas,
        [left + content.canvas.r(2.6), (bounds[1] + bounds[3]) * 0.5],
        selected,
        state,
        action.is_some(),
    )?;
    content.canvas.text(
        title,
        [text_left, content.y + pad],
        width,
        BODY,
        TEXT,
        false,
    )?;
    content.canvas.text(
        subtitle,
        [text_left, content.y + pad + title_height],
        width,
        CAPTION,
        TEXT_DIMMER,
        false,
    )?;
    content.y = bounds[3] + content.canvas.r(0.8);
    Ok(())
}

/// Keeps reload and pack options with the selection they operate on.
fn selected_controls(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    use super::super::widgets::Variant;
    let mut buttons = Vec::new();
    if content.view.shader_options_available {
        buttons.push((
            "Shader pack settings…",
            Variant::Secondary,
            Some(MenuAction::ShaderOptions(Action::Open)),
        ));
    }
    buttons.push((
        "Reload",
        Variant::Secondary,
        Some(MenuAction::ReloadShaders),
    ));
    super::resources::actions(content, &buttons)?;
    content.y += content.canvas.r(1.2);
    Ok(())
}

/// Draws the Video renderer choices with the shared segmented-control focus behavior.
fn picker<'a>(
    content: &mut Content<'_, '_>,
    title: &str,
    description: &str,
    choices: impl Iterator<Item = (RendererChoice, &'a str)> + Clone,
    selected: Option<RendererChoice>,
) -> Result<(), PaintError> {
    let view = content.view;
    let count = choices.clone().count();
    let [left, right] = content.inset();
    let vertical = content.column_width < content.canvas.r(15.0 * count as f32);
    let cell_width = (right - left) / if vertical { 1.0 } else { count as f32 };
    let height = choices.clone().try_fold(0.0_f32, |height, (_, label)| {
        widgets::choice_height(content.canvas, label, cell_width).map(|choice| height.max(choice))
    })?;
    let control_height = height * if vertical { count as f32 } else { 1.0 };
    let extra = control_height + content.canvas.r(0.4);
    let bounds = content.row(title, description, right - left, None, extra)?;
    content.texts(bounds, title, description, right - left, extra)?;
    let top = bounds[3] - control_height - content.canvas.r(1.2);
    let overlap = content.canvas.r(0.2);
    let cell = |index: usize| {
        if vertical {
            [
                left,
                top + index as f32 * height,
                right,
                top + (index + 1) as f32 * height,
            ]
        } else {
            [
                left + index as f32 * cell_width - overlap,
                top,
                left + (index + 1) as f32 * cell_width,
                top + height,
            ]
        }
    };
    for (index, (choice, label)) in choices.clone().enumerate() {
        widgets::choice(
            content.canvas,
            view,
            cell(index),
            label,
            selected == Some(choice),
            MenuAction::SelectRenderer(choice),
        )?;
    }
    for (index, (choice, _)) in choices.clone().enumerate() {
        widgets::choice_focus(
            content.canvas,
            view,
            cell(index),
            selected == Some(choice),
            MenuAction::SelectRenderer(choice),
        )?;
    }
    Ok(())
}
