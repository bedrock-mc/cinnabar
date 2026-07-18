use crate::chunk::*;

use bevy::render::{
    RenderApp, RenderPlugin,
    renderer::{RenderAdapterInfo, WgpuWrapper},
    settings::RenderCreation,
};

/// Opaque render-world state for the cross-crate publication acceptance gate.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicationRenderTerminalSnapshot {
    pub extracted_manifest: Vec<(SubChunkKey, u64)>,
    pub allocation_manifest: Vec<(SubChunkKey, u64)>,
    pub pending_gpu_removals: usize,
    pub fairness_waiters: usize,
    pub retired_allocations: usize,
    pub pending_arena_removals: usize,
    pub in_flight_presented_callbacks: usize,
    pub transparent_presentation_in_flight: bool,
    pub transparent_retirement_in_flight: bool,
}

/// Builds Bevy's real [`RenderPlugin`] around the deterministic wgpu NOOP
/// adapter. The render sub-app, extraction schedule, render schedule, and GPU
/// completion callbacks remain Bevy-owned.
#[doc(hidden)]
pub fn publication_noop_render_plugin() -> RenderPlugin {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::NOOP,
        backend_options: wgpu::BackendOptions {
            noop: wgpu::NoopBackendOptions { enable: true },
            ..Default::default()
        },
        ..Default::default()
    });
    let adapter =
        bevy::tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("the enabled NOOP backend provides an adapter");
    let adapter_info = adapter.get_info();
    let device_descriptor = wgpu::DeviceDescriptor {
        required_limits: wgpu::Limits {
            max_storage_buffers_per_shader_stage: 10,
            ..Default::default()
        },
        ..Default::default()
    };
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&device_descriptor))
        .expect("the NOOP adapter creates a device with its production-visible limits");

    RenderPlugin {
        render_creation: RenderCreation::manual(
            RenderDevice::from(device),
            RenderQueue(Arc::new(WgpuWrapper::new(queue))),
            RenderAdapterInfo(WgpuWrapper::new(adapter_info)),
            RenderAdapter(Arc::new(WgpuWrapper::new(adapter))),
            RenderInstance(Arc::new(WgpuWrapper::new(instance))),
        ),
        synchronous_pipeline_compilation: true,
        ..Default::default()
    }
}

/// Runs one complete main/extract/render iteration and waits for all NOOP GPU
/// completion callbacks created by that iteration.
#[doc(hidden)]
pub fn settle_publication_noop_frame(app: &mut App) {
    app.update();
    app.sub_app(RenderApp)
        .world()
        .resource::<RenderDevice>()
        .poll(PollType::wait_indefinitely())
        .expect("the NOOP publication renderer settles");
}

#[doc(hidden)]
#[must_use]
pub fn publication_render_terminal_snapshot(app: &mut App) -> PublicationRenderTerminalSnapshot {
    let render_world = app.sub_app_mut(RenderApp).world_mut();
    let mut instances = render_world.query::<&ChunkRenderInstance>();
    let mut extracted_manifest = instances
        .iter(render_world)
        .map(|instance| (instance.key, instance.generation))
        .collect::<Vec<_>>();
    extracted_manifest.sort_unstable();

    let arena = render_world.resource::<ChunkGpuArena>();
    let mut allocation_manifest = arena
        .allocations
        .values()
        .map(|allocation| (allocation.gpu.key, allocation.generation))
        .collect::<Vec<_>>();
    allocation_manifest.sort_unstable();
    let retired_allocations = arena.retired_allocations.len();
    let pending_arena_removals = arena.pending_removals.len();

    let gate = render_world.resource::<PresentedFrameGate>();
    let in_flight_presented_callbacks = gate
        .inner
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .in_flight_callbacks;
    let transparent_presentation_in_flight = render_world
        .resource::<TransparentPresentationFence>()
        .is_in_flight();
    let transparent_retirement_in_flight = render_world
        .resource::<TransparentRetirementFence>()
        .is_in_flight();

    PublicationRenderTerminalSnapshot {
        extracted_manifest,
        allocation_manifest,
        pending_gpu_removals: render_world
            .resource::<ChunkGpuRemovalQueue>()
            .pending_len(),
        fairness_waiters: render_world.resource::<GpuUpdateFairness>().wait_ages.len(),
        retired_allocations,
        pending_arena_removals,
        in_flight_presented_callbacks,
        transparent_presentation_in_flight,
        transparent_retirement_in_flight,
    }
}
