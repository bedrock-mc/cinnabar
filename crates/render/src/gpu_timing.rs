//! Nonblocking GPU timing sums elapsed pass latencies, including overlap and gaps, not active work.
//! Metal times owned passes and the stock opaque pass; whole-frame and shared draw categories stay absent.
//! Elsewhere, frames that nothing inspects pass by pass record only the whole-frame span: each
//! marker is its own pass, and two per frame cost far less than two per timed node.

mod categories;
pub(crate) use categories::draw_categories;
mod health;
mod systems;
pub(crate) use systems::profiled;
#[cfg(all(test, target_os = "macos"))]
mod metal_tests;
#[cfg(feature = "tracy")]
mod nodes;
mod overdraw;
mod pass;
pub(crate) mod readback;
#[cfg(test)]
mod tests;
#[cfg(feature = "tracy")]
mod tracy;

use crate::{RuntimeStage, RuntimeStageProfiler};
use bevy::{
    core_pipeline::{Core3d, Core3dSystems},
    ecs::system::{SystemParamItem, lifetimeless::SRes},
    prelude::{
        App, Commands, IntoScheduleConfigs, IntoSystemSet, Plugin, Res, ResMut, Resource,
        SystemSet, World, info,
    },
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_phase::{PhaseItem, RenderCommand, RenderCommandResult, TrackedRenderPass},
        renderer::{RenderContext, RenderDevice, RenderQueue},
    },
};
use readback::{ReadbackRing, SLOTS};
use std::{
    marker::PhantomData,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering},
    },
};

#[cfg(feature = "tracy")]
pub(crate) use nodes::SectionSpan;
pub(crate) use pass::{render_pass_timestamps, ui_pass_timestamps, ui_profiling_requested};

/// Records `record` into `encoder` between `span`'s marks when one was claimed. Claim spans
/// while the node runs, so deferred command-buffer tasks still land in the frame's resolve.
pub(crate) fn within_span<R>(
    span: Option<&SectionSpan<'_>>,
    encoder: &mut wgpu::CommandEncoder,
    record: impl FnOnce(&mut wgpu::CommandEncoder) -> R,
) -> R {
    if let Some(span) = span {
        span.begin(encoder);
    }
    let result = record(encoder);
    if let Some(span) = span {
        span.end(encoder);
    }
    result
}

/// Without Tracy, no section span is ever claimed; see `nodes::SectionSpan`.
#[cfg(not(feature = "tracy"))]
#[derive(Clone, Copy)]
pub(crate) struct SectionSpan<'w>(PhantomData<&'w ()>);

#[cfg(not(feature = "tracy"))]
impl SectionSpan<'_> {
    pub(crate) fn claim<'w>(_: &'w World, _: &'static str) -> Option<SectionSpan<'w>> {
        None
    }

    pub(crate) fn begin(&self, _: &mut wgpu::CommandEncoder) {}

    pub(crate) fn end(&self, _: &mut wgpu::CommandEncoder) {}
}
pub use readback::GpuFrameTimes;
pub(crate) use readback::decode_spans;

/// Node-level spans per frame, ahead of the draw pool so draws can never starve them.
const PASS_SPANS: u32 = 64;
/// Per-draw spans per frame; a frame that needs more drops its draw categories.
const DRAW_SPANS: u32 = 448;
const SLOT_SPANS: u32 = PASS_SPANS + DRAW_SPANS;
const SLOT_BYTES: u64 = SLOT_SPANS as u64 * 2 * TIMESTAMP_BYTES;
const TIMESTAMP_BYTES: u64 = 8;
const NO_SLOT: u32 = u32::MAX;
const NO_SPAN: u32 = u32::MAX;
const NOT_RECORDED: u64 = u64::MAX;

/// The slot awaits GPU completion or buffer mapping.
const PENDING: u8 = 0;
const MAPPED: u8 = 1;
const FAILED: u8 = 2;
/// Metal samples are complete and may be copied by a later frame.
const WRITTEN: u8 = 3;
/// A later frame encoded the copy; mapping starts after that frame's submission.
const RESOLVED: u8 = 4;

/// Feeds `gpu_*` stages into the [`RuntimeStageProfiler`] already present in the app.
pub struct GpuTimingPlugin;

impl Plugin for GpuTimingPlugin {
    fn build(&self, app: &mut App) {
        let Some(profiler) = app.world().get_resource::<RuntimeStageProfiler>().cloned() else {
            return;
        };
        app.init_resource::<DetailedGpuTiming>()
            .add_plugins(ExtractResourcePlugin::<DetailedGpuTiming>::default());
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        crate::device_poll::install(render_app);
        if overdraw::requested() {
            overdraw::install(render_app);
        }
        if categories::requested() {
            render_app.insert_resource(categories::CategoryProfiling);
        }
        render_app
            .insert_resource(profiler)
            // RenderStartup runs inside the render app after every plugin has built the graph,
            // and still reaches it after pipelined rendering moves the app to its own thread.
            .add_systems(
                RenderStartup,
                (
                    init_gpu_timestamps,
                    install_builtin_timing,
                    install_readback,
                )
                    .chain(),
            )
            .add_systems(
                Render,
                (
                    begin_gpu_frame.in_set(RenderSystems::PrepareResources),
                    request_gpu_frame_readback.in_set(crate::device_poll::FrameSubmissions),
                ),
            );
        #[cfg(feature = "tracy")]
        {
            tracy::install(render_app);
            if nodes::requested() {
                nodes::install(render_app);
            }
        }
    }
}

/// Asks for per-pass GPU spans in the next rendered frame when the stage profiler does not
/// already record them, such as when a developer overlay refreshes its slowest passes.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq, ExtractResource)]
#[extract_app(bevy::render::RenderApp)]
pub struct DetailedGpuTiming(pub bool);

/// Adds markers to stock post-processing systems while keeping their schedule boundaries.
fn install_builtin_timing(world: &mut World) {
    if world.contains_resource::<categories::CategoryProfiling>()
        || cfg!(target_os = "macos")
            && world
                .get_resource::<RenderDevice>()
                .is_some_and(|device| device.features().contains(wgpu::Features::TIMESTAMP_QUERY))
    {
        crate::scene_target::install_graph(world);
    }
    let _ = world.try_schedule_scope(Core3d, |world, schedule| {
        use bevy::anti_alias::fxaa::fxaa;
        use bevy::core_pipeline::tonemapping::tonemapping;
        use bevy::ecs::schedule::ScheduleCleanupPolicy;
        for (set, system) in [
            (
                tonemapping.into_system_set().intern(),
                profiled(
                    tonemapping,
                    Some(RuntimeStage::GpuTonemapping),
                    "Tonemapping",
                ),
            ),
            (
                fxaa.into_system_set().intern(),
                profiled(fxaa, Some(RuntimeStage::GpuFxaa), "Fxaa").after(tonemapping),
            ),
        ] {
            if schedule
                .remove_systems_in_set(set, world, ScheduleCleanupPolicy::RemoveSystemsOnly)
                .is_ok_and(|count| count != 0)
            {
                schedule.add_systems(system.in_set(Core3dSystems::PostProcess));
            }
        }
    });
}

/// Resolves frame queries after all cameras have recorded their commands.
fn install_readback(world: &mut World) {
    let _ = world.try_schedule_scope(bevy::render::renderer::RenderGraph, |_, schedule| {
        schedule.add_systems(
            encode_readback
                .after(bevy::core_pipeline::schedule::camera_driver)
                .in_set(bevy::render::renderer::RenderGraphSystems::Render),
        );
    });
}

/// Copies completed pass and draw queries into this frame's nonblocking readback slot.
fn encode_readback(world: &World, mut context: RenderContext) {
    if let Some(timestamps) = world.get_resource::<GpuTimestamps>()
        && (timestamps.frame.slot.load(Ordering::Acquire) != NO_SLOT
            || timestamps
                .slots
                .iter()
                .any(|slot| slot.state.load(Ordering::Acquire) == WRITTEN))
    {
        timestamps.encode_readback(context.command_encoder());
    }
}

/// Times `record` as one node-level span of `stage`, for nodes that record several passes.
pub(crate) fn timed<'w, R>(
    world: &World,
    context: &mut RenderContext<'w, '_>,
    stage: RuntimeStage,
    record: impl FnOnce(&mut RenderContext<'w, '_>) -> R,
) -> R {
    let span = world
        .get_resource::<GpuTimestamps>()
        .and_then(|timestamps| timestamps.open_graph_span(stage));
    if let Some((span, _)) = &span {
        mark(context.command_encoder(), span.queries, span.begin);
    }
    let result = record(context);
    if let Some((span, true)) = &span {
        mark(context.command_encoder(), span.queries, span.begin + 1);
    }
    result
}

/// Writes one timestamp with an empty compute pass, valid between any two passes.
fn mark(encoder: &mut wgpu::CommandEncoder, queries: &wgpu::QuerySet, index: u32) {
    encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some("gpu timestamp"),
        timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
            query_set: queries,
            beginning_of_pass_write_index: None,
            end_of_pass_write_index: Some(index),
        }),
    });
}

/// Times draw command `C` as stage `STAGE` (a [`RuntimeStage`] index) inside its pass.
pub(crate) struct GpuDrawSpan<const STAGE: usize, C>(PhantomData<fn() -> C>);

impl<P: PhaseItem, const STAGE: usize, C: RenderCommand<P>> RenderCommand<P>
    for GpuDrawSpan<STAGE, C>
{
    type Param = (Option<SRes<GpuTimestamps>>, C::Param);
    type ViewQuery = C::ViewQuery;
    type ItemQuery = C::ItemQuery;

    fn render<'w>(
        item: &P,
        view: bevy::ecs::query::ROQueryItem<'w, '_, Self::ViewQuery>,
        entity: Option<bevy::ecs::query::ROQueryItem<'w, '_, Self::ItemQuery>>,
        (timestamps, param): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let span = timestamps
            .map(|timestamps| timestamps.into_inner())
            .and_then(|timestamps| timestamps.open_draw(RuntimeStage::ALL[STAGE]));
        if let Some(span) = &span {
            pass.wgpu_pass().write_timestamp(span.queries, span.begin);
        }
        let result = C::render(item, view, entity, param, pass);
        if let Some(span) = &span {
            pass.wgpu_pass()
                .write_timestamp(span.queries, span.begin + 1);
        }
        result
    }
}

/// A begin/end query pair; the end index is `begin + 1`.
struct Span<'a> {
    queries: &'a wgpu::QuerySet,
    begin: u32,
}

/// Lock-free span allocation for the frame being recorded, shared by graph threads.
struct FrameSpans {
    slot: AtomicU32,
    passes: AtomicU32,
    draws: AtomicU32,
    stages: [AtomicU8; SLOT_SPANS as usize],
    /// Whether the frame's first timed node has run.
    started: AtomicBool,
    /// Begin query of a whole-frame span that [`GpuTimestamps::encode_readback`] still has to close.
    open_frame: AtomicU32,
    /// Pass and draw span counts retained for readback, packed high and low, or `NOT_RECORDED`.
    recorded: AtomicU64,
}

struct ReadbackSlot {
    buffer: wgpu::Buffer,
    state: Arc<AtomicU8>,
    passes: u32,
    draws: u32,
    stages: [RuntimeStage; SLOT_SPANS as usize],
}

#[derive(Resource)]
pub(crate) struct GpuTimestamps {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    slots: [ReadbackSlot; SLOTS],
    ring: ReadbackRing,
    period_ns: f32,
    draw_spans: bool,
    ui_categories: bool,
    /// Whether this frame times every timed node rather than only the whole frame.
    pass_detail: bool,
    frame: FrameSpans,
    health: Option<health::QueryHealth>,
}

impl GpuTimestamps {
    /// `None` when the device lacks `TIMESTAMP_QUERY`.
    fn new(device: &RenderDevice, queue: &RenderQueue, profiling: bool) -> Option<Self> {
        let features = device.features();
        if !features.contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        #[cfg(feature = "tracy")]
        tracy::initialize();
        let device = device.wgpu_device();
        let buffer = |label, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: SLOT_BYTES,
                usage,
                mapped_at_creation: false,
            })
        };
        Some(Self {
            queries: device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("gpu timestamps"),
                ty: wgpu::QueryType::Timestamp,
                count: SLOTS as u32 * SLOT_SPANS * 2,
            }),
            resolve: buffer(
                "gpu timestamp resolve",
                wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            ),
            slots: std::array::from_fn(|_| ReadbackSlot {
                buffer: buffer(
                    "gpu timestamp readback",
                    wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                ),
                state: Arc::new(AtomicU8::new(PENDING)),
                passes: 0,
                draws: 0,
                stages: [RuntimeStage::GpuFrame; SLOT_SPANS as usize],
            }),
            ring: ReadbackRing::default(),
            period_ns: queue.get_timestamp_period(),
            draw_spans: profiling
                && features.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES),
            ui_categories: ui_profiling_requested(),
            pass_detail: profiling,
            frame: FrameSpans {
                slot: AtomicU32::new(NO_SLOT),
                passes: AtomicU32::new(0),
                draws: AtomicU32::new(0),
                stages: std::array::from_fn(|_| AtomicU8::new(0)),
                started: AtomicBool::new(false),
                open_frame: AtomicU32::new(NO_SPAN),
                recorded: AtomicU64::new(NOT_RECORDED),
            },
            health: health::QueryHealth::requested(),
        })
    }

    fn open_pass(&self, stage: RuntimeStage) -> Option<Span<'_>> {
        self.open(stage, &self.frame.passes, 0, PASS_SPANS)
    }

    /// A timed node's span and whether the node closes it. With pass detail every node owns a
    /// span; otherwise the first node opens the whole-frame span and later nodes record nothing.
    fn open_graph_span(&self, stage: RuntimeStage) -> Option<(Span<'_>, bool)> {
        if !self.graph_span_enabled(stage) {
            return None;
        }
        if self.pass_detail {
            return self.open_pass(stage).map(|span| (span, true));
        }
        if self.frame.started.swap(true, Ordering::AcqRel) {
            return None;
        }
        let span = self.open_pass(RuntimeStage::GpuFrame)?;
        self.frame.open_frame.store(span.begin, Ordering::Release);
        Some((span, false))
    }

    fn open_draw(&self, stage: RuntimeStage) -> Option<Span<'_>> {
        self.draw_spans
            .then(|| self.open(stage, &self.frame.draws, PASS_SPANS, DRAW_SPANS))
            .flatten()
    }

    fn open(
        &self,
        stage: RuntimeStage,
        counter: &AtomicU32,
        offset: u32,
        capacity: u32,
    ) -> Option<Span<'_>> {
        let slot = self.frame.slot.load(Ordering::Acquire);
        if slot == NO_SLOT {
            return None;
        }
        let index = counter.fetch_add(1, Ordering::Relaxed);
        if index >= capacity {
            return None;
        }
        let span = offset + index;
        self.frame.stages[span as usize].store(stage as u8, Ordering::Relaxed);
        Some(Span {
            queries: &self.queries,
            begin: (slot * SLOT_SPANS + span) * 2,
        })
    }

    /// Hands mapped frames to `sink` oldest first, then claims a slot for this frame.
    fn begin(&mut self, mut sink: impl FnMut(&GpuFrameTimes)) {
        #[cfg(feature = "tracy")]
        let _zone = bevy::log::info_span!("gpu.timestamps.readback").entered();
        while let Some(index) = self.ring.oldest_in_flight() {
            let slot = &self.slots[index];
            match slot.state.load(Ordering::Acquire) {
                PENDING | WRITTEN | RESOLVED => break,
                MAPPED => {
                    if let Some(health) = &mut self.health {
                        health.readback();
                    }
                    let bytes = slot
                        .buffer
                        .slice(..)
                        .get_mapped_range()
                        .expect("readback buffer is mapped");
                    let tick = |query: u32| {
                        let start = query as usize * TIMESTAMP_BYTES as usize;
                        u64::from_le_bytes(
                            bytes[start..start + TIMESTAMP_BYTES as usize]
                                .try_into()
                                .expect("timestamp is eight bytes"),
                        )
                    };
                    let passes = (0..slot.passes).map(|span| span * 2);
                    let draws = (0..slot.draws).map(|span| (PASS_SPANS + span) * 2);
                    let frame = decode_spans(
                        passes.chain(draws).map(|query| {
                            let stage = slot.stages[query as usize / 2];
                            let (begin, end) = (tick(query), tick(query + 1));
                            if let Some(health) = &mut self.health {
                                health.sample(stage, begin, end);
                            }
                            (stage, begin, end)
                        }),
                        self.period_ns,
                        !cfg!(target_os = "macos"),
                    );
                    drop(bytes);
                    slot.buffer.unmap();
                    #[cfg(feature = "tracy")]
                    tracy::record(&frame);
                    sink(&frame);
                }
                _ => {
                    if let Some(health) = &mut self.health {
                        health.map_failure();
                    }
                }
            }
            slot.state.store(PENDING, Ordering::Relaxed);
            self.ring.release(index);
        }
        let slot = self.ring.acquire().map_or(NO_SLOT, |slot| slot as u32);
        if let Some(health) = &mut self.health {
            health.frame(slot == NO_SLOT);
        }
        self.frame.passes.store(0, Ordering::Relaxed);
        self.frame.draws.store(0, Ordering::Relaxed);
        self.frame.started.store(false, Ordering::Relaxed);
        self.frame.open_frame.store(NO_SPAN, Ordering::Relaxed);
        self.frame.recorded.store(NOT_RECORDED, Ordering::Relaxed);
        self.frame.slot.store(slot, Ordering::Release);
    }

    /// Records this frame's query ranges and copies readable spans without an extra submission.
    /// Metal counter resolves can overtake fragment samples, so only completed frames are copied.
    fn encode_readback(&self, encoder: &mut wgpu::CommandEncoder) {
        #[cfg(feature = "tracy")]
        let _zone = bevy::log::info_span!("gpu.timestamps.resolve").entered();
        if cfg!(target_os = "macos") {
            for (index, target) in self.slots.iter().enumerate() {
                if target.state.load(Ordering::Acquire) == WRITTEN {
                    self.resolve_slot(encoder, index, target.passes, target.draws);
                    target.state.store(RESOLVED, Ordering::Release);
                }
            }
        }
        let slot = self.frame.slot.load(Ordering::Acquire);
        if slot == NO_SLOT {
            return;
        }
        let passes = self.frame.passes.load(Ordering::Relaxed).min(PASS_SPANS);
        let draws = self.frame.draws.load(Ordering::Relaxed);
        let draws = if draws > DRAW_SPANS { 0 } else { draws };
        if passes == 0 && draws == 0 {
            return;
        }
        let open_frame = self.frame.open_frame.swap(NO_SPAN, Ordering::AcqRel);
        if open_frame != NO_SPAN {
            mark(encoder, &self.queries, open_frame + 1);
        }
        if !cfg!(target_os = "macos") {
            self.resolve_slot(encoder, slot as usize, passes, draws);
        }
        self.frame.recorded.store(
            u64::from(passes) << 32 | u64::from(draws),
            Ordering::Release,
        );
    }

    /// Copies one slot's retained query ranges into its own readback buffer.
    fn resolve_slot(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        slot: usize,
        passes: u32,
        draws: u32,
    ) {
        let base = slot as u32 * SLOT_SPANS * 2;
        if passes > 0 {
            encoder.resolve_query_set(&self.queries, base..base + passes * 2, &self.resolve, 0);
        }
        if draws > 0 {
            let first = base + PASS_SPANS * 2;
            encoder.resolve_query_set(
                &self.queries,
                first..first + draws * 2,
                &self.resolve,
                u64::from(PASS_SPANS) * 2 * TIMESTAMP_BYTES,
            );
        }
        let target = &self.slots[slot];
        encoder.copy_buffer_to_buffer(&self.resolve, 0, &target.buffer, 0, SLOT_BYTES);
    }

    /// Maps submitted copies and retains Metal query slots until their writing submission completes.
    /// A frame that recorded no queries gives its slot back; rendering never waits on the GPU.
    fn request_readback(&mut self, queue: &RenderQueue) {
        if cfg!(target_os = "macos") {
            for target in &self.slots {
                if target.state.load(Ordering::Acquire) == RESOLVED {
                    map_readback(target);
                }
            }
        }
        let slot = self.frame.slot.swap(NO_SLOT, Ordering::AcqRel);
        if slot == NO_SLOT {
            return;
        }
        let index = slot as usize;
        let recorded = self.frame.recorded.swap(NOT_RECORDED, Ordering::AcqRel);
        if recorded == NOT_RECORDED {
            self.ring.release(index);
            return;
        }
        let (passes, draws) = ((recorded >> 32) as u32, recorded as u32);
        let target = &mut self.slots[index];
        target.passes = passes;
        target.draws = draws;
        for span in (0..passes).chain(PASS_SPANS..PASS_SPANS + draws) {
            let stage = self.frame.stages[span as usize].load(Ordering::Relaxed);
            target.stages[span as usize] = RuntimeStage::ALL[stage as usize];
        }
        if cfg!(target_os = "macos") {
            target.state.store(PENDING, Ordering::Relaxed);
            let state = target.state.clone();
            crate::device_poll::on_frame_complete(queue, move || {
                state.store(WRITTEN, Ordering::Release);
            });
        } else {
            map_readback(target);
        }
        self.ring.submit(index);
    }
}

/// Maps a submitted copy and publishes completion only after its bytes are readable.
fn map_readback(target: &ReadbackSlot) {
    #[cfg(feature = "tracy")]
    let _zone = bevy::log::info_span!("gpu.timestamps.map_request").entered();
    target.state.store(PENDING, Ordering::Relaxed);
    let state = target.state.clone();
    target
        .buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            state.store(
                if result.is_ok() { MAPPED } else { FAILED },
                Ordering::Release,
            );
        });
}

fn init_gpu_timestamps(
    mut commands: Commands,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    profiler: Res<RuntimeStageProfiler>,
    categories: Option<Res<categories::CategoryProfiling>>,
) {
    match GpuTimestamps::new(&device, &queue, profiler.enabled()) {
        Some(mut timestamps) => {
            // Category passes already split the opaque phase; in-pass spans would double count.
            timestamps.draw_spans &= categories.is_none();
            commands.insert_resource(timestamps);
        }
        None => info!("GPU timestamps unsupported by this adapter; gpu_* stages stay empty"),
    }
}

fn begin_gpu_frame(
    timestamps: Option<ResMut<GpuTimestamps>>,
    profiler: Res<RuntimeStageProfiler>,
    detail: Option<Res<DetailedGpuTiming>>,
    categories: Option<Res<categories::CategoryProfiling>>,
) {
    let Some(mut timestamps) = timestamps else {
        return;
    };
    timestamps.pass_detail = profiler.enabled()
        || timestamps.ui_categories
        || categories.is_some()
        || detail.is_some_and(|detail| detail.0);
    // Reads slots mapped by the previous frame's device poll.
    timestamps.begin(|frame| profiler.record_gpu_frame(frame));
}

fn request_gpu_frame_readback(timestamps: Option<ResMut<GpuTimestamps>>, queue: Res<RenderQueue>) {
    if let Some(mut timestamps) = timestamps {
        timestamps.request_readback(&queue);
    }
}
