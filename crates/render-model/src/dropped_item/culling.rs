use super::{DroppedItemBlock, DroppedItemModel, DroppedItemSprite, block};
use glam::Vec3;
use std::collections::BTreeMap;

impl DroppedItemModel {
    /// Whether this model has a closed, opaque surface for alpha-tested drawing.
    /// Open templates, two-sided planes, partial alpha and padded UVs retain two-sided rendering.
    #[must_use]
    pub fn can_cull_back_faces(&self) -> bool {
        match self {
            Self::Vacant => false,
            // Both sprite slabs sample the same texel at a point, so binary cutouts hide nothing
            // their front faces would not.
            Self::NativeSprite(sprite) | Self::Sprite(sprite) => {
                sprite_alpha(sprite, |alpha| matches!(alpha, 0 | 255))
            }
            Self::Cube(cube) => {
                cube.tints.iter().all(|color| color >> 24 == 255)
                    && cube.faces.iter().all(|pixels| opaque(pixels))
            }
            Self::Block(model) => {
                model
                    .materials
                    .iter()
                    .all(|(sprite, tint)| tint >> 24 == 255 && opaque(&sprite.rgba8))
                    && closed_block(model)
            }
        }
    }
}

/// Validates a sprite's dimensions and requires each alpha to satisfy its model's culling policy.
fn sprite_alpha(sprite: &DroppedItemSprite, allowed: impl Fn(u8) -> bool) -> bool {
    sprite.width != 0
        && sprite.height != 0
        && Some(sprite.rgba8.len())
            == (sprite.width as usize)
                .checked_mul(sprite.height as usize)
                .and_then(|n| n.checked_mul(4))
        && sprite
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| allowed(pixel[3]))
}

/// Requires opaque pixels so cutout or partial coverage cannot reveal removed faces.
fn opaque(pixels: &[u8]) -> bool {
    !pixels.is_empty()
        && pixels.len().is_multiple_of(4)
        && pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255)
}

/// Keeps each triangle's UV interior within the opaque tile, away from atlas padding.
/// Two corners below the upper edge on each axis prevent any emitted triangle from
/// sampling a constant coordinate at that edge, regardless of its winding.
fn block_uvs_stay_in_tile(quad: &assets::ModelQuad) -> bool {
    quad.uvs
        .iter()
        .flatten()
        .all(|&uv| uv <= block::UV_TILE_UNITS)
        && (0..2).all(|axis| {
            quad.uvs
                .iter()
                .filter(|uv| uv[axis] < block::UV_TILE_UNITS)
                .count()
                >= 2
        })
}

/// Proves closure from paired directed edges and outward planes enclosing all corners.
/// Templates with unmatched edges, degenerate faces or non-convex surfaces conservatively fall back.
fn closed_block(model: &DroppedItemBlock) -> bool {
    if model.quads.is_empty() {
        return false;
    }
    let mut edges = BTreeMap::new();
    for quad in model.quads.iter() {
        if quad.flags & assets::MODEL_QUAD_FLAG_TWO_SIDED != 0 || !block_uvs_stay_in_tile(quad) {
            return false;
        }
        let Some(normal) = block::quad_normal(quad) else {
            return false;
        };
        let normal = Vec3::from_array(normal);
        let order = block::quad_order(quad, normal.to_array());
        let positions = order.map(|i| quad.positions[i]);
        let [a, b, c, d] = positions.map(|p| Vec3::from_array(p.map(f32::from)));
        if (b - a).cross(c - a).dot(normal) <= 0.0
            || (c - a).cross(d - a).dot(normal) <= 0.0
            || [b, c, d].iter().any(|p| (*p - a).dot(normal) != 0.0)
            || model
                .quads
                .iter()
                .flat_map(|q| q.positions)
                .any(|p| (Vec3::from_array(p.map(f32::from)) - a).dot(normal) > 0.0)
        {
            return false;
        }
        for i in 0..4 {
            let (a, b) = (positions[i], positions[(i + 1) % 4]);
            if a == b {
                return false;
            }
            *edges.entry((a, b)).or_insert(0usize) += 1;
        }
    }
    edges
        .iter()
        .all(|(&(a, b), &count)| count == 1 && edges.get(&(b, a)) == Some(&1))
}

#[cfg(test)]
mod tests;
