//! Existing world, simulation, queue and renderer counters shown by F3.

use super::{DebugContext, DebugLines, UiPresentationRuntime};

impl DebugContext<'_, '_> {
    pub(super) fn append_world(&self, lines: &mut DebugLines, elapsed_seconds: f64) {
        let Some(stream) = self.client_world.stream.as_ref() else {
            lines.left.push("World: disconnected".to_owned());
            if let Some(error) = &self.client_world.fatal_error {
                lines.left.push(format!("Session error: {error}"));
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
        lines.left.push(format!(
            "Dimension: {name} ({})",
            stream.current_dimension()
        ));
        if let Some(visibility) = self.visibility.as_deref() {
            lines.left.push(format!(
                "Sections: {} cave-visible / {} meshed / {} resident",
                visibility.visible_rendered,
                visibility.rendered.len(),
                stats.resident_sub_chunks
            ));
        } else {
            lines
                .left
                .push(format!("Sections: {} resident", stats.resident_sub_chunks));
        }
        lines.left.push(format!(
            "View distance: {} chunks | publisher: {}",
            optional_radius(stats.received_radius_chunks),
            optional_radius(stats.publisher_radius_chunks)
        ));
        lines.left.push(format!(
            "Entities: {} tracked",
            stream.authority().actor_count()
        ));
        lines.left.push(format!(
            "Decode: {} queued / {} running / {} ready",
            stats.queued_decode_jobs, stats.in_flight_decode_jobs, stats.completed_decode_results
        ));
        lines.left.push(format!(
            "Mesh: {} queued / {} running | light: {} / {}",
            stats.pending_mesh_jobs,
            stats.in_flight_mesh_jobs,
            stats.pending_light_jobs,
            stats.in_flight_light_jobs
        ));
        lines.left.push(format!(
            "Chunk requests: {} queued / {} waiting / {} retries",
            stream.pending_request_count(),
            stats.awaiting_sub_chunk_responses,
            stats.pending_retry_requests
        ));
        lines.left.push(format!(
            "Errors: {} network / {} decode / {} normalized",
            self.client_world.network_decode_errors,
            stats.decode_errors,
            stats.normalization_errors
        ));
        self.append_position(lines, stream);
        if let Some(clock) = self
            .clock
            .as_deref()
            .filter(|clock| clock.server_time().is_some())
        {
            let tick = crate::environment::visual_world_time(*clock, elapsed_seconds);
            lines.left.push(format!(
                "World time: {tick:.0} ticks | daylight: {}",
                if clock.daylight_cycle_enabled() {
                    "cycling"
                } else {
                    "locked"
                }
            ));
        }
        if let Some(weather) = self.weather.as_deref() {
            lines.left.push(format!(
                "Weather: rain {:.2} / lightning {:.2}",
                weather.rain_level(),
                weather.lightning_level()
            ));
        }
        self.append_movement(lines);
        lines.right.push(String::new());
        lines.right.push(format!(
            "Peak worker ms: decode {:.2} / mesh {:.2}",
            stats.max_decode_duration.as_secs_f64() * 1_000.0,
            stats.max_mesh_duration.as_secs_f64() * 1_000.0
        ));
        lines.right.push(format!(
            "Peak light {:.2} ms / remesh {:.2} ms",
            stats.max_light_duration.as_secs_f64() * 1_000.0,
            stats.max_remesh_latency.as_secs_f64() * 1_000.0
        ));
        if let Some(error) = &self.client_world.fatal_error {
            lines.left.push(format!("Session error: {error}"));
        }
    }

    fn append_movement(&self, lines: &mut DebugLines) {
        lines.left.push(String::new());
        if let Some(player) = self.player.as_deref() {
            let mode = player
                .facts
                .player_game_mode()
                .map_or_else(|| "unknown".to_owned(), |mode| format!("{mode:?}"));
            lines.left.push(format!("Game mode: {mode}"));
        }
        if let Some(physics) = self.physics.as_deref()
            && let Some(state) = physics.state()
        {
            // Simulation motion is blocks per tick. Convert with the world's
            // shared tick rate instead of relabeling motion as blocks per second.
            let velocity = state.velocity * f64::from(world::TICKS_PER_SECOND);
            lines.left.push(format!(
                "Velocity: {:.3} / {:.3} / {:.3} blocks/s",
                velocity.x, velocity.y, velocity.z
            ));
            let sneak_sprint = physics.latest_sneak_sprint();
            let flag = |value: Option<bool>| {
                value.map_or("unavailable", |value| if value { "true" } else { "false" })
            };
            lines.left.push(format!("Movement: {:?}", physics.mode()));
            lines.left.push(format!(
                "Ground: {} | sneak: {} | sprint: {}",
                state.on_ground,
                flag(sneak_sprint.map(|flags| flags.0)),
                flag(sneak_sprint.map(|flags| flags.1))
            ));
        }
        if let Some(frame) = self.frame.snapshot() {
            lines.left.push(format!(
                "Session: {} | tick: {} | FIFO: {}",
                frame.session_generation(),
                frame.physics_tick(),
                frame.fifo_sequence()
            ));
        }
        if let Some(movement) = self.movement.as_deref() {
            lines.left.push(format!(
                "Authority: {:?} | inputs pending: {}",
                movement.source(),
                movement.pending_count()
            ));
            lines.left.push(format!(
                "Inputs sent: {} | dropped ticks: {}",
                movement
                    .sent_physics_packet_count()
                    .saturating_add(movement.sent_free_camera_packet_count()),
                movement.dropped_tick_count()
            ));
        }
    }

    pub(super) fn append_client(
        &self,
        lines: &mut DebugLines,
        presentation: &UiPresentationRuntime,
    ) {
        let mut right = vec![
            format!(
                "Rust client ({}, {})",
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
            format!(
                "Wire protocol: {} | content: {}",
                protocol::PROTOCOL_VERSION,
                assets::active_content_registry_protocol()
            ),
        ];
        if let Ok((window, cursor)) = self.window.single() {
            let gui = presentation
                .gui_scale_preference()
                .map_or_else(|| "auto".to_owned(), |scale| scale.to_string());
            right.push(format!(
                "Display: {}x{} | DPI: {:.2} | GUI: {gui}",
                window.physical_width(),
                window.physical_height(),
                window.scale_factor()
            ));
            right.push(format!(
                "Window: {} | mouse: {}",
                if window.focused {
                    "focused"
                } else {
                    "unfocused"
                },
                if crate::camera::input_is_active(window, cursor) {
                    "captured"
                } else {
                    "released"
                }
            ));
        }
        if let Some(settings) = self.camera_settings.as_deref() {
            right.push(format!(
                "Camera: {:?} | FOV: {:.1}",
                settings.perspective(),
                settings.horizontal_fov_degrees()
            ));
        }
        if let Some(graphics) = self
            .graphics
            .as_deref()
            .and_then(|graphics| graphics.graphics_adapter())
        {
            right.push(format!("GPU: {}", graphics.adapter));
            right.push(format!("Renderer: {}", graphics.backend));
            if !graphics.driver.is_empty() {
                right.push(format!("Driver: {}", graphics.driver));
            }
            right.push(format!(
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
            right.push(format!(
                "GPU uploads: {} pending ({:.2} MiB)",
                queue.pending_len(),
                mib(queue.pending_bytes())
            ));
            right.push(format!(
                "Terrain upload total: {:.2} MiB",
                mib(queue.gpu_upload_bytes())
            ));
        }
        if let Some(ui) = self.ui_stats.as_deref().map(|ui| ui.snapshot()) {
            right.push(format!(
                "UI: {} draws / {} vertices ({:.2} MiB GPU)",
                ui.draw_calls,
                ui.uploaded_vertices,
                mib(ui.retained_gpu_bytes)
            ));
        }
        if let Some(network) = self.network.as_deref() {
            right.push(format!(
                "Network queues: {} in / {} out",
                network.pending_event_count(),
                network.pending_command_count()
            ));
        }
        right.push(format!(
            "Missing block visuals: {}",
            self.client_world.missing_asset_count()
        ));
        right.append(&mut lines.right);
        lines.right = right;
    }
}

fn optional_radius(value: Option<i32>) -> String {
    value.map_or_else(|| "unavailable".to_owned(), |radius| radius.to_string())
}

fn mib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}
