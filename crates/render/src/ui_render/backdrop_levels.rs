//! Blur work is bounded by the highest mip a published shader can sample.

use render_model::{UI_BLEND_BACKDROP, UiRenderInput};

/// Keeps a conservative fractional-level margin around CPU/GPU logarithm rounding boundaries.
const LEVEL_MARGIN: f32 = 0.0001;

/// Counts the downsampled levels required by the production shader's fractional LOD.
/// Keeping allocation unchanged avoids reallocating textures throughout a blur transition.
pub(super) fn count(sigma: f32, available: usize) -> usize {
    if sigma <= 0. || available == 0 {
        return 0;
    }
    if !sigma.is_finite() {
        return available;
    }
    // This is the inverse of the shader's reconstruction variance. Pixel regressions
    // compare pruned and complete chains so changes to that kernel cannot go unnoticed.
    let level = 0.5 * ((sigma * sigma + 0.25) * 1.5).log2();
    ((level + LEVEL_MARGIN).floor().max(0.) as usize)
        .saturating_add(1)
        .min(available)
}

/// Reads every indexed backdrop vertex once after validation, including separately painted triangles.
pub(super) fn maximum(input: &UiRenderInput) -> f32 {
    input
        .batches
        .iter()
        .filter(|batch| batch.blend_mode == UI_BLEND_BACKDROP && batch.world_projection == 0)
        .flat_map(|batch| {
            let start = batch.first_index as usize;
            input.indices[start..start + batch.index_count as usize].iter()
        })
        .map(|index| {
            render_api::UiBackdropPaint::from_overlay(input.vertices[*index as usize].overlay_color)
                .blur
        })
        .fold(0., f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Disabled blur skips every downsample; small and large strengths retain only their usable levels.
    #[test]
    fn backdrop_levels_skip_disabled_blur_and_cover_fractional_lods() {
        assert_eq!(count(0., 8), 0);
        assert_eq!(count(-1., 8), 0);
        assert_eq!(count(0.25, 8), 1);
        assert_eq!(count(6., 8), 3);
        assert_eq!(count(12., 8), 4);
        assert_eq!(count(120., 8), 8);
        assert_eq!(count(120., 2), 2);
        assert_eq!(count(12., 0), 0);
        assert_eq!(count(f32::NAN, 8), 8);
        assert_eq!(count(f32::INFINITY, 8), 8);
        assert_eq!(count(f32::MAX, 8), 8);
        for level in 1..8 {
            let boundary = (2. / 3. * 4f32.powi(level) - 0.25).sqrt();
            assert!(count(boundary, 8) >= level as usize);
            assert!(count(boundary.next_up(), 8) > level as usize);
        }
    }

    /// A material batch may contain several paint strengths; unrelated UI never increases its blur.
    #[test]
    fn backdrop_levels_use_every_indexed_paint_and_exclude_other_materials() {
        use render_model::{
            UI_BLEND_ALPHA, UiRenderBatch, UiRenderVertex, UiScissor, UiTextureCatalog,
            UiTexturePage,
        };
        use std::sync::Arc;
        let vertex = |sigma| UiRenderVertex {
            overlay_color: render_api::UiBackdropPaint {
                blur: sigma,
                ..render_api::UiBackdropPaint::IDENTITY
            }
            .overlay(),
            position: [0.; 2],
            clip_z: 0.,
            clip_w: 1.,
            uv: [0.; 2],
            color: [255; 4],
            style_flags: 0,
            alpha_cutoff: -1.,
            model_light: 0.,
        };
        let input = UiRenderInput {
            vertices: [vertex(6.), vertex(12.), vertex(120.)].into(),
            indices: [0, 0, 0, 1, 1, 1, 2, 2, 2].into(),
            batches: [
                UiRenderBatch::new(0, UiScissor::new(0, 0, 16, 16), 0, 6, UI_BLEND_BACKDROP),
                UiRenderBatch::new(0, UiScissor::new(0, 0, 16, 16), 6, 3, UI_BLEND_ALPHA),
            ]
            .into(),
            revision: 1,
            viewport_size: [16; 2],
            safe_area: [0; 4],
            textures: Arc::new(
                UiTextureCatalog::new(
                    vec![UiTexturePage::owned([1, 1], Arc::from([255; 4])).unwrap()],
                    1,
                )
                .unwrap(),
            ),
        };
        input.validate().unwrap();
        assert_eq!(maximum(&input), 12.);
        let mut empty = input;
        empty.batches = [].into();
        assert_eq!(maximum(&empty), 0.);
    }
}
