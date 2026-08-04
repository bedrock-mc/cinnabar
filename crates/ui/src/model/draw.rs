use crate::{BedrockColor, UiLimits, UiPoint, UiRect};

use super::{TextShadow, UiBlendMode, UiDrawBatch, UiError, UiVertex, UiVisual};

pub(super) fn emit_visual(
    visual: &UiVisual,
    bounds: UiRect,
    clip: UiRect,
    vertices: &mut Vec<UiVertex>,
    indices: &mut Vec<u32>,
    batches: &mut Vec<UiDrawBatch>,
) -> Result<(), UiError> {
    match visual {
        UiVisual::None => Ok(()),
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
        } => {
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
                *color,
                0,
                UiBlendMode::Alpha,
                clip,
                vertices,
                indices,
                batches,
            )
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
        } => {
            // Mojang's client draws the entire shadowed run before the run
            // itself, so an overlapping glyph never casts a shadow over an
            // already-drawn neighbour.
            let shadow_pass = match shadow {
                TextShadow::None => None,
                TextShadow::Offset64(offset_64) => Some((
                    f32::from(layout.key().scale_1024) / 1_024.0 * *offset_64 as f32 / 64.0,
                    true,
                )),
            };
            for (offset, shadowed) in shadow_pass.into_iter().chain(std::iter::once((0.0, false))) {
                for glyph in layout.glyphs() {
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
                    let glyph_color = style_color(glyph.style.color, *color);
                    let glyph_color = if shadowed {
                        shadow_color(glyph_color)
                    } else {
                        glyph_color
                    };
                    let style_flags = u8::from(glyph.style.obfuscated)
                        | (u8::from(glyph.style.bold) << 1)
                        | (u8::from(glyph.style.italic) << 2);
                    emit_quad(
                        glyph_bounds,
                        [
                            [glyph.uv[0], glyph.uv[1]],
                            [glyph.uv[2], glyph.uv[1]],
                            [glyph.uv[2], glyph.uv[3]],
                            [glyph.uv[0], glyph.uv[3]],
                        ],
                        glyph.page,
                        glyph_color,
                        style_flags,
                        UiBlendMode::Alpha,
                        clip,
                        vertices,
                        indices,
                        batches,
                    )?;
                }
            }
            Ok(())
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_quad(
    bounds: UiRect,
    uv: [[u16; 2]; 4],
    texture_page: u16,
    color: [u8; 4],
    style_flags: u8,
    blend: UiBlendMode,
    clip: UiRect,
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
        style_flags,
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
    clip: UiRect,
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
        style_flags,
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
    style_flags: u8,
    blend: UiBlendMode,
    clip: UiRect,
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
    vertices.extend(
        positions
            .into_iter()
            .zip(uv)
            .map(|(position, uv)| UiVertex {
                position,
                uv,
                color,
                style_flags,
            }),
    );
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    let start = u32::try_from(indices.len() - 6).map_err(|_| UiError::DrawIndexOverflow)?;
    let end = u32::try_from(indices.len()).map_err(|_| UiError::DrawIndexOverflow)?;
    if let Some(batch) = batches.last_mut()
        && batch.texture_page == texture_page
        && batch.clip == clip
        && batch.blend == blend
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
        clip,
        blend,
        index_range: start..end,
    });
    Ok(())
}

/// Mojang's shadow colour: each channel quartered, alpha preserved.
fn shadow_color(color: [u8; 4]) -> [u8; 4] {
    [color[0] >> 2, color[1] >> 2, color[2] >> 2, color[3]]
}

fn style_color(style: BedrockColor, base: [u8; 4]) -> [u8; 4] {
    let rgb = match style {
        BedrockColor::White => return base,
        BedrockColor::Black => [0, 0, 0],
        BedrockColor::DarkBlue => [0, 0, 170],
        BedrockColor::DarkGreen => [0, 170, 0],
        BedrockColor::DarkAqua => [0, 170, 170],
        BedrockColor::DarkRed => [170, 0, 0],
        BedrockColor::DarkPurple => [170, 0, 170],
        BedrockColor::Gold => [255, 170, 0],
        BedrockColor::Gray => [170, 170, 170],
        BedrockColor::DarkGray => [85, 85, 85],
        BedrockColor::Blue => [85, 85, 255],
        BedrockColor::Green => [85, 255, 85],
        BedrockColor::Aqua => [85, 255, 255],
        BedrockColor::Red => [255, 85, 85],
        BedrockColor::LightPurple => [255, 85, 255],
        BedrockColor::Yellow => [255, 255, 85],
        BedrockColor::MinecoinGold => [221, 214, 5],
        BedrockColor::MaterialQuartz => [227, 212, 209],
        BedrockColor::MaterialIron => [206, 202, 202],
        BedrockColor::MaterialNetherite => [68, 58, 59],
        BedrockColor::MaterialRedstone => [151, 22, 7],
        BedrockColor::MaterialCopper => [180, 104, 77],
        BedrockColor::MaterialGold => [222, 177, 45],
        BedrockColor::MaterialEmerald => [17, 160, 54],
        BedrockColor::MaterialDiamond => [44, 186, 168],
        BedrockColor::MaterialLapis => [35, 98, 180],
        BedrockColor::MaterialAmethyst => [154, 92, 198],
        BedrockColor::MaterialResin => [237, 105, 52],
    };
    [rgb[0], rgb[1], rgb[2], base[3]]
}

pub(super) fn is_empty(rect: UiRect) -> bool {
    rect.width() == 0.0 || rect.height() == 0.0
}
