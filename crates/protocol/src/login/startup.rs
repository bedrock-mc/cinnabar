// Adapted from Jolyne (Axolotl Stack, https://github.com/axolotl-stack/axolotl-stack),
// Copyright (c) 2025 Jeremy Ianne, MIT License; see crates/protocol/vendor/LICENSE.

//! The client's spawn sequence after the core hands over a session at StartGame: request the view
//! radius, wait for the spawn prerequisites, and defer every other packet to play in order.

use std::time::Duration;

use bedrock_protocol_1_26_51::enums::PlayStatusType;
use bedrock_protocol_1_26_51::packets::{
    self, Disconnect, ItemRegistry, PacketId, SetLocalPlayerAsInitialized, StartGame,
};
use bedrock_protocol_1_26_51::types::ActorRuntimeID;

use crate::codec::SessionFailure;
use crate::disconnect::{delivered_texts, disconnect_reason, join_disconnect, packet_texts};
use crate::raw_packet::RawPacket;
use crate::session_transport::{Inbound, SessionTransport, Terminal};
use crate::{LoadingScreenPhase, Packet, ProtocolError, ServerTransferEvent, SessionError};

/// Bounds the bytes deferred to play while the spawn prerequisites arrive.
pub(super) const MAX_DEFERRED_PACKET_BYTES: usize = 16 * 1024 * 1024;
/// The fallback view radius when the caller supplies no saved preference.
pub(super) const DEFAULT_CHUNK_RADIUS: u8 = 16;
/// The stage a join-time Disconnect names once the session has been handed over.
const STAGE: &str = "StartGame";

/// What the spawn sequence leaves for play.
pub(super) struct Spawned {
    pub(super) runtime_id: u64,
    pub(super) terrain_before_spawn: bool, // a publisher update or chunk preceded PlayerSpawn
}

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
}

/// Runs the spawn sequence: requests the radius once StartGame has arrived, then returns once the
/// first item registry, the radius reply and PlayerSpawn have all arrived, leaving every deferred
/// packet ahead of unread ones. `on_startup` receives the first StartGame and item registry as soon
/// as both have arrived, while the sequence keeps reading. Dropping the future cancels it.
pub(super) async fn await_spawn<R: bridge::FrameRead>(
    transport: &mut SessionTransport<R>,
    view_distance_chunks: u8,
    on_startup: impl FnOnce(StartGame, ItemRegistry),
) -> Result<Spawned, ProtocolError> {
    let profile = bridge::join_profile::JoinProfile::current();
    let mut on_startup = Some(on_startup);
    let mut runtime_id: Option<u64> = None;
    let mut start_game: Option<StartGame> = None;
    let mut item_registry: Option<ItemRegistry> = None;
    let mut registry_seen = false;
    let mut sent_chunk_radius = false;
    let mut received_chunk_radius = false;
    let mut received_player_spawn = false;
    let mut terrain_before_spawn = false;
    let mut deferred = DeferredPackets::default();
    loop {
        let raw = recv_startup(transport).await?;
        match raw.id {
            PacketId::StartGame => {
                let packets::Packet::StartGame(start) = *raw.decode()?.data else {
                    unreachable!("packet ID and decoded variant must agree")
                };
                let received = start.runtime_id.0;
                match runtime_id {
                    Some(first) if first != received => {
                        return Err(SessionFailure::UnexpectedStartup(format!(
                            "conflicting StartGame runtime entity ID: first {first}, then {received}"
                        ))
                        .into());
                    }
                    Some(_) => {}
                    None => {
                        profile.mark("start_game_decoded");
                        runtime_id = Some(received);
                        start_game = Some(*start);
                    }
                }
            }
            PacketId::ItemRegistry => {
                let packets::Packet::ItemRegistry(registry) = *raw.decode()?.data else {
                    unreachable!("packet ID and decoded variant must agree")
                };
                // The native registry initializes once: a later empty or custom-only table must
                // not replace the first.
                if registry_seen {
                    continue;
                }
                profile.mark("item_registry_decoded");
                registry_seen = true;
                item_registry = Some(registry);
            }
            PacketId::PlayStatus => {
                let packets::Packet::PlayStatus(status) = *raw.decode()?.data else {
                    unreachable!("packet ID and decoded variant must agree")
                };
                if status.status == PlayStatusType::Playerspawn {
                    if !received_player_spawn {
                        profile.mark("player_spawn");
                    }
                    received_player_spawn = true;
                }
            }
            PacketId::ChunkRadiusUpdated => {
                let packets::Packet::ChunkRadiusUpdated(update) = *raw.clone().decode()?.data
                else {
                    unreachable!("packet ID and decoded variant must agree")
                };
                if update.chunk_radius.0 < 1 {
                    return Err(SessionFailure::UnexpectedStartup(format!(
                        "invalid updated chunk radius {}",
                        update.chunk_radius.0
                    ))
                    .into());
                }
                deferred.push(raw)?;
                if !received_chunk_radius {
                    profile.mark("chunk_radius_reply");
                }
                received_chunk_radius = true;
            }
            PacketId::Disconnect => {
                let packets::Packet::Disconnect(disconnect) = *raw.decode()?.data else {
                    unreachable!("packet ID and decoded variant must agree")
                };
                return Err(packet_disconnect(&disconnect).into());
            }
            packet_id => {
                terrain_before_spawn |= matches!(
                    packet_id,
                    PacketId::NetworkChunkPublisherUpdate
                        | PacketId::LevelChunk
                        | PacketId::SubChunk
                );
                deferred.push(raw)?;
            }
        }

        transport.accept_current();
        if start_game.is_some()
            && item_registry.is_some()
            && let Some(on_startup) = on_startup.take()
        {
            on_startup(
                start_game.take().expect("checked above"),
                item_registry.take().expect("checked above"),
            );
        }
        if !sent_chunk_radius && runtime_id.is_some() {
            if let Err(error) = transport.send(&loading_started(view_distance_chunks)).await {
                return Err(
                    startup_write_failure(transport, error, super::WRITE_FAILURE_DRAIN).await,
                );
            }
            profile.mark("chunk_radius_requested");
            sent_chunk_radius = true;
        }
        if sent_chunk_radius
            && received_chunk_radius
            && received_player_spawn
            && on_startup.is_none()
        {
            break;
        }
    }
    transport.prepend(deferred.into_packets());
    Ok(Spawned {
        runtime_id: runtime_id.expect("the radius request follows StartGame"),
        terrain_before_spawn,
    })
}

/// The radius request and loading-start the client sends once StartGame arrives.
fn loading_started(view_distance_chunks: u8) -> [Packet; 2] {
    [
        crate::request_chunk_radius_packet(
            view_distance_chunks,
            render_api::MAX_VIEW_RADIUS_CHUNKS as u8,
        ),
        crate::loading_screen_packet(LoadingScreenPhase::Start, None),
    ]
}

/// The loading-end and initialization packets that complete spawn for `runtime_id`.
pub(super) fn loading_finished(runtime_id: u64) -> Vec<Packet> {
    vec![
        crate::loading_screen_packet(LoadingScreenPhase::End, None),
        Packet::new(SetLocalPlayerAsInitialized {
            player_id: ActorRuntimeID(runtime_id),
        }),
    ]
}

/// Returns the next startup packet. A usable transfer, from the core or in a batch, ends the join;
/// an unusable one is skipped. The core's terminal Disconnect ends it with the server's texts.
async fn recv_startup<R: bridge::FrameRead>(
    transport: &mut SessionTransport<R>,
) -> Result<RawPacket, ProtocolError> {
    let mut skipped = 0_u64;
    loop {
        let target = match transport.recv().await? {
            Inbound::Packet(raw) if raw.id == PacketId::Transfer => {
                match ServerTransferEvent::from_packet_data(&raw.decode()?.data) {
                    Ok(Some(target)) => Ok(target),
                    Ok(None) => unreachable!("transfer packet ID agrees with its body"),
                    Err(reason) => Err(reason),
                }
            }
            Inbound::Packet(raw) => return Ok(raw),
            Inbound::Terminal(Terminal::Transfer(transfer)) => ServerTransferEvent::from_target(
                &transfer.address,
                transfer.port,
                transfer.reload_world,
            ),
            Inbound::Terminal(Terminal::Disconnect(disconnect)) => {
                let (message, filtered_message) = delivered_texts(&disconnect);
                return Err(join_disconnect(
                    STAGE,
                    &disconnect_reason(disconnect.reason),
                    message,
                    filtered_message,
                )
                .into());
            }
        };
        match target {
            Ok(target) => return Err(SessionFailure::ServerTransfer(target).into()),
            Err(reason) => {
                skipped = skipped.saturating_add(1);
                tracing::warn!(?reason, skipped, "ignoring unusable startup transfer");
            }
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
fn packet_disconnect(disconnect: &Disconnect) -> SessionFailure {
    let (message, filtered_message) = packet_texts(disconnect);
    join_disconnect(STAGE, &disconnect.reason, &message, &filtered_message)
}

#[cfg(test)]
mod tests;
