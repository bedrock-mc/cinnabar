//! Shared outbound UI intent for native and browser session hosts.
use crate::runtime::network::resource_packs::PackApplication;
use bevy::{
    prelude::{Res, ResMut, Time},
    time::Real,
};
use client_runtime::network::NetworkHandle;
use client_shell::ui_resources::UiRuntimeResource;
use client_ui::ui_runtime::interaction::{ChatFlushError, flush_chat_sends, flush_inventory_send};

/// Flushes committed inventory intent through the session FIFO, retaining backpressured packets.
pub(crate) fn flush_inventory_network(
    mut player_runtime: bevy::prelude::ResMut<client_state::PlayerRuntime>,
    time: Res<Time<Real>>,
    mut runtime: ResMut<UiRuntimeResource>,
    network: Res<NetworkHandle<PackApplication>>,
) {
    let now_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
    match flush_inventory_send(&mut player_runtime, &mut runtime, now_millis, |packet| {
        network.send_inventory_packet(packet)
    }) {
        Ok(_) | Err(client_session::PacketSendError::Full(_)) => {}
        Err(client_session::PacketSendError::Closed(_)) => {
            runtime.inventory_transport_closed(&mut player_runtime);
        }
    }
    while let Some(packet) = runtime.take_client_packet() {
        match network.send_inventory_packet(packet) {
            Ok(()) => {}
            Err(client_session::PacketSendError::Full(packet)) => {
                runtime.requeue_client_packet(packet);
                break;
            }
            Err(client_session::PacketSendError::Closed(_)) => break,
        }
    }
}

/// Sends queued chat and wake requests without crossing a replaced session.
pub(crate) fn flush_chat_network(
    player_runtime: bevy::prelude::Res<client_state::PlayerRuntime>,
    mut runtime: ResMut<UiRuntimeResource>,
    network: Res<NetworkHandle<PackApplication>>,
    mut fatal_error: ResMut<crate::runtime::shutdown::FatalError>,
) {
    runtime.service_pending_chat_autocomplete(&player_runtime);
    if network.closed_command_has_pending_control() {
        return;
    }
    let runtime_id = runtime.local_runtime_id(&player_runtime);
    runtime.flush_wake_request(runtime_id, |packet| network.send_inventory_packet(packet));
    match flush_chat_sends(
        &mut runtime,
        8,
        |session, sequence, action, packet| match network
            .send_chat_packet(session, sequence, action, packet)
        {
            Err(client_session::PacketSendError::Closed(packet))
                if network.closed_command_has_pending_control() =>
            {
                Err(client_session::PacketSendError::Full(packet))
            }
            result => result,
        },
    ) {
        Ok(_) | Err(ChatFlushError::Transport(client_session::PacketSendError::Full(_))) => {}
        Err(ChatFlushError::Transport(client_session::PacketSendError::Closed(_))) => {
            crate::runtime::shutdown::record_fatal_error(
                &mut fatal_error.0,
                "chat send failed because the network command channel closed".to_owned(),
            );
        }
        Err(ChatFlushError::Packet(error)) => {
            crate::runtime::shutdown::record_fatal_error(
                &mut fatal_error.0,
                format!("queued chat packet became invalid: {error}"),
            );
        }
        Err(ChatFlushError::SessionChanged { expected, actual }) => {
            crate::runtime::shutdown::record_fatal_error(
                &mut fatal_error.0,
                format!(
                    "queued chat packet crossed a session boundary: expected {expected}, got {actual}"
                ),
            );
        }
    }
}
