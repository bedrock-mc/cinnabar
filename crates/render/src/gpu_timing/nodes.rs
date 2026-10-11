//! Opt-in GPU time for render passes, plotted in Tracy as `gpu node <schedule>/<pass>`.
//! `RUST_MCBE_GPU_NODES=1` adds timestamps between passes. These measure elapsed GPU
//! latency, including overlap and idle gaps; their empty passes are for attribution.
//! Results arrive a few frames late through the shared nonblocking device poll.

use super::{
    SLOTS, mark,
    readback::{ReadbackRing, SpanValidity, span_validity, ticks_to_duration},
};
use crate::device_poll::FrameSubmissions;
use bevy::{
    prelude::{
        IntoScheduleConfigs, Local, ResMut, Resource, Result, SubApp, World, info, resource_exists,
    },
    render::{
        Render, RenderStartup, RenderSystems,
        renderer::{RenderContext, RenderDevice, RenderQueue},
    },
};
use std::sync::{
    Arc, Mutex, PoisonError,
    atomic::{AtomicU8, AtomicU16, AtomicU32, AtomicU64, Ordering},
};
use tracy_client::{Client, PlotName};

/// Node spans per frame; nodes past it go untimed for that frame.
const CAPACITY: u32 = 128;
const SLOT_BYTES: u64 = CAPACITY as u64 * 2 * 8;
const NO_SLOT: u32 = u32::MAX;
const PENDING: u8 = 0;
const MAPPED: u8 = 1;
const FAILED: u8 = 2;

/// Reads only the opt-in flag.
pub(super) fn requested() -> bool {
    std::env::var_os("RUST_MCBE_GPU_NODES").is_some_and(|value| value == "1")
}

/// Installs opt-in pass plots and their nonblocking readback.
pub(super) fn install(render_app: &mut SubApp) {
    render_app.add_systems(RenderStartup, initialize);
    render_app.add_systems(
        Render,
        (
            begin_frame
                .run_if(resource_exists::<NodeTimer>)
                .in_set(RenderSystems::PrepareResources),
            request_readback
                .run_if(resource_exists::<NodeTimer>)
                .in_set(FrameSubmissions),
        ),
    );
}

struct Slot {
    buffer: wgpu::Buffer,
    state: Arc<AtomicU8>,
    spans: u32,
    nodes: [u16; CAPACITY as usize],
}

#[derive(Resource)]
struct NodeTimer {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    slots: [Slot; SLOTS],
    ring: ReadbackRing,
    period_ns: f32,
    /// Node plots first, then section plots in registration order.
    plots: Mutex<Vec<PlotName>>,
    /// Section names and their plot indices, registered on first use.
    sections: Mutex<Vec<(&'static str, u16)>>,
    passes: Mutex<Vec<(&'static str, u16)>>,
    /// Slot of the frame being recorded, or `NO_SLOT`.
    slot: AtomicU32,
    spans: AtomicU32,
    nodes: [AtomicU16; CAPACITY as usize],
    /// Spans the frame resolved, or `u64::MAX` before the resolve node ran.
    resolved: AtomicU64,
}

impl NodeTimer {
    /// The begin query of a new span for plot `index`, while this frame has a slot and room.
    fn open(&self, index: u16) -> Option<u32> {
        let slot = self.slot.load(Ordering::Acquire);
        if slot == NO_SLOT {
            return None;
        }
        let span = self.spans.fetch_add(1, Ordering::Relaxed);
        if span >= CAPACITY {
            return None;
        }
        self.nodes[span as usize].store(index, Ordering::Relaxed);
        Some((slot * CAPACITY + span) * 2)
    }

    /// The plot index of section `name`, adding its plot on first use.
    fn section_index(&self, name: &'static str) -> Option<u16> {
        let mut sections = self.sections.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((_, index)) = sections.iter().find(|(known, _)| *known == name) {
            return Some(*index);
        }
        let mut plots = self.plots.lock().unwrap_or_else(PoisonError::into_inner);
        let index = u16::try_from(plots.len()).ok()?;
        plots.push(PlotName::new_leak(format!("gpu section {name} ms")));
        sections.push((name, index));
        Some(index)
    }
}

/// A GPU section's claimed query pair; claim it while the node runs, then write its marks
/// into whichever encoder records the section, including a deferred command-buffer task.
#[derive(Clone, Copy)]
pub(crate) struct SectionSpan<'w> {
    queries: &'w wgpu::QuerySet,
    pub(super) begin: u32,
}

impl SectionSpan<'_> {
    /// Claims a span for section `name`, plotted as `gpu section <name> ms`, while node
    /// timing is on. Spans claimed after the graph finished would never be resolved.
    pub(crate) fn claim<'w>(world: &'w World, name: &'static str) -> Option<SectionSpan<'w>> {
        let timer = world.get_resource::<NodeTimer>()?;
        let begin = timer.open(timer.section_index(name)?)?;
        Some(SectionSpan {
            queries: &timer.queries,
            begin,
        })
    }

    pub(crate) fn begin(&self, encoder: &mut wgpu::CommandEncoder) {
        mark(encoder, self.queries, self.begin);
    }

    pub(crate) fn end(&self, encoder: &mut wgpu::CommandEncoder) {
        mark(encoder, self.queries, self.begin + 1);
    }
}

/// Claims the current pass's plot without changing command submission order.
pub(super) fn open_node<'w>(world: &'w World, name: &'static str) -> Option<SectionSpan<'w>> {
    let timer = world.get_resource::<NodeTimer>()?;
    let index = {
        let mut passes = timer.passes.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((_, index)) = passes.iter().find(|(known, _)| *known == name) {
            *index
        } else {
            let mut plots = timer.plots.lock().unwrap_or_else(PoisonError::into_inner);
            let index = u16::try_from(plots.len()).ok()?;
            let path = if name.contains('/') {
                name.to_owned()
            } else {
                format!("Core3d/{name}")
            };
            plots.push(PlotName::new_leak(format!("gpu node {path} ms")));
            passes.push((name, index));
            index
        }
    };
    Some(SectionSpan {
        queries: &timer.queries,
        begin: timer.open(index)?,
    })
}

/// Closes a pass plot even when the draw was skipped by its camera query.
pub(super) fn close_node(world: &World, context: &mut RenderContext, begin: u32) {
    if begin != super::NO_SPAN
        && let Some(timer) = world.get_resource::<NodeTimer>()
    {
        mark(context.command_encoder(), &timer.queries, begin + 1);
    }
}

/// Allocates timing storage once and resolves it after every camera.
fn initialize(world: &mut World) {
    let device = world.resource::<RenderDevice>();
    if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
        info!("GPU node timing unavailable: no timestamp queries");
        return;
    }
    let timer = timer(
        device,
        world.resource::<RenderQueue>().get_timestamp_period(),
    );
    world.insert_resource(timer);
    install_stock_passes(world);
    let _ = world.try_schedule_scope(bevy::render::renderer::RenderGraph, |_, schedule| {
        schedule.add_systems(
            resolve
                .after(bevy::core_pipeline::schedule::camera_driver)
                .in_set(bevy::render::renderer::RenderGraphSystems::Render),
        );
    });
}

/// Keeps stock post-processing passes visible in opt-in pass timing.
fn install_stock_passes(world: &mut World) {
    use bevy::{
        core_pipeline::{Core3d, Core3dSystems},
        post_process::bloom::bloom,
    };
    let _ = world.try_schedule_scope(Core3d, |world, schedule| {
        if schedule
            .remove_systems_in_set(
                bloom,
                world,
                bevy::ecs::schedule::ScheduleCleanupPolicy::RemoveSystemsOnly,
            )
            .is_ok_and(|count| count != 0)
        {
            schedule.add_systems(
                super::profiled(bloom, None, "Bloom")
                    .before(bevy::core_pipeline::tonemapping::tonemapping)
                    .in_set(Core3dSystems::PostProcess),
            );
        }
    });
}

/// Copies the frame's pass spans after all camera systems have finished recording.
fn resolve(timer: Res<NodeTimer>, mut context: RenderContext) {
    let slot = timer.slot.load(Ordering::Acquire);
    let spans = timer.spans.load(Ordering::Relaxed).min(CAPACITY);
    if slot == NO_SLOT || spans == 0 {
        return;
    }
    let encoder = context.command_encoder();
    let base = slot * CAPACITY * 2;
    encoder.resolve_query_set(&timer.queries, base..base + spans * 2, &timer.resolve, 0);
    encoder.copy_buffer_to_buffer(
        &timer.resolve,
        0,
        &timer.slots[slot as usize].buffer,
        0,
        u64::from(spans) * 2 * 8,
    );
    timer.resolved.store(u64::from(spans), Ordering::Release);
}

/// Builds bounded query storage for the configured adapter.
fn timer(device: &RenderDevice, period_ns: f32) -> NodeTimer {
    let device = device.wgpu_device();
    let buffer = |label, usage| {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: SLOT_BYTES,
            usage,
            mapped_at_creation: false,
        })
    };
    NodeTimer {
        queries: device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("gpu node timestamps"),
            ty: wgpu::QueryType::Timestamp,
            count: SLOTS as u32 * CAPACITY * 2,
        }),
        resolve: buffer(
            "gpu node timestamp resolve",
            wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        ),
        slots: std::array::from_fn(|_| Slot {
            buffer: buffer(
                "gpu node timestamp readback",
                wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            ),
            state: Arc::new(AtomicU8::new(PENDING)),
            spans: 0,
            nodes: [0; CAPACITY as usize],
        }),
        ring: ReadbackRing::default(),
        period_ns,
        plots: Mutex::new(Vec::new()),
        sections: Mutex::new(Vec::new()),
        passes: Mutex::new(Vec::new()),
        slot: AtomicU32::new(NO_SLOT),
        spans: AtomicU32::new(0),
        nodes: std::array::from_fn(|_| AtomicU16::new(0)),
        resolved: AtomicU64::new(u64::MAX),
    }
}

/// Plots frames mapped by earlier polls, oldest first, then claims a slot for this frame.
fn begin_frame(mut timer: ResMut<NodeTimer>) {
    let timer = &mut *timer;
    let client = Client::running();
    let plots = timer
        .plots
        .get_mut()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let mut totals = vec![0_u64; plots.len()];
    while let Some(index) = timer.ring.oldest_in_flight() {
        let slot = &timer.slots[index];
        match slot.state.load(Ordering::Acquire) {
            PENDING => break,
            MAPPED => {
                totals.fill(0);
                let bytes = slot
                    .buffer
                    .slice(..u64::from(slot.spans) * 2 * 8)
                    .get_mapped_range();
                let tick = |query: usize| {
                    u64::from_le_bytes(bytes[query * 8..query * 8 + 8].try_into().unwrap())
                };
                for span in 0..slot.spans as usize {
                    let (begin, end) = (tick(span * 2), tick(span * 2 + 1));
                    if span_validity(begin, end) == SpanValidity::Valid
                        && let Some(total) = totals.get_mut(usize::from(slot.nodes[span]))
                    {
                        *total += end - begin;
                    }
                }
                drop(bytes);
                slot.buffer.unmap();
                if let Some(client) = &client {
                    for (plot, ticks) in plots.iter().zip(&totals) {
                        if *ticks != 0 {
                            let ms = ticks_to_duration(*ticks, timer.period_ns).as_secs_f64() * 1e3;
                            client.plot(*plot, ms);
                        }
                    }
                }
            }
            _ => {}
        }
        timer.slots[index].state.store(PENDING, Ordering::Relaxed);
        timer.ring.release(index);
    }
    let slot = timer.ring.acquire().map_or(NO_SLOT, |slot| slot as u32);
    timer.spans.store(0, Ordering::Relaxed);
    timer.resolved.store(u64::MAX, Ordering::Relaxed);
    timer.slot.store(slot, Ordering::Release);
}

/// Maps the slot this frame resolved into after its submission; a frame that resolved
/// nothing returns its slot.
fn request_readback(mut timer: ResMut<NodeTimer>) {
    let timer = &mut *timer;
    let slot = timer.slot.swap(NO_SLOT, Ordering::AcqRel);
    if slot == NO_SLOT {
        return;
    }
    let index = slot as usize;
    let resolved = timer.resolved.swap(u64::MAX, Ordering::AcqRel);
    if resolved == u64::MAX {
        timer.ring.release(index);
        return;
    }
    let target = &mut timer.slots[index];
    target.spans = resolved as u32;
    for (node, span) in target
        .nodes
        .iter_mut()
        .zip(&timer.nodes)
        .take(resolved as usize)
    {
        *node = span.load(Ordering::Relaxed);
    }
    let state = target.state.clone();
    target
        .buffer
        .slice(..u64::from(target.spans) * 2 * 8)
        .map_async(wgpu::MapMode::Read, move |result| {
            state.store(
                if result.is_ok() { MAPPED } else { FAILED },
                Ordering::Release,
            );
        });
    timer.ring.submit(index);
}
