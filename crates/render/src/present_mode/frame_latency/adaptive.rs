//! Uses coarse work estimates for probes and acquisition cadence to recover from bad estimates.

use std::time::{Duration, Instant};

use bevy::{
    ecs::{
        schedule::{ScheduleCleanupPolicy, ScheduleConfigs, ScheduleError},
        system::{IntoSystem, ScheduleSystem},
    },
    prelude::*,
    render::{
        Extract, ExtractSchedule, Render, RenderStartup, RenderSystems,
        renderer::RenderAdapter,
        view::window::{ExtractedWindow, prepare_windows},
    },
    window::{PresentMode, PrimaryWindow, WindowOccluded},
};
use render_model::{DisplayTiming, FrameLatencyPolicy, FrameWorkSample, VrrStatus};

use crate::present_mode::PresentModePolicy;

/// A context change discards evidence from the old display, window, size or delivery mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Context {
    window: Entity,
    size: [u32; 2],
    mode: PresentMode,
    display: DisplayTiming,
    active: bool,
    cap: Option<render_model::FrameRate>,
}

/// Frame-tagged CPU work retained long enough to match the nonblocking GPU readback ring.
#[derive(Resource)]
pub(crate) struct AdaptiveFrameLatency {
    epoch: Instant,
    context: Option<Context>,
    model: FrameLatencyPolicy,
    frame: u64,
    started: Option<Instant>,
    surface_started: Option<Instant>,
    surface_elapsed: Duration,
    surface_had_texture: bool,
    last_acquired: Option<(Instant, Option<std::num::NonZeroU32>)>,
    cpu: [Option<FrameWorkSample>; crate::gpu_timing::readback::SLOTS + 1],
}

impl Default for AdaptiveFrameLatency {
    fn default() -> Self {
        Self {
            epoch: Instant::now(),
            context: None,
            model: FrameLatencyPolicy::default(),
            frame: 0,
            started: None,
            surface_started: None,
            surface_elapsed: Duration::ZERO,
            surface_had_texture: false,
            last_acquired: None,
            cpu: [None; crate::gpu_timing::readback::SLOTS + 1],
        }
    }
}

impl AdaptiveFrameLatency {
    /// Identifies this render schedule for its query readback slot.
    pub(crate) fn frame(&self) -> u64 {
        self.frame
    }

    /// Excludes the surface-reconfiguration hitch from workload evidence.
    pub(crate) fn discard_current_frame(&mut self) {
        self.started = None;
        self.last_acquired = None;
        self.cpu.fill(None);
    }

    /// Retains coarse feeding work on the caller's clock, excluding acquisition waits.
    fn finish(&mut self, now: Instant) {
        let Some(started) = self.started.take() else {
            return;
        };
        let index = self.frame as usize % self.cpu.len();
        self.cpu[index] = Some(FrameWorkSample {
            frame: self.frame,
            at: now.saturating_duration_since(self.epoch),
            render: now
                .saturating_duration_since(started)
                .saturating_sub(self.surface_elapsed),
            gpu: Duration::ZERO,
        });
    }

    /// Matches a coarse GPU span to CPU work; missing readbacks leave cadence and backoff intact.
    pub(crate) fn record_gpu(
        &mut self,
        frame: u64,
        gpu: Option<Duration>,
        now: Instant,
        policy: &PresentModePolicy,
    ) {
        let index = frame as usize % self.cpu.len();
        let Some(mut sample) = self.cpu[index].filter(|sample| sample.frame == frame) else {
            return;
        };
        self.cpu[index] = None;
        let Some(gpu) = gpu else {
            return;
        };
        sample.gpu = gpu;
        let refresh = self.refresh();
        let selected = self.model.update(
            now.saturating_duration_since(self.epoch),
            refresh,
            None,
            Some(sample),
        );
        policy.publish_frame_latency(selected);
    }

    /// Observes successful acquisitions at the depth the surface has actually adopted.
    fn record_acquisition(
        &mut self,
        now: Instant,
        depth: Option<std::num::NonZeroU32>,
        policy: &PresentModePolicy,
    ) {
        let selected = self.model.latency();
        let cadence = self
            .last_acquired
            .replace((now, depth))
            .filter(|(_, previous)| *previous == depth && depth == Some(selected))
            .map(|(last, _)| now.saturating_duration_since(last));
        let latency = self.model.update(
            now.saturating_duration_since(self.epoch),
            self.refresh(),
            cadence,
            None,
        );
        policy.publish_frame_latency(latency);
    }

    /// The actual fixed-refresh interval, without substituting a software frame cap.
    fn refresh(&self) -> Option<render_model::FrameRate> {
        self.context
            .filter(|context| {
                context.active
                    && context.display.vrr != VrrStatus::Active
                    && matches!(context.mode, PresentMode::Fifo | PresentMode::FifoRelaxed)
            })
            .and_then(|context| {
                context
                    .display
                    .refresh
                    .filter(|refresh| context.cap.is_none_or(|cap| cap >= *refresh))
            })
    }
}

/// Installs Windows measurements independently of input-admission prediction.
pub(super) fn install(app: &mut SubApp) {
    app.init_resource::<AdaptiveFrameLatency>()
        .add_systems(ExtractSchedule, extract_context)
        .add_systems(RenderStartup, install_acquisition_timing)
        .add_systems(
            Render,
            (
                begin_frame.before(RenderSystems::ExtractCommands),
                end_frame.after(RenderSystems::PostCleanup),
            ),
        );
}

/// Replaces window preparation once all plugins have registered their render systems.
fn install_acquisition_timing(world: &mut World) {
    world.schedule_scope(Render, |world, schedule| {
        let removed = match schedule.remove_systems_in_set(
            prepare_windows,
            world,
            ScheduleCleanupPolicy::RemoveSystemsOnly,
        ) {
            Ok(removed) => removed,
            Err(ScheduleError::SetNotFound) => 0,
            Err(error) => panic!("window preparation cannot be timed: {error}"),
        };
        if removed != 0 {
            schedule.add_systems(
                measure_acquisition(prepare_windows, Instant::now)
                    .in_set(RenderSystems::PrepareViews),
            );
        }
    });
}

/// Times window acquisition using the caller's clock while retaining the wrapped system's sets.
fn measure_acquisition<S, M, C>(system: S, now: C) -> ScheduleConfigs<ScheduleSystem>
where
    S: IntoSystem<(), (), M>,
    C: Fn() -> Instant + Clone + Send + Sync + 'static,
{
    let start_clock = now.clone();
    let begin = move |mut adaptive: ResMut<AdaptiveFrameLatency>,
                      windows: Query<&ExtractedWindow, With<PrimaryWindow>>| {
        adaptive.surface_started = Some(start_clock());
        adaptive.surface_had_texture = windows
            .single()
            .is_ok_and(|window| window.swap_chain_texture.is_some());
    };
    let end = move |mut adaptive: ResMut<AdaptiveFrameLatency>,
                    windows: Query<&ExtractedWindow, With<PrimaryWindow>>,
                    policy: Res<PresentModePolicy>| {
        let now = now();
        if let Some(started) = adaptive.surface_started.take() {
            adaptive.surface_elapsed += now.saturating_duration_since(started);
        }
        if let Ok(window) = windows.single()
            && !adaptive.surface_had_texture
            && window.swap_chain_texture.is_some()
        {
            adaptive.record_acquisition(now, window.desired_maximum_frame_latency, &policy);
        } else {
            adaptive.last_acquired = None;
        }
    };
    begin.pipe(system).pipe(end).into_configs()
}

/// Invalidates measurements when the primary window's delivery context changes.
fn extract_context(
    windows: Extract<Query<(Entity, &Window), With<PrimaryWindow>>>,
    mut events: Extract<MessageReader<WindowOccluded>>,
    mut occluded: Local<(Option<Entity>, bool)>,
    mut adaptive: ResMut<AdaptiveFrameLatency>,
    policy: Res<PresentModePolicy>,
    pacing: Extract<Option<Res<crate::FramePacing>>>,
) {
    let primary = windows.single().ok();
    let entity = primary.map(|(entity, _)| entity);
    if occluded.0 != entity {
        *occluded = (entity, false);
    }
    for event in events.read() {
        if Some(event.window) == entity {
            occluded.1 = event.occluded;
        }
    }
    let context = primary.map(|(entity, window)| Context {
        window: entity,
        size: [window.physical_width(), window.physical_height()],
        mode: window.present_mode,
        display: *policy
            .frame_latency
            .display
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        cap: pacing.as_deref().and_then(|pacing| pacing.rate),
        active: window.focused
            && window.visible
            && !occluded.1
            && window.physical_width() != 0
            && window.physical_height() != 0
            && policy.preference().intent() == render_model::PresentationIntent::Synchronized,
    });
    if adaptive.context != context {
        adaptive.context = context;
        adaptive.model = FrameLatencyPolicy::default();
        adaptive.cpu.fill(None);
        adaptive.last_acquired = None;
        policy.publish_frame_latency(std::num::NonZeroU32::new(2).expect("two is nonzero"));
    }
}

/// Starts coarse work timing before extraction commands without changing input admission.
fn begin_frame(
    mut adaptive: ResMut<AdaptiveFrameLatency>,
    adapter: Res<RenderAdapter>,
    policy: Res<PresentModePolicy>,
    mut supported: Local<Option<bool>>,
) {
    let now = Instant::now();
    adaptive.frame += 1;
    adaptive.surface_elapsed = Duration::ZERO;
    adaptive.surface_started = None;
    let supported = *supported.get_or_insert_with(|| {
        matches!(
            adapter.get_info().backend,
            wgpu::Backend::Dx12 | wgpu::Backend::Vulkan
        )
    });
    adaptive.started = supported.then_some(now);
    let refresh = supported.then(|| adaptive.refresh()).flatten();
    let elapsed = now.saturating_duration_since(adaptive.epoch);
    let selected = adaptive.model.update(elapsed, refresh, None, None);
    policy.publish_frame_latency(selected);
}

/// Retains this frame's render feeding work after subtracting surface acquisition.
fn end_frame(mut adaptive: ResMut<AdaptiveFrameLatency>) {
    adaptive.finish(Instant::now());
}

#[cfg(test)]
pub(crate) mod tests;
