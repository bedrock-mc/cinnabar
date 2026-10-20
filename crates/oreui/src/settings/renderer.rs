//! The Video renderer picker (Off, Enhanced and Java shader packs) uses the same segmented
//! choices as other Video settings.

use super::super::widgets;
use super::Content;
use launcher::menu::{
    MenuAction, MenuView, RendererChoice,
    settings_storage::{StorageAction, StorageFolder},
};
use ui_engine::engine::screen::PaintError;

#[cfg(test)]
mod tests;

/// Draws nothing unless this build compiles an optional renderer. Shader-pack builds add
/// the shader-pack folder's Open folder button when the host can open folders.
pub(super) fn draw(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    if !render_model::ENHANCED_RENDERING_ENABLED && !render_model::SHADER_PACKS_ENABLED {
        return Ok(());
    }
    renderer(content)?;
    if render_model::SHADER_PACKS_ENABLED && content.view.storage.opens_folders {
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

/// The compiled renderer choices in display order with their labels.
fn choices(view: &MenuView) -> impl Iterator<Item = (RendererChoice, &str)> {
    let enhanced =
        render_model::ENHANCED_RENDERING_ENABLED.then_some((RendererChoice::Enhanced, "Enhanced"));
    let packs: &[String] = if render_model::SHADER_PACKS_ENABLED {
        &view.shader_packs
    } else {
        &[]
    };
    let packs = packs.iter().enumerate().filter_map(|(index, name)| {
        Some((
            RendererChoice::ShaderPack(u16::try_from(index).ok()?, view.shader_packs_revision),
            pack_label(name),
        ))
    });
    std::iter::once((RendererChoice::Vanilla, "Off"))
        .chain(enhanced)
        .chain(packs)
}

/// A pack's display name: its file or folder name without a `.zip` extension.
fn pack_label(name: &str) -> &str {
    name.len()
        .checked_sub(".zip".len())
        .filter(|&split| {
            split > 0
                && name
                    .get(split..)
                    .is_some_and(|extension| extension.eq_ignore_ascii_case(".zip"))
        })
        .and_then(|split| name.get(..split))
        .unwrap_or(name)
}

/// One segmented row choosing between Vanilla and the compiled optional renderers.
fn renderer(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    let view = content.view;
    let count = choices(view).count();
    let [left, right] = content.inset();
    let vertical = content.column_width < content.canvas.r(15.0 * count as f32);
    let cell_width = (right - left) / if vertical { 1.0 } else { count as f32 };
    let height = choices(view).try_fold(0.0_f32, |height, (_, label)| {
        widgets::choice_height(content.canvas, label, cell_width).map(|choice| height.max(choice))
    })?;
    let control_height = height * if vertical { count as f32 } else { 1.0 };
    let extra = control_height + content.canvas.r(0.4);
    let description = if !render_model::SHADER_PACKS_ENABLED {
        "Enhanced improves lighting, shadows, sky and reflections."
    } else if view.shader_packs.is_empty() {
        "Java shader packs (zips or folders) in the shaderpacks data folder appear here."
    } else {
        "Java shader packs are experimental; a pack that fails to load draws Vanilla."
    };
    let title = "Renderer";
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
    for (index, (choice, label)) in choices(view).enumerate() {
        widgets::choice(
            content.canvas,
            view,
            cell(index),
            label,
            view.renderer == Some(choice),
            MenuAction::SelectRenderer(choice),
        )?;
    }
    for (index, (choice, _)) in choices(view).enumerate() {
        widgets::choice_focus(
            content.canvas,
            view,
            cell(index),
            view.renderer == Some(choice),
            MenuAction::SelectRenderer(choice),
        )?;
    }
    Ok(())
}
