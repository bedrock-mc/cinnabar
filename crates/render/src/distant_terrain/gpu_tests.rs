//! The renderer's shaders validate, and on a native adapter its production kernels and
//! pipelines draw uploaded sections, keep them through the two-phase passes, cull resident
//! columns and draw water.

use bevy::math::Vec3;
use lod::{
    draw::NeighbourLevels, fixtures::blocks, geometry::SectionPos, select::shown::DrawnSection,
};
use render_api::DistantTerrainTier;
use render_scene::distant_terrain::ResidentColumnMask;

use super::{
    gpu::{cull_source, draw_source, scatter_source},
    gpu_fixture::{EYE, Fixture, STONE, close, section, target, update},
    test_meshes,
};

#[test]
fn every_shader_validates() {
    for (name, source) in [
        ("scatter", scatter_source()),
        ("cull", cull_source()),
        ("draw", draw_source()),
    ] {
        let module = naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|error| panic!("{name}: {}", error.emit_to_string(&source)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap_or_else(|error| panic!("{name}: {error:?}"));
    }
}

#[test]
fn devices_need_every_storage_binding_of_the_busiest_stage() {
    use wgpu::DownlevelFlags as D;
    let flags = D::COMPUTE_SHADERS | D::INDIRECT_EXECUTION | D::VERTEX_STORAGE;
    let needed = super::gpu::storage_buffers_per_stage();
    let limits = |storage| wgpu::Limits {
        max_storage_buffers_per_shader_stage: storage,
        ..wgpu::Limits::default()
    };
    let tier = DistantTerrainTier::CURRENT;
    assert!(super::supported(flags, &limits(needed), tier));
    assert!(!super::supported(flags, &limits(needed - 1), tier));
}

#[test]
fn software_virtual_and_small_adapters_get_the_low_end_tier() {
    let build = DistantTerrainTier::CURRENT;
    let roomy = wgpu::Limits {
        max_storage_buffer_binding_size: 1 << 31,
        max_buffer_size: 1 << 31,
        ..wgpu::Limits::default()
    };
    assert_eq!(
        super::device_tier(wgpu::DeviceType::DiscreteGpu, &roomy),
        build
    );
    assert_eq!(
        super::device_tier(wgpu::DeviceType::IntegratedGpu, &roomy),
        build
    );
    for software in [wgpu::DeviceType::Cpu, wgpu::DeviceType::VirtualGpu] {
        assert_eq!(
            super::device_tier(software, &roomy),
            DistantTerrainTier::LOW_END
        );
    }
    let small = wgpu::Limits {
        max_storage_buffer_binding_size: 128 << 20,
        ..roomy
    };
    assert_eq!(
        super::device_tier(wgpu::DeviceType::DiscreteGpu, &small),
        DistantTerrainTier::LOW_END
    );
}

#[test]
fn cluster_indices_draw_two_triangles_per_quad_slot() {
    let indices = super::gpu::cluster_indices();
    assert_eq!(indices.len(), super::layout::INDEX_COUNT as usize);
    assert_eq!(&indices[..6], &[0, 1, 2, 0, 2, 3]);
    assert_eq!(indices[indices.len() - 1], 255);
}

#[test]
fn uploaded_sections_draw_in_pass_b_then_pass_a() {
    let Some(mut fixture) = Fixture::new("distant terrain two-phase draw") else {
        return;
    };
    let origin = SectionPos::new(0, 0, 0, 0).expect("origin");
    let first = fixture.frame(
        Some(update(vec![section(origin, 4, 0)])),
        EYE,
        target(),
        None,
    );
    let [visible, pass_b, translucent, overflow] = first.counters;
    assert!(visible > 0, "the ground's facing clusters are visible");
    assert_eq!(pass_b, visible, "nothing was visible last frame");
    assert_eq!((translucent, overflow), (0, 0));
    assert!(close(first.centre(), STONE), "{:?}", first.centre());

    let second = fixture.frame(None, EYE, target(), None);
    assert_eq!(second.counters[0], visible);
    assert_eq!(second.counters[1], 0, "pass A drew last frame's clusters");
    assert!(close(second.centre(), STONE), "{:?}", second.centre());

    // Looking away culls every cluster; nothing draws, and no garbage either.
    let away = fixture.frame(None, EYE, Vec3::new(16.0, 20.0, -100.0), None);
    assert_eq!(away.counters[0], 0);
    assert!(away.pixels.chunks(4).all(|pixel| pixel[..3] == [0, 0, 0]));
}

#[test]
fn level_zero_clusters_over_resident_columns_are_culled() {
    let Some(mut fixture) = Fixture::new("distant terrain resident columns") else {
        return;
    };
    let origin = SectionPos::new(0, 0, 0, 0).expect("origin");
    let resident = ResidentColumnMask::new([0, 0], [[0, 0], [1, 0], [0, 1], [1, 1]]);
    let frame = fixture.frame(
        Some(update(vec![section(origin, 4, 0)])),
        EYE,
        target(),
        Some(&resident),
    );
    assert_eq!(frame.counters[0], 0, "every column is resident");
    assert_eq!(frame.centre(), [0, 0, 0]);
    let partial = ResidentColumnMask::new([0, 0], [[0, 0]]);
    let frame = fixture.frame(None, EYE, target(), Some(&partial));
    assert!(
        frame.counters[0] > 0,
        "clusters over other columns still draw"
    );
}

#[test]
fn water_draws_blended_after_the_ground() {
    let Some(mut fixture) = Fixture::new("distant terrain water") else {
        return;
    };
    let origin = SectionPos::new(0, 0, 0, 0).expect("origin");
    let frame = fixture.frame(
        Some(update(vec![section(origin, 4, 8)])),
        EYE,
        target(),
        None,
    );
    assert!(frame.counters[2] > 0, "the water surface is visible");
    let [red, _, blue] = frame.centre();
    assert!(blue > red, "{:?}", frame.centre());
}

#[test]
fn hidden_sections_drop_out_after_the_frame_that_draws_their_occluder() {
    let Some(mut fixture) = Fixture::new("distant terrain hi-z occlusion") else {
        return;
    };
    fixture.enable_hiz();
    let solid = |pos| DrawnSection {
        pos,
        mesh: test_meshes::mesh(pos, |_, _, _| Some(blocks::STONE)),
        neighbours: NeighbourLevels::NONE,
    };
    let near = SectionPos::new(0, 0, 0, 0).expect("near");
    let far = SectionPos::new(0, 0, 0, 3).expect("far");
    let eye = [16, 16, -10];
    let target = Vec3::new(16.0, 16.0, 100.0);
    // Nothing was drawn before: the pyramid is empty, so both walls are visible.
    let first = fixture.frame(
        Some(update(vec![solid(near), solid(far)])),
        eye,
        target,
        None,
    );
    let both = first.counters[0];
    assert!(
        both >= 2,
        "both walls face the camera: {:?}",
        first.counters
    );
    assert_eq!(first.counters[1], both);
    // Pass A draws both; the pyramid then hides the far wall behind the near one.
    let second = fixture.frame(None, eye, target, None);
    assert!(second.counters[0] < both, "{:?}", second.counters);
    let third = fixture.frame(None, eye, target, None);
    assert_eq!(
        third.counters[0], second.counters[0],
        "only the near wall stays listed"
    );
    assert_eq!(third.counters[1], 0);
    // The near wall's side face, shaded darker than a top face.
    let [red, green, blue] = third.centre();
    assert!(
        red < STONE[0] && red > green && green > blue,
        "{:?}",
        third.centre()
    );
}

#[test]
fn clusters_spanning_planes_draw_while_the_eye_is_between_them() {
    let Some(mut fixture) = Fixture::new("distant terrain stepped backface cull") else {
        return;
    };
    // Steps rising along +x: the top faces lie on planes y = 1..=8 in one cluster.
    let origin = SectionPos::new(0, 0, 0, 0).expect("origin");
    let steps = DrawnSection {
        pos: origin,
        mesh: test_meshes::mesh(origin, |x, y, _| (y <= x / 4).then_some(blocks::STONE)),
        neighbours: NeighbourLevels::NONE,
    };
    // Below the highest step's top, looking down onto the lowest step's top.
    let frame = fixture.frame(
        Some(update(vec![steps])),
        [-10, 5, 16],
        Vec3::new(2.0, 1.0, 16.0),
        None,
    );
    assert!(close(frame.centre(), STONE), "{:?}", frame.centre());
}
