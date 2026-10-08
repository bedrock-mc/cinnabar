//! The head block draws the complete bed geometry from the runtime entity catalog.

use bevy::math::{Mat4, Vec3};

use super::{
    atlas::BlockEntityAtlas,
    heads::HeadModel,
    mesh::{Layer, MeshBuilder, WHITE, model_matrix},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BedModel {
    /// Texture stem under `textures/entity/bed`, for example `red` or `silver`.
    pub color: &'static str,
    /// Whether this block is the pillow half.
    pub head: bool,
    /// Bedrock `direction` state: 0 head toward south, 1 west, 2 north, 3 east.
    pub direction: u8,
}

impl BedModel {
    /// Offset of the other half, in blocks, from this block.
    #[must_use]
    pub const fn other_half_offset(self) -> [i32; 3] {
        let offset = match self.direction % 4 {
            0 => [0, 0, 1],
            1 => [-1, 0, 0],
            2 => [0, 0, -1],
            _ => [1, 0, 0],
        };
        if self.head {
            [-offset[0], 0, -offset[2]]
        } else {
            offset
        }
    }
}

/// The texture stem for a bed block entity's `color` (dye id, white first).
#[must_use]
pub const fn bed_color(dye_id: i64) -> Option<&'static str> {
    Some(match dye_id {
        0 => "white",
        1 => "orange",
        2 => "magenta",
        3 => "light_blue",
        4 => "yellow",
        5 => "lime",
        6 => "pink",
        7 => "gray",
        8 => "silver",
        9 => "cyan",
        10 => "purple",
        11 => "blue",
        12 => "brown",
        13 => "green",
        14 => "red",
        15 => "black",
        _ => return None,
    })
}

/// Yaw that points the model's +Z (head end) along the `direction` state.
fn yaw_degrees(direction: u8) -> f32 {
    match direction % 4 {
        0 => 0.0,
        1 => 270.0,
        2 => 180.0,
        _ => 90.0,
    }
}

pub(super) fn emit(
    builder: &mut MeshBuilder,
    atlas: &BlockEntityAtlas,
    block: [i32; 3],
    model: &BedModel,
    geometry: Option<&HeadModel>,
) {
    if !model.head {
        return;
    }
    let Some(geometry) = geometry else {
        return;
    };
    let Some(texture) = atlas.texture(
        &format!("textures/entity/bed/{}", model.color),
        geometry.texture,
    ) else {
        return;
    };
    let base = model_matrix(block, [0.5, 0.0, 0.5], yaw_degrees(model.direction));
    let frame = base
        * Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2)
        * Mat4::from_translation(Vec3::new(8.0, -24.0, -9.0));
    for part in &geometry.boxes {
        builder.cuboid(
            Layer::Solid,
            &texture,
            frame * part.matrix,
            part.spec,
            WHITE,
        );
    }
}

#[cfg(test)]
mod tests {
    mod geometry;

    use super::*;

    #[test]
    fn colors_follow_dye_order_with_silver_for_light_gray() {
        assert_eq!(bed_color(0), Some("white"));
        assert_eq!(bed_color(8), Some("silver"));
        assert_eq!(bed_color(15), Some("black"));
        assert_eq!(bed_color(16), None);
    }

    #[test]
    fn yaw_points_the_head_end_along_the_direction_state() {
        for (direction, expected) in [
            (0, Vec3::Z),
            (1, Vec3::NEG_X),
            (2, Vec3::NEG_Z),
            (3, Vec3::X),
        ] {
            let head = Mat4::from_rotation_y(yaw_degrees(direction).to_radians())
                .transform_vector3(Vec3::Z);
            assert!(head.abs_diff_eq(expected, 1.0e-5), "{direction} {head:?}");
        }
    }

    #[test]
    fn the_slab_lies_flat_between_the_leg_tops_and_nine_pixels() {
        let slab = Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2);
        let bottom = slab.transform_point3(Vec3::new(0.0, 0.0, -3.0));
        let top = slab.transform_point3(Vec3::new(0.0, 0.0, -9.0));
        assert!((bottom.y - 3.0).abs() < 1.0e-5 && (top.y - 9.0).abs() < 1.0e-5);
    }

    #[test]
    fn both_halves_locate_each_other_for_every_direction() {
        for (direction, offset) in [
            (0, [0, 0, 1]),
            (1, [-1, 0, 0]),
            (2, [0, 0, -1]),
            (3, [1, 0, 0]),
        ] {
            let mut model = BedModel {
                color: "red",
                head: false,
                direction,
            };
            assert_eq!(model.other_half_offset(), offset);
            model.head = true;
            assert_eq!(model.other_half_offset(), offset.map(|value| -value));
        }
    }
}
