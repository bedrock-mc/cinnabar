use crate as render_api;
use crate::{gpu_snapshot, shader_source};
use bevy::math::{Mat4, Vec3, Vec4};

#[path = "../../tests/it/support/actor_raster.rs"]
mod actor_raster;

const CHART_TEXTURE_SIDE: u32 = 48;
const CHART_START_X: [u32; 2] = [18, 4];
const CHART_START_Y: u32 = 21;
const CHART_SIZE: [u32; 2] = [12, 13];

/// Places a transparent face next to unrelated opaque artwork inside one texture.
fn transparent_region_plane() -> Vec<render_model::ActorRigVertex> {
    let mut vertices = actor_raster::cube([16, 0, 16], true, false);
    for vertex in &mut vertices {
        // A fractional screen edge exercises coverage where the face starts between samples.
        vertex.position[0] = vertex.position[0] * 0.75 + 0.00137;
        vertex.uv[0] += 0.5;
        vertex.back_uv[0] += 0.5;
    }
    vertices
}

#[test]
fn actor_cutout_does_not_draw_artwork_outside_a_transparent_face_region() {
    let Some(gpu) = gpu_snapshot::Gpu::for_fixture(
        "actor_cutout_does_not_draw_artwork_outside_a_transparent_face_region",
    ) else {
        return;
    };
    let plane = transparent_region_plane();
    let material = render_model::ActorMaterial {
        state: Some(assets::EntityRenderMaterialState {
            alpha_test: true,
            cull: false,
            depth_write: true,
            ..Default::default()
        }),
        ..Default::default()
    };
    for below in [false, true] {
        let raster = |texels| {
            actor_raster::raster_material_with_state(
                &gpu,
                &plane,
                material,
                below,
                texels,
                false,
                false,
                render_model::pack_actor_light_without_lightmap(),
                0,
                0,
                &[super::SHADER_DEF],
                gpu_snapshot::RasterState {
                    multisample: wgpu::MultisampleState {
                        count: 4,
                        alpha_to_coverage_enabled: true,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
        };
        let background = raster([[0; 4]; 2]);
        let opaque = raster([[0; 4], [0, 255, 0, 255]]);
        let center = (gpu_snapshot::SNAPSHOT_SIDE as usize / 2
            * gpu_snapshot::SNAPSHOT_SIDE as usize
            + gpu_snapshot::SNAPSHOT_SIDE as usize / 2)
            * 4;
        assert!(opaque[center + 1] > background[center + 1] + 50);
        let unrelated_artwork = raster([[255, 0, 0, 255], [0; 4]]);
        assert_eq!(
            unrelated_artwork, background,
            "opaque texels outside a fully transparent face must not become its coverage; below={below}"
        );
    }
}

/// Gives two opposing faces independent interior charts in a non-power-of-two texture.
fn chart_plane() -> Vec<render_model::ActorRigVertex> {
    let mut vertices = actor_raster::cube([16, 0, 16], true, true);
    for vertex in &mut vertices {
        let side = usize::from(vertex.normal[1] < 0.0);
        let local_u = vertex.uv[0] * 2.0 - side as f32;
        vertex.uv = [
            (CHART_START_X[side] as f32 + local_u * CHART_SIZE[0] as f32)
                / CHART_TEXTURE_SIDE as f32,
            (CHART_START_Y as f32 + vertex.uv[1] * CHART_SIZE[1] as f32)
                / CHART_TEXTURE_SIDE as f32,
        ];
        vertex.back_uv = vertex.uv;
    }
    vertices
}

/// Paints original charts without borrowing artwork from a resource pack.
fn chart_texture(interior: [u8; 4], exterior: [u8; 4]) -> Vec<[u8; 4]> {
    let mut texels = vec![exterior; (CHART_TEXTURE_SIDE * CHART_TEXTURE_SIDE) as usize];
    for start_x in CHART_START_X {
        for y in CHART_START_Y..CHART_START_Y + CHART_SIZE[1] {
            for x in start_x..start_x + CHART_SIZE[0] {
                texels[(y * CHART_TEXTURE_SIDE + x) as usize] = interior;
            }
        }
    }
    texels
}

/// Saves original GPU controls and subject images only when a capture directory is requested.
fn capture_chart(name: &str, pixels: &[u8]) {
    let Some(directory) = std::env::var_os("CINNABAR_ACTOR_CHART_SNAPSHOT_DIR") else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    std::fs::create_dir_all(&directory).unwrap();
    image::save_buffer(
        directory.join(format!("{name}.png")),
        pixels,
        gpu_snapshot::SNAPSHOT_SIDE,
        gpu_snapshot::SNAPSHOT_SIDE,
        image::ColorType::Rgba8,
    )
    .unwrap();
}

/// Draws chart coverage with a shared camera and an illumination-independent control.
fn raster_chart(
    gpu: &gpu_snapshot::Gpu,
    plane: &[render_model::ActorRigVertex],
    below: bool,
    phase: f32,
    samples: u32,
    coverage: bool,
    texels: &[[u8; 4]],
) -> Vec<u8> {
    let material = render_model::ActorMaterial {
        state: Some(assets::EntityRenderMaterialState {
            alpha_test: true,
            cull: true,
            depth_write: true,
            ..Default::default()
        }),
        ..Default::default()
    };
    let sign = if below { -1.0 } else { 1.0 };
    let clip = Mat4::from_cols(
        Vec4::new(0.75, 0.16, 0.0, 0.35),
        Vec4::ZERO,
        Vec4::new(-0.08 * sign, -0.65 * sign, 0.0, 0.2),
        Vec4::new(
            phase * 2.0 / gpu_snapshot::SNAPSHOT_SIDE as f32,
            phase * 2.0 / gpu_snapshot::SNAPSHOT_SIDE as f32,
            0.5,
            1.0,
        ),
    );
    actor_raster::raster_material_in_scene(
        gpu,
        plane,
        material,
        actor_raster::RasterScene {
            clip_from_world: clip,
            world_from_actor: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            camera_position: Vec3::ZERO,
            texture_size: [CHART_TEXTURE_SIDE; 2],
            texels,
        },
        true,
        false,
        0,
        0,
        0,
        if coverage { &[super::SHADER_DEF] } else { &[] },
        gpu_snapshot::RasterState {
            multisample: wgpu::MultisampleState {
                count: samples,
                alpha_to_coverage_enabled: coverage,
                ..Default::default()
            },
            ..Default::default()
        },
    )
}

#[test]
fn actor_transparent_charts_remain_empty_at_perspective_multisample_edges() {
    let Some(gpu) = gpu_snapshot::Gpu::for_fixture(
        "actor_transparent_charts_remain_empty_at_perspective_multisample_edges",
    ) else {
        return;
    };
    let plane = chart_plane();

    let blank = chart_texture([0; 4], [0; 4]);
    let opaque = chart_texture([0, 255, 0, 255], [0; 4]);
    let unrelated = chart_texture([0; 4], [255, 0, 0, 255]);
    let mut failures = Vec::new();
    for below in [false, true] {
        for phase in [0.0, 0.13, 0.37, 0.61, 0.87] {
            for (samples, coverage) in [(1, false), (4, false), (4, true)] {
                let raster = |texels: &[[u8; 4]]| {
                    raster_chart(&gpu, &plane, below, phase, samples, coverage, texels)
                };
                let background = raster(&blank);
                let positive = raster(&opaque);
                let positive_pixels = positive
                    .chunks_exact(4)
                    .zip(background.chunks_exact(4))
                    .filter(|(pixel, clear)| pixel[1] > clear[1] + 50)
                    .count();
                assert!(
                    positive_pixels > 2_000,
                    "the selected face must actually draw: below={below} phase={phase} samples={samples} coverage={coverage} positive_pixels={positive_pixels}"
                );
                let subject = raster(&unrelated);
                if !below && phase == 0.13 {
                    let case = format!("samples-{samples}-coverage-{coverage}");
                    capture_chart(&format!("{case}-background"), &background);
                    capture_chart(&format!("{case}-opaque"), &positive);
                    capture_chart(&format!("{case}-subject"), &subject);
                }
                let changed_pixels = subject
                    .chunks_exact(4)
                    .zip(background.chunks_exact(4))
                    .filter(|(pixel, clear)| pixel != clear)
                    .count();
                eprintln!(
                    "below={below} phase={phase} samples={samples} coverage={coverage}: \
                     positive_pixels={positive_pixels} outside_chart_pixels={changed_pixels}"
                );
                if changed_pixels != 0 {
                    failures.push((below, phase, samples, coverage, changed_pixels));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "unrelated artwork must not appear on transparent charts: {failures:?}"
    );
}

#[test]
fn actor_coverage_preserves_transparent_side_of_interior_contours() {
    let Some(gpu) = gpu_snapshot::Gpu::for_fixture(
        "actor_coverage_preserves_transparent_side_of_interior_contours",
    ) else {
        return;
    };
    let plane = chart_plane();
    let mut contour = chart_texture([0; 4], [0; 4]);
    for start in CHART_START_X {
        for y in CHART_START_Y..CHART_START_Y + CHART_SIZE[1] {
            for x in start + CHART_SIZE[0] / 2..start + CHART_SIZE[0] {
                contour[(y * CHART_TEXTURE_SIDE + x) as usize] = [0, 255, 0, 255];
            }
        }
    }
    let blank = chart_texture([0; 4], [0; 4]);
    for below in [false, true] {
        let raster = |pixels: &[[u8; 4]], coverage| {
            raster_chart(&gpu, &plane, below, 0.13, 4, coverage, pixels)
        };
        let clear = raster(&blank, false);
        let hard = raster(&contour, false);
        let smooth = raster(&contour, true);
        let opaque_pixels = hard
            .chunks_exact(4)
            .zip(clear.chunks_exact(4))
            .filter(|(pixel, background)| pixel[1] > background[1] + 50)
            .count();
        assert!(
            opaque_pixels > 2_000,
            "interior contour must draw its opaque half"
        );
        let recovered = hard
            .chunks_exact(4)
            .zip(smooth.chunks_exact(4))
            .zip(clear.chunks_exact(4))
            .filter(|((hard, smooth), clear)| *hard == *clear && *smooth != *clear)
            .count();
        eprintln!(
            "below={below} opaque_pixels={opaque_pixels} recovered_contour_pixels={recovered}"
        );
        capture_chart(&format!("interior-{below}-hard"), &hard);
        capture_chart(&format!("interior-{below}-smooth"), &smooth);
        assert!(
            recovered > 10,
            "valid coverage must extend across an interior alpha contour"
        );
    }
}
