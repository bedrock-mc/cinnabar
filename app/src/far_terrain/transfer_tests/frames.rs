//! Renders every frame of a scripted dimension round trip with the production far pass.

use super::*;
use bevy::{
    asset::{AssetPlugin, Assets, Handle},
    camera::{CameraPlugin, RenderTarget},
    core_pipeline::{CorePipelinePlugin, tonemapping::Tonemapping},
    image::{Image, ImagePlugin},
    mesh::MeshPlugin,
    prelude::*,
    render::{
        RenderApp, RenderPlugin,
        render_asset::RenderAssets,
        renderer::{RenderAdapter, RenderAdapterInfo, RenderDevice, RenderInstance, RenderQueue},
        settings::RenderCreation,
        texture::GpuImage,
    },
    window::WindowPlugin,
};

const WIDTH: u32 = 256;
const HEIGHT: u32 = 192;

struct Capture {
    app: App,
    target: Handle<Image>,
    device: RenderDevice,
    queue: RenderQueue,
    rendered: Option<(u64, u64)>,
}

impl Capture {
    /// Creates a display-free native renderer, skipping only when no adapter exists.
    #[allow(clippy::print_stderr, reason = "names the missing GPU fixture")]
    fn new() -> Option<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = match bevy::tasks::block_on(
            instance.request_adapter(&wgpu::RequestAdapterOptions::default()),
        ) {
            Ok(adapter) => adapter,
            Err(wgpu::RequestAdapterError::NotFound { .. }) => {
                eprintln!("skipping transfer frames: missing native GPU adapter fixture");
                return None;
            }
            Err(error) => panic!("transfer adapter: {error}"),
        };
        let info = adapter.get_info();
        let (device, queue) =
            bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                required_limits: adapter.limits(),
                ..Default::default()
            }))
            .unwrap();
        let device = RenderDevice::from(device);
        let queue = RenderQueue::new(queue);
        let mut app = far_terrain_app(64);
        app.add_plugins(MinimalPlugins)
            .add_plugins(WindowPlugin {
                primary_window: None,
                ..Default::default()
            })
            .add_plugins((AssetPlugin::default(), TransformPlugin))
            .add_plugins(RenderPlugin {
                render_creation: RenderCreation::manual(
                    device.clone(),
                    queue.clone(),
                    RenderAdapterInfo::new(info),
                    RenderAdapter::new(adapter),
                    RenderInstance::new(instance),
                ),
                synchronous_pipeline_compilation: true,
                ..Default::default()
            })
            .add_plugins((
                ImagePlugin::default(),
                MeshPlugin,
                CameraPlugin,
                CorePipelinePlugin,
            ))
            .add_plugins((
                render::ChunkRenderPlugin::default(),
                render::FarTerrainRenderPlugin,
                render::PanoramaRenderPlugin,
            ));
        app.insert_resource(bevy::render::error_handler::RenderErrorHandler(
            |error, _, _| panic!("transfer rendering failed: {error:?}"),
        ));
        let mut image =
            Image::new_target_texture(WIDTH, HEIGHT, wgpu::TextureFormat::Rgba8Unorm, None);
        image.texture_descriptor.usage |= wgpu::TextureUsages::COPY_SRC;
        let target = app.world_mut().resource_mut::<Assets<Image>>().add(image);
        app.world_mut().spawn((
            Camera3d::default(),
            Camera::default(),
            RenderTarget::Image(target.clone().into()),
            Msaa::Off,
            Tonemapping::Linear,
            Transform::from_xyz(530.0, 120.0, -90.0)
                .looking_at(Vec3::new(500.0, 75.0, 24.0), Vec3::Y),
        ));
        app.finish();
        app.cleanup();
        Some(Self {
            app,
            target,
            device,
            queue,
            rendered: None,
        })
    }

    /// Completes and reads this exact render frame before the next update can run.
    fn frame(&mut self) -> Vec<u8> {
        let generation = self
            .app
            .world()
            .resource::<ClientWorld>()
            .stream
            .as_ref()
            .map(WorldStream::terrain_generation);
        let acknowledgements = self
            .app
            .world()
            .resource::<render_scene::ChunkUploadAcknowledgements>()
            .clone();
        crate::runtime::world::synchronize_terrain_generation(
            &mut self.rendered,
            generation,
            &mut self
                .app
                .world_mut()
                .resource_mut::<render_scene::ChunkRenderQueue>(),
            &acknowledgements,
        );
        self.app.update();
        let images = self
            .app
            .sub_app(RenderApp)
            .world()
            .resource::<RenderAssets<GpuImage>>();
        let image = images.get(&self.target).expect("prepared capture target");
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("transfer frame readback"),
            size: u64::from(WIDTH * HEIGHT * 4),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            image.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(WIDTH * 4),
                    rows_per_image: Some(HEIGHT),
                },
            },
            image.texture.size(),
        );
        self.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        rx.recv().unwrap().unwrap();
        let pixels = buffer.slice(..).get_mapped_range().unwrap().to_vec();
        buffer.unmap();
        pixels
    }
}

/// Admits an original flat summary with a distinct world color.
fn surface(app: &mut App, position: ColumnPos, height: i16, rgb: [u8; 3]) {
    let mut summary = ColumnSummary::default();
    summary.cells_mut().fill(far_terrain::SurfaceCell {
        height,
        rgb,
        water_depth: 0,
    });
    app.world_mut()
        .resource_mut::<FarTerrain>()
        .cache
        .store()
        .admit(position, &summary);
}

/// Adds original near geometry to the same production upload queue as the client.
fn near(app: &mut App, y: i32) -> world::SubChunkKey {
    let key = world::SubChunkKey::new(0, 29, y, 0);
    let source = world::SubChunk::decode(&[9, 1, 0, 1, 2], &world::RawBlockIds { air: 0 });
    let mesh = meshing::mesh_sub_chunk(
        &meshing::BlockClassifier::new(0),
        &assets::RuntimeAssets::diagnostic(),
        assets::NetworkIdMode::Sequential,
        &meshing::Neighbourhood::empty(),
        &source,
    );
    app.world_mut()
        .resource_mut::<render_scene::ChunkRenderQueue>()
        .try_insert(key, mesh, render_scene::ChunkUploadPriority::new(0.0))
        .unwrap();
    key
}

/// Reports old near geometry that survives the generation boundary, even behind loading.
fn has_near(app: &mut App, key: world::SubChunkKey) -> bool {
    let mut query = app
        .world_mut()
        .query::<&render_scene::ChunkRenderInstance>();
    query.iter(app.world()).any(|instance| instance.key == key)
}

/// Counts the old world's green pixels independently of face lighting.
fn old_pixels(pixels: &[u8]) -> usize {
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| u16::from(p[1]) > 2 * u16::from(p[0]) && u16::from(p[1]) > 2 * u16::from(p[2]))
        .count()
}

/// Optionally saves every original GPU frame for before/after contact sheets.
fn save(name: &str, pixels: &[u8]) {
    let Some(root) = std::env::var_os("CINNABAR_TRANSFER_CAPTURE_DIR") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    std::fs::create_dir_all(&root).unwrap();
    image::save_buffer(
        root.join(format!("{name}.png")),
        pixels,
        WIDTH,
        HEIGHT,
        image::ColorType::Rgba8,
    )
    .unwrap();
}

/// No frame after a dimension packet burst may show the old world's marked columns.
#[test]
fn dimension_round_trip_renders_no_previous_world_in_any_frame() {
    let Some(mut capture) = Capture::new() else {
        return;
    };
    capture.frame();
    let old_near = near(&mut capture.app, 4);
    for (x, z) in [(30, 0), (32, 0), (30, 2)] {
        surface(&mut capture.app, ColumnPos::new(x, z), 96, [20, 240, 20]);
    }
    let mut before = Vec::new();
    test_time::eventually("old world's far columns are rendered", || {
        before = capture.frame();
        old_pixels(&before) > 50
            && [(30, 0), (32, 0), (30, 2)].into_iter().all(|(x, z)| {
                capture
                    .app
                    .world()
                    .resource::<FarTerrain>()
                    .tiles
                    .ready(ColumnPos::new(x, z))
            })
    });
    save("world-a", &before);
    assert!(has_near(&mut capture.app, old_near));
    round_trip(&mut capture.app);
    capture
        .app
        .world_mut()
        .resource_mut::<render_scene::PanoramaScene>()
        .set_game_visible(false);
    let mut leaks = Vec::new();
    let mut near_leaks = Vec::new();
    for frame in 0..8 {
        if frame == 2 {
            capture
                .app
                .world_mut()
                .resource_mut::<render_scene::PanoramaScene>()
                .set_game_visible(true);
        }
        let pixels = capture.frame();
        save(&format!("transfer-{frame:02}"), &pixels);
        let count = old_pixels(&pixels);
        if count != 0 {
            leaks.push((frame, count));
        }
        if has_near(&mut capture.app, old_near) {
            near_leaks.push(frame);
        }
    }
    near(&mut capture.app, 1);
    surface(&mut capture.app, ColumnPos::new(30, 0), 48, [20, 20, 240]);
    for (x, z) in [(32, 0), (30, 2)] {
        surface(
            &mut capture.app,
            ColumnPos::new(x, z),
            far_terrain::SurfaceCell::EMPTY_HEIGHT,
            [0; 3],
        );
    }
    let mut after = Vec::new();
    let mut frame = 8;
    test_time::eventually("destination terrain is rendered", || {
        after = capture.frame();
        save(&format!("transfer-{frame:02}"), &after);
        let count = old_pixels(&after);
        if count != 0 {
            leaks.push((frame, count));
        }
        frame += 1;
        old_pixels(&after) == 0
            && after.as_chunks::<4>().0.iter().any(|p| {
                u16::from(p[2]) > 2 * u16::from(p[0]) && u16::from(p[2]) > 2 * u16::from(p[1])
            })
    });
    save("world-b", &after);
    if let Some(root) = std::env::var_os("CINNABAR_TRANSFER_CAPTURE_DIR") {
        std::fs::write(std::path::PathBuf::from(root).join("frames.json"), serde_json::to_vec_pretty(&serde_json::json!({
            "frames": frame, "old_far_pixels": leaks, "old_near_frames": near_leaks,
            "world_a_green_pixels": old_pixels(&before), "world_b_green_pixels": old_pixels(&after),
        })).unwrap()).unwrap();
    }
    assert!(
        leaks.is_empty(),
        "old far terrain leaked (frame, pixels): {leaks:?}"
    );
    assert!(
        near_leaks.is_empty(),
        "old near terrain survived frames {near_leaks:?}"
    );
    assert_eq!(
        old_pixels(&after),
        0,
        "destination contains old-world columns"
    );
}
