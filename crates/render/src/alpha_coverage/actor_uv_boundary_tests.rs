use crate as render_api;
use crate::{gpu_snapshot, shader_source};

#[path = "../../tests/it/support/actor_raster.rs"]
mod actor_raster;

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
