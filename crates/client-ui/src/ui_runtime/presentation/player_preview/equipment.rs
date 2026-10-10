//! Worn armor and the held item on the preview model: the humanoid armor
//! boxes (`geometry.humanoid.armor.*`: helmet and chestplate inflated one
//! pixel, leggings half a pixel, left limbs mirrored) over their 64x32 armor
//! textures. Native held model sources are separate from the CPU icon fallback.

use std::sync::Arc;

use render_model::ActorVertex;

/// One texture the preview samples, optionally tinted (dyed leather).
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewTexture {
    pub rgba: Arc<[u8]>,
    pub width: u16,
    pub height: u16,
    pub tint: Option<[u8; 3]>,
}

impl PreviewTexture {
    pub(super) fn sample(&self, uv: [f32; 2]) -> Option<[u8; 4]> {
        let (width, height) = (usize::from(self.width), usize::from(self.height));
        if width == 0
            || height == 0
            || self.rgba.len() != width * height * 4
            || !uv.iter().all(|value| value.is_finite())
        {
            return None;
        }
        let x = ((uv[0] * width as f32).floor() as isize).clamp(0, width as isize - 1) as usize;
        let y = ((uv[1] * height as f32).floor() as isize).clamp(0, height as isize - 1) as usize;
        let offset = (y * width + x) * 4;
        let mut texel: [u8; 4] = self.rgba[offset..offset + 4].try_into().ok()?;
        if let Some(tint) = self.tint {
            texel = assets::color_mask_texel(texel, tint);
        }
        Some(texel)
    }
}

/// The armor pieces worn (helmet, chestplate, leggings, boots) and the held item.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PreviewEquipment {
    pub armor: [Option<PreviewTexture>; 4],
    /// Small compatibility raster only; GPU previews use `hands` and real models.
    pub held: Option<PreviewTexture>,
    pub hands: [Option<PreviewHandItem>; 2],
}

/// Identity and authoritative item state used to select each hand's native model.
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewHandItem {
    pub identifier: Arc<str>,
    pub metadata: u32,
    pub charged_projectile: Option<Arc<str>>,
}

/// Real item-space geometry and the source atlas region, never a projected GUI icon.
#[derive(Clone, Debug)]
pub struct PreviewHeldModel {
    pub source: ui::IconRef,
    pub vertices: Arc<[render_model::ActorRigVertex]>,
    pub placements: [PreviewHeldPlacement; 2],
    /// Native player `rightItem`/`leftItem` bind origins in mirrored rig blocks.
    pub hand_pivots: [[f32; 3]; 2],
}

#[derive(Clone, Copy, Debug)]
pub enum PreviewHeldPlacement {
    Sprite {
        hand_equipped: bool,
    },
    Block,
    /// Authored bound-root channels, already in the native mirrored rig frame.
    Authored {
        bone: render_model::RenderBoneTransform,
        pivot: [f32; 3],
    },
}

/// One armor box: biped part, min corner and size in pixels, inflation, UV
/// origin, and whether its UVs mirror (left limbs).
struct ArmorBox {
    part: u32,
    min: [f32; 3],
    size: [f32; 3],
    inflate: f32,
    uv: [f32; 2],
    mirror: bool,
}

const fn armor_box(
    part: u32,
    min: [f32; 3],
    size: [f32; 3],
    inflate: f32,
    uv: [f32; 2],
    mirror: bool,
) -> ArmorBox {
    ArmorBox {
        part,
        min,
        size,
        inflate,
        uv,
        mirror,
    }
}

const HEAD: [f32; 3] = [-4.0, 24.0, -4.0];
const BODY: [f32; 3] = [-4.0, 12.0, -2.0];
const RIGHT_ARM: [f32; 3] = [-8.0, 12.0, -2.0];
const LEFT_ARM: [f32; 3] = [4.0, 12.0, -2.0];
const RIGHT_LEG: [f32; 3] = [-4.0, 0.0, -2.0];
const LEFT_LEG: [f32; 3] = [0.0, 0.0, -2.0];
const TORSO: [f32; 3] = [8.0, 12.0, 4.0];
const LIMB: [f32; 3] = [4.0, 12.0, 4.0];

// Pinned vanilla models/entity/player_armor.json: UV coordinates use the model's
// logical size, regardless of the replacement image's physical texel density.
const ARMOR_UV_SIZE: [f32; 2] = [64.0, 32.0];

/// The boxes of each armor slot, helmet to boots.
fn slot_boxes(slot: usize) -> &'static [ArmorBox] {
    const HELMET: [ArmorBox; 1] = [armor_box(0, HEAD, [8.0; 3], 1.0, [0.0, 0.0], false)];
    const CHESTPLATE: [ArmorBox; 3] = [
        armor_box(1, BODY, TORSO, 1.0, [16.0, 16.0], false),
        armor_box(2, RIGHT_ARM, LIMB, 1.0, [40.0, 16.0], false),
        armor_box(3, LEFT_ARM, LIMB, 1.0, [40.0, 16.0], true),
    ];
    const LEGGINGS: [ArmorBox; 3] = [
        armor_box(1, BODY, TORSO, 0.5, [16.0, 16.0], false),
        armor_box(4, RIGHT_LEG, LIMB, 0.5, [0.0, 16.0], false),
        armor_box(5, LEFT_LEG, LIMB, 0.5, [0.0, 16.0], true),
    ];
    const BOOTS: [ArmorBox; 2] = [
        armor_box(4, RIGHT_LEG, LIMB, 1.0, [0.0, 16.0], false),
        armor_box(5, LEFT_LEG, LIMB, 1.0, [0.0, 16.0], true),
    ];
    match slot {
        0 => &HELMET,
        1 => &CHESTPLATE,
        2 => &LEGGINGS,
        _ => &BOOTS,
    }
}

/// The triangles of armor slot `slot`, with normalized model UVs, in blocks.
pub(super) fn armor_vertices(slot: usize) -> Vec<ActorVertex> {
    let mut vertices = Vec::new();
    for armor in slot_boxes(slot) {
        let px = 1.0 / 16.0;
        let min = armor.min.map(|axis| (axis - armor.inflate) * px);
        let max = [0, 1, 2].map(|axis| (armor.min[axis] + armor.size[axis] + armor.inflate) * px);
        append_box(&mut vertices, armor, min, max, ARMOR_UV_SIZE);
    }
    vertices
}

/// A cuboid laid out like the skin (`render_model::standard_biped_vertices`), its UVs
/// over `texture_size`, mirrored left to right for `mirror`.
fn append_box(
    vertices: &mut Vec<ActorVertex>,
    armor: &ArmorBox,
    min: [f32; 3],
    max: [f32; 3],
    texture_size: [f32; 2],
) {
    let [x0, y0, z0] = min;
    let [x1, y1, z1] = max;
    let [u, v] = armor.uv;
    let [dx, dy, dz] = armor.size;
    let (east, west) = if armor.mirror { (x0, x1) } else { (x1, x0) };
    let faces = [
        (
            [
                [east, y0, z0],
                [east, y0, z1],
                [east, y1, z1],
                [east, y1, z0],
            ],
            [u, v + dz, dz, dy],
        ),
        (
            [[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]],
            [u + dz, v + dz, dx, dy],
        ),
        (
            [
                [west, y0, z1],
                [west, y0, z0],
                [west, y1, z0],
                [west, y1, z1],
            ],
            [u + dz + dx, v + dz, dz, dy],
        ),
        (
            [[x1, y0, z0], [x0, y0, z0], [x0, y1, z0], [x1, y1, z0]],
            [u + dz + dx + dz, v + dz, dx, dy],
        ),
        (
            [[x0, y1, z1], [x1, y1, z1], [x1, y1, z0], [x0, y1, z0]],
            [u + dz, v, dx, dz],
        ),
        (
            [[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]],
            [u + dz + dx, v, dx, dz],
        ),
    ];
    for (positions, [face_u, face_v, face_width, face_height]) in faces {
        let (mut u0, mut u1) = (
            face_u / texture_size[0],
            (face_u + face_width) / texture_size[0],
        );
        if armor.mirror {
            std::mem::swap(&mut u0, &mut u1);
        }
        let v0 = face_v / texture_size[1];
        let v1 = (face_v + face_height) / texture_size[1];
        let uvs = [[u0, v1], [u1, v1], [u1, v0], [u0, v0]];
        for index in [0, 1, 2, 0, 2, 3] {
            vertices.push(ActorVertex {
                position: positions[index],
                uv: uvs[index],
                part: armor.part,
            });
        }
    }
}

/// The held item's icon as a flat square in the right hand, pointing forward
/// (provisional: the third-person item transform is not modelled).
pub(super) fn held_vertices() -> Vec<ActorVertex> {
    let px = 1.0 / 16.0;
    let (x, y0, y1, z0, z1) = (-6.0 * px, 8.0 * px, 18.0 * px, -3.0 * px, 7.0 * px);
    let positions = [[x, y0, z0], [x, y0, z1], [x, y1, z1], [x, y1, z0]];
    let uvs = [[1.0, 1.0], [0.0, 1.0], [0.0, 0.0], [1.0, 0.0]];
    [0, 1, 2, 0, 2, 3]
        .into_iter()
        .map(|index| ActorVertex {
            position: positions[index],
            uv: uvs[index],
            part: 2,
        })
        .collect()
}

#[cfg(test)]
mod review_tests {
    use super::*;

    #[test]
    fn review_zero_sized_preview_textures_cannot_be_sampled() {
        for (width, height) in [(0, 0), (0, 16), (16, 0)] {
            let texture = PreviewTexture {
                rgba: Arc::from([]),
                width,
                height,
                tint: None,
            };
            assert_eq!(texture.sample([0.5, 0.5]), None);
        }
    }

    #[test]
    fn leather_color_mask_keeps_low_alpha_trim_and_weights_the_dye() {
        let texture = PreviewTexture {
            rgba: Arc::from([200, 100, 50, 1, 200, 100, 50, 255, 0, 0, 0, 0]),
            width: 3,
            height: 1,
            tint: Some([128, 255, 0]),
        };
        assert_eq!(texture.sample([0.0, 0.0]), Some([199, 100, 49, 255]));
        assert_eq!(texture.sample([0.5, 0.0]), Some([100, 100, 0, 255]));
        assert_eq!(texture.sample([1.0, 0.0]), Some([0, 0, 0, 0]));
    }
}
