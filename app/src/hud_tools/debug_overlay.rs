//! Java-style F3 diagnostics requested for Cinnabar. This is a developer tool,
//! not a Bedrock parity screen; its visual reference is the supplied 19w05a image.

use crate::runtime::network::resource_packs::PackApplication;
use client_presentation::environment::{WeatherState, WorldClock};
use client_runtime::network::NetworkHandle;
mod bindings;
mod details;
mod entities;
mod lines;
mod log_status;
mod spatial;
#[cfg(test)]
mod tests;

use std::{fmt::Write, time::Duration};

use lines::{Column, Lines, warm_buffers};

use bevy::{
    ecs::system::SystemParam,
    prelude::*,
    time::Real,
    window::{CursorOptions, PrimaryWindow},
};
use render_scene::UiRenderStatsResource;
use render_scene::{
    ChunkRenderQueue, GpuFrameTimes, RuntimeStage, RuntimeStageProfiler, VisibilityDiagnostics,
};

use client_shell::ui_resources::UiPresentationResource;
use client_ui::ui_runtime::presentation::DebugLines;
use {
    crate::runtime::{shutdown::FatalError, visibility::CaveVisibilityCache, world::ClientWorld},
    client_interaction::movement::{
        LocalPhysicsController, MovementTicker, PhysicsCollisionRegistries,
    },
    client_presentation::{
        camera::CameraSettingsAuthority,
        local_player::{LocalPlayerFrameCarrier, LocalViewPose},
    },
    client_state::{ClientFrameSet, PlayerRuntime},
};

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
    bindings: bindings::DebugBindings,
    visible: bool,
    pub show_chunk_borders: bool,
    f3_chord_used: bool,
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
    log_status: log_status::LogStatus,
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
    client_world: ResMut<'w, ClientWorld>,
    fatal_error: Res<'w, FatalError>,
    frame: Res<'w, LocalPlayerFrameCarrier>,
    view: Option<Res<'w, LocalViewPose>>,
    collisions: Option<Res<'w, PhysicsCollisionRegistries>>,
    physics: Option<Res<'w, LocalPhysicsController>>,
    movement: Option<Res<'w, MovementTicker>>,
    player: Option<Res<'w, PlayerRuntime>>,
    ui: Option<ResMut<'w, client_shell::ui_resources::UiRuntimeResource>>,
    menu: Option<Res<'w, client_shell::menu::MenuRuntime>>,
    mouse: Option<Res<'w, ButtonInput<MouseButton>>>,
    clock: Option<Res<'w, WorldClock>>,
    weather: Option<Res<'w, WeatherState>>,
    network: Option<Res<'w, NetworkHandle<PackApplication>>>,
    visibility: Option<Res<'w, CaveVisibilityCache>>,
    graphics: Option<Res<'w, VisibilityDiagnostics>>,
    queue: Option<Res<'w, ChunkRenderQueue>>,
    ui_stats: Option<Res<'w, UiRenderStatsResource>>,
    camera_settings: Option<Res<'w, CameraSettingsAuthority>>,
    profiler: Option<Res<'w, RuntimeStageProfiler>>,
    focus: Option<Res<'w, client_state::CursorFocus>>,
    driven: Option<Res<'w, client_state::DrivenInput>>,
    window: Query<'w, 's, (&'static Window, &'static CursorOptions), With<PrimaryWindow>>,
}

/// Publish sampled diagnostics only while the overlay is visible.
fn publish_debug_overlay(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time<Real>>,
    mut state: ResMut<DebugOverlayState>,
    mut presentation: ResMut<UiPresentationResource>,
    gpu_detail: Option<ResMut<render::DetailedGpuTiming>>,
    mut mod_scene: Option<ResMut<render_scene::ModRenderScene>>,
    mut context: DebugContext,
) {
    let timing_updated = state.sample_frame(time.delta());
    state.timing_pending |= timing_updated;
    let menu = context.menu.as_deref();
    let settings = menu.map(client_shell::menu::MenuRuntime::settings_snapshot);
    state
        .bindings
        .update(settings.as_ref().map(|(settings, _)| settings));
    let record_session = settings
        .as_ref()
        .is_some_and(|(settings, _)| settings.record_network_session());
    let toggled = !menu.is_some_and(client_shell::menu::MenuRuntime::is_remapping_key)
        && state.bindings.pressed(&keys, context.mouse.as_deref());
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

    let f3_is_down = state.bindings.is_down(&keys, context.mouse.as_deref());
    if !f3_is_down {
        state.f3_chord_used = false;
    } else {
        let chord_g = keys.just_pressed(KeyCode::KeyG);
        let chord_a = keys.just_pressed(KeyCode::KeyA);
        let chord_d = keys.just_pressed(KeyCode::KeyD);
        let chord_q = keys.just_pressed(KeyCode::KeyQ);

        if chord_g || chord_a || chord_d || chord_q {
            if !state.f3_chord_used {
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
                state.f3_chord_used = true;
            }

            let now = (time.elapsed_secs_f64() * 1000.0) as u64;

            if chord_g {
                state.show_chunk_borders = !state.show_chunk_borders;
                let status = if state.show_chunk_borders { "shown" } else { "hidden" };
                if let Some(ui) = context.ui.as_deref_mut() {
                    ui.push_local_chat_line(std::sync::Arc::from(format!("[Debug] Chunk borders: {status}")), now);
                }
            }
            if chord_a {
                if let Some(stream) = context.client_world.stream.as_mut() {
                    let _ = stream.remesh_all_resident(chunk_pipeline::web_time::Instant::now());
                }
                if let Some(ui) = context.ui.as_deref_mut() {
                    ui.push_local_chat_line(std::sync::Arc::from("[Debug] Reloading all chunks"), now);
                }
            }
            if chord_d {
                if let Some(ui) = context.ui.as_deref_mut() {
                    ui.clear_chat_messages();
                    ui.push_local_chat_line(std::sync::Arc::from("[Debug] Cleared chat"), now);
                }
            }
            if chord_q {
                if let Some(ui) = context.ui.as_deref_mut() {
                    ui.push_local_chat_line(std::sync::Arc::from("[Debug] F3 + A = Reload chunks"), now);
                    ui.push_local_chat_line(std::sync::Arc::from("[Debug] F3 + D = Clear chat"), now);
                    ui.push_local_chat_line(std::sync::Arc::from("[Debug] F3 + G = Show chunk borders"), now);
                    ui.push_local_chat_line(std::sync::Arc::from("[Debug] F3 + Q = Show this help list"), now);
                }
            }
        }
    }

    update_debug_visuals(
        state.show_chunk_borders,
        &context,
        &mut mod_scene,
    );
    // The overlay names the slowest passes once per refresh, so one timed frame per refresh
    // suffices; it is read back before the next refresh.
    let wanted = state.visible && (toggled || timing_updated);
    if let Some(mut detail) = gpu_detail
        && detail.0 != wanted
    {
        detail.0 = wanted;
    }
    if !state.visible {
        return;
    }
    let tick = time.elapsed().as_nanos() / simulation_clock::TICK_DURATION.as_nanos();
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
            .and_then(RuntimeStageProfiler::latest_pass_frame)
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
    lines.left.push(state.log_status.refresh(record_session));
    lines.left.push("");
    lines.left.push(state.bindings.hint());
    let mut published = Some(lines.finish());
    let first_publication = !state.publication_present;
    let changed = presentation
        .bypass_change_detection()
        .swap_debug_lines(&mut published);
    if changed {
        presentation.set_changed();
    }
    if let Some(previous) = published {
        state.staging = previous;
    }
    if changed || first_publication {
        warm_buffers(
            presentation.debug_lines().unwrap(),
            &mut state.staging,
            &mut state.spare_rows,
        );
    }
    state.publication_present = true;
}

fn append_debug_line(
    out: &mut Vec<mod_render::geometry::ModVertex>,
    a: [f32; 3],
    b: [f32; 3],
    color: [f32; 4],
    width_pixels: f32,
) {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let dz = b[2] - a[2];
    let len_sq = dx * dx + dy * dy + dz * dz;
    if len_sq < 1e-8 {
        return;
    }
    let inv_len = 1.0 / len_sq.sqrt();
    let dir = [dx * inv_len, dy * inv_len, dz * inv_len];
    let half_w = width_pixels * 0.5;
    for (side, p) in [
        (-1.0, a),
        (1.0, a),
        (1.0, b),
        (-1.0, a),
        (1.0, b),
        (-1.0, b),
    ] {
        out.push(mod_render::geometry::ModVertex {
            anchor: [p[0], p[1], p[2], mod_render::geometry::KIND_STRIP],
            shape: [dir[0], dir[1], dir[2], half_w],
            color,
            uv: [side, 0.0, 0.0, 0.0],
            style: [mod_render::geometry::STYLE_UNTAPERED_RIBBON, 0.0, 0.0, 1.0],
        });
    }
}

fn append_debug_box(
    out: &mut Vec<mod_render::geometry::ModVertex>,
    min: [f32; 3],
    max: [f32; 3],
    color: [f32; 4],
    width_pixels: f32,
) {
    let corners: [[f32; 3]; 8] = [
        [min[0], min[1], min[2]],
        [max[0], min[1], min[2]],
        [max[0], min[1], max[2]],
        [min[0], min[1], max[2]],
        [min[0], max[1], min[2]],
        [max[0], max[1], min[2]],
        [max[0], max[1], max[2]],
        [min[0], max[1], max[2]],
    ];
    let edges = [
        (0, 1), (1, 2), (2, 3), (3, 0),
        (4, 5), (5, 6), (6, 7), (7, 4),
        (0, 4), (1, 5), (2, 6), (3, 7),
    ];
    for (i, j) in edges {
        append_debug_line(out, corners[i], corners[j], color, width_pixels);
    }
}

fn update_debug_visuals(
    show_chunk_borders: bool,
    context: &DebugContext,
    mod_scene: &mut Option<ResMut<render_scene::ModRenderScene>>,
) {
    let Some(scene) = mod_scene.as_mut() else {
        return;
    };
    if !show_chunk_borders {
        if !scene.debug_vertices.is_empty() {
            crate::modding::render::update_overlay(scene, |s| {
                s.set_debug_vertices(std::sync::Arc::from([]));
            });
        }
        return;
    }
    let mut debug_vertices = Vec::new();
    if show_chunk_borders {
        if let Some((pos, _, _)) = spatial::player_origins(&context.frame, context.view.as_deref()) {
            let cx = (pos.x / 16.0).floor() as i32;
            let cz = (pos.z / 16.0).floor() as i32;
            let min_x = cx as f32 * 16.0;
            let max_x = min_x + 16.0;
            let min_z = cz as f32 * 16.0;
            let max_z = min_z + 16.0;

            append_debug_line(&mut debug_vertices, [min_x, -64.0, min_z], [min_x, 320.0, min_z], [1.0, 1.0, 0.0, 1.0], 1.5);
            append_debug_line(&mut debug_vertices, [max_x, -64.0, min_z], [max_x, 320.0, min_z], [1.0, 1.0, 0.0, 1.0], 1.5);
            append_debug_line(&mut debug_vertices, [min_x, -64.0, max_z], [min_x, 320.0, max_z], [1.0, 1.0, 0.0, 1.0], 1.5);
            append_debug_line(&mut debug_vertices, [max_x, -64.0, max_z], [max_x, 320.0, max_z], [1.0, 1.0, 0.0, 1.0], 1.5);

            let mut y = -64.0;
            while y <= 320.0 {
                append_debug_box(
                    &mut debug_vertices,
                    [min_x, y - 0.01, min_z],
                    [max_x, y + 0.01, max_z],
                    [1.0, 1.0, 0.0, 0.7],
                    1.0,
                );
                y += 16.0;
            }

            let sub_y = ((pos.y + 64.0) / 16.0).floor() * 16.0 - 64.0;
            let sub_top = sub_y + 16.0;
            for step in 1..8 {
                let offset = step as f32 * 2.0;
                append_debug_line(
                    &mut debug_vertices,
                    [min_x + offset, sub_y, min_z],
                    [min_x + offset, sub_top, min_z],
                    [1.0, 0.2, 0.2, 0.5],
                    1.0,
                );
                append_debug_line(
                    &mut debug_vertices,
                    [min_x + offset, sub_y, max_z],
                    [min_x + offset, sub_top, max_z],
                    [1.0, 0.2, 0.2, 0.5],
                    1.0,
                );
                append_debug_line(
                    &mut debug_vertices,
                    [min_x, sub_y, min_z + offset],
                    [min_x, sub_top, min_z + offset],
                    [0.2, 0.6, 1.0, 0.5],
                    1.0,
                );
                append_debug_line(
                    &mut debug_vertices,
                    [max_x, sub_y, min_z + offset],
                    [max_x, sub_top, min_z + offset],
                    [0.2, 0.6, 1.0, 0.5],
                    1.0,
                );
            }
        }
    }
    crate::modding::render::update_overlay(scene, |s| {
        s.set_debug_vertices(debug_vertices.into());
    });
}
