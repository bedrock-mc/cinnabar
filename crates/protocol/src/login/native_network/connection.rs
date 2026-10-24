//! Application packet, UI-limit and terminal projection at the runtime driver's boundary.

#[cfg(test)]
use crate::{Packet, packet::GameHeader};
use bedrock_client_tokio::{connection::Connection, error::DeferredLimit, transport::Transport};
use bedrock_net::BatchLimits;
use bedrock_protocol_1_26_51::packets;
use std::io;

/// Applies the desktop's original upstream input and startup-prefix policies.
pub(super) fn upstream(transport: Transport) -> Connection {
    let mut connection = Connection::new(
        transport,
        BatchLimits::default(),
        crate::codec::server_limits().clone(),
        super::super::startup::MAX_DEFERRED_PACKET_BYTES,
    )
    .with_incoming_decoded_limit(bridge::contract::MAX_UPSTREAM_DECOMPRESSED_BATCH_BYTES);
    connection.known_packets();
    connection
}

/// Projects the generated packet into the application's existing packet owner.
#[cfg(test)]
pub(super) async fn next_packet(connection: &mut Connection) -> io::Result<Packet> {
    let (header, data) = connection.next_packet().await?;
    Ok(Packet::from_parts(
        GameHeader {
            id: packets::PacketId::from_raw(header.id).expect("known generated packet"),
            from_subclient: header.sender_subclient.into(),
            to_subclient: header.target_subclient.into(),
        },
        data,
    ))
}

/// Projects only the last actual received frame into the application's terminal representation.
pub(super) fn current_terminal(
    connection: &Connection,
) -> io::Result<Option<crate::session_wire::CoreMessage>> {
    connection
        .current_raw()
        .map(super::relay::terminal)
        .transpose()
        .map(Option::flatten)
}

/// Retains usable redirects and counts unusable decoded targets without ending the join.
pub(super) fn negotiation_failure(
    connection: &mut Connection,
    packet: packets::Packet,
) -> Option<crate::ProtocolError> {
    match super::negotiation_failure(packet) {
        Ok(error) => Some(error),
        Err(reason) => {
            let skipped = connection.record_skip();
            tracing::warn!(?reason, skipped, "skipped unusable negotiation transfer");
            None
        }
    }
}

/// Handles actual terminals and moves other unhandled input into the same lower bounded prefix.
pub(super) fn defer_unhandled(
    connection: &mut Connection,
    packet: packets::Packet,
) -> Result<(), crate::ProtocolError> {
    if matches!(
        packet,
        packets::Packet::Disconnect(_) | packets::Packet::Transfer(_)
    ) {
        if let Some(error) = negotiation_failure(connection, packet) {
            return Err(error);
        }
        return Ok(());
    }
    connection.defer_current().map_err(deferred_failure)
}

/// Attaches tap/trace metadata only when the original generated raw prefix enters the app.
pub(super) fn take_deferred(connection: &mut Connection) -> Vec<crate::raw_packet::RawPacket> {
    connection
        .take_deferred()
        .into_iter()
        .filter_map(|frame| {
            let backing = frame.frame().len();
            crate::raw_packet::raw_packet(frame, backing)
        })
        .collect()
}

/// Retains the relay prefix through first StartGame without decoding unrelated known bodies.
pub(super) async fn defer_through_start_game(
    connection: &mut Connection,
) -> Result<(), crate::ProtocolError> {
    for raw in connection.deferred() {
        if raw.id() == packets::StartGame::ID {
            return Ok(());
        }
        if raw.id() == packets::PlayStatus::ID {
            check_startup_packet(
                &connection
                    .decode_body(packets::PacketId::PlayStatus, &raw.body())
                    .map_err(super::failure)?,
            )?;
        }
    }
    loop {
        let (header, body) = connection.next_known_raw().await.map_err(super::failure)?;
        let id = packets::PacketId::from_raw(header.id).expect("known generated packet");
        if matches!(
            id,
            packets::PacketId::StartGame
                | packets::PacketId::Disconnect
                | packets::PacketId::Transfer
                | packets::PacketId::PlayStatus
        ) {
            let packet = connection
                .decode_body(id, &body)
                .inspect_err(|_| connection.finish_current())
                .map_err(super::failure)?;
            check_startup_packet(&packet)?;
            defer_unhandled(connection, packet)?;
        } else {
            connection.defer_current().map_err(deferred_failure)?;
        }
        if id == packets::PacketId::StartGame {
            return Ok(());
        }
    }
}

/// Restores the app's existing typed aggregate-limit failure without losing its original cause.
fn deferred_failure(error: io::Error) -> crate::ProtocolError {
    if let Some(limit) = error
        .get_ref()
        .and_then(|error| error.downcast_ref::<DeferredLimit>())
    {
        return crate::codec::SessionFailure::BatchTooLarge {
            actual: limit.actual,
            max: limit.maximum,
        }
        .into();
    }
    super::failure(error)
}

/// Preserves startup refusal and Disconnect causes before waiting for another original packet.
fn check_startup_packet(packet: &packets::Packet) -> Result<(), crate::ProtocolError> {
    if let packets::Packet::Disconnect(disconnect) = packet {
        return Err(super::super::startup::packet_disconnect(disconnect).into());
    }
    if let packets::Packet::PlayStatus(status) = packet
        && let Ok(status_kind) = crate::ui::player_status(status.status)
        && !matches!(
            status_kind,
            crate::ui::PlayerStatus::LoginSuccess | crate::ui::PlayerStatus::PlayerSpawn
        )
    {
        return Err(crate::codec::SessionFailure::ClientHandshake(
            bedrock_client::handshake::HandshakeError::LoginRefused(status.status),
        )
        .into());
    }
    Ok(())
}

/// Projects the lower driver's typed cause without moving application UI or redirect policy down.
pub(super) async fn driver_result<T>(
    connection: &mut Connection,
    result: Result<T, bedrock_client_tokio::error::NegotiationError>,
) -> Result<T, crate::ProtocolError> {
    use bedrock_client_tokio::error::NegotiationError;
    match result {
        Ok(value) => Ok(value),
        Err(NegotiationError::Io(error)) => Err(deferred_failure(error)),
        Err(NegotiationError::Write(error)) if super::authentication::is_expired(&error) => {
            Err(crate::ProtocolError::Authentication(error))
        }
        Err(NegotiationError::Write(error)) => {
            Err(super::preserve_negotiation_boundary(connection, error).await)
        }
        Err(NegotiationError::Handshake(error)) => Err(super::handshake_failure(error)),
        Err(NegotiationError::Packs(error)) => Err(super::failure(io::Error::new(
            io::ErrorKind::InvalidData,
            error,
        ))),
        Err(NegotiationError::Disconnect(packet)) => Err(super::disconnected(*packet)),
        Err(NegotiationError::Transfer(target)) => Err(
            crate::codec::SessionFailure::ServerTransfer(crate::ServerTransferEvent {
                host: target.host,
                port: target.port,
                reload_world: target.reload_world,
            })
            .into(),
        ),
    }
}
