//! The real render app on a validating NOOP device with count-driven indirect draws.

use bevy::{
    asset::{AssetPlugin, Assets},
    camera::{CameraPlugin, RenderTarget},
    core_pipeline::CorePipelinePlugin,
    image::ImagePlugin,
    mesh::MeshPlugin,
    render::{
        RenderPlugin,
        renderer::{RenderAdapterInfo, WgpuWrapper},
        settings::RenderCreation,
    },
    window::WindowPlugin,
};

use super::model::CullRecord;
use super::*;

/// A render plugin on the first adapter of `backends`; `None` when none is present.
pub(super) fn render_plugin(
    backends: wgpu::Backends,
    required_features: WgpuFeatures,
) -> Option<RenderPlugin> {
    let noop = backends == wgpu::Backends::NOOP;
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends,
        backend_options: wgpu::BackendOptions {
            noop: wgpu::NoopBackendOptions { enable: noop },
            ..Default::default()
        },
        ..Default::default()
    });
    let adapter =
        bevy::tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .ok()?;
    if !adapter.features().contains(required_features) {
        return None;
    }
    let required_limits = if noop {
        wgpu::Limits {
            max_storage_buffers_per_shader_stage: required_vertex_storage_buffers(),
            ..Default::default()
        }
    } else {
        adapter.limits()
    };
    let descriptor = wgpu::DeviceDescriptor {
        required_features,
        required_limits,
        ..Default::default()
    };
    let adapter_info = adapter.get_info();
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&descriptor)).unwrap();
    Some(RenderPlugin {
        render_creation: RenderCreation::manual(
            RenderDevice::from(device),
            RenderQueue(Arc::new(WgpuWrapper::new(queue))),
            RenderAdapterInfo(WgpuWrapper::new(adapter_info)),
            RenderAdapter(Arc::new(WgpuWrapper::new(adapter))),
            RenderInstance(Arc::new(WgpuWrapper::new(instance))),
        ),
        synchronous_pipeline_compilation: true,
        ..Default::default()
    })
}

pub(crate) fn noop_render_plugin(required_features: WgpuFeatures) -> RenderPlugin {
    render_plugin(wgpu::Backends::NOOP, required_features).expect("the NOOP adapter is built in")
}

/// A sub-chunk filled with one solid block.
pub(super) fn mesh() -> meshing::ChunkMesh {
    let source = world::SubChunk::decode(&[9, 1, 0, 1, 2], &world::RawBlockIds { air: 0 });
    meshing::mesh_sub_chunk(
        &meshing::BlockClassifier::new(0),
        &assets::RuntimeAssets::diagnostic(),
        assets::NetworkIdMode::Sequential,
        &meshing::Neighbourhood::empty(),
        &source,
    )
}

pub(super) fn frame(app: &mut App) {
    app.update();
    app.sub_app(RenderApp)
        .world()
        .resource::<RenderDevice>()
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
}

/// The chunk renderer on `render`'s device, with a camera at `transform` drawing to an image.
pub(super) fn chunk_app(render: RenderPlugin, msaa: Msaa, transform: Transform) -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(WindowPlugin {
            primary_window: None,
            ..Default::default()
        })
        .add_plugins((AssetPlugin::default(), TransformPlugin))
        .add_plugins(render)
        .add_plugins((
            ImagePlugin::default(),
            MeshPlugin,
            CameraPlugin,
            CorePipelinePlugin,
        ))
        .add_plugins(ChunkRenderPlugin::default());
    let image = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            64,
            48,
            TextureFormat::Rgba8Unorm,
            None,
        ));
    let camera = app
        .world_mut()
        .spawn((
            Camera3d::default(),
            Camera::default(),
            RenderTarget::Image(image.into()),
            msaa,
            transform,
        ))
        .id();
    app.finish();
    app.cleanup();
    (app, camera)
}

pub(super) const KEYS: [SubChunkKey; 2] =
    [SubChunkKey::new(0, 0, 0, 0), SubChunkKey::new(0, 1, 0, 0)];

pub(super) fn insert_meshes(app: &mut App, keys: &[SubChunkKey]) {
    for &key in keys {
        app.world_mut()
            .resource_mut::<ChunkRenderQueue>()
            .try_insert(key, mesh(), ChunkUploadPriority::new(0.0))
            .unwrap();
    }
}

pub(super) fn camera_transform() -> Transform {
    Transform::from_xyz(-6.0, 20.0, -6.0).looking_at(Vec3::new(8.0, 8.0, 8.0), Vec3::Y)
}

/// Every frame queues, prepares, culls and draws the GPU path without a validation error.
#[test]
fn count_capable_devices_run_the_two_phase_cull_through_the_render_graph() {
    let (mut app, _) = chunk_app(
        noop_render_plugin(
            WgpuFeatures::MULTI_DRAW_INDIRECT_COUNT | WgpuFeatures::INDIRECT_FIRST_INSTANCE,
        ),
        Msaa::Sample4,
        camera_transform(),
    );
    // The chunk-only renderer has no shadow plugin; every queued edge must still resolve.
    let graph = app
        .sub_app(RenderApp)
        .world()
        .resource::<bevy::render::render_graph::RenderGraph>()
        .get_sub_graph(bevy::core_pipeline::core_3d::graph::Core3d)
        .unwrap();
    assert!(
        graph
            .get_node_state(crate::entity_shadow_render::EntityShadowLabel)
            .is_err()
    );
    let outputs: Vec<_> = graph
        .iter_node_outputs(node::GpuCullLateLabel)
        .unwrap()
        .collect();
    assert!(outputs.iter().any(|(_, state)| state.label
        == bevy::render::render_graph::RenderLabel::intern(
            &bevy::core_pipeline::core_3d::graph::Node3d::MainTransmissivePass
        )));
    insert_meshes(&mut app, &KEYS);
    let keys = KEYS;
    for _ in 0..4 {
        frame(&mut app);
    }
    let render_world = app.sub_app(RenderApp).world();
    assert_eq!(
        render_world.resource::<GpuCullSupport>(),
        &GpuCullSupport(true)
    );
    let cull = render_world.resource::<GpuCull>();
    assert_eq!(cull.slot_count(), 2);
    assert!(cull.table.records().iter().all(CullRecord::is_live));
    assert!(
        model::slot_enabled(cull.table.enabled(), 0)
            && model::slot_enabled(cull.table.enabled(), 1)
    );
    assert!(cull.bind_groups.is_some(), "the culled view was prepared");
    assert!(
        cull.pyramid.is_some(),
        "the depth target admits sampling, so the late phase tests Hi-Z"
    );

    // Removing a chunk frees its slot on the next frame.
    let entity = app.world().resource::<ChunkEntities>().0[&keys[1]];
    app.world_mut()
        .entity_mut(entity)
        .remove::<ChunkRenderInstance>();
    frame(&mut app);
    frame(&mut app);
    let cull = app.sub_app(RenderApp).world().resource::<GpuCull>();
    assert_eq!(cull.slot_count(), 1);
}

/// wgpu-core 27 expands validated DX12 indexed commands to include three special constants.
/// The workspace dev profile disables its bad source-stride assertion, so ordinary MDI remains
/// available instead of falling back to one direct draw per terrain section.
#[cfg(all(target_os = "windows", debug_assertions))]
#[test]
fn dx12_debug_batches_expanded_indexed_indirect_commands() {
    let Some(render) = render_plugin(wgpu::Backends::DX12, WgpuFeatures::INDIRECT_FIRST_INSTANCE)
    else {
        eprintln!("skipping DX12 debug MDI app: missing compatible native adapter");
        return;
    };
    let (mut app, _) = chunk_app(render, Msaa::Off, camera_transform());
    insert_meshes(&mut app, &KEYS);
    for _ in 0..4 {
        frame(&mut app);
    }

    let render_world = app.sub_app(RenderApp).world();
    assert_eq!(
        render_world.resource::<GpuCullSupport>(),
        &GpuCullSupport(false),
        "DX12 count draws still use the CPU-prepared MDI path"
    );
    assert_eq!(
        render_world.resource::<DirectOcclusionSupport>(),
        &DirectOcclusionSupport(false),
        "DX12 debug must not regress to per-section direct draws"
    );
    let command_count: u32 = render_world
        .resource::<pipeline::solid::ChunkSolidIndirectBatches>()
        .0
        .values()
        .map(|batch| batch.cubes.command_count)
        .sum();
    assert!(
        command_count > 1,
        "fixture must exercise a multi-command indexed indirect batch"
    );
}
