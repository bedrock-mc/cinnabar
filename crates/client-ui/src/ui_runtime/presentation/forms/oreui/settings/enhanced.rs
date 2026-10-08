//! Enhanced rendering uses the same switch and segmented choices as other Video settings.

use super::super::super::super::UiPresentationError;
use super::super::theme::{self, BODY, EDGE};
use super::super::widgets;
use super::{Content, button};
use crate::menu::MenuAction;
use ui::{EnhancedQuality, RenderMode};

pub(super) fn draw(content: &mut Content<'_, '_>) -> Result<(), UiPresentationError> {
    if !render_model::ENHANCED_RENDERING_ENABLED {
        return Ok(());
    }
    let enabled = content.view.render_mode == RenderMode::Enhanced;
    content.boolean(
        "Enhanced rendering",
        "Improved lighting, shadows, sky and reflections.",
        enabled,
        MenuAction::ToggleRenderMode,
        true,
    )?;
    let [left, right] = content.inset();
    let count = EnhancedQuality::ALL.len();
    let vertical = content.column_width < content.canvas.r(15.0 * count as f32);
    let cell_width = (right - left) / if vertical { 1.0 } else { count as f32 };
    let height = EnhancedQuality::ALL
        .into_iter()
        .try_fold(0.0_f32, |height, quality| {
            widgets::choice_height(content.canvas, quality.label(), cell_width)
                .map(|choice| height.max(choice))
        })?;
    let control_height = height * if vertical { count as f32 } else { 1.0 };
    let extra = control_height + content.canvas.r(0.4);
    let description = if enabled {
        "Choose the balance between frame rate and visual detail."
    } else {
        "Enable Enhanced rendering to change its quality."
    };
    let title = "Enhanced shader quality";
    let bounds = content.row(title, description, right - left, None, extra)?;
    content.texts(bounds, title, description, right - left, extra)?;
    let top = bounds[3] - control_height - content.canvas.r(1.2);
    let cells = std::array::from_fn::<_, { EnhancedQuality::ALL.len() }, _>(|index| {
        if vertical {
            [
                left,
                top + index as f32 * height,
                right,
                top + (index + 1) as f32 * height,
            ]
        } else {
            [
                left + index as f32 * cell_width - content.canvas.r(0.2),
                top,
                left + (index + 1) as f32 * cell_width,
                top + height,
            ]
        }
    });
    for (quality, cell) in EnhancedQuality::ALL.into_iter().zip(cells) {
        let selected = content.view.enhanced_quality == quality;
        if enabled {
            widgets::choice(
                content.canvas,
                content.view,
                cell,
                quality.label(),
                selected,
                MenuAction::SetEnhancedQuality(quality),
            )?;
        } else {
            let (_, face) =
                button::procedural_face(content.canvas, cell, Default::default(), false)?;
            let pad = content.canvas.r(1.2);
            let width = (face[2] - face[0] - pad * 2.0).max(1.0);
            let text_height = content
                .canvas
                .measure_height(quality.label(), width, BODY)?;
            let role = content.canvas.role(theme::DISABLED);
            content.canvas.centered_wrapped_text(
                quality.label(),
                [face[0] + pad, (face[1] + face[3] - text_height) * 0.5],
                width,
                BODY,
                role.text,
            )?;
            if selected {
                let centre = (face[0] + face[2]) * 0.5;
                let half = content.canvas.r(2.4).min((face[2] - face[0]) * 0.5);
                content.canvas.fill(
                    [
                        centre - half,
                        face[3] - content.canvas.r(EDGE),
                        centre + half,
                        face[3],
                    ],
                    role.text,
                )?;
            }
        }
    }
    if enabled {
        for (quality, cell) in EnhancedQuality::ALL.into_iter().zip(cells) {
            widgets::choice_focus(
                content.canvas,
                content.view,
                cell,
                content.view.enhanced_quality == quality,
                MenuAction::SetEnhancedQuality(quality),
            )?;
        }
    }
    Ok(())
}
