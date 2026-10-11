//! Item and glow item frames: a flat backing panel on the wall; the framed item is drawn by
//! the dropped-item renderer at the pose from [`item_frame_item_transform`].
//!
//! Map presentation and the complete frame border/material remain incomplete.

use glam::{Mat4, Vec3};

use super::{
    atlas::{AtlasRect, BlockEntityAtlas},
    items::matrix_rows,
    mesh::{Facing, Layer, MeshBuilder, WHITE},
};

const PANEL_HALF: f32 = 6.0;
/// Frame-local depth of the panel front, in pixels from the block center.
pub const ITEM_FRAME_PANEL_FRONT_PIXELS: f32 = 7.0;
const PANEL_BACK: f32 = 8.0;
const ITEM_SCALE: f32 = 0.5;
const MAP_LIFT: f32 = 0.05;
/// Degrees per legacy item-frame rotation turn.
pub const ITEM_FRAME_ROTATION_STEP_DEGREES: f32 = 45.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemFrameModel {
    pub glow: bool,
    /// Direction the frame faces, as a Bedrock face id: 0 down, 1 up, 2 north, 3 south, 4 west,
    /// 5 east; other values read as north.
    pub outward: u8,
    /// The atlas cell of a filled map's image, shown in place of an item sprite.
    pub map: Option<AtlasRect>,
}

/// Maps frame-local block-center space (front toward -Z, pixels scaled to blocks) into the world.
fn frame_matrix(block: [i32; 3], outward: u8) -> Mat4 {
    let center = Vec3::new(
        block[0] as f32 + 0.5,
        block[1] as f32 + 0.5,
        block[2] as f32 + 0.5,
    );
    let turn = match outward {
        0 => {
            Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2)
                * Mat4::from_rotation_z(std::f32::consts::PI)
        }
        1 => {
            Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2)
                * Mat4::from_rotation_z(std::f32::consts::PI)
        }
        other => Mat4::from_rotation_y(
            Facing::from_facing_direction(i64::from(other))
                .unwrap_or(Facing::North)
                .yaw_degrees()
                .to_radians(),
        ),
    };
    Mat4::from_translation(center) * turn
}

pub(super) fn emit(
    builder: &mut MeshBuilder,
    atlas: &BlockEntityAtlas,
    block: [i32; 3],
    model: &ItemFrameModel,
) {
    let name = if model.glow {
        "textures/blocks/glow_item_frame"
    } else {
        "textures/blocks/itemframe_background"
    };
    let Some(texture) = atlas.texture(name, [16.0, 16.0]) else {
        return;
    };
    let matrix = frame_matrix(block, model.outward) * Mat4::from_scale(Vec3::splat(1.0 / 16.0));
    builder.tile_cuboid(
        Layer::Solid,
        matrix,
        [-PANEL_HALF, -PANEL_HALF, ITEM_FRAME_PANEL_FRONT_PIXELS],
        [PANEL_HALF, PANEL_HALF, PANEL_BACK],
        [texture.rect; 6],
        WHITE,
    );
    if let Some(rect) = model.map {
        // Just in front of the panel, filling its inner area.
        let z = ITEM_FRAME_PANEL_FRONT_PIXELS - MAP_LIFT;
        let corners = [
            [PANEL_HALF, PANEL_HALF, z],
            [-PANEL_HALF, PANEL_HALF, z],
            [-PANEL_HALF, -PANEL_HALF, z],
            [PANEL_HALF, -PANEL_HALF, z],
        ]
        .map(|corner| matrix.transform_point3(Vec3::from_array(corner)).to_array());
        builder.textured_quad(Layer::Solid, corners, rect, WHITE);
    }
}

/// Pose of the framed item, rotated clockwise by `rotation_degrees` in its face plane.
#[must_use]
pub fn item_frame_item_transform(
    block: [i32; 3],
    outward: u8,
    rotation_degrees: f32,
) -> [[f32; 4]; 3] {
    // The sprite faces +Z, so a half turn points it at the viewer in front of the panel.
    let matrix = frame_matrix(block, outward)
        * Mat4::from_translation(Vec3::new(0.0, 0.0, ITEM_FRAME_PANEL_FRONT_PIXELS / 16.0))
        * Mat4::from_rotation_y(std::f32::consts::PI)
        * Mat4::from_rotation_z(-rotation_degrees.to_radians())
        * Mat4::from_scale(Vec3::splat(ITEM_SCALE));
    matrix_rows(matrix)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Applies an item pose to a point or vector without changing its published row layout.
    fn apply(rows: [[f32; 4]; 3], value: Vec3, point: bool) -> Vec3 {
        Vec3::from_array(rows.map(|row| {
            Vec3::new(row[0], row[1], row[2]).dot(value) + if point { row[3] } else { 0.0 }
        }))
    }

    #[test]
    fn framed_sprites_remain_centered_on_every_support_face_at_every_rotation() {
        let block = [7, -3, 11];
        let center = Vec3::from_array(block.map(|v| v as f32 + 0.5));
        let normals = [
            Vec3::NEG_Y,
            Vec3::Y,
            Vec3::NEG_Z,
            Vec3::Z,
            Vec3::NEG_X,
            Vec3::X,
        ];
        for (outward, normal) in normals.into_iter().enumerate() {
            for step in 0..8_u8 {
                let rows = item_frame_item_transform(
                    block,
                    outward as u8,
                    f32::from(step) * ITEM_FRAME_ROTATION_STEP_DEGREES,
                );
                let anchor = center - normal * (ITEM_FRAME_PANEL_FRONT_PIXELS / 16.0);
                assert!(
                    apply(rows, Vec3::ZERO, true).abs_diff_eq(anchor, 1.0e-5),
                    "facing {outward}, rotation {step}: item must sit on the panel front"
                );
                assert!(
                    apply(rows, Vec3::Z, false)
                        .normalize()
                        .abs_diff_eq(normal, 1.0e-5)
                );
                assert!((apply(rows, Vec3::X, false).length() - ITEM_SCALE).abs() < 1.0e-5);
            }
        }
    }

    #[test]
    fn item_turns_clockwise_in_each_face_plane_with_the_vertical_frames_upright() {
        let bases = [
            (Vec3::X, Vec3::Z),
            (Vec3::X, Vec3::NEG_Z),
            (Vec3::NEG_X, Vec3::Y),
            (Vec3::X, Vec3::Y),
            (Vec3::Z, Vec3::Y),
            (Vec3::NEG_Z, Vec3::Y),
        ];
        for (outward, (right, up)) in bases.into_iter().enumerate() {
            for degrees in (0..8_u8)
                .map(|step| f32::from(step) * ITEM_FRAME_ROTATION_STEP_DEGREES)
                .chain([22.5])
            {
                let rows = item_frame_item_transform([0; 3], outward as u8, degrees);
                let angle = degrees.to_radians();
                let expected_top = up * angle.cos() + right * angle.sin();
                assert!(
                    apply(rows, Vec3::Y, false)
                        .normalize()
                        .abs_diff_eq(expected_top, 1.0e-5),
                    "facing {outward}, rotation {degrees}: top must follow the face's clockwise turn"
                );
            }
        }
    }

    #[test]
    fn the_item_sits_in_front_of_the_wall_panel() {
        // North-facing frame at the origin: the item plane is toward -Z of the block center
        // relative to the panel, so it is nearer the north edge than the wall (south) edge.
        let rows = item_frame_item_transform([0, 0, 0], 2, 0.0);
        let z = rows[2][3];
        assert!(z > 0.5 && z < 1.0, "{z}");
        assert!((rows[0][3] - 0.5).abs() < 1.0e-6 && (rows[1][3] - 0.5).abs() < 1.0e-6);
    }

    #[test]
    fn outward_ids_pick_the_turn() {
        let up = frame_matrix([0; 3], 1).transform_vector3(Vec3::NEG_Z);
        let down = frame_matrix([0; 3], 0).transform_vector3(Vec3::NEG_Z);
        let west = frame_matrix([0; 3], 4).transform_vector3(Vec3::NEG_Z);
        assert!(up.abs_diff_eq(Vec3::Y, 1.0e-5));
        assert!(down.abs_diff_eq(Vec3::NEG_Y, 1.0e-5));
        assert!(west.abs_diff_eq(Vec3::NEG_X, 1.0e-5));
    }
}
