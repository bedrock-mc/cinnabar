//! Live view-radius requests; the server response updates residency and atmosphere distance.

use super::RuntimeSettings;
use crate::runtime::{network::NetworkHandle, world::ClientWorld};
use bevy::prelude::{Local, Res};
use client_ui::ui_runtime::UiRuntime;

/// Sends the selected radius after joining and whenever it changes, retrying backpressure.
pub(crate) fn apply_render_distance(
    settings: Res<RuntimeSettings>,
    ui: Option<Res<UiRuntime>>,
    world: Option<Res<ClientWorld>>,
    network: Option<Res<NetworkHandle>>,
    mut sent: Local<Option<(u64, u8)>>,
) {
    let (Some(ui), Some(world), Some(network)) = (ui, world, network) else {
        return;
    };
    if ui.session_id() == 0 || world.stream.is_none() {
        *sent = None;
        return;
    }
    let radius = settings
        .user_settings_update()
        .1
        .video
        .render_distance_chunks;
    let revision = (ui.session_id(), radius);
    if *sent == Some(revision) {
        return;
    }
    let packet =
        protocol::request_chunk_radius_packet(radius, render_api::MAX_VIEW_RADIUS_CHUNKS as u8);
    if network.send_settings_packet(revision.0, packet).is_ok() {
        *sent = Some(revision);
    }
}
