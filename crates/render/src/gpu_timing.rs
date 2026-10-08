//! Nonblocking GPU timing sums elapsed pass latencies, including overlap and gaps, not active work.
//! Metal times owned passes and the stock opaque pass; whole-frame and shared draw categories stay absent.

mod categories;
mod health;
#[cfg(all(test, target_os = "macos"))]
mod metal_tests;
mod opaque;
mod overdraw;
mod pass;
pub(crate) mod readback;
#[cfg(test)]
mod tests;
#[cfg(feature = "tracy")]
mod tracy;

use crate::{RuntimeStage, RuntimeStageProfiler};
use bevy::{
    core_pipeline::core_3d::graph::{Core3d, Node3d},
    ecs::system::{SystemParamItem, lifetimeless::SRes},
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        render_graph::{
            EmptyNode, InternedRenderLabel, Node, NodeRunError, RenderGraph, RenderGraphContext,
            RenderLabel, SlotInfo,
        },
        render_phase::{PhaseItem, RenderCommand, RenderCommandResult, TrackedRenderPass},
        renderer::{RenderContext, RenderDevice, RenderQueue, render_system},
    },
};
use readback::{ReadbackRing, SLOTS};
use std::{
    marker::PhantomData,
    sync::{
        Arc,
        atomic::{AtomicU8, AtomicU32, Ordering},
    },
};

pub(crate) use pass::{render_pass_timestamps, ui_pass_timestamps, ui_profiling_requested};
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

const PENDING: u8 = 0;
const MAPPED: u8 = 1;
const FAILED: u8 = 2;

/// Feeds `gpu_*` stages into the [`RuntimeStageProfiler`] already present in the app.
pub struct GpuTimingPlugin;

impl Plugin for GpuTimingPlugin {
    fn build(&self, app: &mut App) {
        let Some(profiler) = app.world().get_resource::<RuntimeStageProfiler>().cloned() else {
            return;
        };
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
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
            .add_systems(RenderStartup, (init_gpu_timestamps, wrap_timed_nodes))
            .add_systems(
                Render,
                (
                    begin_gpu_frame.in_set(RenderSystems::PrepareResources),
                    submit_gpu_frame
                        .in_set(RenderSystems::Render)
                        .after(render_system),
                ),
            );
        #[cfg(feature = "tracy")]
        tracy::install(render_app);
    }
}

/// The timed Core3d nodes; absent labels are skipped.
fn timed_nodes() -> Vec<(InternedRenderLabel, RuntimeStage)> {
    use crate::ui_render::{UiOverlayLabel, UiWorldLabel, overlay::UiOverlayPostLabel};
    let mut nodes = vec![
        (Node3d::MainOpaquePass.intern(), RuntimeStage::GpuOpaque),
        (Node3d::EndMainPass.intern(), RuntimeStage::GpuBlit),
        (
            crate::chunk::TerrainPassLabel.intern(),
            RuntimeStage::GpuOpaque,
        ),
        (
            Node3d::MainTransparentPass.intern(),
            RuntimeStage::GpuTransparent,
        ),
        (UiWorldLabel.intern(), RuntimeStage::GpuUi),
        (UiOverlayLabel.intern(), RuntimeStage::GpuUi),
        (UiOverlayPostLabel.intern(), RuntimeStage::GpuUi),
        (
            crate::viewmodel_render::HandLabel.intern(),
            RuntimeStage::GpuHand,
        ),
        (
            crate::hand_rig_render::HandRigLabel.intern(),
            RuntimeStage::GpuHand,
        ),
        (Node3d::Tonemapping.intern(), RuntimeStage::GpuTonemapping),
        (Node3d::Fxaa.intern(), RuntimeStage::GpuFxaa),
    ];
    // Timestamps written just before presentation make macOS 26 flicker.
    if !cfg!(target_os = "macos") {
        nodes.push((Node3d::Upscaling.intern(), RuntimeStage::GpuBlit));
    }
    #[cfg(feature = "enhanced")]
    nodes.extend(crate::enhanced::graph::timed_nodes());
    nodes
}

fn wrap_timed_nodes(world: &mut World) {
    let mut replacement = categories::replacement(world);
    let categories = replacement.is_some();
    replacement = replacement.or_else(|| opaque::replacement(world));
    let Some(mut graphs) = world.get_resource_mut::<RenderGraph>() else {
        return;
    };
    let Some(graph) = graphs.get_sub_graph_mut(Core3d) else {
        return;
    };
    for (label, stage) in timed_nodes() {
        let Ok(state) = graph.get_node_state_mut(label) else {
            continue;
        };
        // Replacing the node alone preserves its slots and edges.
        let mut inner = std::mem::replace(&mut state.node, Box::new(EmptyNode));
        if label == Node3d::MainOpaquePass.intern()
            && categories::replaceable(&*inner, categories)
            && let Some(replacement) = replacement.take()
        {
            inner = replacement;
        }
        state.node = Box::new(TimedNode { inner, stage });
    }
}

struct TimedNode {
    inner: Box<dyn Node>,
    stage: RuntimeStage,
}

impl Node for TimedNode {
    fn input(&self) -> Vec<SlotInfo> {
        self.inner.input()
    }

    fn output(&self) -> Vec<SlotInfo> {
        self.inner.output()
    }

    fn update(&mut self, world: &mut World) {
        self.inner.update(world);
    }

    fn run<'w>(
        &self,
        graph: &mut RenderGraphContext,
        render_context: &mut RenderContext<'w>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let span = world
            .get_resource::<GpuTimestamps>()
            .filter(|timestamps| timestamps.graph_span_enabled(self.stage))
            .and_then(|timestamps| timestamps.open_pass(self.stage));
        if let Some(span) = &span {
            mark(render_context, span.queries, span.begin);
        }
        let result = self.inner.run(graph, render_context, world);
        if let Some(span) = &span {
            mark(render_context, span.queries, span.begin + 1);
        }
        result
    }
}

/// Times `record` as one node-level span of `stage`, for nodes that record several passes.
pub(crate) fn timed<'w, R>(
    world: &World,
    context: &mut RenderContext<'w>,
    stage: RuntimeStage,
    record: impl FnOnce(&mut RenderContext<'w>) -> R,
) -> R {
    let span = world
        .get_resource::<GpuTimestamps>()
        .filter(|timestamps| timestamps.graph_span_enabled(stage))
        .and_then(|timestamps| timestamps.open_pass(stage));
    if let Some(span) = &span {
        mark(context, span.queries, span.begin);
    }
    let result = record(context);
    if let Some(span) = &span {
        mark(context, span.queries, span.begin + 1);
    }
    result
}

/// Writes one timestamp with an empty compute pass, valid between any two passes.
fn mark(context: &mut RenderContext, queries: &wgpu::QuerySet, index: u32) {
    context
        .command_encoder()
        .begin_compute_pass(&wgpu::ComputePassDescriptor {
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
            frame: FrameSpans {
                slot: AtomicU32::new(NO_SLOT),
                passes: AtomicU32::new(0),
                draws: AtomicU32::new(0),
                stages: std::array::from_fn(|_| AtomicU8::new(0)),
            },
            health: health::QueryHealth::requested(),
        })
    }

    fn open_pass(&self, stage: RuntimeStage) -> Option<Span<'_>> {
        self.open(stage, &self.frame.passes, 0, PASS_SPANS)
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
                PENDING => break,
                MAPPED => {
                    if let Some(health) = &mut self.health {
                        health.readback();
                    }
                    let bytes = slot.buffer.slice(..).get_mapped_range();
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
        self.frame.slot.store(slot, Ordering::Release);
    }

    /// Resolves this frame's spans and maps them asynchronously; nothing waits on the GPU.
    fn submit(&mut self, device: &RenderDevice, queue: &RenderQueue) {
        #[cfg(feature = "tracy")]
        let _zone = bevy::log::info_span!("gpu.timestamps.resolve_submit").entered();
        let slot = self.frame.slot.swap(NO_SLOT, Ordering::AcqRel);
        if slot == NO_SLOT {
            return;
        }
        let passes = self.frame.passes.load(Ordering::Relaxed).min(PASS_SPANS);
        let draws = self.frame.draws.load(Ordering::Relaxed);
        let draws = if draws > DRAW_SPANS { 0 } else { draws };
        let index = slot as usize;
        if passes == 0 && draws == 0 {
            self.ring.release(index);
            return;
        }
        let base = slot * SLOT_SPANS * 2;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("gpu timestamp readback"),
        });
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
        let target = &mut self.slots[index];
        encoder.copy_buffer_to_buffer(&self.resolve, 0, &target.buffer, 0, SLOT_BYTES);
        let command = encoder.finish();
        {
            #[cfg(feature = "tracy")]
            let _zone = bevy::log::info_span!("gpu.timestamps.queue_submit", slot).entered();
            queue.submit([command]);
        }
        target.passes = passes;
        target.draws = draws;
        for span in (0..passes).chain(PASS_SPANS..PASS_SPANS + draws) {
            let stage = self.frame.stages[span as usize].load(Ordering::Relaxed);
            target.stages[span as usize] = RuntimeStage::ALL[stage as usize];
        }
        let state = target.state.clone();
        let slice = target.buffer.slice(..);
        {
            #[cfg(feature = "tracy")]
            let _zone = bevy::log::info_span!("gpu.timestamps.map_request", slot).entered();
            slice.map_async(wgpu::MapMode::Read, move |result| {
                state.store(
                    if result.is_ok() { MAPPED } else { FAILED },
                    Ordering::Release,
                );
            });
        }
        self.ring.submit(index);
    }
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
    device: Res<RenderDevice>,
    profiler: Res<RuntimeStageProfiler>,
) {
    let Some(mut timestamps) = timestamps else {
        return;
    };
    // Non-blocking: only fires map callbacks the GPU has already completed.
    {
        #[cfg(feature = "tracy")]
        let _zone = bevy::log::info_span!("gpu.timestamps.device_poll").entered();
        let _ = device.poll(wgpu::PollType::Poll);
    }
    timestamps.begin(|frame| profiler.record_gpu_frame(frame));
}

fn submit_gpu_frame(
    timestamps: Option<ResMut<GpuTimestamps>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    if let Some(mut timestamps) = timestamps {
        timestamps.submit(&device, &queue);
    }
}
