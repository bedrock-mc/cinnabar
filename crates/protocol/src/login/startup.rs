// Adapted from Jolyne (Axolotl Stack, https://github.com/axolotl-stack/axolotl-stack),
// Copyright (c) 2025 Jeremy Ianne, MIT License; see crates/protocol/vendor/LICENSE.

//! The client's spawn sequence after the core hands over a session at StartGame: request the view
//! radius, wait for the spawn prerequisites, and defer every other packet to play in order.

use std::time::Duration;

use bedrock_client::spawn::{SpawnError, SpawnNegotiation, SpawnReady, SpawnUpdate};
use bedrock_protocol_1_26_51::packets::{self, Disconnect, ItemRegistry, PacketId, StartGame};

use crate::codec::SessionFailure;
use crate::disconnect::{delivered_texts, disconnect_reason, join_disconnect, packet_texts};
use crate::packet_tap::PacketOutcome;
use crate::raw_packet::RawPacket;
use crate::session_transport::{Inbound, SessionTransport, Terminal};
use crate::ui::{PlayerStatus, player_status};
use crate::{
    LoadingScreenPhase, Packet, ProtocolError, ServerTransferEvent, ServerTransferRejection,
    SessionError,
};

/// Bounds the bytes deferred to play while the spawn prerequisites arrive.
pub(super) const MAX_DEFERRED_PACKET_BYTES: usize = 16 * 1024 * 1024;
/// The fallback view radius when the caller supplies no saved preference.
pub(super) const DEFAULT_CHUNK_RADIUS: u8 = 16;
/// The stage a join-time Disconnect names once the session has been handed over.
const STAGE: &str = "StartGame";

/// Packets held back for play while the client spawns, each detached from its batch.
#[derive(Default)]
pub(super) struct DeferredPackets {
    packets: Vec<RawPacket>,
    bytes: usize,
}

impl DeferredPackets {
    /// Holds `packet` for play; the prerequisites can follow many batches, so the queue is
    /// bounded by bytes rather than by any one batch's packet count.
    pub(super) fn push(&mut self, packet: RawPacket) -> Result<(), SessionError> {
        let bytes = self.bytes.saturating_add(packet.inner_frame().len());
        if bytes > MAX_DEFERRED_PACKET_BYTES {
            return Err(SessionFailure::BatchTooLarge {
                actual: bytes,
                max: MAX_DEFERRED_PACKET_BYTES,
            }
            .into());
        }
        self.bytes = bytes;
        self.packets.push(packet.into_compact());
        Ok(())
    }

    pub(super) fn into_packets(self) -> Vec<RawPacket> {
        self.packets
    }

    /// Borrows the retained wire order without releasing its aggregate byte charge.
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    pub(super) fn as_slice(&self) -> &[RawPacket] {
        &self.packets
    }
}

/// Runs the spawn sequence: requests the radius once StartGame has arrived, then returns once the
/// first item registry and PlayerSpawn have arrived, leaving every deferred
/// packet ahead of unread ones. `on_startup` receives the first StartGame and item registry as soon
/// as both have arrived, while the sequence keeps reading. Dropping the future cancels it.
pub(super) async fn await_spawn<R: bridge::FrameRead>(
    transport: &mut SessionTransport<R>,
    view_distance_chunks: u8,
    on_startup: impl FnOnce(StartGame, ItemRegistry),
) -> Result<SpawnReady, ProtocolError> {
    let profile = bridge::join_profile::JoinProfile::current();
    let mut start_game_marked = false;
    let mut item_registry_marked = false;
    let mut player_spawn_marked = false;
    let mut chunk_radius_marked = false;
    let mut on_startup = Some(on_startup);
    let request = crate::request_chunk_radius_packet(
        view_distance_chunks,
        render_api::MAX_VIEW_RADIUS_CHUNKS as u8,
    );
    let packets::Packet::RequestChunkRadius(request) = *request.data else {
        unreachable!("view-distance helper returns RequestChunkRadius")
    };
    let mut negotiation = SpawnNegotiation::new(request).map_err(spawn_failure)?;
    let mut deferred = DeferredPackets::default();
    let mut skipped_statuses = 0_u64;
    loop {
        let raw = recv_startup(transport).await?;
        let update = match raw.id {
            PacketId::Disconnect => {
                let packets::Packet::Disconnect(disconnect) = *raw.decode()?.data else {
                    unreachable!("packet ID and decoded variant must agree")
                };
                return Err(packet_disconnect(&disconnect).into());
            }
            PacketId::PlayStatus => {
                let packets::Packet::PlayStatus(status) = *raw.decode()?.data else {
                    unreachable!("packet ID and decoded variant must agree")
                };
                match player_status(status.status) {
                    Ok(PlayerStatus::LoginSuccess) => SpawnUpdate::default(),
                    Ok(PlayerStatus::PlayerSpawn) => {
                        let update = negotiation
                            .receive(packets::Packet::PlayStatus(status))
                            .map_err(spawn_failure)?;
                        if !player_spawn_marked {
                            profile.mark("player_spawn");
                            player_spawn_marked = true;
                        }
                        update
                    }
                    Ok(_) => {
                        transport.accept_current();
                        return Err(SessionFailure::ClientHandshake(
                            bedrock_client::handshake::HandshakeError::LoginRefused(status.status),
                        )
                        .into());
                    }
                    Err(reason) => {
                        skipped_statuses = skipped_statuses.saturating_add(1);
                        transport.reject_current(PacketOutcome::Skipped);
                        tracing::warn!(
                            ?reason,
                            skipped_statuses,
                            "ignoring unknown startup status"
                        );
                        SpawnUpdate::default()
                    }
                }
            }
            PacketId::StartGame | PacketId::ItemRegistry | PacketId::ChunkRadiusUpdated => {
                let update = negotiation
                    .receive(*raw.clone().decode()?.data)
                    .map_err(spawn_failure)?;
                let id = raw.id;
                if update.deferred.is_some() {
                    deferred.push(raw)?;
                }
                match id {
                    PacketId::StartGame if !start_game_marked => {
                        profile.mark("start_game_decoded");
                        start_game_marked = true;
                    }
                    PacketId::ItemRegistry if !item_registry_marked => {
                        profile.mark("item_registry_decoded");
                        item_registry_marked = true;
                    }
                    PacketId::ChunkRadiusUpdated if !chunk_radius_marked => {
                        profile.mark("chunk_radius_reply");
                        chunk_radius_marked = true;
                    }
                    _ => {}
                }
                update
            }
            _ => {
                negotiation
                    .receive_deferred(raw.id)
                    .map_err(spawn_failure)?;
                deferred.push(raw)?;
                SpawnUpdate::default()
            }
        };
        transport.accept_current();
        if let Some(startup) = update.startup {
            on_startup
                .take()
                .expect("the pure owner supplies first definitions once")(
                *startup.start_game,
                startup.item_registry,
            );
        }
        if let Some(request) = update.request_radius {
            let packets = [
                Packet::new(request),
                crate::loading_screen_packet(LoadingScreenPhase::Start, None),
            ];
            if let Err(error) = transport.send(&packets).await {
                return Err(
                    startup_write_failure(transport, error, super::WRITE_FAILURE_DRAIN).await,
                );
            }
            profile.mark("chunk_radius_requested");
        }
        if let Some(ready) = update.ready {
            transport.prepend(deferred.into_packets());
            return Ok(ready);
        }
    }
}

/// Adds application loading policy to the ready core's one-shot initialization response.
/// The private play constructor receives this owner only after startup readiness.
pub(super) fn loading_finished(ready: SpawnReady) -> Vec<Packet> {
    let initialized = ready.finish_loading();
    vec![
        crate::loading_screen_packet(LoadingScreenPhase::End, None),
        Packet::new(initialized),
    ]
}

/// Retains the reusable typed cause and packet-rejection classification at the I/O edge.
fn spawn_failure(error: SpawnError) -> ProtocolError {
    SessionFailure::ClientSpawn(error).into()
}

/// Supplies a real ready core for play-only fixtures without running an I/O join.
#[cfg(test)]
pub(super) fn ready_fixture(runtime_id: u64) -> SpawnReady {
    let radius = i32::from(DEFAULT_CHUNK_RADIUS);
    let mut negotiation = SpawnNegotiation::new(packets::RequestChunkRadius {
        chunk_radius: bedrock_protocol_1_26_51::wire::ZigZag32(radius),
        max_chunk_radius: bedrock_protocol_1_26_51::wire::U8(DEFAULT_CHUNK_RADIUS),
    })
    .unwrap();
    let mut ready = None;
    for packet in [
        packets::Packet::StartGame(Box::new(StartGame {
            runtime_id: bedrock_protocol_1_26_51::types::ActorRuntimeID(runtime_id),
            ..Default::default()
        })),
        packets::Packet::ItemRegistry(ItemRegistry::default()),
        packets::Packet::ChunkRadiusUpdated(packets::ChunkRadiusUpdated {
            chunk_radius: bedrock_protocol_1_26_51::wire::ZigZag32(radius),
        }),
        packets::Packet::PlayStatus(packets::PlayStatus {
            status: bedrock_protocol_1_26_51::enums::PlayStatusType::Playerspawn,
        }),
    ] {
        ready = negotiation.receive(packet).unwrap().ready.or(ready);
    }
    ready.expect("actual startup prerequisites supplied readiness")
}

/// Returns the next startup packet. A usable transfer, from the core or in a batch, ends the join;
/// an unusable one is skipped. The core's terminal Disconnect ends it with the server's texts.
async fn recv_startup<R: bridge::FrameRead>(
    transport: &mut SessionTransport<R>,
) -> Result<RawPacket, ProtocolError> {
    let mut skipped = 0_u64;
    loop {
        let error = match transport.recv().await? {
            Inbound::Packet(raw) if raw.id == PacketId::Transfer => {
                match ServerTransferEvent::from_packet_data(&raw.decode()?.data) {
                    Ok(Some(target)) => Ok(SessionFailure::ServerTransfer(target).into()),
                    Ok(None) => unreachable!("transfer packet ID agrees with its body"),
                    Err(reason) => Err(reason),
                }
            }
            Inbound::Packet(raw) => return Ok(raw),
            Inbound::Terminal(terminal) => terminal_error(terminal),
        };
        match error {
            Ok(error) => return Err(error),
            Err(reason) => {
                skipped = skipped.saturating_add(1);
                tracing::warn!(?reason, skipped, "ignoring unusable startup transfer");
            }
        }
    }
}

/// Maps a core terminal to the same validated startup outcome before or during preparation.
/// Unusable transfer targets remain semantic skips rather than early startup failures.
pub(super) fn terminal_error(terminal: Terminal) -> Result<ProtocolError, ServerTransferRejection> {
    match terminal {
        Terminal::Transfer(transfer) => ServerTransferEvent::from_target(
            &transfer.address,
            transfer.port,
            transfer.reload_world,
        )
        .map(|target| SessionFailure::ServerTransfer(target).into()),
        Terminal::Disconnect(disconnect) => {
            let (message, filtered_message) = delivered_texts(&disconnect);
            Ok(join_disconnect(
                STAGE,
                &disconnect_reason(disconnect.reason),
                message,
                filtered_message,
            )
            .into())
        }
    }
}

/// Drains briefly after a failed startup write, reporting a transfer or disconnect that arrived
/// instead of the write error.
pub(super) async fn startup_write_failure<R: bridge::FrameRead>(
    transport: &mut SessionTransport<R>,
    write_error: ProtocolError,
    remaining: Duration,
) -> ProtocolError {
    let expires = transport.sleep(remaining);
    tokio::pin!(expires);
    loop {
        match bridge::tasks::until(recv_startup(transport), &mut expires).await {
            Ok(Err(
                error @ ProtocolError::Session(SessionError(
                    SessionFailure::ServerTransfer(_) | SessionFailure::ServerDisconnect { .. },
                )),
            )) => return error,
            Err(_) | Ok(Err(_)) => return write_error,
            Ok(Ok(raw)) if raw.id == PacketId::Disconnect => {
                return match raw.decode().map(|packet| *packet.data) {
                    Ok(packets::Packet::Disconnect(disconnect)) => {
                        packet_disconnect(&disconnect).into()
                    }
                    _ => write_error,
                };
            }
            Ok(Ok(_)) => {}
        }
    }
}

/// The failure a server's Disconnect packet ends the spawn sequence with.
pub(super) fn packet_disconnect(disconnect: &Disconnect) -> SessionFailure {
    let (message, filtered_message) = packet_texts(disconnect);
    join_disconnect(STAGE, &disconnect.reason, &message, &filtered_message)
}

#[cfg(test)]
mod tests;
