//! Bounded frame uploads share mapped storage until asynchronous GPU completion permits reuse.
//! Every update to a participating buffer must use this owner before the camera graph runs.

#[path = "upload_staging/pool.rs"]
mod pool;
#[cfg(test)]
#[path = "upload_staging/tests.rs"]
mod tests;

use bevy::{
    prelude::*,
    render::{
        RenderApp, RenderStartup,
        renderer::{RenderContext, RenderDevice, RenderQueue},
    },
};
use pool::Pool;
use std::sync::Mutex;

const SLOT_COUNT: usize = 8;
const SLOT_BYTES: u64 = 64 * 1024;
const MAX_COPIES: usize = 128;

pub(crate) type BufferWrite<'a> = (&'a wgpu::Buffer, u64, &'a [u8]);

#[derive(Resource)]
struct Installed;

/// Installs one upload prefix regardless of how many render owners request it.
pub(crate) fn install(app: &mut App) {
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    if render_app.world().contains_resource::<Installed>() {
        return;
    }
    // Slots return to the pool through the frame's device poll.
    crate::device_poll::install(render_app);
    render_app
        .insert_resource(Installed)
        .add_systems(RenderStartup, initialize);
}

/// Creates retained staging only when a camera graph will consume its pending copies.
fn initialize(world: &mut World) {
    let staging = BufferUploadStaging(Mutex::new(Pool::new(
        world.resource::<RenderDevice>(),
        SLOT_BYTES,
    )));
    let installed = world
        .try_schedule_scope(bevy::render::renderer::RenderGraph, |_, schedule| {
            schedule.add_systems(
                crate::gpu_timing::profiled(upload_buffers, None, "main/BufferUploads")
                    .before(bevy::core_pipeline::schedule::camera_driver)
                    .in_set(bevy::render::renderer::RenderGraphSystems::Render),
            );
        })
        .is_ok();
    if installed {
        world.insert_resource(staging);
    }
}

/// Uses retained storage when installed; isolated owners retain their ordinary queue path.
pub(crate) fn write_batch(
    staging: Option<&BufferUploadStaging>,
    device: &RenderDevice,
    queue: &RenderQueue,
    writes: &[BufferWrite<'_>],
) {
    if let Some(staging) = staging {
        staging.write_batch(device, queue, writes);
    } else {
        fallback(queue, writes);
    }
}

/// Queues every nonempty fallback write without waiting for reusable storage.
fn fallback(queue: &RenderQueue, writes: &[BufferWrite<'_>]) {
    for &(target, offset, bytes) in writes.iter().filter(|write| !write.2.is_empty()) {
        queue.write_buffer(target, offset, bytes);
    }
}

/// Preparation owns destination writes; the graph consumes copies before any camera reads them.
#[derive(Resource)]
pub(crate) struct BufferUploadStaging(Mutex<Pool>);

impl BufferUploadStaging {
    /// Serializes fallback with earlier copies so overlapping updates cannot overtake each other.
    fn write_batch(&self, device: &RenderDevice, queue: &RenderQueue, writes: &[BufferWrite<'_>]) {
        let mut pool = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if pool.try_stage(writes) {
            return;
        }
        let overlap = pool.overlaps(writes);
        let count = writes.iter().filter(|write| !write.2.is_empty()).count() as u64;
        let bytes = writes.iter().map(|write| write.2.len() as u64).sum::<u64>();
        pool.stats.fallback_writes += count;
        pool.stats.fallback_bytes += bytes;
        #[cfg(feature = "tracy")]
        let ([ready, active, pending, failed], queued_copies) = pool.diagnostic_counts();
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!(
            "uploads.fallback",
            writes = count,
            bytes,
            overlap,
            ready,
            active,
            pending,
            failed,
            queued_copies,
            total_writes = pool.stats.fallback_writes,
            total_bytes = pool.stats.fallback_bytes,
        )
        .entered();
        if overlap {
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ordered upload fallback"),
            });
            pool.encode(&mut encoder);
            pool.stats.overflow_submissions += 1;
            queue.submit([encoder.finish()]);
        }
        fallback(queue, writes);
    }
}

/// Records all pending copies before any camera reads their destination buffers.
fn upload_buffers(staging: Res<BufferUploadStaging>, mut context: RenderContext) {
    let mut pool = staging.0.lock().unwrap_or_else(|error| error.into_inner());
    if pool.has_copies() {
        pool.encode(context.command_encoder());
    }
}
