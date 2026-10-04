//! Armor layers reuse the body's bone poses by bone name.

use render::RenderBoneTransform;

/// Undyed leather colour (RGB); needs native measurement against the retail client.
pub(super) const DEFAULT_LEATHER_RGB: u32 = 0x00a0_6540;

/// The zero-scale pose vanilla uses to hide a bone.
pub(super) fn hidden_bone() -> RenderBoneTransform {
    RenderBoneTransform {
        rotation: [0.0, 0.0, 0.0, 1.0],
        translation_scale: [0.0; 4],
        axis_scale: render::UNIT_AXIS_SCALE,
    }
}

/// For each armor bone, the body bone of the same name (ASCII case-insensitive).
pub(super) fn bone_map(armor_names: &[Box<str>], body_names: &[Box<str>]) -> Vec<Option<usize>> {
    armor_names
        .iter()
        .map(|name| {
            body_names
                .iter()
                .position(|body| body.eq_ignore_ascii_case(name))
        })
        .collect()
}

/// Armor bone poses taken from `body` through `map`; unmatched bones hide.
pub(super) fn remap_pose(
    map: &[Option<usize>],
    body: &[RenderBoneTransform],
) -> Vec<RenderBoneTransform> {
    map.iter()
        .map(|index| {
            index
                .and_then(|index| body.get(index).copied())
                .unwrap_or_else(hidden_bone)
        })
        .collect()
}

/// Packs a 24-bit RGB dye into the instance tint word (`0xAABBGGRR`, alpha marks it enabled).
pub(super) const fn pack_tint(rgb: u32) -> u32 {
    let (red, green, blue) = ((rgb >> 16) & 0xff, (rgb >> 8) & 0xff, rgb & 0xff);
    0xff00_0000 | (blue << 16) | (green << 8) | red
}
