//! Tile-local world item geometry prepared alongside its held block-entity rig.

use super::*;
use crate::{DroppedItemBlock, DroppedItemSprite};

/// Copies only the textures addressed by the emitted solid faces, retaining their local UVs.
pub(super) fn model(
    atlas: &BlockEntityAtlas,
    vertices: &[BlockEntityVertex],
    center: glam::Vec3,
) -> Option<DroppedItemBlock> {
    let (faces, remainder) = vertices.as_chunks::<6>();
    if faces.is_empty() || !remainder.is_empty() {
        return None;
    }
    let size = atlas.size().map(|side| side as f32);
    let mut keys = Vec::new();
    let mut materials = Vec::new();
    let mut quads = Vec::with_capacity(faces.len());
    for face in faces {
        let texture = face_texture(atlas, face)?;
        let rect = [texture.x, texture.y, texture.width, texture.height].map(|v| v as u32);
        if rect[2] == 0 || rect[3] == 0 {
            return None;
        }
        let color = crate::pack_overlay_rgba8(face[0].color);
        let key = (rect, color);
        let material = match keys.iter().position(|candidate| *candidate == key) {
            Some(index) => index,
            None => {
                let index = keys.len();
                keys.push(key);
                materials.push((crop(atlas, rect)?, color));
                index
            }
        };
        // MeshBuilder emits (b, c, a), (a, c, d); reverse the corners for its outward normal.
        let corners = [2, 5, 1, 0].map(|index| face[index]);
        let positions = corners.map(|vertex| {
            std::array::from_fn::<_, 3, _>(|axis| {
                (vertex.position[axis] - center[axis] + 0.5) * 256.0
            })
        });
        let uvs = corners.map(|vertex| {
            std::array::from_fn::<_, 2, _>(|axis| {
                (vertex.uv[axis] * size[axis] - rect[axis] as f32) / rect[axis + 2] as f32
            })
        });
        if positions.iter().flatten().any(|value| {
            !value.is_finite() || *value < f32::from(i16::MIN) || *value > f32::from(i16::MAX)
        }) || uvs
            .iter()
            .flatten()
            .any(|value| !(-0.0001..=1.0001).contains(value))
        {
            return None;
        }
        quads.push(assets::ModelQuad {
            positions: positions.map(|point| point.map(|value| value.round() as i16)),
            uvs: uvs.map(|uv| {
                uv.map(|value| {
                    (value.clamp(0.0, 1.0) * f32::from(assets::MODEL_UV_SCALE)).round() as u16
                })
            }),
            material: material as u32,
            flags: 0,
        });
    }
    Some(DroppedItemBlock {
        materials: materials.into(),
        quads: quads.into(),
        rotation: 0,
    })
}

/// Crops one static texture without retaining unrelated atlas pixels or animated strips.
fn crop(atlas: &BlockEntityAtlas, [x, y, width, height]: [u32; 4]) -> Option<DroppedItemSprite> {
    let [atlas_width, _] = atlas.size();
    if x.checked_add(width)? > atlas_width || y.checked_add(height)? > atlas.static_height() {
        return None;
    }
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for row in y..y + height {
        let start = (row as usize * atlas_width as usize + x as usize) * 4;
        pixels.extend_from_slice(
            atlas
                .static_rgba8()
                .get(start..start + width as usize * 4)?,
        );
    }
    Some(DroppedItemSprite {
        width,
        height,
        rgba8: pixels.into(),
    })
}
