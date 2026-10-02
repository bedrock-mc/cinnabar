//! Break-progress overlay: the destroy-stage texture over the block's own model faces.

use std::sync::Arc;

use assets::{MODEL_TEMPLATE_FLAG_COMPOUND_NEXT, RuntimeAssets};
use bevy::math::Vec3;

use super::{
    atlas::BlockEntityAtlas,
    mesh::{Layer, MeshBuilder, WHITE},
    scene::CrackInstance,
};

/// Outward push that keeps the overlay in front of the block's own faces.
pub(super) const FACE_OFFSET: f32 = 0.002;
/// Model-quad UVs are in 1/4096 of a texture tile.
const UV_TILE: f32 = 4096.0;
/// Model-quad positions are in 1/256 block.
const POSITION_UNITS: f32 = 256.0;

/// One face of the cracked block's shape, in block-local coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CrackQuad {
    pub corners: [[f32; 3]; 4],
    /// Per-corner fractions of the destroy-stage tile, taken from the block face's own UVs.
    pub uvs: [[f32; 2]; 4],
}

/// The surface a crack covers.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum CrackShape {
    /// The full unit cube.
    #[default]
    Cube,
    /// The block model's own faces.
    Quads(Arc<[CrackQuad]>),
}

/// Stages are `0..=9`; larger values clamp to the last.
#[must_use]
pub fn crack_texture_name(stage: u8) -> String {
    format!("textures/environment/destroy_stage_{}", stage.min(9))
}

fn tile_fraction(value: u16) -> f32 {
    let tile = f32::from(value) / UV_TILE;
    if tile <= 1.0 {
        return tile;
    }
    // Wrapped UVs on greedy-compatible templates land back inside one tile.
    match tile.fract() {
        0.0 => 1.0,
        fraction => fraction,
    }
}

/// The shape of a model-template block, following compound template chains; `None` when
/// the template is out of range or has no quads.
#[must_use]
pub fn crack_shape_from_template(assets: &RuntimeAssets, template: u32) -> Option<CrackShape> {
    let templates = assets.model_templates();
    let quads = assets.model_quads();
    let mut shape = Vec::new();
    let mut index = usize::try_from(template).ok()?;
    loop {
        let part = templates.get(index)?;
        let start = usize::try_from(part.quad_start).ok()?;
        let end = start.checked_add(usize::try_from(part.quad_count).ok()?)?;
        for quad in quads.get(start..end)? {
            shape.push(CrackQuad {
                corners: quad
                    .positions
                    .map(|corner| corner.map(|axis| f32::from(axis) / POSITION_UNITS)),
                uvs: quad.uvs.map(|uv| uv.map(tile_fraction)),
            });
        }
        if part.flags & MODEL_TEMPLATE_FLAG_COMPOUND_NEXT == 0 {
            break;
        }
        index += 1;
    }
    (!shape.is_empty()).then(|| CrackShape::Quads(shape.into()))
}

pub(super) fn emit_crack(
    builder: &mut MeshBuilder,
    atlas: &BlockEntityAtlas,
    crack: &CrackInstance,
) {
    let Some(texture) = atlas.texture(&crack_texture_name(crack.stage), [16.0, 16.0]) else {
        return;
    };
    let block = Vec3::from_array(crack.block.map(|value| value as f32));
    let rect = texture.rect;
    match &crack.shape {
        CrackShape::Cube => emit_cube(builder, block, rect),
        CrackShape::Quads(quads) => {
            for quad in quads.iter() {
                let corners = quad.corners.map(Vec3::from_array);
                let center = corners.iter().copied().sum::<Vec3>() / 4.0;
                let normal = (corners[1] - corners[0])
                    .cross(corners[2] - corners[0])
                    .normalize_or_zero();
                // Push outward from the block center so both windings sit in front.
                let outward = if normal.dot(center - Vec3::splat(0.5)) < 0.0 {
                    -normal
                } else {
                    normal
                } * FACE_OFFSET;
                builder.quad_uv(
                    Layer::Crack,
                    corners.map(|corner| (block + corner + outward).to_array()),
                    quad.uvs
                        .map(|[u, v]| [rect.x + u * rect.width, rect.y + v * rect.height]),
                    WHITE,
                );
            }
        }
    }
}

fn emit_cube(builder: &mut MeshBuilder, block: Vec3, rect: super::atlas::AtlasRect) {
    let [bx, by, bz] = block.to_array();
    let (x0, x1) = (bx - FACE_OFFSET, bx + 1.0 + FACE_OFFSET);
    let (y0, y1) = (by - FACE_OFFSET, by + 1.0 + FACE_OFFSET);
    let (z0, z1) = (bz - FACE_OFFSET, bz + 1.0 + FACE_OFFSET);
    let faces: [[[f32; 3]; 4]; 6] = [
        [[x1, y1, z0], [x0, y1, z0], [x0, y0, z0], [x1, y0, z0]],
        [[x0, y1, z1], [x1, y1, z1], [x1, y0, z1], [x0, y0, z1]],
        [[x1, y1, z1], [x1, y1, z0], [x1, y0, z0], [x1, y0, z1]],
        [[x0, y1, z0], [x0, y1, z1], [x0, y0, z1], [x0, y0, z0]],
        [[x1, y1, z1], [x0, y1, z1], [x0, y1, z0], [x1, y1, z0]],
        [[x1, y0, z0], [x0, y0, z0], [x0, y0, z1], [x1, y0, z1]],
    ];
    for corners in faces {
        builder.textured_quad(Layer::Crack, corners, rect, WHITE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_names_clamp_to_the_last_texture() {
        assert_eq!(
            crack_texture_name(3),
            "textures/environment/destroy_stage_3"
        );
        assert_eq!(
            crack_texture_name(200),
            "textures/environment/destroy_stage_9"
        );
    }

    #[test]
    fn wrapped_uvs_fold_into_one_tile_and_full_tile_stays_full() {
        assert_eq!(tile_fraction(4096), 1.0);
        assert_eq!(tile_fraction(2048), 0.5);
        assert!((tile_fraction(6144) - 0.5).abs() < 1.0e-6);
        assert_eq!(tile_fraction(8192), 1.0);
    }

    #[test]
    fn out_of_range_templates_have_no_shape() {
        assert!(crack_shape_from_template(&RuntimeAssets::diagnostic(), 999).is_none());
    }
}
