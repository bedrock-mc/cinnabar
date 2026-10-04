//! Java-style F3 diagnostics requested for Cinnabar. This is a developer tool,
//! not a Bedrock parity screen; its visual reference is the supplied 19w05a image.

mod details;
mod spatial;
#[cfg(test)]
mod tests;

use std::time::Duration;

use bevy::{
    ecs::system::SystemParam,
    prelude::*,
    time::Real,
    window::{CursorOptions, PrimaryWindow},
};
use render::{ChunkRenderQueue, UiRenderStats, VisibilityDiagnostics};

use crate::{
    app::ClientFrameSet,
    camera::CameraSettingsAuthority,
    environment::{WeatherState, WorldClock},
    local_player::{LocalPlayerFrameCarrier, LocalViewPose},
    movement::{LocalPhysicsController, MovementTicker, PhysicsCollisionRegistries},
    player_runtime::PlayerRuntime,
    runtime::{network::NetworkHandle, visibility::CaveVisibilityCache, world::ClientWorld},
    ui_runtime::presentation::{DebugLines, UiPresentationRuntime},
};

/// Aggregate frame timing over a short window; live diagnostics still publish
/// every frame while F3 is enabled.
const FRAME_TIMING_WINDOW: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Default)]
struct FrameTiming {
    fps: f64,
    average_ms: f64,
    max_ms: f64,
}

#[derive(Resource, Default)]
pub(super) struct DebugOverlayState {
    visible: bool,
    elapsed: Duration,
    frames: u32,
    worst_frame: Duration,
    timing: FrameTiming,
}

impl DebugOverlayState {
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
    clock: Option<Res<'w, WorldClock>>,
    weather: Option<Res<'w, WeatherState>>,
    network: Option<Res<'w, NetworkHandle>>,
    visibility: Option<Res<'w, CaveVisibilityCache>>,
    graphics: Option<Res<'w, VisibilityDiagnostics>>,
    queue: Option<Res<'w, ChunkRenderQueue>>,
    ui_stats: Option<Res<'w, UiRenderStats>>,
    camera_settings: Option<Res<'w, CameraSettingsAuthority>>,
    window: Query<'w, 's, (&'static Window, &'static CursorOptions), With<PrimaryWindow>>,
}

fn publish_debug_overlay(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time<Real>>,
    mut state: ResMut<DebugOverlayState>,
    mut presentation: ResMut<UiPresentationRuntime>,
    context: DebugContext,
) {
    state.sample_frame(time.delta());
    let toggled = keys.just_pressed(KeyCode::F3);
    if toggled {
        state.visible = !state.visible;
        if !state.visible {
            presentation.set_debug_lines(None);
        }
    }
    if !state.visible {
        return;
    }
    let mut lines = DebugLines {
        left: vec![
            format!(
                "{} {} (Bedrock {})",
                launcher::PRODUCT_NAME,
                env!("CARGO_PKG_VERSION"),
                protocol::GAME_VERSION
            ),
            if state.timing.fps > 0.0 {
                format!(
                    "{:.0} fps | {:.1} ms avg / {:.1} ms max",
                    state.timing.fps, state.timing.average_ms, state.timing.max_ms
                )
            } else {
                "FPS: sampling frame timing...".to_owned()
            },
        ],
        right: Vec::new(),
    };
    context.append_world(&mut lines, time.elapsed_secs_f64());
    context.append_client(&mut lines, &presentation);
    lines.left.push(String::new());
    lines
        .left
        .push("F3: hide debug | F2: screenshot".to_owned());
    presentation.set_debug_lines(Some(lines));
}
