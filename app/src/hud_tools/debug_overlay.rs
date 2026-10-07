//! Java-style F3 diagnostics requested for Cinnabar. This is a developer tool,
//! not a Bedrock parity screen; its visual reference is the supplied 19w05a image.

mod details;
mod entities;
mod lines;
mod spatial;
#[cfg(test)]
mod tests;

use std::{fmt::Write, time::Duration};

use lines::{Column, Lines, copy_buffers};

use bevy::{
    ecs::system::SystemParam,
    prelude::*,
    time::Real,
    window::{CursorOptions, PrimaryWindow},
};
use render::UiRenderStatsResource;
use render::{
    ChunkRenderQueue, GpuFrameTimes, RuntimeStage, RuntimeStageProfiler, VisibilityDiagnostics,
};

use crate::{
    app::ClientFrameSet,
    camera::CameraSettingsAuthority,
    environment::{WeatherState, WorldClock},
    local_player::{LocalPlayerFrameCarrier, LocalViewPose},
    movement::{LocalPhysicsController, MovementTicker, PhysicsCollisionRegistries},
    player_runtime::PlayerRuntime,
    runtime::{network::NetworkHandle, visibility::CaveVisibilityCache, world::ClientWorld},
};
use client_ui::ui_runtime::presentation::{DebugLines, UiPresentationRuntime};

/// Publish frame statistics once per second, independently of render frequency.
const FRAME_TIMING_WINDOW: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Default)]
struct FrameTiming {
    fps: f64,
    average_ms: f64,
    max_ms: f64,
}

#[derive(Resource, Default)]
pub(super) struct DebugOverlayState {
    visible: bool,
    publication_present: bool,
    elapsed: Duration,
    frames: u32,
    worst_frame: Duration,
    timing: FrameTiming,
    diagnostic_tick: Option<u128>,
    staging: DebugLines,
    spare_rows: [Vec<String>; 2],
    timing_pending: bool,
    gpu: String,
    ui_line: String,
    has_gpu: bool,
    block_states: spatial::BlockStates,
    #[cfg(test)]
    gathers: usize,
}

impl DebugOverlayState {
    /// Samples every frame without formatting or allocating.
    fn sample_frame(&mut self, delta: Duration) -> bool {
        if delta.is_zero() {
            return false;
        }
        self.elapsed = self.elapsed.saturating_add(delta);
        self.frames = self.frames.saturating_add(1);
        self.worst_frame = self.worst_frame.max(delta);
        if self.elapsed < FRAME_TIMING_WINDOW {
            return false;
        }
        self.timing = FrameTiming {
            fps: f64::from(self.frames) / self.elapsed.as_secs_f64(),
            average_ms: self.elapsed.as_secs_f64() * 1_000.0 / f64::from(self.frames),
            max_ms: self.worst_frame.as_secs_f64() * 1_000.0,
        };
        self.elapsed = Duration::ZERO;
        self.frames = 0;
        self.worst_frame = Duration::ZERO;
        true
    }
}

pub(super) fn configure(app: &mut App) {
    app.init_resource::<DebugOverlayState>().add_systems(
        Update,
        publish_debug_overlay
            .after(ClientFrameSet::UiPreparation)
            .before(ClientFrameSet::UiPublication),
    );
}

/// Format already-collected GPU statistics without sorting an allocated list.
fn gpu_line(frame: &GpuFrameTimes, line: &mut String) -> bool {
    line.clear();
    let Some(total) = frame.get(RuntimeStage::GpuFrame) else {
        return false;
    };
    let passes = top_gpu_passes(frame.iter());
    write!(line, "GPU: {:.2} ms", total.as_secs_f64() * 1_000.0).unwrap();
    for (index, (stage, elapsed)) in passes.into_iter().flatten().enumerate() {
        let name = stage.name().trim_start_matches("gpu_");
        let separator = if index == 0 { " | " } else { " / " };
        write!(
            line,
            "{separator}{name} {:.2}",
            elapsed.as_secs_f64() * 1_000.0
        )
        .unwrap();
    }
    true
}

/// Retains the three slowest measured passes, including valid zero-duration samples.
fn top_gpu_passes(
    passes: impl Iterator<Item = (RuntimeStage, Duration)>,
) -> [Option<(RuntimeStage, Duration)>; 3] {
    let mut top = [None; 3];
    for (stage, elapsed) in passes.filter(|(stage, _)| *stage != RuntimeStage::GpuFrame) {
        if let Some(index) = top
            .iter()
            .position(|kept: &Option<(RuntimeStage, Duration)>| {
                kept.is_none_or(|(_, kept)| elapsed > kept)
            })
        {
            for next in (index + 1..top.len()).rev() {
                top[next] = top[next - 1];
            }
            top[index] = Some((stage, elapsed));
        }
    }
    top
}

/// Read existing authorities; never change gameplay or drain queues.
#[derive(SystemParam)]
struct DebugContext<'w, 's> {
    client_world: Res<'w, ClientWorld>,
    frame: Res<'w, LocalPlayerFrameCarrier>,
    view: Option<Res<'w, LocalViewPose>>,
    collisions: Option<Res<'w, PhysicsCollisionRegistries>>,
    physics: Option<Res<'w, LocalPhysicsController>>,
    movement: Option<Res<'w, MovementTicker>>,
    player: Option<Res<'w, PlayerRuntime>>,
    ui: Option<Res<'w, client_ui::ui_runtime::UiRuntime>>,
    clock: Option<Res<'w, WorldClock>>,
    weather: Option<Res<'w, WeatherState>>,
    network: Option<Res<'w, NetworkHandle>>,
    visibility: Option<Res<'w, CaveVisibilityCache>>,
    graphics: Option<Res<'w, VisibilityDiagnostics>>,
    queue: Option<Res<'w, ChunkRenderQueue>>,
    ui_stats: Option<Res<'w, UiRenderStatsResource>>,
    camera_settings: Option<Res<'w, CameraSettingsAuthority>>,
    profiler: Option<Res<'w, RuntimeStageProfiler>>,
    focus: Option<Res<'w, client_presentation::camera::CursorFocus>>,
    driven: Option<Res<'w, crate::camera::DrivenInput>>,
    window: Query<'w, 's, (&'static Window, &'static CursorOptions), With<PrimaryWindow>>,
}

/// Publish sampled diagnostics only while the overlay is visible.
fn publish_debug_overlay(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time<Real>>,
    mut state: ResMut<DebugOverlayState>,
    mut presentation: ResMut<UiPresentationRuntime>,
    context: DebugContext,
) {
    let timing_updated = state.sample_frame(time.delta());
    state.timing_pending |= timing_updated;
    let toggled = keys.just_pressed(KeyCode::F3);
    if toggled {
        state.visible = !state.visible;
        if !state.visible {
            let mut cleared = None;
            if presentation
                .bypass_change_detection()
                .swap_debug_lines(&mut cleared)
            {
                presentation.set_changed();
            }
            state.publication_present = false;
        }
    }
    if !state.visible {
        return;
    }
    let tick = time.elapsed().as_nanos() / world::TICK_DURATION.as_nanos();
    if !toggled && state.diagnostic_tick == Some(tick) {
        return;
    }
    if let (Some(player), Some(ui)) = (context.player.as_deref(), context.ui.as_deref())
        && !presentation.debug_overlay_allowed(player, ui)
    {
        return;
    }
    state.diagnostic_tick = Some(tick);
    #[cfg(test)]
    {
        state.gathers += 1;
    }
    #[cfg(feature = "tracy")]
    let _zone = bevy::log::info_span!("ui.f3.gather").entered();
    if toggled || state.timing_pending || !state.publication_present {
        state.has_gpu = context
            .profiler
            .as_deref()
            .and_then(RuntimeStageProfiler::latest_gpu_frame)
            .is_some_and(|frame| gpu_line(&frame, &mut state.gpu));
        context.sample_ui_stats(&mut state.ui_line);
    }
    state.timing_pending = false;
    let state = &mut *state;
    let mut lines = Lines::new(std::mem::take(&mut state.staging), &mut state.spare_rows);
    lines.left.push(format_args!(
        "{} {} (Bedrock {})",
        launcher::PRODUCT_NAME,
        env!("CARGO_PKG_VERSION"),
        protocol::GAME_VERSION
    ));
    if state.timing.fps > 0.0 {
        lines.left.push(format_args!(
            "{:.0} fps | {:.1} ms avg / {:.1} ms max",
            state.timing.fps, state.timing.average_ms, state.timing.max_ms
        ));
    } else {
        lines.left.push("FPS: sampling frame timing...");
    }
    if state.has_gpu {
        lines.left.push(&state.gpu);
    }
    context.append_client(&mut lines, &presentation, &state.ui_line);
    context.append_world(&mut lines, time.elapsed_secs_f64(), &mut state.block_states);
    lines.left.push("");
    lines.left.push("F3: hide debug | F2: screenshot");
    let mut published = Some(lines.finish());
    if !state.publication_present {
        state.staging = copy_buffers(published.as_ref().unwrap());
        state.publication_present = true;
    }
    if presentation
        .bypass_change_detection()
        .swap_debug_lines(&mut published)
    {
        presentation.set_changed();
    }
    if let Some(previous) = published {
        state.staging = previous;
    }
}
