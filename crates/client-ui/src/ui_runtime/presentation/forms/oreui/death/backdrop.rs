//! A single analytic gradient quad preserves the smooth elliptical death vignette.

use super::super::super::super::UiPresentationError;
use super::super::paint::Canvas;
use launcher::menu::death::{BACKDROP_SECONDS, OVERLAY_FADE_SECONDS};
use std::sync::Arc;
use ui::{UiBlendMode, UiMesh, UiMeshBatch, UiMeshVertex};

/// Draws an expanding radial gradient without tessellation or texture resampling.
pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    size: [f32; 2],
    age: f64,
    animations: bool,
) -> Result<(), UiPresentationError> {
    let ease = |t| super::bezier(t, [0.25, 0.1, 0.25, 1.0]);
    let opacity = if age < 0.0 {
        0.0
    } else if animations {
        ease(age / OVERLAY_FADE_SECONDS)
    } else {
        1.0
    };
    let scale = if animations {
        3.0 - 2.0 * ease(age / BACKDROP_SECONDS)
    } else {
        1.0
    };
    let opacity = opacity * canvas.alpha;
    let vertices: Vec<_> = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]
        .into_iter()
        .map(|position| UiMeshVertex {
            position,
            clip_z: 0.0,
            clip_w: 1.0,
            uv: position.map(|coordinate| (coordinate * 2.0 - 1.0) / scale),
            color: [0, 0, 0, (102.0 * opacity).round() as u8],
            model_light: 1.0,
            overlay_color: [45.0 / 255.0, 4.0 / 255.0, 4.0 / 255.0, 0.8 * opacity],
            style_flags: render_model::UI_STYLE_RADIAL_GRADIENT as u8,
            alpha_test: false,
        })
        .collect();
    let mesh = UiMesh::new(
        vertices.into(),
        Arc::from([0, 1, 2, 0, 2, 3]),
        Arc::from([UiMeshBatch {
            texture_page: canvas.solid_page,
            index_range: 0..6,
            blend: UiBlendMode::Alpha,
            depth_test: false,
            depth_write: false,
            alpha_cutoff: None,
        }]),
    )
    .map_err(|_| UiPresentationError::Tree(ui::UiError::DrawIndexOverflow))?;
    canvas.mesh([0.0, 0.0, size[0], size[1]], Arc::new(mesh))
}
