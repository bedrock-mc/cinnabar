//! Item and glow item frames: a wooden rim around a recessed backing; the item is drawn by
//! the dropped-item renderer at the pose from [`item_frame_item_transform`].
//!
//! A frame whose block state carries the map or photo bit widens its rim to the whole block
//! face, and a filled map covers that face so neighbouring framed maps join edge to edge.
//! Special contained-item materials remain incomplete.

use glam::{Mat4, Vec3};

use super::{
    atlas::{AtlasRect, BlockEntityAtlas, TextureRef},
    items::matrix_rows,
    mesh::{Facing, Layer, MeshBuilder, WHITE, tile_face_shade},
};

const PANEL_HALF: f32 = 6.0;
/// Frame-local depth of the rim front and item plane, in pixels from the block center.
pub const ITEM_FRAME_PANEL_FRONT_PIXELS: f32 = 7.0;
const PANEL_BACK: f32 = 8.0;
/// Width of each rim strip, in pixels.
const RIM_WIDTH: f32 = 1.0;
/// Depth of the backing in front of the supporting wall, in pixels.
const BACKING_DEPTH: f32 = 0.5;
/// Half-width of a map frame's rim, which reaches the block face edges.
const MAP_PANEL_HALF: f32 = 8.0;
/// Depth of a map frame's rim in front of the supporting wall, in pixels.
const MAP_RIM_DEPTH: f32 = 0.9;
/// Depth of a framed map image in front of the supporting wall, in pixels.
const MAP_DEPTH: f32 = 1.0;
const ITEM_SCALE: f32 = 0.5;
/// Degrees per legacy item-frame rotation turn.
pub const ITEM_FRAME_ROTATION_STEP_DEGREES: f32 = 45.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemFrameModel {
    pub glow: bool,
    /// Direction the frame faces, as a Bedrock face id: 0 down, 1 up, 2 north, 3 south, 4 west,
    /// 5 east; other values read as north.
    pub outward: u8,
    /// Whether the block state's map or photo bit selects the full-face rim.
    pub full_face: bool,
    /// A filled map shown in place of an item sprite.
    pub map: Option<FramedMap>,
}

/// A filled map's image in a frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FramedMap {
    /// The atlas cell holding the map's image.
    pub image: AtlasRect,
    /// The frame's item rotation; a map turns twice as far, so each legacy step is a quarter turn.
    pub rotation_degrees: f32,
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

/// Emits selected cuboid faces with terrain UVs cropped to their world block coordinates.
fn emit_frame_cuboid(
    builder: &mut MeshBuilder,
    texture: &TextureRef,
    matrix: Mat4,
    bounds: [[f32; 3]; 2],
    faces: &[usize],
) {
    let corners = assets::block_entity_geometry::tile_box_faces(bounds);
    for &face in faces {
        // Derive UVs before world translation so distant blocks retain the same texel crop.
        let texels = corners[face].map(|corner| {
            matrix.transform_vector3(Vec3::from_array(corner)) * 16.0 + Vec3::splat(8.0)
        });
        let normal = (texels[2] - texels[0])
            .cross(texels[1] - texels[0])
            .normalize();
        let world_face = if normal.x.abs() > 0.5 {
            usize::from(normal.x > 0.0)
        } else if normal.y.abs() > 0.5 {
            2 + usize::from(normal.y > 0.0)
        } else {
            4 + usize::from(normal.z > 0.0)
        };
        let uvs = texels.map(|point| {
            let [u, v] = match world_face {
                0 => [point.z, 16.0 - point.y],
                1 => [16.0 - point.z, 16.0 - point.y],
                2 => [point.x, 16.0 - point.z],
                3 => [point.x, point.z],
                4 => [16.0 - point.x, 16.0 - point.y],
                _ => [point.x, 16.0 - point.y],
            };
            let [u, v, _, _] = texture.rect_uv([u, v, 0.0, 0.0]);
            [u, v]
        });
        let world = corners[face]
            .map(|corner| matrix.transform_point3(Vec3::from_array(corner)).to_array());
        let shade = tile_face_shade(world_face);
        builder.quad_uv(Layer::Solid, world, uvs, [shade, shade, shade, 1.0]);
    }
}

/// Emits the frame's rim and backing, and a filled map over its face.
pub(super) fn emit(
    builder: &mut MeshBuilder,
    atlas: &BlockEntityAtlas,
    block: [i32; 3],
    model: &ItemFrameModel,
) {
    let textures = assets::block_entity_geometry::ITEM_FRAME_TEXTURES;
    let Some(backing) = atlas.texture(textures[1 + usize::from(model.glow)], [16.0; 2]) else {
        return;
    };
    let matrix = frame_matrix(block, model.outward) * Mat4::from_scale(Vec3::splat(1.0 / 16.0));
    let light = builder.light;
    if model.glow {
        builder.light = 1.0;
    }
    let (half, rim_front) = if model.full_face {
        (MAP_PANEL_HALF, PANEL_BACK - MAP_RIM_DEPTH)
    } else {
        (PANEL_HALF, ITEM_FRAME_PANEL_FRONT_PIXELS)
    };
    let inner = half - RIM_WIDTH;
    if let Some(wood) = atlas.texture(textures[0], [16.0; 2]) {
        let full = [0, 1, 2, 3, 4, 5];
        let horizontal = [2, 3, 4, 5];
        for (min, max, faces) in [
            (
                [-half, -half, rim_front],
                [-inner, half, PANEL_BACK],
                full.as_slice(),
            ),
            (
                [inner, -half, rim_front],
                [half, half, PANEL_BACK],
                full.as_slice(),
            ),
            (
                [-inner, -half, rim_front],
                [inner, -inner, PANEL_BACK],
                horizontal.as_slice(),
            ),
            (
                [-inner, inner, rim_front],
                [inner, half, PANEL_BACK],
                horizontal.as_slice(),
            ),
        ] {
            emit_frame_cuboid(builder, &wood, matrix, [min, max], faces);
        }
    }
    emit_frame_cuboid(
        builder,
        &backing,
        matrix,
        [
            [-inner, -inner, PANEL_BACK - BACKING_DEPTH],
            [inner, inner, PANEL_BACK],
        ],
        &[4, 5],
    );
    if let Some(map) = model.map {
        emit_map(builder, matrix, map);
    }
    builder.light = light;
}

/// Covers the whole block face with the map image, turned in quarter steps of the frame rotation.
fn emit_map(builder: &mut MeshBuilder, frame: Mat4, map: FramedMap) {
    let degrees = (map.rotation_degrees * 2.0).rem_euclid(360.0);
    let matrix = frame * Mat4::from_rotation_z(degrees.to_radians());
    let z = PANEL_BACK - MAP_DEPTH;
    let half = MAP_PANEL_HALF;
    let corners = [
        [half, half, z],
        [-half, half, z],
        [-half, -half, z],
        [half, -half, z],
    ]
    .map(|corner| matrix.transform_point3(Vec3::from_array(corner)).to_array());
    builder.textured_quad(Layer::Solid, corners, map.image, WHITE);
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

    /// Packs distinct replacement textures so mesh samples identify their material and crop.
    fn test_atlas() -> BlockEntityAtlas {
        let mut placements: Vec<_> = assets::block_entity_geometry::ITEM_FRAME_TEXTURES
            .into_iter()
            .enumerate()
            .map(|(index, name)| assets::BlockEntityPlacement {
                name: name.into(),
                x: index as u32 * 32,
                y: 0,
                width: 32,
                height: 32,
            })
            .collect();
        placements.sort_by(|a, b| a.name.cmp(&b.name));
        let bytes = assets::encode_block_entity_catalog(
            b"{}",
            96,
            32,
            &vec![255; 96 * 32 * 4],
            &placements,
        )
        .unwrap();
        BlockEntityAtlas::from_assets(&assets::RuntimeBlockEntityAssets::decode(&bytes).unwrap())
    }

    /// Samples the nearest triangle along a ray, retaining interpolated texture and lighting.
    fn sample(builder: &MeshBuilder, origin: Vec3, direction: Vec3) -> (Vec3, Vec3) {
        try_sample(builder, origin, direction).expect("frame surface must intersect the ray")
    }

    /// The nearest surface along a ray as its atlas UV and ray depth, plus its interpolated color.
    fn try_sample(builder: &MeshBuilder, origin: Vec3, direction: Vec3) -> Option<(Vec3, Vec3)> {
        let mut hit = None;
        for [a, b, c] in builder.solid.as_chunks::<3>().0 {
            let edge1 = Vec3::from_array(b.position) - Vec3::from_array(a.position);
            let edge2 = Vec3::from_array(c.position) - Vec3::from_array(a.position);
            let cross = direction.cross(edge2);
            let determinant = edge1.dot(cross);
            if determinant.abs() < 1.0e-6 {
                continue;
            }
            let offset = origin - Vec3::from_array(a.position);
            let u = offset.dot(cross) / determinant;
            let v = direction.dot(offset.cross(edge1)) / determinant;
            let depth = edge2.dot(offset.cross(edge1)) / determinant;
            if u < -1.0e-5 || v < -1.0e-5 || u + v > 1.00001 || depth < 0.0 {
                continue;
            }
            if hit
                .as_ref()
                .is_some_and(|(previous, _, _)| depth >= *previous)
            {
                continue;
            }
            let interpolate = |values: [[f32; 3]; 3]| {
                Vec3::from_array(values[0]) * (1.0 - u - v)
                    + Vec3::from_array(values[1]) * u
                    + Vec3::from_array(values[2]) * v
            };
            let uv = interpolate([a, b, c].map(|vertex| [vertex.uv[0], vertex.uv[1], 0.0]));
            let color = interpolate(
                [a, b, c].map(|vertex| [vertex.color[0], vertex.color[1], vertex.color[2]]),
            );
            hit = Some((depth, uv, color));
        }
        let (depth, uv, color) = hit?;
        Some((Vec3::new(uv.x, uv.y, depth), color))
    }

    #[test]
    fn frames_separate_the_wooden_rim_from_the_recessed_backing_on_every_mount() {
        let atlas = test_atlas();
        for outward in 0..6 {
            for glow in [false, true] {
                let mut builder = MeshBuilder::new(atlas.size());
                builder.light = 0.2;
                emit(
                    &mut builder,
                    &atlas,
                    [0; 3],
                    &ItemFrameModel {
                        glow,
                        outward,
                        full_face: false,
                        map: None,
                    },
                );
                let pose = frame_matrix([0; 3], outward);
                let direction = pose.transform_vector3(Vec3::Z);
                for (x, y, material, depth) in [
                    (5.5, 0.0, 0, 7.0),
                    (0.0, 0.0, if glow { 2 } else { 1 }, 7.5),
                ] {
                    let origin = pose.transform_point3(Vec3::new(x, y, 0.0) / 16.0);
                    let (uv_depth, color) = sample(&builder, origin, direction);
                    let texel_x = uv_depth.x * atlas.size()[0] as f32;
                    assert!(
                        (material as f32 * 32.0..(material + 1) as f32 * 32.0).contains(&texel_x),
                        "facing {outward}, glow {glow}: rim and backing must select separate textures ({texel_x})"
                    );
                    assert!(
                        (uv_depth.z * 16.0 - depth).abs() < 1.0e-4,
                        "backing must be recessed behind the rim"
                    );
                    let brightness = if outward < 2 {
                        if outward == 0 { 0.5 } else { 1.0 }
                    } else if outward < 4 {
                        0.8
                    } else {
                        0.6
                    };
                    assert!((color.x - brightness * if glow { 1.0 } else { 0.2 }).abs() < 1.0e-5);
                }
                let back = pose.transform_point3(Vec3::new(0.0, 0.0, 16.0) / 16.0);
                let (uv_depth, _) = sample(&builder, back, -direction);
                assert!((uv_depth.z * 16.0 - 8.0).abs() < 1.0e-4);
                let backing = if glow { 2.0 } else { 1.0 };
                assert!((uv_depth.x * 96.0 - (backing * 32.0 + 16.0)).abs() < 1.0e-4);
            }
        }
    }

    #[test]
    fn frame_backing_keeps_block_texel_scale_in_replacement_packs() {
        let atlas = test_atlas();
        let mut builder = MeshBuilder::new(atlas.size());
        emit(
            &mut builder,
            &atlas,
            [0; 3],
            &ItemFrameModel {
                glow: false,
                outward: 2,
                full_face: false,
                map: None,
            },
        );
        let (uv_depth, _) = sample(&builder, Vec3::new(0.625, 0.5625, 0.5), Vec3::Z);
        assert!(
            (uv_depth.x * 96.0 - 44.0).abs() < 1.0e-4,
            "north face must crop and mirror block X rather than stretch the full image"
        );
        assert!(
            (uv_depth.y * atlas.size()[1] as f32 - 14.0).abs() < 1.0e-4,
            "backing must crop block Y at the replacement texture scale"
        );
    }

    /// A map image cell to the right of the test atlas's frame textures.
    const MAP_IMAGE: AtlasRect = AtlasRect {
        x: 96.0,
        y: 0.0,
        width: 128.0,
        height: 128.0,
    };

    /// A frame whose block state selects the map rim, optionally holding a map image.
    fn map_frame(outward: u8, glow: bool, rotation_degrees: Option<f32>) -> ItemFrameModel {
        ItemFrameModel {
            glow,
            outward,
            full_face: true,
            map: rotation_degrees.map(|rotation_degrees| FramedMap {
                image: MAP_IMAGE,
                rotation_degrees,
            }),
        }
    }

    #[test]
    fn framed_maps_cover_the_whole_block_face_on_every_mount() {
        let atlas = test_atlas();
        for outward in 0..6 {
            for glow in [false, true] {
                let mut builder = MeshBuilder::new(atlas.size());
                emit(
                    &mut builder,
                    &atlas,
                    [0; 3],
                    &map_frame(outward, glow, Some(0.0)),
                );
                let pose = frame_matrix([0; 3], outward);
                let direction = pose.transform_vector3(Vec3::Z);
                for [x, y] in [[7.9, 7.9], [-7.9, 0.0], [0.0, 0.0], [3.0, -7.9]] {
                    let origin = pose.transform_point3(Vec3::new(x, y, 0.0) / 16.0);
                    let (uv_depth, _) = sample(&builder, origin, direction);
                    assert!(
                        uv_depth.x * atlas.size()[0] as f32 >= MAP_IMAGE.x,
                        "facing {outward}, glow {glow}: ({x}, {y}) must show the map"
                    );
                    assert!(
                        (uv_depth.z * 16.0 - (PANEL_BACK - MAP_DEPTH)).abs() < 1.0e-4,
                        "the map lies one pixel in front of the supporting wall"
                    );
                }
                let outside = pose.transform_point3(Vec3::new(8.1, 0.0, 0.0) / 16.0);
                assert!(try_sample(&builder, outside, direction).is_none());
            }
        }
    }

    #[test]
    fn neighbouring_framed_maps_meet_without_a_gap() {
        let atlas = test_atlas();
        let mut builder = MeshBuilder::new(atlas.size());
        for block in [[0, 0, 0], [1, 0, 0]] {
            emit(&mut builder, &atlas, block, &map_frame(2, false, Some(0.0)));
        }
        for x in [0.999, 1.001] {
            let (uv_depth, _) = sample(&builder, Vec3::new(x, 0.5, 0.0), Vec3::Z);
            assert!(
                uv_depth.x * atlas.size()[0] as f32 >= MAP_IMAGE.x,
                "x {x} must show a map at the shared edge"
            );
            assert!((uv_depth.z - (1.0 - MAP_DEPTH / 16.0)).abs() < 1.0e-5);
        }
    }

    #[test]
    fn map_frames_widen_the_rim_before_their_image_arrives() {
        let atlas = test_atlas();
        let mut builder = MeshBuilder::new(atlas.size());
        emit(&mut builder, &atlas, [0; 3], &map_frame(2, false, None));
        let pose = frame_matrix([0; 3], 2);
        let direction = pose.transform_vector3(Vec3::Z);
        for (x, material, depth) in [
            (7.5, 0, PANEL_BACK - MAP_RIM_DEPTH),
            (0.0, 1, PANEL_BACK - BACKING_DEPTH),
        ] {
            let origin = pose.transform_point3(Vec3::new(x, 0.0, 0.0) / 16.0);
            let (uv_depth, _) = sample(&builder, origin, direction);
            let texel_x = uv_depth.x * atlas.size()[0] as f32;
            assert!(
                (material as f32 * 32.0..(material + 1) as f32 * 32.0).contains(&texel_x),
                "x {x}: rim and backing must select separate textures ({texel_x})"
            );
            assert!((uv_depth.z * 16.0 - depth).abs() < 1.0e-4);
        }
    }

    #[test]
    fn framed_maps_turn_a_quarter_per_legacy_step() {
        let atlas = test_atlas();
        for outward in 0..6 {
            for step in 0..8_u8 {
                let degrees = f32::from(step) * ITEM_FRAME_ROTATION_STEP_DEGREES;
                let mut builder = MeshBuilder::new(atlas.size());
                emit(
                    &mut builder,
                    &atlas,
                    [0; 3],
                    &map_frame(outward, false, Some(degrees)),
                );
                let map: Vec<_> = builder
                    .solid
                    .iter()
                    .filter(|vertex| vertex.uv[0] * atlas.size()[0] as f32 >= MAP_IMAGE.x)
                    .collect();
                assert!(!map.is_empty());
                let edge = |top: bool| {
                    let v = if top {
                        MAP_IMAGE.y
                    } else {
                        MAP_IMAGE.y + MAP_IMAGE.height
                    };
                    // Triangles repeat corners, so average each distinct corner once.
                    let mut corners: Vec<Vec3> = Vec::new();
                    for vertex in map
                        .iter()
                        .filter(|vertex| (vertex.uv[1] * atlas.size()[1] as f32 - v).abs() < 1.0e-3)
                    {
                        let corner = Vec3::from_array(vertex.position);
                        if !corners
                            .iter()
                            .any(|known| known.abs_diff_eq(corner, 1.0e-6))
                        {
                            corners.push(corner);
                        }
                    }
                    assert_eq!(corners.len(), 2, "one map edge has two corners");
                    corners.iter().sum::<Vec3>() / 2.0
                };
                let top = (edge(true) - edge(false)).normalize();
                let item = item_frame_item_transform([0; 3], outward, degrees * 2.0);
                let expected = apply(item, Vec3::Y, false).normalize();
                assert!(
                    top.abs_diff_eq(expected, 1.0e-4),
                    "facing {outward}, step {step}: the map top {top} must turn twice as far as \
                     an item's {expected}"
                );
            }
        }
    }

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
