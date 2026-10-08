//! Existing world, simulation, queue and renderer counters shown by F3.

use super::{DebugContext, Lines, UiPresentationRuntime};
use std::fmt::{self, Display, Write};

impl DebugContext<'_, '_> {
    pub(super) fn append_world(
        &self,
        lines: &mut Lines<'_>,
        elapsed_seconds: f64,
        block_states: &mut super::spatial::BlockStates,
    ) {
        #[cfg(feature = "tracy")]
        let _zone = bevy::log::info_span!("ui.f3.world").entered();
        let Some(stream) = self.client_world.stream.as_ref() else {
            lines.left.push("World: disconnected");
            if let Some(error) = &self.client_world.fatal_error {
                lines.left.push(format_args!("Session error: {error}"));
            }
            return;
        };
        let stats = stream.stats();
        let name = match stream.current_dimension() {
            0 => "Overworld",
            1 => "Nether",
            2 => "The End",
            _ => "Custom",
        };
        lines.left.push(format_args!(
            "Dimension: {name} ({})",
            stream.current_dimension()
        ));
        if let Some(visibility) = self.visibility.as_deref() {
            lines.left.push(format_args!(
                "Sections: {} cave-visible / {} meshed / {} resident",
                visibility.visible_rendered,
                visibility.rendered.len(),
                stats.resident_sub_chunks
            ));
        } else {
            lines.left.push(format_args!(
                "Sections: {} resident",
                stats.resident_sub_chunks
            ));
        }
        lines.left.push(format_args!(
            "View distance: {} chunks | publisher: {}",
            optional_radius(stats.received_radius_chunks),
            optional_radius(stats.publisher_radius_chunks)
        ));
        lines.left.push(format_args!(
            "Entities: {} tracked",
            stream.authority().actor_count()
        ));
        lines.left.push(format_args!(
            "Decode: {} queued / {} running / {} ready",
            stats.queued_decode_jobs, stats.in_flight_decode_jobs, stats.completed_decode_results
        ));
        lines.left.push(format_args!(
            "Mesh: {} queued / {} running | light: {} / {}",
            stats.pending_mesh_jobs,
            stats.in_flight_mesh_jobs,
            stats.pending_light_jobs,
            stats.in_flight_light_jobs
        ));
        lines.left.push(format_args!(
            "Chunk requests: {} queued / {} waiting / {} retries",
            stream.pending_request_count(),
            stats.awaiting_sub_chunk_responses,
            stats.pending_retry_requests
        ));
        lines.left.push(format_args!(
            "Errors: {} network / {} decode / {} normalized",
            self.client_world.network_decode_errors,
            stats.decode_errors,
            stats.normalization_errors
        ));
        self.append_position(lines, stream, block_states);
        if let Some(clock) = self
            .clock
            .as_deref()
            .filter(|clock| clock.server_time().is_some())
        {
            let tick = crate::environment::visual_world_time(*clock, elapsed_seconds);
            lines.left.push(format_args!(
                "World time: {tick:.0} ticks | daylight: {}",
                if clock.daylight_cycle_enabled() {
                    "cycling"
                } else {
                    "locked"
                }
            ));
        }
        if let Some(weather) = self.weather.as_deref() {
            lines.left.push(format_args!(
                "Weather: rain {:.2} / lightning {:.2}",
                weather.rain_level(),
                weather.lightning_level()
            ));
        }
        self.append_movement(lines);
        lines.right.push("");
        lines.right.push(format_args!(
            "Peak worker ms: decode {:.2} / mesh {:.2}",
            stats.max_decode_duration.as_secs_f64() * 1_000.0,
            stats.max_mesh_duration.as_secs_f64() * 1_000.0
        ));
        lines.right.push(format_args!(
            "Peak light {:.2} ms / remesh {:.2} ms",
            stats.max_light_duration.as_secs_f64() * 1_000.0,
            stats.max_remesh_latency.as_secs_f64() * 1_000.0
        ));
        if let Some(error) = &self.client_world.fatal_error {
            lines.left.push(format_args!("Session error: {error}"));
        }
    }

    fn append_movement(&self, lines: &mut Lines<'_>) {
        lines.left.push("");
        if let Some(player) = self.player.as_deref() {
            lines.left.push_with(|line| {
                line.push_str("Game mode: ");
                if let Some(mode) = player.facts.player_game_mode() {
                    write!(line, "{mode:?}")?;
                } else {
                    line.push_str("unknown");
                }
                write!(line, " | immobile: {}", player.facts.is_immobile())
            });
        }
        if let Some(physics) = self.physics.as_deref()
            && let Some(state) = physics.state()
        {
            // Simulation motion is blocks per tick. Convert with the world's
            // shared tick rate instead of relabeling motion as blocks per second.
            let velocity = state.velocity * f64::from(world::TICKS_PER_SECOND);
            lines.left.push(format_args!(
                "Velocity: {:.3} / {:.3} / {:.3} blocks/s",
                velocity.x, velocity.y, velocity.z
            ));
            let sneak_sprint = physics.latest_sneak_sprint();
            let flag = |value: Option<bool>| {
                value.map_or("unavailable", |value| if value { "true" } else { "false" })
            };
            lines
                .left
                .push(format_args!("Movement: {:?}", physics.mode()));
            lines.left.push(format_args!(
                "Ground: {} | sneak: {} | sprint: {}",
                state.on_ground,
                flag(sneak_sprint.map(|flags| flags.0)),
                flag(sneak_sprint.map(|flags| flags.1))
            ));
        }
        if let Some(frame) = self.frame.snapshot() {
            lines.left.push(format_args!(
                "Session: {} | tick: {} | FIFO: {}",
                frame.session_generation(),
                frame.physics_tick(),
                frame.fifo_sequence()
            ));
        }
        if let Some(movement) = self.movement.as_deref() {
            lines.left.push(format_args!(
                "Authority: {:?} | inputs pending: {}",
                movement.source(),
                movement.pending_count()
            ));
            lines.left.push(format_args!(
                "Inputs sent: {} | dropped ticks: {}",
                movement
                    .sent_physics_packet_count()
                    .saturating_add(movement.sent_free_camera_packet_count()),
                movement.dropped_tick_count()
            ));
        }
    }

    /// Formats existing UI frame counters at the one-second statistics cadence.
    pub(super) fn sample_ui_stats(&self, line: &mut String) {
        line.clear();
        if let Some(ui) = self.ui_stats.as_deref().map(|ui| ui.snapshot()) {
            if line.capacity() == 0 {
                line.reserve(super::lines::ROW_CAPACITY);
            }
            write!(
                line,
                "UI: {} draws / {} vertices ({:.2} MiB GPU)",
                ui.draw_calls,
                ui.uploaded_vertices,
                mib(ui.retained_gpu_bytes)
            )
            .unwrap();
        }
    }

    pub(super) fn append_client(
        &self,
        lines: &mut Lines<'_>,
        presentation: &UiPresentationRuntime,
        ui_line: &str,
    ) {
        let right = &mut lines.right;
        right.extend([
            format_args!(
                "Rust client ({}, {})",
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
            format_args!(
                "Wire protocol: {} | content: {}",
                protocol::PROTOCOL_VERSION,
                assets::active_content_registry_protocol()
            ),
        ]);
        if let Ok((window, cursor)) = self.window.single() {
            right.push_with(|line| {
                write!(
                    line,
                    "Display: {}x{} | DPI: {:.2} | GUI: ",
                    window.physical_width(),
                    window.physical_height(),
                    window.scale_factor()
                )?;
                if let Some(scale) = presentation.gui_scale_preference() {
                    write!(line, "{scale}")
                } else {
                    line.push_str("auto");
                    Ok(())
                }
            });
            right.push(format_args!(
                "Window: {} | mouse: {}",
                if window.focused {
                    "focused"
                } else {
                    "unfocused"
                },
                if crate::camera::mouse_input_active(
                    window,
                    cursor,
                    self.focus.as_deref(),
                    self.driven.is_some(),
                ) {
                    "captured"
                } else {
                    "released"
                }
            ));
        }
        if let Some(settings) = self.camera_settings.as_deref() {
            right.push(format_args!(
                "Camera: {:?} | FOV: {:.1}",
                settings.perspective(),
                settings.horizontal_fov_degrees()
            ));
        }
        if let Some(graphics) = self
            .graphics
            .as_deref()
            .and_then(|graphics| graphics.graphics_adapter_ref())
        {
            right.push(format_args!("GPU: {}", graphics.adapter));
            right.push(format_args!("Renderer: {}", graphics.backend));
            if !graphics.driver.is_empty() {
                right.push(format_args!("Driver: {}", graphics.driver));
            }
            right.push(format_args!(
                "Present: {}{}",
                if graphics.present_mode_proven {
                    &graphics.effective_present_mode
                } else {
                    &graphics.requested_present_mode
                },
                if graphics.present_mode_proven {
                    ""
                } else {
                    " (requested)"
                }
            ));
        }
        if let Some(queue) = self.queue.as_deref() {
            right.push(format_args!(
                "GPU uploads: {} pending ({:.2} MiB)",
                queue.pending_len(),
                mib(queue.pending_bytes())
            ));
            right.push(format_args!(
                "Terrain upload total: {:.2} MiB",
                mib(queue.gpu_upload_bytes())
            ));
        }
        if !ui_line.is_empty() {
            right.push(ui_line);
        }
        if let Some(network) = self.network.as_deref() {
            right.push(format_args!(
                "Network queues: {} in / {} out",
                network.pending_event_count(),
                network.pending_command_count()
            ));
        }
        right.push(format_args!(
            "Missing block visuals: {}",
            self.client_world.missing_asset_count()
        ));
    }
}

/// Formats an optional radius without an intermediate string.
fn optional_radius(value: Option<i32>) -> impl Display {
    Radius(value)
}

struct Radius(Option<i32>);

impl Display for Radius {
    /// Writes the optional value into the existing formatter buffer.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(radius) => write!(formatter, "{radius}"),
            None => formatter.write_str("unavailable"),
        }
    }
}

/// Converts retained GPU-byte counters to the displayed unit.
fn mib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}
