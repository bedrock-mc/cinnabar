//! The open book on enchanting tables and lecterns, and the lectern stand.
//!
//! Book part boxes and UV origins follow the enchanting-book texture unwrap; the hover height,
//! tilt, spread and page-flip timing need native measurement.

use assets::block_entity_geometry as geometry;
use bevy::math::Mat4;

use super::{
    atlas::BlockEntityAtlas,
    mesh::{BoxSpec, Layer, MeshBuilder, WHITE, model_matrix},
    scene::SceneClock,
};

const HOVER_HEIGHT_PIXELS: f32 = 12.0;
const BOB_PIXELS: f32 = 0.6;
const BOB_PERIOD_TICKS: f64 = 63.0;
const SPREAD_DEGREES: f32 = 20.0;
const SPREAD_WOBBLE_DEGREES: f32 = 4.0;
const TILT_DEGREES: f32 = 10.0;
const FLIP_PERIOD_TICKS: f64 = 40.0;
const THIN: f32 = 0.01;

/// Emits the book with its spine along local Y, pages facing +Z, in `matrix` space; `flip`
/// is the turning page's angle about the spine in radians.
fn emit_book(
    builder: &mut MeshBuilder,
    atlas: &BlockEntityAtlas,
    matrix: Mat4,
    spread: f32,
    flip: f32,
) {
    let Some(texture) = atlas.texture("textures/entity/enchanting_table_book", [64.0, 32.0]) else {
        return;
    };
    let left = matrix * Mat4::from_rotation_y(spread);
    let right = matrix * Mat4::from_rotation_y(-spread);
    let turning = matrix * Mat4::from_rotation_y(flip);
    let parts: [(Mat4, BoxSpec); 6] = [
        (
            left,
            BoxSpec::new([-6.0, -5.0, -THIN], [6.0, 10.0, THIN], [0.0, 0.0]),
        ),
        (
            right,
            BoxSpec::new([0.0, -5.0, -THIN], [6.0, 10.0, THIN], [16.0, 0.0]),
        ),
        (
            matrix,
            BoxSpec::new([-1.0, -5.0, -THIN], [2.0, 10.0, THIN], [12.0, 0.0]),
        ),
        (
            left,
            BoxSpec::new([-5.0, -4.0, 0.0], [5.0, 8.0, 1.0], [0.0, 10.0]),
        ),
        (
            right,
            BoxSpec::new([0.0, -4.0, 0.0], [5.0, 8.0, 1.0], [12.0, 10.0]),
        ),
        (
            turning,
            BoxSpec::new([0.0, -4.0, 0.5], [5.0, 8.0, THIN], [24.0, 10.0]),
        ),
    ];
    for (part_matrix, spec) in parts {
        builder.cuboid(Layer::Solid, &texture, part_matrix, spec, WHITE);
    }
}

pub(super) fn emit_enchant_table(
    builder: &mut MeshBuilder,
    atlas: &BlockEntityAtlas,
    block: [i32; 3],
    facing_yaw_degrees: f32,
    clock: SceneClock,
) {
    let phase = (clock.ticks / BOB_PERIOD_TICKS).fract() as f32;
    let bob = BOB_PIXELS * (std::f32::consts::TAU * phase).sin();
    let flip_phase = (clock.ticks / FLIP_PERIOD_TICKS).fract() as f32;
    let spread = SPREAD_DEGREES.to_radians()
        + SPREAD_WOBBLE_DEGREES.to_radians() * (std::f32::consts::TAU * phase).sin();
    // The turning page sweeps between the two covers.
    let flip = spread * (std::f32::consts::TAU * flip_phase).cos();
    // Lay the book flat (pages up) and tip it toward the viewer.
    let matrix = model_matrix(
        block,
        [0.5, (HOVER_HEIGHT_PIXELS + bob) / 16.0, 0.5],
        facing_yaw_degrees,
    ) * Mat4::from_rotation_x(-TILT_DEGREES.to_radians())
        * Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    emit_book(builder, atlas, matrix, spread, flip);
}

pub(super) fn emit_lectern(
    builder: &mut MeshBuilder,
    atlas: &BlockEntityAtlas,
    block: [i32; 3],
    facing_yaw_degrees: f32,
    has_book: bool,
) {
    let base = model_matrix(block, [0.5, 0.0, 0.5], facing_yaw_degrees);
    let [Some(bottom), Some(sides), Some(top), Some(front)] =
        geometry::LECTERN_TEXTURES.map(|name| atlas.texture(name, [16.0; 2]))
    else {
        return;
    };
    let textures = [bottom, sides, top, front];
    for (index, (texture_index, (corners, texels))) in
        geometry::lectern_faces().into_iter().enumerate()
    {
        let texture = textures[texture_index];
        let [u0, v0, u1, v1] = texture.rect_uv(texels);
        let shade = super::mesh::tile_face_shade(index % 6);
        builder.quad_uv(
            Layer::Solid,
            corners.map(|corner| {
                base.transform_point3(bevy::math::Vec3::from_array(corner))
                    .to_array()
            }),
            [[u0, v0], [u1, v0], [u1, v1], [u0, v1]],
            [shade, shade, shade, 1.0],
        );
    }
    if has_book {
        let board = base
            * Mat4::from_translation(bevy::math::Vec3::from_array(geometry::LECTERN_BOARD_OFFSET))
            * Mat4::from_translation(bevy::math::Vec3::from_array(geometry::LECTERN_BOARD_PIVOT))
            * Mat4::from_rotation_x(-geometry::LECTERN_SLOPE_DEGREES.to_radians())
            * Mat4::from_translation(-bevy::math::Vec3::from_array(geometry::LECTERN_BOARD_PIVOT));
        let book = board
            * Mat4::from_translation(bevy::math::Vec3::new(
                0.0,
                geometry::LECTERN_BOARD[1][1] + 0.5,
                (geometry::LECTERN_BOARD[0][2] + geometry::LECTERN_BOARD[1][2]) * 0.5,
            ))
            * Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2);
        emit_book(builder, atlas, book, SPREAD_DEGREES.to_radians(), 0.0);
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::Vec3;

    use super::*;

    #[test]
    fn flattening_turns_the_page_normal_upward() {
        let normal = Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2).transform_vector3(Vec3::Z);
        assert!(normal.abs_diff_eq(Vec3::Y, 1.0e-5));
    }

    #[test]
    fn the_lectern_slope_lowers_the_reader_edge() {
        let slope = Mat4::from_rotation_x(-geometry::LECTERN_SLOPE_DEGREES.to_radians());
        let reader_edge = slope.transform_point3(Vec3::new(0.0, 0.0, -8.0));
        let far_edge = slope.transform_point3(Vec3::new(0.0, 0.0, 8.0));
        assert!(reader_edge.y < far_edge.y);
    }
    #[test]
    fn lectern_stand_uses_cropped_faces_and_the_sloped_board_dimensions() {
        let mut placements: Vec<_> = [
            "lectern_base",
            "lectern_sides",
            "lectern_top",
            "lectern_front",
        ]
        .into_iter()
        .enumerate()
        .map(|(index, name)| assets::BlockEntityPlacement {
            name: format!("textures/blocks/{name}").into(),
            x: index as u32 * 16,
            y: 0,
            width: 16,
            height: 16,
        })
        .collect();
        placements.sort_unstable_by(|left, right| left.name.cmp(&right.name));
        let encoded = assets::encode_block_entity_catalog(
            b"{}",
            64,
            16,
            &vec![255; 64 * 16 * 4],
            &placements,
        )
        .unwrap();
        let atlas = BlockEntityAtlas::from_assets(
            &assets::RuntimeBlockEntityAssets::decode(&encoded).unwrap(),
        );
        for yaw in [0.0, 90.0, 180.0, 270.0] {
            let mut builder = MeshBuilder::new(atlas.size());
            emit_lectern(&mut builder, &atlas, [0; 3], yaw, false);
            let inverse = model_matrix([0; 3], [0.5, 0.0, 0.5], yaw).inverse();
            let points: Vec<_> = builder
                .solid
                .iter()
                .map(|vertex| inverse.transform_point3(Vec3::from_array(vertex.position)))
                .collect();
            let board = &points[72..];
            assert!(((board[0] - board[1]).length() - 4.0).abs() < 1.0e-4);
            assert!(((board[0] - board[2]).length() - 13.0).abs() < 1.0e-4);
            let (min_x, max_x) = board
                .iter()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), point| {
                    (min.min(point.x), max.max(point.x))
                });
            assert!((max_x - min_x - 15.8).abs() < 1.0e-4);
            let front = &builder.solid[60..66];
            for (vertex, point) in front.iter().zip(&points[60..66]) {
                let expected = [
                    if (point.y - 14.0).abs() < 1.0e-4 {
                        56.0
                    } else {
                        48.0
                    },
                    if point.x > 0.0 { 0.0 } else { 13.0 },
                ];
                let uv = [
                    vertex.uv[0] * atlas.size()[0] as f32,
                    vertex.uv[1] * atlas.size()[1] as f32,
                ];
                assert!(
                    (uv[0] - expected[0]).abs() < 1.0e-4 && (uv[1] - expected[1]).abs() < 1.0e-4
                );
            }
            let top = &builder.solid[90..96];
            let min_v = top
                .iter()
                .map(|vertex| vertex.uv[1] * atlas.size()[1] as f32)
                .reduce(f32::min)
                .unwrap();
            let max_v = top
                .iter()
                .map(|vertex| vertex.uv[1] * atlas.size()[1] as f32)
                .reduce(f32::max)
                .unwrap();
            assert!((min_v - 1.0).abs() < 1.0e-4 && (max_v - 14.0).abs() < 1.0e-4);
        }
    }
}
