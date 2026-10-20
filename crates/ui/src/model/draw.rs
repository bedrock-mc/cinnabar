use crate::{BedrockColor, GlyphQuad, TEXT_BOLD_OFFSET_64, UiLimits, UiPoint, UiRect};

use super::{
    TextEffects, TextShadow, UI_STYLE_BILINEAR, UiBlendMode, UiDrawBatch, UiError, UiVertex,
    UiVisual, UiWorldProjection,
};

mod mesh;

#[derive(Clone, Copy)]
pub(super) struct DrawSpace<'a> {
    pub(super) clip: UiRect,
    pub(super) projection: Option<&'a UiWorldProjection>,
    pub(super) node: super::UiNodeId,
    /// The node's paint transform in frame coordinates, applied to every emitted vertex.
    pub(super) transform: Option<FrameAffine>,
    /// The filter group every emitted alpha batch draws into.
    pub(super) filter: Option<u32>,
}

impl DrawSpace<'_> {
    /// The filter group a batch of `blend` draws into: only alpha draws filter, while
    /// inverted, backdrop and filter geometry keep their own compositing.
    pub(super) fn filter_for(&self, blend: UiBlendMode) -> Option<u32> {
        self.filter.filter(|_| blend == UiBlendMode::Alpha)
    }
}

/// A 2-D affine map in frame coordinates: `p -> linear * p + translation`, `linear`
/// column-major `[a, b, c, d]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct FrameAffine {
    pub(super) linear: [f32; 4],
    pub(super) translation: [f32; 2],
}

impl FrameAffine {
    /// `transform`, authored in coordinates local to a node, as a frame map for a node laid
    /// out at `bounds` under `scale` frame units per node unit.
    pub(super) fn for_node(transform: &super::UiTransform, bounds: UiRect, scale: f32) -> Self {
        let [a, b, c, d] = transform.linear;
        let origin = [bounds.min().x(), bounds.min().y()];
        let moved = [a * origin[0] + c * origin[1], b * origin[0] + d * origin[1]];
        Self {
            linear: transform.linear,
            translation: [
                origin[0] - moved[0] + scale * transform.translation[0],
                origin[1] - moved[1] + scale * transform.translation[1],
            ],
        }
    }

    /// Maps each vertex's screen position, preserving its homogeneous `clip_w`.
    fn apply(&self, vertices: &mut [UiVertex]) {
        let [a, b, c, d] = self.linear;
        for vertex in vertices {
            let w = vertex.clip_w;
            if w == 0.0 {
                continue;
            }
            let [x, y] = vertex.position.map(|value| value / w);
            vertex.position = [
                (a * x + c * y + self.translation[0]) * w,
                (b * x + d * y + self.translation[1]) * w,
            ];
        }
    }
}

/// Design-pixel lean of an italic glyph's top edge, scaled by the layout
/// scale. Native visual confirmation pending.
const ITALIC_SHEAR_PX: f32 = 1.0;

/// Emits `visual` at `bounds`, then moves its vertices by the space's paint transform; the
/// batch clip stays a frame-space scissor.
pub(super) fn emit_visual(
    visual: &UiVisual,
    bounds: UiRect,
    clip: DrawSpace<'_>,
    effects: TextEffects<'_>,
    vertices: &mut Vec<UiVertex>,
    indices: &mut Vec<u32>,
    batches: &mut Vec<UiDrawBatch>,
) -> Result<(), UiError> {
    let start = vertices.len();
    emit_untransformed(visual, bounds, clip, effects, vertices, indices, batches)?;
    if let Some(transform) = clip.transform.filter(|_| clip.projection.is_none()) {
        transform.apply(&mut vertices[start..]);
    }
    Ok(())
}

fn emit_untransformed(
    visual: &UiVisual,
    bounds: UiRect,
    clip: DrawSpace<'_>,
    effects: TextEffects<'_>,
    vertices: &mut Vec<UiVertex>,
    indices: &mut Vec<u32>,
    batches: &mut Vec<UiDrawBatch>,
) -> Result<(), UiError> {
    match visual {
        UiVisual::None => Ok(()),
        UiVisual::Mesh(mesh) => mesh::emit_mesh(mesh, bounds, clip, vertices, indices, batches),
        UiVisual::Solid {
            texture_page,
            color,
        } => {
            if is_empty(bounds) {
                return Ok(());
            }
            emit_quad(
                bounds,
                [[0, 0], [1, 0], [1, 1], [0, 1]],
                *texture_page,
                *color,
                0,
                UiBlendMode::Alpha,
                clip,
                vertices,
                indices,
                batches,
            )
        }
        UiVisual::Sprite {
            texture_page,
            uv,
            color,
        }
        | UiVisual::GlintSprite {
            texture_page,
            uv,
            color,
        } => {
            if is_empty(bounds) {
                return Ok(());
            }
            let style = if matches!(visual, UiVisual::GlintSprite { .. }) {
                super::UI_STYLE_GLINT
            } else {
                0
            };
            emit_quad(
                bounds,
                [
                    [uv[0], uv[1]],
                    [uv[2], uv[1]],
                    [uv[2], uv[3]],
                    [uv[0], uv[3]],
                ],
                *texture_page,
                *color,
                style,
                UiBlendMode::Alpha,
                clip,
                vertices,
                indices,
                batches,
            )
        }
        UiVisual::Gradient {
            texture_page,
            colors: [from, to],
            horizontal,
        } => {
            if is_empty(bounds) {
                return Ok(());
            }
            let corners = if *horizontal {
                [*from, *to, *to, *from]
            } else {
                [*from, *from, *to, *to]
            };
            emit_colored_quad(
                [
                    [bounds.min().x(), bounds.min().y()],
                    [bounds.max().x(), bounds.min().y()],
                    [bounds.max().x(), bounds.max().y()],
                    [bounds.min().x(), bounds.max().y()],
                ],
                [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                *texture_page,
                corners,
                0,
                UiBlendMode::Alpha,
                clip,
                vertices,
                indices,
                batches,
            )
        }
        UiVisual::StyledSprite {
            texture_page,
            uv,
            color,
            style,
        } => {
            if is_empty(bounds) {
                return Ok(());
            }
            emit_colored_quad(
                [
                    [bounds.min().x(), bounds.min().y()],
                    [bounds.max().x(), bounds.min().y()],
                    [bounds.max().x(), bounds.max().y()],
                    [bounds.min().x(), bounds.max().y()],
                ],
                [
                    [uv[0], uv[1]],
                    [uv[2], uv[1]],
                    [uv[2], uv[3]],
                    [uv[0], uv[3]],
                ],
                *texture_page,
                [*color; 4],
                u16::from(*style),
                UiBlendMode::Alpha,
                clip,
                vertices,
                indices,
                batches,
            )
        }
        UiVisual::RepeatedSprite {
            texture_page,
            tile_uv: uv,
            source,
            color,
            style,
        } => {
            if is_empty(bounds) {
                return Ok(());
            }
            let first = vertices.len();
            emit_colored_quad(
                [
                    [bounds.min().x(), bounds.min().y()],
                    [bounds.max().x(), bounds.min().y()],
                    [bounds.max().x(), bounds.max().y()],
                    [bounds.min().x(), bounds.max().y()],
                ],
                [
                    [uv[0], uv[1]],
                    [uv[2], uv[1]],
                    [uv[2], uv[3]],
                    [uv[0], uv[3]],
                ],
                *texture_page,
                [*color; 4],
                0,
                UiBlendMode::Alpha,
                clip,
                vertices,
                indices,
                batches,
            )?;
            for vertex in &mut vertices[first..] {
                vertex.style_flags |= *style;
                vertex.overlay_color = *source;
            }
            Ok(())
        }
        UiVisual::RotatedSprite {
            texture_page,
            uv,
            color,
            angle_radians,
        } => {
            if is_empty(bounds) {
                return Ok(());
            }
            emit_rotated_quad(
                bounds,
                [
                    [uv[0], uv[1]],
                    [uv[2], uv[1]],
                    [uv[2], uv[3]],
                    [uv[0], uv[3]],
                ],
                *texture_page,
                *color,
                *angle_radians,
                0,
                UiBlendMode::Alpha,
                clip,
                vertices,
                indices,
                batches,
            )
        }
        UiVisual::SpriteOutline {
            texture_page,
            uv,
            source,
            overlap_offset,
            color,
            blend,
        } => {
            if is_empty(bounds) {
                return Ok(());
            }
            let start = vertices.len();
            emit_colored_quad(
                [
                    [bounds.min().x(), bounds.min().y()],
                    [bounds.max().x(), bounds.min().y()],
                    [bounds.max().x(), bounds.max().y()],
                    [bounds.min().x(), bounds.max().y()],
                ],
                [
                    [uv[0], uv[1]],
                    [uv[2], uv[1]],
                    [uv[2], uv[3]],
                    [uv[0], uv[3]],
                ],
                *texture_page,
                [*color; 4],
                0,
                *blend,
                clip,
                vertices,
                indices,
                batches,
            )?;
            for vertex in &mut vertices[start..] {
                vertex.style_flags = render_api::UI_STYLE_SPRITE_OUTLINE;
                vertex.overlay_color = *source;
                vertex.model_light = overlap_offset[0];
                vertex.alpha_cutoff = overlap_offset[1];
            }
            Ok(())
        }
        UiVisual::InvertedSprite { texture_page, uv } => {
            if is_empty(bounds) {
                return Ok(());
            }
            emit_quad(
                bounds,
                [
                    [uv[0], uv[1]],
                    [uv[2], uv[1]],
                    [uv[2], uv[3]],
                    [uv[0], uv[3]],
                ],
                *texture_page,
                [255; 4],
                0,
                UiBlendMode::Invert,
                clip,
                vertices,
                indices,
                batches,
            )
        }
        UiVisual::Text {
            layout,
            color,
            shadow,
        } => emit_text(
            layout, *color, *shadow, None, bounds, clip, effects, vertices, indices, batches,
        ),
        UiVisual::RotatedText {
            layout,
            color,
            shadow,
            angle_radians,
        } => {
            let angle = if angle_radians.is_finite() {
                *angle_radians
            } else {
                0.0
            };
            let (sin, cos) = angle.sin_cos();
            let center = [
                (bounds.min().x() + bounds.max().x()) * 0.5,
                (bounds.min().y() + bounds.max().y()) * 0.5,
            ];
            emit_text(
                layout,
                *color,
                *shadow,
                Some(Rotation { center, sin, cos }),
                bounds,
                clip,
                effects,
                vertices,
                indices,
                batches,
            )
        }
    }
}

/// A rotation about `center`, applied to each glyph's corners.
#[derive(Clone, Copy)]
struct Rotation {
    center: [f32; 2],
    sin: f32,
    cos: f32,
}

impl Rotation {
    fn apply(self, point: [f32; 2]) -> [f32; 2] {
        let offset = [point[0] - self.center[0], point[1] - self.center[1]];
        [
            self.center[0] + offset[0] * self.cos - offset[1] * self.sin,
            self.center[1] + offset[0] * self.sin + offset[1] * self.cos,
        ]
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_text(
    layout: &crate::TextLayout,
    color: [u8; 4],
    shadow: TextShadow,
    rotation: Option<Rotation>,
    bounds: UiRect,
    clip: DrawSpace<'_>,
    effects: TextEffects<'_>,
    vertices: &mut Vec<UiVertex>,
    indices: &mut Vec<u32>,
    batches: &mut Vec<UiDrawBatch>,
) -> Result<(), UiError> {
    // Mojang's client draws the entire shadowed run before the run
    // itself, so an overlapping glyph never casts a shadow over an
    // already-drawn neighbour.
    let scale = f32::from(layout.key().scale_1024) / 1_024.0;
    let effect_scale = layout.key().effect_scale();
    let layout_id = layout.id();
    let wrap = layout.key().wrap;
    let device_grid = (wrap.snap_glyphs_to_device_pixels
        && wrap.device_scale_65536 != 0
        && rotation.is_none()
        && clip.projection.is_none()
        && clip.transform.is_none())
    .then_some(wrap.device_scale_65536 as f32 / 65_536.0);
    let shadow_pass = match shadow {
        TextShadow::None => None,
        TextShadow::Offset64(offset_64) => Some((scale * offset_64 as f32 / 64.0, true)),
    };
    for (offset, shadowed) in shadow_pass.into_iter().chain(std::iter::once((0.0, false))) {
        for (index, glyph) in layout.glyphs().iter().enumerate() {
            let glyph_bounds = UiRect::new(
                UiPoint::new(
                    bounds.min().x() + glyph.bounds_64[0] as f32 / 64.0 + offset,
                    bounds.min().y() + glyph.bounds_64[1] as f32 / 64.0 + offset,
                )
                .map_err(|_| UiError::DrawIndexOverflow)?,
                UiPoint::new(
                    bounds.min().x() + glyph.bounds_64[2] as f32 / 64.0 + offset,
                    bounds.min().y() + glyph.bounds_64[3] as f32 / 64.0 + offset,
                )
                .map_err(|_| UiError::DrawIndexOverflow)?,
            )
            .map_err(|_| UiError::DrawIndexOverflow)?;
            if is_empty(glyph_bounds) {
                continue;
            }
            let glyph_color = style_color(glyph.style.color, color, effects.palette);
            let glyph_color = if glyph.rendering == assets::FontRendering::Coverage
                && assets::is_private_use_glyph(glyph.resolved_codepoint)
            {
                [255, 255, 255, glyph_color[3]]
            } else {
                glyph_color
            };
            let glyph_color = if shadowed {
                shadow_color(glyph_color)
            } else {
                glyph_color
            };
            // §k swaps to a same-width raster, stable within a frame
            // (so both passes agree) and animated across frames.
            let (page, uv) = obfuscated_raster(glyph, index, layout_id, effects);
            let shear = if glyph.style.italic {
                ITALIC_SHEAR_PX * effect_scale
            } else {
                0.0
            };
            let bold_offset = glyph
                .style
                .bold
                .then_some(TEXT_BOLD_OFFSET_64 as f32 / 64.0 * effect_scale);
            emit_text_glyph(
                glyph_bounds,
                uv,
                page,
                glyph_color,
                u16::from(u8::from(glyph.linear_sampling) * UI_STYLE_BILINEAR)
                    | glyph.rendering.style_flags(),
                shear,
                bold_offset,
                device_grid,
                rotation,
                clip,
                vertices,
                indices,
                batches,
            )?;
        }
    }
    Ok(())
}

/// The `(page, uv)` to draw for one glyph: a same-width scramble target when
/// obfuscated and a pool is present, otherwise the glyph's own raster.
fn obfuscated_raster(
    glyph: &GlyphQuad,
    index: usize,
    layout_id: u64,
    effects: TextEffects<'_>,
) -> (u16, [u16; 4]) {
    if glyph.style.obfuscated
        && let Some(pool) = effects.obfuscation
    {
        let width = glyph.uv[2].saturating_sub(glyph.uv[0]);
        let selector = obfuscation_selector(effects.obfuscation_seed, layout_id, index);
        if let Some(swapped) = pool.pick(width, selector) {
            return swapped;
        }
    }
    (glyph.page, glyph.uv)
}

/// splitmix64 mix so a `(seed, layout, glyph)` triple selects one pool slot;
/// keying on the glyph index decorrelates adjacent cells and both draw passes
/// resolve the same slot within a frame.
fn obfuscation_selector(seed: u64, layout_id: u64, index: usize) -> u64 {
    let mut value = seed
        ^ layout_id.wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (index as u64).wrapping_mul(0xD1B5_4A32_D192_ED03);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

/// Emits one glyph: italic leans the top edge right by `shear`, and a
/// `Some(bold_offset)` draws a second copy shifted right to embolden it. Styles
/// use geometry; vertices retain the font's sampling and coverage flags.
#[allow(clippy::too_many_arguments)]
fn emit_text_glyph(
    glyph_bounds: UiRect,
    uv: [u16; 4],
    page: u16,
    color: [u8; 4],
    style_flags: u16,
    shear: f32,
    bold_offset: Option<f32>,
    device_grid: Option<f32>,
    rotation: Option<Rotation>,
    clip: DrawSpace<'_>,
    vertices: &mut Vec<UiVertex>,
    indices: &mut Vec<u32>,
    batches: &mut Vec<UiDrawBatch>,
) -> Result<(), UiError> {
    let uv_corners = [
        [uv[0], uv[1]],
        [uv[2], uv[1]],
        [uv[2], uv[3]],
        [uv[0], uv[3]],
    ];
    let x0 = glyph_bounds.min().x();
    let y0 = glyph_bounds.min().y();
    let x1 = glyph_bounds.max().x();
    let y1 = glyph_bounds.max().y();
    for dx in [Some(0.0_f32), bold_offset].into_iter().flatten() {
        let positions = [
            [x0 + shear + dx, y0],
            [x1 + shear + dx, y0],
            [x1 + dx, y1],
            [x0 + dx, y1],
        ]
        .map(|point| rotation.map_or(point, |rotation| rotation.apply(point)))
        .map(|point| device_grid.map_or(point, |dpi| point.map(|v| (v * dpi).round() / dpi)));
        emit_positioned_quad(
            positions,
            uv_corners,
            page,
            color,
            style_flags,
            UiBlendMode::Alpha,
            clip,
            vertices,
            indices,
            batches,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_quad(
    bounds: UiRect,
    uv: [[u16; 2]; 4],
    texture_page: u16,
    color: [u8; 4],
    style_flags: u8,
    blend: UiBlendMode,
    clip: DrawSpace<'_>,
    vertices: &mut Vec<UiVertex>,
    indices: &mut Vec<u32>,
    batches: &mut Vec<UiDrawBatch>,
) -> Result<(), UiError> {
    let positions = [
        [bounds.min().x(), bounds.min().y()],
        [bounds.max().x(), bounds.min().y()],
        [bounds.max().x(), bounds.max().y()],
        [bounds.min().x(), bounds.max().y()],
    ];
    emit_positioned_quad(
        positions,
        uv,
        texture_page,
        color,
        u16::from(style_flags),
        blend,
        clip,
        vertices,
        indices,
        batches,
    )
}

#[allow(clippy::too_many_arguments)]
fn emit_rotated_quad(
    bounds: UiRect,
    uv: [[u16; 2]; 4],
    texture_page: u16,
    color: [u8; 4],
    angle_radians: f32,
    style_flags: u8,
    blend: UiBlendMode,
    clip: DrawSpace<'_>,
    vertices: &mut Vec<UiVertex>,
    indices: &mut Vec<u32>,
    batches: &mut Vec<UiDrawBatch>,
) -> Result<(), UiError> {
    let angle = if angle_radians.is_finite() {
        angle_radians
    } else {
        0.0
    };
    let (sin, cos) = angle.sin_cos();
    let center = [
        (bounds.min().x() + bounds.max().x()) * 0.5,
        (bounds.min().y() + bounds.max().y()) * 0.5,
    ];
    let unrotated = [
        [bounds.min().x(), bounds.min().y()],
        [bounds.max().x(), bounds.min().y()],
        [bounds.max().x(), bounds.max().y()],
        [bounds.min().x(), bounds.max().y()],
    ];
    let positions = unrotated.map(|position| {
        let offset = [position[0] - center[0], position[1] - center[1]];
        [
            center[0] + offset[0] * cos - offset[1] * sin,
            center[1] + offset[0] * sin + offset[1] * cos,
        ]
    });
    emit_positioned_quad(
        positions,
        uv,
        texture_page,
        color,
        u16::from(style_flags),
        blend,
        clip,
        vertices,
        indices,
        batches,
    )
}

#[allow(clippy::too_many_arguments)]
fn emit_positioned_quad(
    positions: [[f32; 2]; 4],
    uv: [[u16; 2]; 4],
    texture_page: u16,
    color: [u8; 4],
    style_flags: u16,
    blend: UiBlendMode,
    clip: DrawSpace<'_>,
    vertices: &mut Vec<UiVertex>,
    indices: &mut Vec<u32>,
    batches: &mut Vec<UiDrawBatch>,
) -> Result<(), UiError> {
    emit_colored_quad(
        positions,
        uv.map(|corner| corner.map(f32::from)),
        texture_page,
        [color; 4],
        style_flags,
        blend,
        clip,
        vertices,
        indices,
        batches,
    )
}

/// [`emit_positioned_quad`] with a colour per corner.
#[allow(clippy::too_many_arguments)]
fn emit_colored_quad(
    positions: [[f32; 2]; 4],
    uv: [[f32; 2]; 4],
    texture_page: u16,
    colors: [[u8; 4]; 4],
    style_flags: u16,
    blend: UiBlendMode,
    clip: DrawSpace<'_>,
    vertices: &mut Vec<UiVertex>,
    indices: &mut Vec<u32>,
    batches: &mut Vec<UiDrawBatch>,
) -> Result<(), UiError> {
    let next_vertices = vertices
        .len()
        .checked_add(4)
        .ok_or(UiError::DrawIndexOverflow)?;
    if next_vertices > UiLimits::MAX_UI_VERTICES {
        return Err(UiError::VertexLimitExceeded {
            actual: next_vertices,
            limit: UiLimits::MAX_UI_VERTICES,
        });
    }
    let next_indices = indices
        .len()
        .checked_add(6)
        .ok_or(UiError::DrawIndexOverflow)?;
    if next_indices > UiLimits::MAX_UI_INDICES {
        return Err(UiError::IndexLimitExceeded {
            actual: next_indices,
            limit: UiLimits::MAX_UI_INDICES,
        });
    }
    let base = u32::try_from(vertices.len()).map_err(|_| UiError::DrawIndexOverflow)?;
    for ((position, uv), color) in positions.into_iter().zip(uv).zip(colors) {
        let (position, clip_z, clip_w) = match clip.projection {
            Some(projection) => projection
                .project(position)
                .ok_or(UiError::DrawIndexOverflow)?,
            None => (position, 0.0, 1.0),
        };
        vertices.push(UiVertex {
            position,
            clip_z,
            clip_w,
            uv,
            color,
            style_flags,
            alpha_test: clip
                .projection
                .is_some_and(|projection| projection.alpha_test),
            alpha_cutoff: -1.0,
            model_light: 1.0,
            overlay_color: [0.0; 4],
        });
    }
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    let start = u32::try_from(indices.len() - 6).map_err(|_| UiError::DrawIndexOverflow)?;
    let end = u32::try_from(indices.len()).map_err(|_| UiError::DrawIndexOverflow)?;
    if let Some(batch) = batches.last_mut()
        && batch.texture_page == texture_page
        && batch.clip == clip.clip
        && batch.blend == blend
        && batch.depth_test
            == clip
                .projection
                .is_some_and(|projection| projection.depth_test)
        && batch.depth_write
            == clip
                .projection
                .is_some_and(|projection| projection.depth_write)
        && batch.world_projection == clip.projection.is_some()
        && batch.isolated_depth_scope.is_none()
        && batch.filter_group == clip.filter_for(blend)
        && batch.index_range.end == start
    {
        batch.index_range.end = end;
        return Ok(());
    }
    let actual = batches
        .len()
        .checked_add(1)
        .ok_or(UiError::DrawIndexOverflow)?;
    if actual > UiLimits::MAX_DRAW_BATCHES {
        return Err(UiError::DrawBatchLimitExceeded {
            actual,
            limit: UiLimits::MAX_DRAW_BATCHES,
        });
    }
    batches.push(UiDrawBatch {
        texture_page,
        clip: clip.clip,
        blend,
        depth_test: clip
            .projection
            .is_some_and(|projection| projection.depth_test),
        depth_write: clip
            .projection
            .is_some_and(|projection| projection.depth_write),
        world_projection: clip.projection.is_some(),
        isolated_depth_scope: None,
        filter_group: clip.filter_for(blend),
        index_range: start..end,
    });
    Ok(())
}

/// Mojang's shadow colour: each channel quartered, alpha preserved.
fn shadow_color(color: [u8; 4]) -> [u8; 4] {
    [color[0] >> 2, color[1] >> 2, color[2] >> 2, color[3]]
}

/// Replaces RGB from the active formatting table while preserving the label alpha.
fn style_color(
    style: BedrockColor,
    base: [u8; 4],
    palette: Option<&crate::FormattingPalette>,
) -> [u8; 4] {
    let Some(rgb) = palette.map_or_else(|| style.rgb(), |palette| palette.rgb(style)) else {
        return base;
    };
    [rgb[0], rgb[1], rgb[2], base[3]]
}

pub(super) fn is_empty(rect: UiRect) -> bool {
    rect.width() == 0.0 || rect.height() == 0.0
}
