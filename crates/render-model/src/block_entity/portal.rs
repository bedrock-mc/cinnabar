//! Encoded portal planes and palette coordinates for the parallax material.

use super::{
    atlas::BlockEntityAtlas,
    mesh::{Layer, MeshBuilder},
    scene::SceneClock,
};

const PORTAL_SURFACE_HEIGHT: f32 = 0.75;
const PALETTE_SIDE: usize = 4;
const STAR_LAYERS: usize = PALETTE_SIDE * PALETTE_SIDE;
const STAR_TEXTURE: &str = "textures/entity/end_portal";
const COLOR_TEXTURE: &str = "textures/environment/end_portal_colors";

pub(super) fn star_rect(atlas: &BlockEntityAtlas) -> [f32; 4] {
    let Some(texture) = atlas.texture(STAR_TEXTURE, [256.0; 2]) else {
        return [0.0; 4];
    };
    let [width, height] = atlas.size().map(|value| value as f32);
    let rect = texture.rect;
    [
        rect.x / width,
        rect.y / height,
        rect.width / width,
        rect.height / height,
    ]
}

/// Native alpha stores a truncated byte, rather than the unquantized layer depth.
fn layer_phase(layer: usize) -> f32 {
    if layer == 0 {
        1.0
    } else {
        ((STAR_LAYERS - layer) as f32 / STAR_LAYERS as f32 * 255.0) as u8 as f32 / 255.0
    }
}

fn emit_face_layer(
    builder: &mut MeshBuilder,
    atlas: &BlockEntityAtlas,
    corners: [[f32; 3]; 4],
    normal: [u8; 3],
    layer: usize,
) {
    let Some(colors) = atlas.texture(COLOR_TEXTURE, [PALETTE_SIDE as f32; 2]) else {
        return;
    };
    if atlas.texture(STAR_TEXTURE, [256.0; 2]).is_none() {
        return;
    }
    let lookup = if layer == 0 {
        [0.0; 2]
    } else {
        let cell = layer - 1;
        [
            (cell % PALETTE_SIDE) as f32 + 0.5,
            (cell / PALETTE_SIDE) as f32 + 0.5,
        ]
    };
    let [u, v, _, _] = colors.rect_uv([lookup[0], lookup[1], 0.0, 0.0]);
    let [red, green, blue] = normal.map(|value| f32::from(value) / 255.0);
    builder.quad_uv(
        Layer::Portal,
        corners,
        [[u, v]; 4],
        [red, green, blue, layer_phase(layer)],
    );
}

pub(super) fn emit(
    builder: &mut MeshBuilder,
    atlas: &BlockEntityAtlas,
    block: [i32; 3],
    gateway: bool,
    _clock: SceneClock,
) {
    let [x, y, z] = block.map(|value| value as f32);
    if gateway {
        let faces = [
            (
                [
                    [x, y, z],
                    [x, y, z + 1.0],
                    [x, y + 1.0, z + 1.0],
                    [x, y + 1.0, z],
                ],
                [0, 127, 127],
            ),
            (
                [
                    [x + 1.0, y, z + 1.0],
                    [x + 1.0, y, z],
                    [x + 1.0, y + 1.0, z],
                    [x + 1.0, y + 1.0, z + 1.0],
                ],
                [255, 127, 127],
            ),
            (
                [
                    [x, y, z],
                    [x + 1.0, y, z],
                    [x + 1.0, y, z + 1.0],
                    [x, y, z + 1.0],
                ],
                [127, 0, 127],
            ),
            (
                [
                    [x, y + 1.0, z + 1.0],
                    [x + 1.0, y + 1.0, z + 1.0],
                    [x + 1.0, y + 1.0, z],
                    [x, y + 1.0, z],
                ],
                [127, 255, 127],
            ),
            (
                [
                    [x + 1.0, y, z],
                    [x, y, z],
                    [x, y + 1.0, z],
                    [x + 1.0, y + 1.0, z],
                ],
                [127, 127, 0],
            ),
            (
                [
                    [x, y, z + 1.0],
                    [x + 1.0, y, z + 1.0],
                    [x + 1.0, y + 1.0, z + 1.0],
                    [x, y + 1.0, z + 1.0],
                ],
                [127, 127, 255],
            ),
        ];
        for layer in 0..=STAR_LAYERS {
            for face in [3, 2, 0, 1, 4, 5] {
                let (corners, normal) = faces[face];
                emit_face_layer(builder, atlas, corners, normal, layer);
            }
        }
    } else {
        let y = y + PORTAL_SURFACE_HEIGHT;
        for layer in 0..=STAR_LAYERS {
            emit_face_layer(
                builder,
                atlas,
                [
                    [x, y, z + 1.0],
                    [x + 1.0, y, z + 1.0],
                    [x + 1.0, y, z],
                    [x, y, z],
                ],
                [127, 255, 127],
                layer,
            );
        }
    }
}

#[cfg(test)]
#[path = "portal/tests.rs"]
mod tests;
