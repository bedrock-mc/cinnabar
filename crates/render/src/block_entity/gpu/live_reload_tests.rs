use super::reinstall_tests::gpu_snapshot;
use super::*;
use bevy::ecs::system::RunSystemOnce;
use render_model::{BlockEntityScene, SceneClock, TEXT_CELL};

const SIGN_COUNT: usize = 32;
const PREVIEW_COLUMNS: u32 = 8;

/// Creates original static artwork; a taller replacement moves every sign's atlas rect.
fn artwork(height: u32, shade: u8) -> assets::RuntimeBlockEntityAssets {
    let bytes = assets::encode_block_entity_catalog(
        b"{}",
        1024,
        height,
        &vec![shade; 1024 * height as usize * 4],
        &[],
    )
    .unwrap();
    assets::RuntimeBlockEntityAssets::decode(&bytes).unwrap()
}

/// Creates one transparent sign canvas with original bars distinct from the static artwork.
fn canvas(key: usize) -> Vec<u8> {
    let [width, height] = TEXT_CELL.map(|v| v as usize);
    let mut pixels = vec![0; width * height * 4];
    for y in 8..height - 8 {
        for x in 8..width - 8 {
            if (x / 8 + y / 8 + key) % 3 == 0 {
                pixels[(y * width + x) * 4..(y * width + x + 1) * 4]
                    .copy_from_slice(&[220, 240, 255, 255]);
            }
        }
    }
    pixels
}

/// Previews 32 sign cells from the production GPU atlas after its upload system runs.
fn preview(gpu: &gpu_snapshot::Gpu, world: &World, scene: &BlockEntityScene) -> Vec<u8> {
    let view = world.resource::<BlockEntityGpu>().view.as_ref().unwrap();
    let rects: Vec<_> = (0..SIGN_COUNT)
        .flat_map(|slot| {
            let rect = scene.atlas().unwrap().text_cell(slot).unwrap();
            [rect.x, rect.y, rect.width, rect.height]
        })
        .collect();
    let rects = gpu.buffer(&rects, wgpu::BufferUsages::UNIFORM);
    let shader = format!(
        r#"
@group(0) @binding(0) var atlas: texture_2d<f32>;
@group(0) @binding(1) var<uniform> rects: array<vec4<f32>, {count}>;
@vertex fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {{
    let corners = array(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    return vec4(corners[index], 0.5, 1.0);
}}
@fragment fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {{
    let cell = vec2({preview_width}u, {preview_height}u);
    let slot = u32(position.x) / cell.x + u32(position.y) / cell.y * {columns}u;
    let rect = rects[slot];
    let pixel = (vec2<u32>(position.xy) % cell) * vec2<u32>(rect.zw) / cell;
    return textureLoad(atlas, vec2<i32>(rect.xy) + vec2<i32>(pixel), 0);
}}
"#,
        count = SIGN_COUNT,
        columns = PREVIEW_COLUMNS,
        preview_width = gpu_snapshot::SNAPSHOT_SIDE / PREVIEW_COLUMNS,
        preview_height = gpu_snapshot::SNAPSHOT_SIDE / (SIGN_COUNT as u32 / PREVIEW_COLUMNS)
    );
    gpu.render_srgb(
        &shader,
        "vertex",
        &[gpu_snapshot::Draw {
            fragment: "fragment",
            vertices: 0..3,
            bindings: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: rects.as_entire_binding(),
                },
            ],
            blend: None,
            write_depth: false,
        }],
    )
}

#[test]
fn live_artwork_reload_uploads_all_32_cached_sign_canvases_without_a_blank_frame() {
    let Some(gpu) = gpu_snapshot::Gpu::for_fixture("32 cached sign canvases after artwork reload")
    else {
        return;
    };
    eprintln!("sign reload snapshot backend: {:?}", gpu.backend);
    let mut scene = BlockEntityScene::default();
    scene.install_assets(&artwork(64, 64));
    let mut world = World::new();
    world.insert_resource(RenderDevice::from(gpu.device.clone()));
    world.insert_resource(RenderQueue::new(gpu.queue.clone()));
    world.init_resource::<render_scene::BlockEntityFrameResource>();
    world.init_resource::<render_scene::BlockSelectionFrameResource>();
    world.run_system_once(init_gpu).unwrap();
    let mut system = IntoSystem::into_system(prepare_resources);
    system.initialize(&mut world);
    for _ in 0..2 {
        for key in 0..SIGN_COUNT {
            let _ = scene.text_rect(key as u64, || canvas(key));
        }
        scene.update(SceneClock::default(), &[], &[]);
    }
    world.insert_resource(render_scene::BlockEntityFrameResource(
        scene.frame().clone(),
    ));
    system.run((), &mut world).unwrap();
    let before = preview(&gpu, &world, &scene);
    assert!(before.chunks_exact(4).filter(|pixel| pixel[0] > 0).count() > 1_000);
    scene.reload_assets(&artwork(512, 128));
    for tick in 0..3 {
        for key in 0..SIGN_COUNT {
            let _ = scene.text_rect(key as u64, || canvas(key));
        }
        world.insert_resource(render_scene::BlockEntityFrameResource(
            scene
                .update(
                    SceneClock {
                        ticks: f64::from(tick),
                    },
                    &[],
                    &[],
                )
                .clone(),
        ));
        system.run((), &mut world).unwrap();
        let after = preview(&gpu, &world, &scene);
        if tick == 0 {
            gpu_snapshot::save("sign-reload-before", &before);
            gpu_snapshot::save("sign-reload-after", &after);
        }
        assert!(
            after == before,
            "every sign remains visible on frame {tick}"
        );
    }
}
