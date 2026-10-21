//! Renderer and shader-pack settings share segmented choices and keep separate tabs.

use super::super::widgets;
use super::Content;
use launcher::menu::{
    MenuAction, MenuView, RendererChoice,
    settings_storage::{StorageAction, StorageFolder},
    shader_pack_status::pack_label,
};
use launcher::shader_options::Action;
use ui_engine::engine::screen::PaintError;

#[cfg(test)]
mod tests;

/// Keeps the renderer switch in Video and links to the selected shader pack.
pub(super) fn draw(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    if render_model::ENHANCED_RENDERING_ENABLED || render_model::SHADER_PACKS_ENABLED {
        picker(
            content,
            "Renderer",
            if render_model::ENHANCED_RENDERING_ENABLED {
                "Enhanced improves lighting, shadows, sky and reflections."
            } else {
                ""
            },
            std::iter::once((RendererChoice::Vanilla, "Vanilla")).chain(
                render_model::ENHANCED_RENDERING_ENABLED
                    .then_some((RendererChoice::Enhanced, "Enhanced")),
            ),
            content.view.renderer,
        )?;
    }
    if render_model::SHADER_PACKS_ENABLED {
        let selected = content
            .view
            .shader_pack
            .as_deref()
            .map_or("Off", pack_label);
        super::services::action_row(
            content,
            &format!("Shaders: {selected}"),
            "",
            "›",
            Some(MenuAction::SettingsShaders),
        )?;
    }
    Ok(())
}

/// Draws the pack picker, feedback, options, reload and folder import controls.
pub(super) fn shaders(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    if !render_model::SHADER_PACKS_ENABLED {
        return Ok(());
    }
    content.heading("Shaders", "")?;
    let view = content.view;
    let description = if let Some(failure) = view.renderer_failure.as_deref() {
        failure
    } else if view.shader_packs.is_empty() {
        "Java shader packs (zips or folders) in the shaderpacks data folder appear here."
    } else {
        "Java shader packs are experimental; a pack that fails to load draws Vanilla."
    };
    let selected = if view.render_mode == ui::RenderMode::ShaderPack {
        view.renderer
    } else {
        Some(RendererChoice::Vanilla)
    };
    picker(
        content,
        "Shader packs",
        description,
        choices(view),
        selected,
    )?;
    if view.shader_options_available {
        super::services::action_row(
            content,
            "options.cinnabar.shaderOptions",
            "options.cinnabar.shaderOptions.description",
            "options.cinnabar.shaderOptions.buttonLabel",
            Some(MenuAction::ShaderOptions(Action::Open)),
        )?;
    }
    super::services::action_row(
        content,
        "Reload shaders",
        "",
        "Reload",
        Some(MenuAction::ReloadShaders),
    )?;
    if view.storage.opens_folders {
        super::services::action_row(
            content,
            "options.cinnabar.shaderPacksFolder",
            "options.cinnabar.shaderPacksFolder.description",
            "options.cinnabar.openFolder.buttonLabel",
            Some(MenuAction::SettingsStorage(StorageAction::OpenFolder(
                StorageFolder::ShaderPacks,
            ))),
        )?;
    }
    Ok(())
}

/// The discovered packs and Off choice, preserving the list revision in every action.
fn choices(view: &MenuView) -> impl Iterator<Item = (RendererChoice, &str)> + Clone {
    std::iter::once((RendererChoice::Vanilla, "Off")).chain(
        view.shader_packs
            .iter()
            .enumerate()
            .filter_map(|(index, name)| {
                Some((
                    RendererChoice::ShaderPack(
                        u16::try_from(index).ok()?,
                        view.shader_packs_revision,
                    ),
                    pack_label(name),
                ))
            }),
    )
}

/// Draws a segmented picker with the same sizing and focus behavior in either settings tab.
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
