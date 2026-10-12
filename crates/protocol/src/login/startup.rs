// Adapted from Jolyne (Axolotl Stack, https://github.com/axolotl-stack/axolotl-stack),
// Copyright (c) 2025 Jeremy Ianne, MIT License; see crates/protocol/vendor/LICENSE.

//! The client's spawn sequence after the core hands over a session at StartGame: request the view
//! radius, wait for the spawn prerequisites, and defer every other packet to play in order.

use std::time::Duration;

use valentine::bedrock::version::v1_26_51::{
    ActorRuntimeId, DisconnectPacket, EnumsPlayStatus, ItemRegistryPacket, McpePacketData,
    McpePacketName, SetLocalPlayerAsInitializedPacket, StartGamePacket,
};

use crate::codec::SessionFailure;
use crate::disconnect::{delivered_texts, disconnect_reason, join_disconnect};
use crate::raw_packet::RawPacket;
use crate::session_transport::{Inbound, SessionTransport, Terminal};
use crate::{LoadingScreenPhase, Packet, ProtocolError, ServerTransferEvent, SessionError};

/// Bounds the drain that surfaces a disconnect arriving with a failed startup write.
const STARTUP_WRITE_FAILURE_DRAIN: Duration = Duration::from_secs(5);
/// Bounds the bytes deferred to play while the spawn prerequisites arrive.
pub(super) const MAX_DEFERRED_PACKET_BYTES: usize = 16 * 1024 * 1024;
/// The view radius the client requests before it spawns.
const STARTUP_CHUNK_RADIUS: u8 = 16;
/// The stage a join-time Disconnect names once the session has been handed over.
const STAGE: &str = "StartGame";

/// What the spawn sequence leaves for play.
pub(super) struct Spawned {
    pub(super) start_game: StartGamePacket,
    pub(super) item_registry: ItemRegistryPacket,
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
/// packet ahead of unread ones. Dropping the future cancels it.
pub(super) async fn await_spawn(
    transport: &mut SessionTransport,
) -> Result<Spawned, ProtocolError> {
    let mut start_game: Option<(u64, StartGamePacket)> = None;
    let mut item_registry: Option<ItemRegistryPacket> = None;
    let mut sent_chunk_radius = false;
    let mut received_chunk_radius = false;
    let mut received_player_spawn = false;
    let mut terrain_before_spawn = false;
    let mut deferred = DeferredPackets::default();
    loop {
        let raw = recv_startup(transport).await?;
        match raw.id {
            McpePacketName::StartGamePacket => {
                let McpePacketData::StartGamePacket(start) = raw.decode()?.data else {
                    unreachable!("packet ID and decoded variant must agree")
                };
                let runtime_id = start.runtime_id.actor_runtime_id;
                match start_game.as_ref().map(|(first, _)| *first) {
                    Some(first) if first != runtime_id => {
                        return Err(SessionFailure::UnexpectedStartup(format!(
                            "conflicting StartGame runtime entity ID: first {first}, then {runtime_id}"
                        ))
                        .into());
                    }
                    Some(_) => {}
                    None => start_game = Some((runtime_id, *start)),
                }
            }
            McpePacketName::ItemRegistryPacket => {
                let McpePacketData::ItemRegistryPacket(registry) = raw.decode()?.data else {
                    unreachable!("packet ID and decoded variant must agree")
                };
                // The native registry initializes once: a later empty or custom-only table must
                // not replace the first.
                if item_registry.is_some() {
                    continue;
                }
                item_registry = Some(registry);
            }
            McpePacketName::PlayStatusPacket => {
                let McpePacketData::PlayStatusPacket(status) = raw.decode()?.data else {
                    unreachable!("packet ID and decoded variant must agree")
                };
                if status.status == EnumsPlayStatus::Playerspawn {
                    received_player_spawn = true;
                }
            }
            McpePacketName::ChunkRadiusUpdatedPacket => {
                let McpePacketData::ChunkRadiusUpdatedPacket(update) = raw.clone().decode()?.data
                else {
                    unreachable!("packet ID and decoded variant must agree")
                };
                if update.chunk_radius < 1 {
                    return Err(SessionFailure::UnexpectedStartup(format!(
                        "invalid updated chunk radius {}",
                        update.chunk_radius
                    ))
                    .into());
                }
                deferred.push(raw)?;
                received_chunk_radius = true;
            }
            McpePacketName::DisconnectPacket => {
                let McpePacketData::DisconnectPacket(disconnect) = raw.decode()?.data else {
                    unreachable!("packet ID and decoded variant must agree")
                };
                return Err(packet_disconnect(&disconnect).into());
            }
            packet_id => {
                terrain_before_spawn |= matches!(
                    packet_id,
                    McpePacketName::NetworkChunkPublisherUpdatePacket
                        | McpePacketName::LevelChunkPacket
                        | McpePacketName::SubChunkPacket
                );
                deferred.push(raw)?;
            }
        }

        if !sent_chunk_radius && start_game.is_some() {
            if let Err(error) = transport.send(&loading_started()).await {
                return Err(
                    startup_write_failure(transport, error, STARTUP_WRITE_FAILURE_DRAIN).await,
                );
            }
            sent_chunk_radius = true;
        }
        if sent_chunk_radius
            && received_chunk_radius
            && received_player_spawn
            && item_registry.is_some()
        {
            break;
        }
    }
    let (runtime_id, start_game) = start_game.expect("the radius request follows StartGame");
    transport.prepend(deferred.into_packets());
    Ok(Spawned {
        start_game,
        item_registry: item_registry.expect("spawn waits for the item registry"),
        runtime_id,
        terrain_before_spawn,
    })
}

/// The radius request and loading-start the client sends once StartGame arrives.
fn loading_started() -> [Packet; 2] {
    [
        crate::request_chunk_radius_packet(STARTUP_CHUNK_RADIUS, STARTUP_CHUNK_RADIUS),
        crate::loading_screen_packet(LoadingScreenPhase::Start, None),
    ]
}

/// The loading-end and initialization packets that complete spawn for `runtime_id`.
pub(super) fn loading_finished(runtime_id: u64) -> Vec<Packet> {
    vec![
        crate::loading_screen_packet(LoadingScreenPhase::End, None),
        Packet::new(SetLocalPlayerAsInitializedPacket {
            player_id: ActorRuntimeId {
                actor_runtime_id: runtime_id,
            },
        }),
    ]
}

/// Returns the next startup packet. A usable transfer, from the core or in a batch, ends the join;
/// an unusable one is skipped. The core's terminal Disconnect ends it with the server's texts.
async fn recv_startup(transport: &mut SessionTransport) -> Result<RawPacket, ProtocolError> {
    let mut skipped = 0_u64;
    loop {
        let target = match transport.recv().await? {
            Inbound::Packet(raw) if raw.id == McpePacketName::TransferPacket => {
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
pub(super) async fn startup_write_failure(
    transport: &mut SessionTransport,
    write_error: ProtocolError,
    remaining: Duration,
) -> ProtocolError {
    let deadline = tokio::time::Instant::now() + remaining;
    loop {
        match tokio::time::timeout_at(deadline, recv_startup(transport)).await {
            Ok(Err(
                error @ ProtocolError::Session(SessionError(
                    SessionFailure::ServerTransfer(_) | SessionFailure::ServerDisconnect { .. },
                )),
            )) => return error,
            Err(_) | Ok(Err(_)) => return write_error,
            Ok(Ok(raw)) if raw.id == McpePacketName::DisconnectPacket => {
                return match raw.decode() {
                    Ok(Packet {
                        data: McpePacketData::DisconnectPacket(disconnect),
                        ..
                    }) => packet_disconnect(&disconnect).into(),
                    _ => write_error,
                };
            }
            Ok(Ok(_)) => {}
        }
    }
}

/// The failure a server's Disconnect packet ends the spawn sequence with.
fn packet_disconnect(disconnect: &DisconnectPacket) -> SessionFailure {
    join_disconnect(
        STAGE,
        &disconnect.reason,
        &disconnect.messages.message,
        &disconnect.messages.filtered_message,
    )
}

#[cfg(test)]
mod tests;
