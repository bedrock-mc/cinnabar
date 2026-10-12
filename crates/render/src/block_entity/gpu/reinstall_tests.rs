use super::*;
use bevy::ecs::system::RunSystemOnce;
use render_model::{BlockEntityScene, MAP_CELL, SceneClock};

#[path = "../../../tests/it/support/gpu_snapshot.rs"]
pub(super) mod gpu_snapshot;

/// Makes an original map canvas with a colored background and a session marker.
fn canvas(second: bool) -> Vec<u8> {
    let color = if second {
        [32, 160, 64, 255]
    } else {
        [192, 48, 32, 255]
    };
    let [width, height] = MAP_CELL.map(|side| side as usize);
    let mut pixels = color.repeat(width * height);
    for y in 32..96 {
        for x in 32..96 {
            let ink = if second {
                x < 40 || y < 40 || (60..68).contains(&y) || y >= 88 || x >= 88
            } else {
                x < 40 || x >= 88 || y < 40 || (60..68).contains(&y)
            };
            if ink {
                let offset = (y * width + x) * 4;
                pixels[offset..offset + 4].copy_from_slice(&[255; 4]);
            }
        }
    }
    pixels
}

/// Previews the uploaded map cell through a native offscreen draw.
fn preview(gpu: &gpu_snapshot::Gpu, world: &World) -> Vec<u8> {
    let view = world.resource::<BlockEntityGpu>().view.as_ref().unwrap();
    let shader = r#"
@group(0) @binding(0) var atlas: texture_2d<f32>;
@vertex fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corners = array(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    return vec4(corners[index], 0.5, 1.0);
}
@fragment fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(position.xy / 2.0) + vec2(0, 1);
    return textureLoad(atlas, pixel, 0);
}
"#;
    gpu.render_srgb(
        shader,
        "vertex",
        &[gpu_snapshot::Draw {
            fragment: "fragment",
            vertices: 0..3,
            bindings: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            }],
            blend: None,
            write_depth: false,
        }],
    )
}

#[test]
fn identical_atlas_reinstall_uploads_the_new_session_canvas_on_the_gpu() {
    let Some(gpu) = gpu_snapshot::Gpu::for_fixture("block-entity reinstall upload") else {
        return;
    };
    let bytes =
        assets::encode_block_entity_catalog(b"{}", 1024, 1, &vec![255; 1024 * 4], &[]).unwrap();
    let assets = assets::RuntimeBlockEntityAssets::decode(&bytes).unwrap();
    let mut scene = BlockEntityScene::default();
    let mut world = World::new();
    world.insert_resource(RenderDevice::from(gpu.device.clone()));
    world.insert_resource(RenderQueue::new(gpu.queue.clone()));
    world.init_resource::<render_scene::BlockEntityFrameResource>();
    world.init_resource::<render_scene::BlockSelectionFrameResource>();
    world.run_system_once(init_gpu).unwrap();
    let mut system = IntoSystem::into_system(prepare_resources);
    system.initialize(&mut world);

    scene.install_assets(&assets);
    scene.map_rect(1, || canvas(false)).unwrap();
    world.insert_resource(render_scene::BlockEntityFrameResource(
        scene.update(SceneClock::default(), &[], &[]).clone(),
    ));
    system.run((), &mut world).unwrap();
    let first = preview(&gpu, &world);
    let texture = world
        .resource::<BlockEntityGpu>()
        .texture
        .as_ref()
        .unwrap()
        .id();

    scene.install_assets(&assets);
    scene.map_rect(1, || canvas(true)).unwrap();
    world.insert_resource(render_scene::BlockEntityFrameResource(
        scene.update(SceneClock::default(), &[], &[]).clone(),
    ));
    system.run((), &mut world).unwrap();
    assert_eq!(
        world
            .resource::<BlockEntityGpu>()
            .texture
            .as_ref()
            .unwrap()
            .id(),
        texture
    );
    let second = preview(&gpu, &world);
    gpu_snapshot::save("reinstall-session-canvas", &second);
    assert!(
        first != second,
        "the reused GPU atlas must display the new session's map"
    );
    assert_eq!(&second[..4], &[32, 160, 64, 255]);
    system.run((), &mut world).unwrap();
    assert_eq!(preview(&gpu, &world), second);
}
