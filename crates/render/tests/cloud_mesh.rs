use std::mem::size_of;

use assets::{AtmosphereRole, AtmosphereTexture};
use render::{
    CLOUD_MASK_SIZE, CLOUD_TOP_Y, CLOUD_UNDERSIDE_Y, CloudFace, CloudMeshError, MAX_CLOUD_BYTES,
    MAX_CLOUD_QUADS, PackedCloudQuad, cloud_instance_origins, mesh_cloud_texture,
};

const BORDER_QUADS_PER_FACE: usize = (CLOUD_MASK_SIZE * CLOUD_MASK_SIZE / 2) as usize;

#[test]
fn packed_cloud_quad_is_exactly_two_words_and_round_trips_every_field() {
    fn assert_pod<T: bytemuck::Pod + bytemuck::Zeroable>() {}
    assert_pod::<PackedCloudQuad>();
    assert_eq!(size_of::<PackedCloudQuad>(), 8);

    let packed = PackedCloudQuad::try_pack(255, 254, 256, 4, CloudFace::East).unwrap();
    assert_eq!(
        packed.words(),
        [
            255 | (254 << 8) | (255 << 16) | (3 << 24),
            CloudFace::East as u32,
        ]
    );
    assert_eq!(packed.axis0_start(), 255);
    assert_eq!(packed.axis1_start(), 254);
    assert_eq!(packed.axis0_extent(), 256);
    assert_eq!(packed.axis1_extent(), 4);
    assert_eq!(packed.face(), CloudFace::East);
    assert_eq!(
        PackedCloudQuad::try_from_words(packed.words()),
        Some(packed)
    );

    assert!(PackedCloudQuad::try_pack(256, 0, 1, 1, CloudFace::Down).is_none());
    assert!(PackedCloudQuad::try_pack(0, 256, 1, 1, CloudFace::Down).is_none());
    assert!(PackedCloudQuad::try_pack(0, 0, 0, 1, CloudFace::Down).is_none());
    assert!(PackedCloudQuad::try_pack(0, 0, 257, 1, CloudFace::Down).is_none());
    assert!(PackedCloudQuad::try_from_words([0, 1 << 3]).is_none());
    assert!(PackedCloudQuad::try_from_words([0, 6]).is_none());
}

#[test]
fn cloud_texture_validation_rejects_wrong_role_dimensions_and_byte_length() {
    let mut wrong_role = empty_cloud_texture();
    wrong_role.role = AtmosphereRole::Sun;
    assert_eq!(
        mesh_cloud_texture(&wrong_role),
        Err(CloudMeshError::WrongRole {
            actual: AtmosphereRole::Sun,
        })
    );

    let mut wrong_dimensions = empty_cloud_texture();
    wrong_dimensions.width = CLOUD_MASK_SIZE - 1;
    assert_eq!(
        mesh_cloud_texture(&wrong_dimensions),
        Err(CloudMeshError::WrongDimensions {
            width: CLOUD_MASK_SIZE - 1,
            height: CLOUD_MASK_SIZE,
        })
    );

    let mut wrong_length = empty_cloud_texture();
    wrong_length.rgba8 = vec![0; wrong_length.rgba8.len() - 1].into_boxed_slice();
    assert_eq!(
        mesh_cloud_texture(&wrong_length),
        Err(CloudMeshError::WrongByteLength {
            actual: (CLOUD_MASK_SIZE * CLOUD_MASK_SIZE * 4 - 1) as usize,
            expected: (CLOUD_MASK_SIZE * CLOUD_MASK_SIZE * 4) as usize,
        })
    );
}

#[test]
fn alpha_one_is_empty_and_alpha_255_emits_all_six_fixed_height_faces() {
    assert!(
        mesh_cloud_texture(&empty_cloud_texture())
            .unwrap()
            .is_empty()
    );

    let quads = mesh_cloud_texture(&cloud_with_occupied(&[[7, 11]])).unwrap();
    assert_eq!(
        quads.as_ref(),
        [
            quad(7, 11, 1, 1, CloudFace::Down),
            quad(7, 11, 1, 1, CloudFace::Up),
            quad(7, 11, 1, 4, CloudFace::North),
            quad(7, 12, 1, 4, CloudFace::South),
            quad(11, 7, 1, 4, CloudFace::West),
            quad(11, 8, 1, 4, CloudFace::East),
        ]
    );
    assert_eq!(CLOUD_UNDERSIDE_Y, 128.0);
    assert_eq!(CLOUD_TOP_Y, 132.0);
    assert_eq!(CLOUD_TOP_Y - CLOUD_UNDERSIDE_Y, 4.0);
}

#[test]
fn adjacent_cells_cull_the_internal_face_and_greedily_merge_each_plane() {
    let quads = mesh_cloud_texture(&cloud_with_occupied(&[[10, 20], [11, 20]])).unwrap();
    assert_eq!(
        quads.as_ref(),
        [
            quad(10, 20, 2, 1, CloudFace::Down),
            quad(10, 20, 2, 1, CloudFace::Up),
            quad(10, 20, 2, 4, CloudFace::North),
            quad(10, 21, 2, 4, CloudFace::South),
            quad(20, 10, 1, 4, CloudFace::West),
            quad(20, 12, 1, 4, CloudFace::East),
        ]
    );
}

#[test]
fn toroidal_edge_neighbours_cull_both_shared_seam_faces() {
    let quads = mesh_cloud_texture(&cloud_with_occupied(&[[0, 20], [255, 20]])).unwrap();
    assert_eq!(quads.len(), 10);
    assert!(!quads.iter().any(|quad| {
        matches!(quad.face(), CloudFace::West | CloudFace::East) && quad.axis1_start() == 0
    }));
    assert_eq!(face_count(&quads, CloudFace::West), 1);
    assert_eq!(face_count(&quads, CloudFace::East), 1);
}

#[test]
fn all_filled_torus_emits_only_one_full_top_and_bottom_quad() {
    let quads = mesh_cloud_texture(&cloud_texture_with_alpha(255)).unwrap();
    assert_eq!(
        quads.as_ref(),
        [
            quad(0, 0, 256, 256, CloudFace::Down),
            quad(0, 0, 256, 256, CloudFace::Up),
        ]
    );
}

#[test]
fn checkerboard_hits_the_checked_worst_case_record_and_byte_ceilings() {
    let mut texture = empty_cloud_texture();
    for z in 0..CLOUD_MASK_SIZE {
        for x in 0..CLOUD_MASK_SIZE {
            if (x + z) % 2 == 0 {
                set_alpha(&mut texture, x, z, 255);
            }
        }
    }

    let quads = mesh_cloud_texture(&texture).unwrap();
    assert_eq!(BORDER_QUADS_PER_FACE * 6, MAX_CLOUD_QUADS);
    assert_eq!(quads.len(), MAX_CLOUD_QUADS);
    assert_eq!(quads.len() * size_of::<PackedCloudQuad>(), MAX_CLOUD_BYTES);
}

#[test]
fn cloud_meshing_order_is_deterministic_by_face_then_coordinate() {
    let texture = cloud_with_occupied(&[[9, 3], [10, 3], [40, 80], [40, 81]]);
    assert_eq!(
        mesh_cloud_texture(&texture).unwrap(),
        mesh_cloud_texture(&texture).unwrap()
    );
    let quads = mesh_cloud_texture(&texture).unwrap();
    assert!(
        quads
            .windows(2)
            .all(|pair| pair[0].face() as u8 <= pair[1].face() as u8)
    );
}

#[test]
fn cloud_instance_origins_are_canonical_row_major_and_snap_at_period_boundaries() {
    assert_eq!(
        cloud_instance_origins([0.0, 0.0], 0.0),
        [
            [-256.0, -256.0],
            [0.0, -256.0],
            [256.0, -256.0],
            [-256.0, 0.0],
            [0.0, 0.0],
            [256.0, 0.0],
            [-256.0, 256.0],
            [0.0, 256.0],
            [256.0, 256.0],
        ]
    );
    assert_eq!(
        cloud_instance_origins([255.999, 255.999], 0.0),
        cloud_instance_origins([0.0, 0.0], 0.0)
    );
    assert_eq!(
        cloud_instance_origins([256.0, 256.0], 0.0),
        cloud_instance_origins([0.0, 0.0], 0.0).map(|[x, z]| [x + 256.0, z + 256.0])
    );
    assert_eq!(cloud_instance_origins([0.0, 0.0], 0.0)[4], [0.0, 0.0]);
    assert_eq!(
        cloud_instance_origins([-0.001, -0.001], 0.0)[4],
        [-256.0, -256.0]
    );
}

#[test]
fn cloud_instance_origins_preserve_wrapped_fractional_motion_without_non_finite_values() {
    assert_eq!(cloud_instance_origins([1.25, 0.0], 257.25)[4], [1.25, 0.0]);
    assert_eq!(
        cloud_instance_origins([1.249, 0.0], 257.25)[4],
        [-254.75, 0.0]
    );

    for input in [
        ([f64::NAN, 0.0], 0.0),
        ([f64::INFINITY, f64::NEG_INFINITY], f64::NAN),
        ([f64::MAX, -f64::MAX], f64::MAX),
    ] {
        assert!(
            cloud_instance_origins(input.0, input.1)
                .into_iter()
                .flatten()
                .all(f32::is_finite)
        );
    }
}

fn empty_cloud_texture() -> AtmosphereTexture {
    cloud_texture_with_alpha(1)
}

fn cloud_texture_with_alpha(alpha: u8) -> AtmosphereTexture {
    let mut rgba8 = vec![255; (CLOUD_MASK_SIZE * CLOUD_MASK_SIZE * 4) as usize];
    for pixel in rgba8.chunks_exact_mut(4) {
        pixel[3] = alpha;
    }
    AtmosphereTexture {
        role: AtmosphereRole::Clouds,
        source_path: "textures/environment/clouds.png".into(),
        source_bytes: 1,
        source_sha256: [1; 32],
        pixels_sha256: [2; 32],
        width: CLOUD_MASK_SIZE,
        height: CLOUD_MASK_SIZE,
        rgba8: rgba8.into_boxed_slice(),
    }
}

fn cloud_with_occupied(coordinates: &[[u32; 2]]) -> AtmosphereTexture {
    let mut texture = empty_cloud_texture();
    for [x, z] in coordinates {
        set_alpha(&mut texture, *x, *z, 255);
    }
    texture
}

fn set_alpha(texture: &mut AtmosphereTexture, x: u32, z: u32, alpha: u8) {
    let offset = ((z * CLOUD_MASK_SIZE + x) * 4 + 3) as usize;
    texture.rgba8[offset] = alpha;
}

fn quad(
    axis0_start: u16,
    axis1_start: u16,
    axis0_extent: u16,
    axis1_extent: u16,
    face: CloudFace,
) -> PackedCloudQuad {
    PackedCloudQuad::try_pack(axis0_start, axis1_start, axis0_extent, axis1_extent, face).unwrap()
}

fn face_count(quads: &[PackedCloudQuad], face: CloudFace) -> usize {
    quads.iter().filter(|quad| quad.face() == face).count()
}
