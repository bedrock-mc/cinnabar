use assets::{AtmosphereRole, AtmosphereTexture};
use meshing::{
    CLOUD_MASK_SIZE, CloudFace, MAX_CLOUD_QUADS,
    cloud_viewport::{
        CLOUD_REBUILD_DISTANCE_SQUARED, CloudViewport, MAX_VIEWPORT_CLOUD_BYTES,
        MAX_VIEWPORT_CLOUD_QUADS, ViewportCloudQuad, mesh_cloud_viewport,
    },
};

fn texture() -> AtmosphereTexture {
    AtmosphereTexture {
        role: AtmosphereRole::Clouds,
        source_path: "textures/environment/clouds.png".into(),
        source_bytes: 1,
        source_sha256: [1; 32],
        pixels_sha256: [2; 32],
        width: CLOUD_MASK_SIZE,
        height: CLOUD_MASK_SIZE,
        rgba8: vec![0; (CLOUD_MASK_SIZE * CLOUD_MASK_SIZE * 4) as usize].into_boxed_slice(),
    }
}

fn set(texture: &mut AtmosphereTexture, x: usize, z: usize, rgba: [u8; 4]) {
    let offset = (z * CLOUD_MASK_SIZE as usize + x) * 4;
    texture.rgba8[offset..offset + 4].copy_from_slice(&rgba);
}

fn window(sample: [f64; 2], above: bool, both: bool) -> CloudViewport {
    CloudViewport::try_new(sample, 2, 1, above, both).unwrap()
}

#[test]
fn native_alpha_admission_and_byte_colour_bake_preserve_source_alpha() {
    let mut texture = texture();
    for alpha in [0, 1] {
        set(&mut texture, 0, 0, [200, 100, 50, alpha]);
        assert!(
            mesh_cloud_viewport(&texture, window([0.0; 2], false, false))
                .unwrap()
                .is_empty()
        );
    }
    set(&mut texture, 0, 0, [200, 100, 50, 2]);
    let quads = mesh_cloud_viewport(&texture, window([0.0; 2], false, false)).unwrap();
    assert_eq!(quads.len(), 5);
    let down = quads
        .iter()
        .find(|quad| quad.face == CloudFace::Down as u32)
        .unwrap();
    assert_eq!(down.colour.to_le_bytes(), [150, 75, 37, 2]);
    assert!(!quads.iter().any(|quad| quad.face == CloudFace::Up as u32));
    assert!(quads.iter().all(|quad| quad.cell == [0, 0]));
}

#[test]
fn native_caps_follow_view_height_or_explicit_both_cap_request() {
    let mut texture = texture();
    set(&mut texture, 0, 0, [255; 4]);
    for (above, both, expected_down, expected_up) in [
        (false, false, true, false),
        (true, false, false, true),
        (false, true, true, true),
        (true, true, true, true),
    ] {
        let quads = mesh_cloud_viewport(&texture, window([0.0; 2], above, both)).unwrap();
        assert_eq!(
            quads.iter().any(|quad| quad.face == CloudFace::Down as u32),
            expected_down
        );
        assert_eq!(
            quads.iter().any(|quad| quad.face == CloudFace::Up as u32),
            expected_up
        );
        assert_eq!(quads.len(), if both { 6 } else { 5 });
    }
}

#[test]
fn adjacent_texels_keep_unit_caps_but_never_emit_the_shared_side() {
    let mut texture = texture();
    set(&mut texture, 0, 0, [255; 4]);
    set(&mut texture, 1, 0, [255; 4]);
    let viewport = CloudViewport::try_new([16.0, 0.0], 2, 1, false, false).unwrap();
    let quads = mesh_cloud_viewport(&texture, viewport).unwrap();
    assert_eq!(quads.len(), 8);
    assert_eq!(
        quads
            .iter()
            .filter(|quad| quad.face == CloudFace::Down as u32)
            .count(),
        2
    );
    assert!(
        !quads
            .iter()
            .any(|quad| quad.cell == [0, 0] && quad.face == CloudFace::East as u32)
    );
    assert!(
        !quads
            .iter()
            .any(|quad| quad.cell == [1, 0] && quad.face == CloudFace::West as u32)
    );
}

#[test]
fn negative_sampling_wraps_texture_not_world_coordinates() {
    let mut texture = texture();
    set(
        &mut texture,
        CLOUD_MASK_SIZE as usize - 1,
        CLOUD_MASK_SIZE as usize - 1,
        [255; 4],
    );
    let quads = mesh_cloud_viewport(&texture, window([-0.001; 2], false, false)).unwrap();
    assert_eq!(quads.len(), 5);
    assert!(quads.iter().all(|quad| quad.cell == [-1, -1]));
    assert_eq!(window([-0.001; 2], false, false).centre(), [-16.0, -16.0]);
}

#[test]
fn native_cache_rebuilds_strictly_after_fifteen_blocks_or_geometry_changes() {
    let admitted = window([0.0; 2], false, false);
    let distance = CLOUD_REBUILD_DISTANCE_SQUARED.sqrt();
    assert!(!admitted.needs_rebuild(window([distance, 0.0], false, false)));
    assert!(admitted.needs_rebuild(window([distance + 0.0001, 0.0], false, false)));
    assert!(admitted.needs_rebuild(window([12.0, 12.0], false, false)));
    assert!(admitted.needs_rebuild(window([0.0; 2], true, false)));
    assert!(admitted.needs_rebuild(window([0.0; 2], false, true)));
    assert!(admitted.needs_rebuild(CloudViewport::try_new([0.0; 2], 4, 1, false, false).unwrap()));
    // Crossing a texel boundary alone does not invalidate the native cache.
    assert!(!window([15.9, 0.0], false, false).needs_rebuild(window([16.1, 0.0], false, false)));
}

#[test]
fn invalid_sampling_and_configuration_never_reach_gpu_addresses() {
    for sample in [
        [f64::NAN, 0.0],
        [f64::INFINITY, 0.0],
        [f64::MAX, 0.0],
        [f64::MIN, 0.0],
    ] {
        assert!(CloudViewport::try_new(sample, 2, 1, false, false).is_none());
    }
    for (mesh, grid) in [(0, 1), (2, 0), (3, 1), (u16::MAX, u8::MAX), (512, 1)] {
        assert!(CloudViewport::try_new([0.0; 2], mesh, grid, false, false).is_none());
    }
    assert!(CloudViewport::try_new([0.0; 2], CLOUD_MASK_SIZE as u16, 1, false, true).is_some());
}

#[test]
fn largest_checkerboard_window_has_the_exact_finite_record_budget() {
    let mut texture = texture();
    for z in 0..CLOUD_MASK_SIZE as usize {
        for x in 0..CLOUD_MASK_SIZE as usize {
            if (x + z) % 2 == 0 {
                set(&mut texture, x, z, [255; 4]);
            }
        }
    }
    let viewport =
        CloudViewport::try_new([0.0; 2], CLOUD_MASK_SIZE as u16, 1, false, true).unwrap();
    let quads = mesh_cloud_viewport(&texture, viewport).unwrap();
    assert_eq!(quads.len(), MAX_CLOUD_QUADS);
    assert!(quads.capacity() <= MAX_VIEWPORT_CLOUD_QUADS);
    assert!(quads.len() * size_of::<ViewportCloudQuad>() <= MAX_VIEWPORT_CLOUD_BYTES);
    assert!(quads.iter().all(|quad| quad.face <= CloudFace::East as u32));
}

#[test]
fn finite_closed_edges_are_admitted_even_when_the_periodic_quad_bound_is_exceeded() {
    let mut texture = texture();
    for z in 0..CLOUD_MASK_SIZE as usize {
        for x in 0..CLOUD_MASK_SIZE as usize {
            if (x + z) % 2 == 0 {
                set(&mut texture, x, z, [255; 4]);
            }
        }
    }
    let half = CLOUD_MASK_SIZE as usize / 2;
    set(&mut texture, half, half - 1, [255; 4]);
    set(&mut texture, half - 1, half, [255; 4]);
    let viewport =
        CloudViewport::try_new([0.0; 2], CLOUD_MASK_SIZE as u16, 1, false, true).unwrap();
    let quads = mesh_cloud_viewport(&texture, viewport).unwrap();
    assert_eq!(quads.len(), MAX_CLOUD_QUADS + 4);
    assert!(quads.len() <= MAX_VIEWPORT_CLOUD_QUADS);
    assert!(quads.capacity() <= MAX_VIEWPORT_CLOUD_QUADS);
}

#[test]
fn solid_window_closes_its_outer_edges_without_sampling_beyond_the_admitted_grid() {
    let mut texture = texture();
    for texel in texture.rgba8.as_chunks_mut::<4>().0 {
        texel.copy_from_slice(&[255; 4]);
    }
    let quads = mesh_cloud_viewport(&texture, window([0.0; 2], false, false)).unwrap();
    assert_eq!(quads.len(), 4 + 8);
    assert_eq!(
        quads
            .iter()
            .filter(|quad| quad.face == CloudFace::Down as u32)
            .count(),
        4
    );
    assert_eq!(
        quads
            .iter()
            .filter(|quad| quad.face > CloudFace::Up as u32)
            .count(),
        8
    );
}
