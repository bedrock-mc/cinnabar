//! Sign text: rasterized canvases mapped onto the board faces. The board and post are
//! drawn by the block state.
//!
//! Face planes follow the sign board boxes the terrain compiler emits (16x8 standing and wall
//! boards, 14-wide hanging boards, one pixel thick); those extents need native measurement.

use super::{
    atlas::AtlasRect,
    mesh::{Facing, Layer, MeshBuilder, WHITE, model_matrix},
};

/// Board half-thickness in pixels.
const BOARD_HALF_DEPTH: f32 = 0.5;
/// Gap that keeps text in front of the board face.
const TEXT_LIFT: f32 = 0.02;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SignMount {
    /// Post-mounted; `rotation_degrees` follows the skull convention (0 faces south).
    Standing { rotation_degrees: f32 },
    /// Flat on a wall, facing away from it.
    Wall(Facing),
    /// Chained to a ceiling.
    Hanging { rotation_degrees: f32 },
    /// Chained to a wall bracket; the board is edge-on to the wall.
    HangingWall(Facing),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SignFace {
    /// The canvas cell in the atlas.
    pub rect: AtlasRect,
    /// Glowing ink ignores world light.
    pub glowing: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SignModel {
    pub mount: SignMount,
    pub front: Option<SignFace>,
    pub back: Option<SignFace>,
}

/// Face-plane extents in pixels: center height, width, height, and distance from the
/// pivot to the front face along -Z (negative means the front sits at +Z).
struct Plane {
    center_y: f32,
    width: f32,
    height: f32,
    front_z: f32,
    back_z: Option<f32>,
}

pub(super) fn emit(builder: &mut MeshBuilder, block: [i32; 3], model: &SignModel) {
    let (matrix, plane) = match model.mount {
        SignMount::Standing { rotation_degrees } => (
            model_matrix(block, [0.5, 0.0, 0.5], 180.0 - rotation_degrees),
            Plane {
                center_y: 11.0,
                width: 16.0,
                height: 8.0,
                front_z: -BOARD_HALF_DEPTH,
                back_z: Some(BOARD_HALF_DEPTH),
            },
        ),
        SignMount::Wall(facing) => (
            model_matrix(block, [0.5, 0.0, 0.5], facing.yaw_degrees()),
            Plane {
                center_y: 8.5,
                width: 16.0,
                height: 8.0,
                front_z: 7.0,
                back_z: None,
            },
        ),
        SignMount::Hanging { rotation_degrees } => (
            model_matrix(block, [0.5, 0.0, 0.5], 180.0 - rotation_degrees),
            Plane {
                center_y: 7.0,
                width: 14.0,
                height: 7.0,
                front_z: -BOARD_HALF_DEPTH,
                back_z: Some(BOARD_HALF_DEPTH),
            },
        ),
        SignMount::HangingWall(facing) => (
            model_matrix(block, [0.5, 0.0, 0.5], facing.yaw_degrees()),
            Plane {
                center_y: 7.0,
                width: 14.0,
                height: 7.0,
                front_z: -BOARD_HALF_DEPTH,
                back_z: Some(BOARD_HALF_DEPTH),
            },
        ),
    };
    let half_width = plane.width * 0.5;
    let (top, bottom) = (
        plane.center_y + plane.height * 0.5,
        plane.center_y - plane.height * 0.5,
    );
    let mut draw = |face: Option<SignFace>, z: f32, mirrored: bool| {
        let Some(face) = face else {
            return;
        };
        // Front corners run +X (left) to -X (right) as seen from -Z; the back mirrors that.
        let (left, right) = if mirrored {
            (-half_width, half_width)
        } else {
            (half_width, -half_width)
        };
        let corners = [
            [left, top, z],
            [right, top, z],
            [right, bottom, z],
            [left, bottom, z],
        ]
        .map(|corner| {
            matrix
                .transform_point3(glam::Vec3::from_array(corner))
                .to_array()
        });
        let saved = builder.light;
        if face.glowing {
            builder.light = 1.0;
        }
        builder.textured_quad(Layer::Solid, corners, face.rect, WHITE);
        builder.light = saved;
    };
    draw(model.front, plane.front_z - TEXT_LIFT, false);
    if let Some(back_z) = plane.back_z {
        draw(model.back, back_z + TEXT_LIFT, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face() -> SignFace {
        SignFace {
            rect: AtlasRect {
                x: 0.0,
                y: 0.0,
                width: 96.0,
                height: 48.0,
            },
            glowing: false,
        }
    }

    #[test]
    fn front_and_back_quads_sit_on_opposite_sides_of_a_standing_board() {
        let mut builder = MeshBuilder::new([1024, 1024]);
        // Rotation 180 faces north: the front plane is toward -Z of the block center.
        let model = SignModel {
            mount: SignMount::Standing {
                rotation_degrees: 180.0,
            },
            front: Some(face()),
            back: Some(face()),
        };
        emit(&mut builder, [0, 0, 0], &model);
        assert_eq!(builder.solid.len(), 12);
        assert!(
            builder.solid[..6]
                .iter()
                .all(|vertex| vertex.position[2] < 0.5)
        );
        assert!(
            builder.solid[6..]
                .iter()
                .all(|vertex| vertex.position[2] > 0.5)
        );
    }

    #[test]
    fn wall_signs_draw_only_the_front() {
        let mut builder = MeshBuilder::new([1024, 1024]);
        let model = SignModel {
            mount: SignMount::Wall(Facing::North),
            front: Some(face()),
            back: Some(face()),
        };
        emit(&mut builder, [0, 0, 0], &model);
        assert_eq!(builder.solid.len(), 6);
    }

    #[test]
    fn glowing_text_ignores_light() {
        let mut builder = MeshBuilder::new([1024, 1024]);
        builder.light = 0.25;
        let mut lit = face();
        lit.glowing = true;
        let model = SignModel {
            mount: SignMount::Wall(Facing::South),
            front: Some(lit),
            back: None,
        };
        emit(&mut builder, [0, 0, 0], &model);
        assert_eq!(builder.solid[0].color[0], 1.0);
        assert_eq!(builder.light, 0.25);
    }
}
