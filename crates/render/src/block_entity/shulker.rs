//! Shulker boxes: base plus a lid that rises and spins open along the facing axis.
//!
//! Geometry is the pack's `geometry.shulker` (base and lid; the head is hidden inside the
//! box). Lid rise and spin per unit of openness need native measurement.

use assets::block_entity_geometry as geometry;
use bevy::math::{Mat4, Vec3};

use super::{
    atlas::BlockEntityAtlas,
    mesh::{BoxSpec, Layer, MeshBuilder, WHITE, model_matrix},
};

/// Lid travel along the facing axis at full openness, in pixels.
const LID_RISE_PIXELS: f32 = 8.0;
const LID_SPIN_DEGREES: f32 = 270.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShulkerModel {
    /// Texture suffix, for example `"red"` or `"undyed"`.
    pub color: &'static str,
    /// Direction the lid faces: 0 down, 1 up, 2 north, 3 south, 4 west, 5 east.
    pub facing: u8,
    /// Lid openness in `0.0..=1.0`.
    pub open: f32,
}

/// Rotation carrying the model's +Y (lid) axis onto the facing direction.
fn facing_rotation(facing: u8) -> Mat4 {
    match facing {
        0 => Mat4::from_rotation_x(std::f32::consts::PI),
        2 => Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2),
        3 => Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2),
        4 => Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2),
        5 => Mat4::from_rotation_z(-std::f32::consts::FRAC_PI_2),
        _ => Mat4::IDENTITY,
    }
}

pub(super) fn emit(
    builder: &mut MeshBuilder,
    atlas: &BlockEntityAtlas,
    block: [i32; 3],
    model: &ShulkerModel,
) {
    let Some(texture) = atlas.texture(
        &geometry::shulker_texture(model.color),
        geometry::SHULKER_TEXTURE_SIZE,
    ) else {
        return;
    };
    // Origin at the block center so facing turns about it; the model's y=0 is the block floor.
    let base = model_matrix(block, [0.5, 0.5, 0.5], 0.0)
        * facing_rotation(model.facing)
        * Mat4::from_translation(Vec3::new(0.0, -8.0, 0.0));
    builder.cuboid(
        Layer::Solid,
        &texture,
        base,
        BoxSpec::from(geometry::SHULKER_BASE),
        WHITE,
    );
    let open = model.open.clamp(0.0, 1.0);
    let lid = base
        * Mat4::from_translation(Vec3::new(0.0, LID_RISE_PIXELS * open, 0.0))
        * Mat4::from_rotation_y((LID_SPIN_DEGREES * open).to_radians());
    builder.cuboid(
        Layer::Solid,
        &texture,
        lid,
        BoxSpec::from(geometry::SHULKER_LID),
        WHITE,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use assets::block_entity_geometry::shulker_color_from_block_name;

    #[test]
    fn block_names_map_to_texture_suffixes() {
        assert_eq!(
            shulker_color_from_block_name("minecraft:silver_shulker_box"),
            Some("silver")
        );
        assert_eq!(
            shulker_color_from_block_name("minecraft:undyed_shulker_box"),
            Some("undyed")
        );
        assert_eq!(shulker_color_from_block_name("minecraft:chest"), None);
    }

    #[test]
    fn facing_rotation_sends_the_lid_axis_to_the_named_side() {
        for (facing, expected) in [
            (0, Vec3::NEG_Y),
            (1, Vec3::Y),
            (2, Vec3::NEG_Z),
            (3, Vec3::Z),
            (4, Vec3::NEG_X),
            (5, Vec3::X),
        ] {
            let axis = facing_rotation(facing).transform_vector3(Vec3::Y);
            assert!(axis.abs_diff_eq(expected, 1.0e-5), "{facing} {axis:?}");
        }
    }
}
