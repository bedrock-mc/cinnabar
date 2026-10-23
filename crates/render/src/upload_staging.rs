//! Bounded frame uploads share mapped storage until asynchronous GPU completion permits reuse.
//! Every update to a participating buffer must use its pool before the camera graph runs.

use crate::queue_writes::ProfiledQueue as _;
#[path = "upload_staging/pool.rs"]
mod pool;
#[cfg(test)]
#[path = "upload_staging/tests.rs"]
mod tests;

use bevy::{
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        renderer::{RenderContext, RenderDevice, RenderQueue},
    },
};
use pool::{FRAME, Pool, Shape, TERRAIN};
use std::sync::{Mutex, MutexGuard};

pub(crate) type BufferWrite<'a> = (&'a wgpu::Buffer, u64, &'a [u8]);

/// Cumulative work one pool has staged, refused and encoded.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Stats {
    pub(crate) staged_writes: u64,
    /// Batches admitted under one pool lock.
    #[cfg(test)]
    pub(crate) batches: u64,
    pub(crate) staged_bytes: u64,
    /// Pool misses: writes that took the queue path, each allocating backend staging.
    pub(crate) fallback_writes: u64,
    pub(crate) fallback_bytes: u64,
    pub(crate) overflow_submissions: u64,
    /// Copy commands encoded; abutting writes share one.
    pub(crate) encoded_copies: u64,
    /// Slots added after missed frames.
    pub(crate) grown_slots: u64,
}

/// Pools requested by render owners; the graph prefix encodes every installed pool.
#[derive(Resource, Default)]
struct Installed {
    frame: bool,
    terrain: bool,
}

/// Installs the frame pool regardless of how many render owners request it.
pub(crate) fn install(app: &mut App) {
    request(app, |installed| installed.frame = true);
}

/// Installs the terrain arena pool beside any frame pool.
pub(crate) fn install_terrain(app: &mut App) {
    request(app, |installed| installed.terrain = true);
}

/// Records a requested pool and installs the one upload prefix on first use.
fn request(app: &mut App, select: impl FnOnce(&mut Installed)) {
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    if let Some(mut installed) = render_app.world_mut().get_resource_mut::<Installed>() {
        select(&mut installed);
        return;
    }
    let mut installed = Installed::default();
    select(&mut installed);
    // Slots return to their pool through the frame's device poll.
    crate::device_poll::install(render_app);
    render_app
        .insert_resource(installed)
        .add_systems(RenderStartup, initialize)
        .add_systems(
            Render,
            grow_upload_pools
                .in_set(UploadGrowth)
                .after(RenderSystems::PrepareBindGroups)
                .before(RenderSystems::Render),
        );
}

/// The render-graph system that encodes staged copies; later graph work that reads those
/// buffers orders after it.
#[derive(bevy::ecs::schedule::SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct BufferUploads;

/// Creates retained staging only when a camera graph will consume its pending copies.
fn initialize(world: &mut World) {
    let installed = world
        .try_schedule_scope(bevy::render::renderer::RenderGraph, |_, schedule| {
            schedule.add_systems(
                crate::gpu_timing::profiled(upload_buffers, None, "main/BufferUploads")
                    .in_set(BufferUploads)
                    .before(bevy::core_pipeline::schedule::camera_driver)
                    .in_set(bevy::render::renderer::RenderGraphSystems::Render),
            );
        })
        .is_ok();
    if !installed {
        return;
    }
    let requested = world.resource::<Installed>();
    let (frame, terrain) = (requested.frame, requested.terrain);
    let device = world.resource::<RenderDevice>().clone();
    if frame {
        world.insert_resource(BufferUploadStaging::new(&device));
    }
    if terrain {
        world.insert_resource(TerrainUploadStaging::new(&device));
    }
}

/// Frame upload dependencies shared by render preparation systems and their helpers.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct FrameUploads<'w> {
    pub(crate) device: Res<'w, RenderDevice>,
    pub(crate) queue: Res<'w, RenderQueue>,
    staging: Option<Res<'w, BufferUploadStaging>>,
}

impl FrameUploads<'_> {
    /// Stages one batch under one lock, or queues it when no frame pool is installed.
    #[track_caller]
    pub(crate) fn write_batch(&self, writes: &[BufferWrite<'_>]) {
        if let Some(staging) = &self.staging {
            staging.write_batch(&self.device, &self.queue, writes);
        } else {
            fallback(&self.queue, writes, std::panic::Location::caller());
        }
    }

    /// Uploads one recurring buffer update through the same ordering contract as a batch.
    #[track_caller]
    pub(crate) fn write(&self, target: &wgpu::Buffer, offset: u64, bytes: &[u8]) {
        self.write_batch(&[(target, offset, bytes)]);
    }
}

/// Queues every nonempty fallback write without waiting for reusable storage.
#[track_caller]
fn fallback(
    queue: &RenderQueue,
    writes: &[BufferWrite<'_>],
    site: &'static std::panic::Location<'static>,
) {
    for &(target, offset, bytes) in writes.iter().filter(|write| !write.2.is_empty()) {
        queue.profiled_write_buffer_at(target, offset, bytes, site);
    }
}

/// Small recurring frame writes.
#[derive(Resource, Deref)]
pub(crate) struct BufferUploadStaging(Staging);

/// Terrain arena writes; a migration slice must encode pending copies before it copies.
#[derive(Resource, Deref)]
pub(crate) struct TerrainUploadStaging(Staging);

impl BufferUploadStaging {
    /// Creates retained staging for recurring frame uploads.
    pub(crate) fn new(device: &RenderDevice) -> Self {
        Self(Staging::new(device, FRAME))
    }
}

impl TerrainUploadStaging {
    /// Requests one real slot-growth attempt without filling the pool in a recovery fixture.
    #[cfg(test)]
    pub(crate) fn request_test_growth(&self) {
        self.lock().record_miss(1, TERRAIN.slot_bytes);
    }

    pub(crate) fn new(device: &RenderDevice) -> Self {
        Self(Staging::new(device, TERRAIN))
    }
}

/// Preparation owns destination writes; the graph consumes copies before any camera reads them.
pub(crate) struct Staging(Mutex<Pool>);

/// Writes a pool could not hold, and older overlapping copies to submit before them.
struct Miss<'a> {
    rest: Vec<BufferWrite<'a>>,
    flush: Option<wgpu::CommandBuffer>,
    #[cfg(feature = "tracy")]
    span: bevy::log::tracing::Span,
}

impl Staging {
    fn new(device: &RenderDevice, shape: Shape) -> Self {
        Self(Mutex::new(Pool::new(device, shape)))
    }

    /// Recovers the pool after a panicking writer; its slot states remain authoritative.
    fn lock(&self) -> MutexGuard<'_, Pool> {
        self.0.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// Stages what fits and queues the rest of the batch.
    #[track_caller]
    pub(crate) fn write_batch(
        &self,
        device: &RenderDevice,
        queue: &RenderQueue,
        writes: &[BufferWrite<'_>],
    ) {
        let site = std::panic::Location::caller();
        self.write_batch_or(device, queue, writes, |rest| fallback(queue, rest, site));
    }

    /// Stages writes in order until the pool fills and passes the unstaged rest to `fallback`,
    /// after submitting older copies it overlaps. The lock is released before that submission:
    /// each participating buffer is written by one system at a time, so no other writer can
    /// interleave updates to the same bytes.
    #[track_caller]
    pub(crate) fn write_batch_or(
        &self,
        device: &RenderDevice,
        queue: &RenderQueue,
        writes: &[BufferWrite<'_>],
        fallback: impl FnOnce(&[BufferWrite<'_>]),
    ) {
        let Some(miss) = self.admit(device, writes) else {
            return;
        };
        #[cfg(feature = "tracy")]
        let _span = miss.span.entered();
        if let Some(command) = miss.flush {
            queue.submit([command]);
        }
        fallback(&miss.rest);
    }

    /// Stages under the lock and returns what must take the queue path, if anything.
    #[track_caller]
    fn admit<'a>(&self, device: &RenderDevice, writes: &[BufferWrite<'a>]) -> Option<Miss<'a>> {
        let mut pool = self.lock();
        #[cfg(test)]
        {
            pool.stats.batches += 1;
        }
        let (index, staged) = pool.stage_prefix(writes)?;
        let (target, offset, bytes) = writes[index];
        let mut rest = Vec::with_capacity(writes.len() - index);
        rest.push((target, offset + staged, &bytes[staged as usize..]));
        rest.extend(
            writes[index + 1..]
                .iter()
                .copied()
                .filter(|write| !write.2.is_empty()),
        );
        let bytes = rest.iter().map(|write| write.2.len() as u64).sum::<u64>();
        pool.record_miss(rest.len() as u64, bytes);
        let overlap = pool.overlaps(&rest);
        let flush = overlap.then(|| {
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ordered upload fallback"),
            });
            pool.encode(&mut encoder);
            pool.stats.overflow_submissions += 1;
            encoder.finish()
        });
        #[cfg(feature = "tracy")]
        let span = {
            let ([ready, active, pending, failed], queued_copies) = pool.diagnostic_counts();
            bevy::log::info_span!(
                "uploads.fallback",
                pool = pool.name(),
                writes = rest.len(),
                bytes,
                staged,
                overlap,
                ready,
                active,
                pending,
                failed,
                queued_copies,
                total_writes = pool.stats.fallback_writes,
                total_bytes = pool.stats.fallback_bytes,
            )
        };
        Some(Miss {
            rest,
            flush,
            #[cfg(feature = "tracy")]
            span,
        })
    }

    /// Cumulative staged, fallback and encoded work since creation.
    #[cfg(test)]
    pub(crate) fn stats(&self) -> Stats {
        self.lock().stats
    }

    /// Bytes the pool's current slots can hold at once.
    #[cfg(test)]
    pub(crate) fn capacity_bytes(&self) -> u64 {
        let pool = self.lock();
        let ([ready, active, pending, failed], _) = pool.diagnostic_counts();
        (ready + active + pending + failed) as u64 * pool.slot_bytes()
    }

    /// Encodes pending copies ahead of `encoder`'s later commands, which may read their bytes.
    pub(crate) fn encode_pending(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut pool = self.lock();
        if pool.has_copies() {
            pool.encode(encoder);
        }
    }
}

/// Records all pending copies before any camera reads their destination buffers.
fn upload_buffers(
    frame: Option<Res<BufferUploadStaging>>,
    terrain: Option<Res<TerrainUploadStaging>>,
    mut context: RenderContext,
) {
    let pools = [
        frame.as_deref().map(|staging| &staging.0),
        terrain.as_deref().map(|staging| &staging.0),
    ];
    for staging in pools.into_iter().flatten() {
        let mut pool = staging.lock();
        if pool.has_copies() {
            pool.encode(context.command_encoder());
        }
    }
}

/// Orders recoverable pool growth before terrain decides whether its frame can draw.
#[derive(bevy::ecs::schedule::SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct UploadGrowth;

/// Grows retained pools after preparation records misses and before any draw is submitted.
fn grow_upload_pools(
    frame: Option<Res<BufferUploadStaging>>,
    terrain: Option<Res<TerrainUploadStaging>>,
) {
    for staging in [
        frame.as_deref().map(|s| &s.0),
        terrain.as_deref().map(|s| &s.0),
    ]
    .into_iter()
    .flatten()
    {
        staging.lock().grow();
    }
}
