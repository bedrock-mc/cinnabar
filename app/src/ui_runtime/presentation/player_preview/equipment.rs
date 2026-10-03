//! Worn armor and the held item on the preview model: the humanoid armor
//! boxes (`geometry.humanoid.armor.*`: helmet and chestplate inflated one
//! pixel, leggings half a pixel, left limbs mirrored) over their 64x32 armor
//! textures. Native held model sources are separate from the CPU icon fallback.

use std::sync::Arc;

use render::ActorVertex;

/// One texture the preview samples, optionally tinted (dyed leather).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PreviewTexture {
    pub(crate) rgba: Arc<[u8]>,
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) tint: Option<[u8; 3]>,
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
            for channel in 0..3 {
                texel[channel] = (u16::from(texel[channel]) * u16::from(tint[channel]) / 255) as u8;
            }
        }
        Some(texel)
    }
}

/// The armor pieces worn (helmet, chestplate, leggings, boots) and the held item.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PreviewEquipment {
    pub(crate) armor: [Option<PreviewTexture>; 4],
    /// Small compatibility raster only; GPU previews use `hands` and real models.
    pub(crate) held: Option<PreviewTexture>,
    pub(crate) hands: [Option<PreviewHandItem>; 2],
}

/// Identity and authoritative item state used to select each hand's native model.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PreviewHandItem {
    pub(crate) identifier: Arc<str>,
    pub(crate) metadata: u32,
    pub(crate) charged_projectile: Option<Arc<str>>,
}

/// Real item-space geometry and the source atlas region, never a projected GUI icon.
#[derive(Clone, Debug)]
pub(crate) struct PreviewHeldModel {
    pub(crate) source: super::IconRef,
    pub(crate) vertices: Arc<[render::ActorRigVertex]>,
    pub(crate) placements: [PreviewHeldPlacement; 2],
    /// Native player `rightItem`/`leftItem` bind origins in mirrored rig blocks.
    pub(crate) hand_pivots: [[f32; 3]; 2],
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum PreviewHeldPlacement {
    Sprite {
        hand_equipped: bool,
    },
    Block,
    /// Authored bound-root channels, already in the native mirrored rig frame.
    Authored {
        bone: render::RenderBoneTransform,
        pivot: [f32; 3],
    },
}

impl PreviewHeldPlacement {
    /// `setupAttachableNoChecks` preserves expression-bound ModelPart defaults:
    /// its root origin is authored pivot Y minus the shared model-part height.
    /// Keep the mesh's original bind pivot: it is still subtracted during skinning.
    pub(crate) fn authored(
        mut bone: render::RenderBoneTransform,
        pivot: [f32; 3],
        expression_bound: bool,
    ) -> Self {
        if expression_bound {
            bone.translation_scale[1] -= client_world::MODEL_PART_ORIGIN_Y / 16.0;
        }
        Self::Authored { bone, pivot }
    }
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

/// The triangles of armor slot `slot` over a `texture_size` texture, in blocks.
pub(super) fn armor_vertices(slot: usize, texture_size: [f32; 2]) -> Vec<ActorVertex> {
    let mut vertices = Vec::new();
    for armor in slot_boxes(slot) {
        let px = 1.0 / 16.0;
        let min = armor.min.map(|axis| (axis - armor.inflate) * px);
        let max = [0, 1, 2].map(|axis| (armor.min[axis] + armor.size[axis] + armor.inflate) * px);
        append_box(&mut vertices, armor, min, max, texture_size);
    }
    vertices
}

/// A cuboid laid out like the skin (`render::standard_biped_vertices`), its UVs
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
}
