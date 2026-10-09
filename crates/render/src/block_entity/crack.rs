//! Break-progress overlay: the destroy-stage texture over the block's own model faces.

use std::sync::Arc;

use assets::{MODEL_TEMPLATE_FLAG_COMPOUND_NEXT, RuntimeAssets};
use bevy::math::Vec3;

use super::{
    atlas::BlockEntityAtlas,
    mesh::{Layer, MeshBuilder, WHITE},
    scene::CrackInstance,
};

use render_api::BLOCK_OVERLAY_FACE_OFFSET as FACE_OFFSET;
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
    /// Both camera sides cover the same surface without duplicating the blend.
    pub two_sided: bool,
}

impl CrackQuad {
    /// Two-sided overlays retain their plane and let the vertex shader face the bias toward the view.
    pub(super) fn overlay_geometry(self) -> ([[f32; 3]; 4], [f32; 3]) {
        let outward = self.outward_offset();
        if self.two_sided {
            (self.corners, outward.normalize_or_zero().to_array())
        } else {
            (
                self.corners
                    .map(|corner| (Vec3::from_array(corner) + outward).to_array()),
                [0.0; 3],
            )
        }
    }

    /// Template windings describe the actual outward surface, including inset
    /// stair treads and thin snow tops. The full cell's center cannot identify
    /// the outside of either surface.
    pub(super) fn outward_offset(self) -> Vec3 {
        let corners = self.corners.map(Vec3::from_array);
        (corners[1] - corners[0])
            .cross(corners[2] - corners[0])
            .normalize_or_zero()
            * FACE_OFFSET
    }
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
/// the template is out of range or has no quads. Resolved variants and block positions
/// produce the same rotations and column offsets as the terrain pass.
#[must_use]
pub fn crack_shape_from_template(
    assets: &RuntimeAssets,
    template: u32,
    variant: u32,
    block: [i32; 3],
) -> Option<CrackShape> {
    let templates = assets.model_templates();
    let quads = assets.model_quads();
    let mut shape = Vec::new();
    let mut index = usize::try_from(template).ok()?;
    loop {
        let part = templates.get(index)?;
        let start = usize::try_from(part.quad_start).ok()?;
        let end = start.checked_add(usize::try_from(part.quad_count).ok()?)?;
        let transform = meshing::bamboo::transform_for_template(part.flags, variant, block);
        let random_offset = assets
            .model_random_offset(index as u32)
            .map(|component| component.offset(block));
        for (quad_index, quad) in quads.get(start..end)?.iter().enumerate() {
            shape.push(CrackQuad {
                two_sided: quad.flags & assets::MODEL_QUAD_FLAG_TWO_SIDED != 0,
                corners: quad.positions.map(|corner| {
                    if part.flags & assets::MODEL_TEMPLATE_FLAG_BAMBOO != 0 {
                        let mut offset = meshing::bamboo::quad_offset(transform, quad_index as u32);
                        if let Some(custom) = random_offset {
                            let default = block_transform::bamboo::offset_from_transform(transform);
                            offset = std::array::from_fn(|axis| {
                                offset[axis] - default[axis] + custom[axis]
                            });
                        }
                        std::array::from_fn(|axis| {
                            f32::from(corner[axis]) / POSITION_UNITS + offset[axis]
                        })
                    } else {
                        let corner = rotate_corner(corner, transform);
                        std::array::from_fn(|axis| {
                            corner[axis] + random_offset.unwrap_or([0.0; 3])[axis]
                        })
                    }
                }),
                uvs: quad.uvs.map(|uv| {
                    let mut uv = uv.map(tile_fraction);
                    if part.flags & assets::MODEL_TEMPLATE_FLAG_BAMBOO != 0 {
                        uv[0] += meshing::bamboo::stem_uv_offset(transform, quad_index as u32);
                    }
                    uv
                }),
            });
        }
        if part.flags & MODEL_TEMPLATE_FLAG_COMPOUND_NEXT == 0 {
            break;
        }
        index += 1;
    }
    (!shape.is_empty()).then(|| CrackShape::Quads(shape.into()))
}

fn rotate_corner(corner: [i16; 3], variant: u32) -> [f32; 3] {
    let [x, y, z] = corner.map(|axis| f32::from(axis) / POSITION_UNITS);
    // Same cell-centered transform as model.wgsl::rotate_cross. The template
    // already encodes vertical halves; unrelated high semantic bits are ignored.
    match variant & 3 {
        1 => [1.0 - z, y, x],
        2 => [1.0 - x, y, 1.0 - z],
        3 => [z, y, 1.0 - x],
        _ => [x, y, z],
    }
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
        CrackShape::Quads(quads) => emit_model(builder, block, rect, quads),
    }
}

fn emit_model(
    builder: &mut MeshBuilder,
    block: Vec3,
    rect: super::atlas::AtlasRect,
    quads: &[CrackQuad],
) {
    for quad in quads {
        let (corners, normal) = quad.overlay_geometry();
        let first = builder.crack.len();
        builder.quad_uv(
            Layer::Crack,
            corners.map(|corner| (block + Vec3::from_array(corner)).to_array()),
            quad.uvs
                .map(|[u, v]| [rect.x + u * rect.width, rect.y + v * rect.height]),
            WHITE,
        );
        for vertex in &mut builder.crack[first..] {
            vertex.normal = normal;
            vertex.actor_light = 0;
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
mod tests;
