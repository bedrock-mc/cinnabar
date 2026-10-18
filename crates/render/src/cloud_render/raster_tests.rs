use super::*;
use meshing::cloud_viewport::mesh_cloud_viewport;

use crate::gpu_snapshot::{self, Draw, Gpu, RasterState, SNAPSHOT_SIDE};

/// Draws the production shader using either original or retained cloud records.
fn draw(gpu: &Gpu, records: &wgpu::Buffer, count: u32, eye: Vec3) -> Vec<u8> {
    let focus = Vec3::new(24.0, (CLOUD_UNDERSIDE_Y + CLOUD_TOP_Y) * 0.5, -24.0);
    let matrix = glam::camera::rh::proj::directx::perspective_infinite_reverse(1.2, 1.0, 0.1)
        * glam::camera::rh::view::look_at_mat4(eye, focus, Vec3::Y);
    let view = gpu.buffer(
        &gpu_snapshot::view(matrix, eye),
        wgpu::BufferUsages::UNIFORM,
    );
    let mut atmosphere = crate::gpu_snapshot::atmosphere();
    atmosphere[23] = 160.0;
    let frame = gpu.buffer(&atmosphere, wgpu::BufferUsages::UNIFORM);
    let colour = gpu.buffer(
        &crate::gpu_snapshot::cloud_uniform(),
        wgpu::BufferUsages::UNIFORM,
    );
    let bindings = crate::gpu_snapshot::cloud_bindings(&view, &frame, records, &colour);
    let source = crate::shader_source::standalone(
        &meshing::cloud_viewport::shader_source(include_str!("../cloud.wesl")),
        &[],
    );
    gpu.render_with_state(
        &source,
        "cloud_vertex",
        &[Draw {
            fragment: "cloud_fragment",
            vertices: 0..count.checked_mul(6).unwrap(),
            bindings: &bindings,
            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
            write_depth: false,
        }],
        RasterState {
            primitive: wgpu::PrimitiveState {
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_compare: wgpu::CompareFunction::Greater,
            write_mask: wgpu::ColorWrites::RED | wgpu::ColorWrites::GREEN | wgpu::ColorWrites::BLUE,
            ..Default::default()
        },
    )
}

#[test]
fn retained_worker_windows_match_original_pixels_above_inside_and_below() {
    let Some(gpu) = Gpu::for_fixture("cloud retained worker windows") else {
        return;
    };
    let mut world = world();
    world.insert_resource(RenderDevice::from(gpu.device.clone()));
    world.insert_resource(RenderQueue::new(gpu.queue.clone()));
    world.run_system_once(init_cloud_gpu).unwrap();
    let runtime = runtime_with_cloud_mask(255, true);
    world.insert_resource(AtmosphereTextureAssets::new(
        Arc::clone(&runtime),
        [0x41; 32],
    ));
    let view = spawn_view(&mut world, Vec3::ZERO);
    let captures = env_compat::var_os("CINNABAR_CLOUD_CAPTURE_DIR").map(std::path::PathBuf::from);
    for (label, y) in [
        ("below", CLOUD_UNDERSIDE_Y - 12.0),
        ("inside", (CLOUD_UNDERSIDE_Y + CLOUD_TOP_Y) * 0.5),
        ("above", CLOUD_TOP_Y + 12.0),
    ] {
        let eye = Vec3::new(-24.125, y, -24.125);
        world
            .get_mut::<ExtractedView>(view)
            .unwrap()
            .world_from_view = GlobalTransform::from_translation(eye);
        world.run_system_once(prepare_cloud_records).unwrap();
        let prepared = &world.resource::<CloudGpu>().views[&view];
        let records = mesh_cloud_viewport(
            runtime.texture(AtmosphereRole::Clouds).unwrap(),
            prepared.viewport,
        )
        .unwrap();
        let original = gpu.words(bytemuck::cast_slice(&records), wgpu::BufferUsages::STORAGE);
        let before = draw(&gpu, &original, records.len() as u32, eye);
        let after = draw(
            &gpu,
            prepared.record_buffer.as_ref().unwrap(),
            prepared.record_count,
            eye,
        );
        assert_eq!(before, after, "{label}: cloud pixels changed");
        assert!(
            after
                .as_chunks::<4>()
                .0
                .windows(2)
                .any(|pair| pair[0] != pair[1]),
            "{label}: fixture must draw cloud edges"
        );
        if let Some(directory) = &captures {
            std::fs::create_dir_all(directory).unwrap();
            for (version, pixels) in [("before", before), ("after", after)] {
                image::save_buffer(
                    directory.join(format!("cloud-{label}-{version}.png")),
                    &pixels,
                    SNAPSHOT_SIDE,
                    SNAPSHOT_SIDE,
                    image::ColorType::Rgba8,
                )
                .unwrap();
            }
        }
    }
}
