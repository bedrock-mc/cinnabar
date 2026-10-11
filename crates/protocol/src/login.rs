use std::collections::VecDeque;

use bytes::Bytes;
use jolyne::error::JolyneError;
use jolyne::raw::RawPacket;
use jolyne::stream::client::ClientHandshakeConfig;
use jolyne::stream::transport::{BedrockTransport, Transport};
use jolyne::stream::{BedrockStream, Client, Handshake, Play};
use valentine::bedrock::version::v1_26_51::{McpePacketData, McpePacketName};
#[cfg(test)]
use valentine::protocol::wire;

use crate::blob_cache::ResolverReady;
use crate::session_transport::SessionTransport;
use crate::{
    BlobCacheResolver, BlobCacheStats, ClientBlobCache, GameData, LevelChunkEvent, Packet,
    ProtocolError, ResourcePackHandoff, ServerDisconnectEvent, ServerTransferEvent, WorldEvent,
    into_world_event,
};

mod boundary;
mod client_data;
mod latency_probe;
mod outbound;
mod packet_trace;
mod raw_equipment;
pub(crate) mod session_join;
use boundary::boundary_wakeup;
pub use client_data::LoginSettings;
pub use latency_probe::network_stack_latency_reply;
pub use outbound::PlayOutbound;
pub use packet_trace::PacketIdTraceSnapshot;
use packet_trace::PacketIdTraceState;
#[cfg(test)]
use packet_trace::{MAX_PACKET_ID_TRACE_ENTRIES, PACKET_ID_TRACE_DURATION};

/// A server may batch a whole pack transfer, or a large item registry, into one batch,
/// so the local leg accepts batches far larger than a typical client's 16 MiB default.
/// The core already bounds pack transfers before they reach this hop.
const MAX_DECOMPRESSED_BATCH_SIZE: usize = 256 * 1024 * 1024;

/// Entry point for joining through the core.
pub struct LoginSequence;

impl LoginSequence {
    /// Headless test seam that treats the received spawn prerequisites as presentation readiness.
    #[doc(hidden)]
    pub async fn connect_transport<T: Transport>(
        transport: T,
        display_name: &str,
    ) -> Result<(PlaySession<T>, GameData), ProtocolError> {
        let (mut session, data) =
            Self::connect_transport_inner(transport, display_name, None).await?;
        session.finish_loading().await?;
        Ok((session, data))
    }

    /// Headless cache test seam that completes loading immediately after negotiation.
    #[doc(hidden)]
    pub async fn connect_transport_with_blob_cache<T: Transport>(
        transport: T,
        display_name: &str,
        cache: ClientBlobCache,
    ) -> Result<(PlaySession<T>, GameData), ProtocolError> {
        let (mut session, data) =
            Self::connect_transport_inner(transport, display_name, Some(cache)).await?;
        session.finish_loading().await?;
        Ok((session, data))
    }

    async fn connect_transport_inner<T: Transport>(
        transport: T,
        display_name: &str,
        cache: Option<ClientBlobCache>,
    ) -> Result<(PlaySession<T>, GameData), ProtocolError> {
        let peer_addr = transport.peer_addr();
        let mut transport = BedrockTransport::new(transport);
        transport.set_max_decompressed_batch_size(Some(MAX_DECOMPRESSED_BATCH_SIZE));
        let stream: BedrockStream<Handshake, Client, T> = BedrockStream::from_transport(transport);
        let config = ClientHandshakeConfig::random(peer_addr, display_name)
            .with_client_cache_enabled(cache.is_some());
        let (stream, game_data) = stream.join(config).await?;
        Ok((PlaySession::new(stream, cache), game_data))
    }
}

/// An authenticated, spawned Bedrock session.
pub struct PlaySession<T: Transport = SessionTransport> {
    stream: BedrockStream<Play, Client, T>,
    decode_errors: u64,
    world_skips: u64,
    transfer_skips: u64,
    blob_cache: Option<BlobCacheResolver>,
    packet_id_trace: PacketIdTraceState,
    pending_blob_cache_delivery: Option<PendingBlobCacheDelivery>,
    server_disconnect: Option<ServerDisconnectEvent>,
    server_transfer: Option<ServerTransferEvent>,
}

struct PendingBlobCacheDelivery {
    status_packets: VecDeque<Packet>,
    status_send_in_flight: bool,
    events: VecDeque<WorldEvent>,
}

enum WorldIngress {
    Event(WorldEvent),
    // This slice avoids a payload-sized copy, but intentionally retains the
    // decompressed batch allocation until client-world finishes the decode job.
    LevelChunk(LevelChunkEvent, Bytes),
}

impl WorldIngress {
    fn into_world_event(self) -> WorldEvent {
        match self {
            Self::Event(event) => event,
            Self::LevelChunk(mut event, payload) => {
                event.payload = payload.to_vec();
                WorldEvent::LevelChunk(event)
            }
        }
    }
}

impl<T: Transport> PlaySession<T> {
    /// Sends loading-end and initialization once the client can present the world.
    pub async fn finish_loading(&mut self) -> Result<(), ProtocolError> {
        self.stream
            .finish_loading()
            .await
            .map_err(ProtocolError::from)
    }

    fn new(stream: BedrockStream<Play, Client, T>, cache: Option<ClientBlobCache>) -> Self {
        Self {
            stream,
            decode_errors: 0,
            world_skips: 0,
            transfer_skips: 0,
            blob_cache: cache.map(BlobCacheResolver::new),
            packet_id_trace: PacketIdTraceState::default(),
            pending_blob_cache_delivery: None,
            server_disconnect: None,
            server_transfer: None,
        }
    }

    /// Takes the most recent normalized server-initiated disconnect, if any.
    pub fn take_server_disconnect(&mut self) -> Option<ServerDisconnectEvent> {
        self.server_disconnect.take()
    }

    /// Takes the most recent normalized server-directed transfer target.
    ///
    /// Like the retained disconnect reason this is one-shot: the play pump
    /// consumes it once to classify the session as transferred.
    pub fn take_server_transfer(&mut self) -> Option<ServerTransferEvent> {
        self.server_transfer.take()
    }

    /// Count of well-formed transfer packets whose target was unusable.
    ///
    /// These are counted semantic skips, not failures: the wire decoded
    /// completely and the session survives.
    pub fn transfer_skip_count(&self) -> u64 {
        self.transfer_skips
    }

    /// Takes the validated ordered resource-pack archives captured during login.
    /// No archive is parsed or applied, and a second call returns an empty handoff.
    pub fn take_resource_pack_handoff(&mut self) -> ResourcePackHandoff {
        self.stream.take_resource_pack_handoff()
    }

    /// Whether the server sent terrain before login completed at PlayerSpawn.
    /// Servers such as Dragonfly stream none until the client reports initialized.
    #[must_use]
    pub fn terrain_before_spawn(&self) -> bool {
        self.stream.terrain_before_spawn()
    }

    /// Skips a well-formed but semantically unusable world packet instead of
    /// tearing down the session, counting it for observability. Genuine wire
    /// decode/transport errors stay fatal and are returned unchanged.
    fn skip_or_fail_world(&mut self, error: ProtocolError) -> Result<(), ProtocolError> {
        match skip_semantic_world_error(error, &mut self.world_skips) {
            Ok(()) => Ok(()),
            Err(error) => {
                self.reset_blob_cache_pending();
                Err(error)
            }
        }
    }

    /// Count of world packets skipped because normalization rejected them.
    pub fn world_skip_count(&self) -> u64 {
        self.world_skips
    }

    /// Receives one packet, counting malformed/decompression failures.
    pub async fn recv(&mut self) -> Result<Packet, ProtocolError> {
        match self.stream.recv_packet().await {
            Ok(packet) => {
                if matches!(
                    &packet.data,
                    McpePacketData::TransferPacket(_) | McpePacketData::DisconnectPacket(_)
                ) {
                    self.reset_blob_cache_pending();
                }
                Ok(packet)
            }
            Err(error) => {
                if is_decode_error(&error) {
                    self.decode_errors = self.decode_errors.saturating_add(1);
                }
                self.reset_blob_cache_pending();
                Err(error.into())
            }
        }
    }

    /// Receives the next world-streaming event without decoding unrelated play packets.
    pub async fn recv_world_event(
        &mut self,
        current_dimension: i32,
    ) -> Result<WorldEvent, ProtocolError> {
        loop {
            let event = self
                .recv_world_ingress(current_dimension)
                .await?
                .into_world_event();
            if let WorldEvent::NetworkStackLatency(creation_time) = event {
                self.send(network_stack_latency_reply(creation_time))
                    .await?;
            } else {
                return Ok(event);
            }
        }
    }

    /// Receives world work while allowing the app to retain an uncopied LevelChunk payload.
    /// Other callers should continue using [`Self::recv_world_event`].
    #[doc(hidden)]
    pub async fn recv_world_event_mapped<U>(
        &mut self,
        current_dimension: i32,
        map_event: impl FnOnce(WorldEvent) -> U,
        map_level_chunk: impl FnOnce(LevelChunkEvent, Bytes) -> U,
    ) -> Result<U, ProtocolError> {
        Ok(match self.recv_world_ingress(current_dimension).await? {
            WorldIngress::Event(event) => map_event(event),
            WorldIngress::LevelChunk(event, payload) => map_level_chunk(event, payload),
        })
    }

    async fn recv_world_ingress(
        &mut self,
        current_dimension: i32,
    ) -> Result<WorldIngress, ProtocolError> {
        if self.blob_cache.is_some() {
            return self
                .recv_world_ingress_with_blob_cache(current_dimension)
                .await;
        }
        loop {
            let raw = match self.stream.recv_packet_raw().await {
                Ok(raw) => raw,
                Err(error) => {
                    if is_decode_error(&error) {
                        self.decode_errors = self.decode_errors.saturating_add(1);
                    }
                    return Err(error.into());
                }
            };
            self.packet_id_trace.observe(raw.id);
            if raw.id == McpePacketName::LevelChunkPacket {
                let raw = raw.into_retention_bounded();
                let borrowed = raw
                    .decode_borrowed()
                    .map_err(|error| self.fail_session(error))?;
                let valentine::bedrock::version::v1_26_51::BorrowedMcpePacketData::LevelChunkPacket(
                    packet,
                ) = borrowed.data
                else {
                    unreachable!("LevelChunk packet ID decoded to another borrowed variant")
                };
                match crate::world::normalize_borrowed_level_chunk(packet) {
                    Ok((event, payload)) => return Ok(WorldIngress::LevelChunk(event, payload)),
                    Err(error) => {
                        self.skip_or_fail_world(error.into())?;
                        continue;
                    }
                }
            }
            if matches!(
                raw.id,
                McpePacketName::TransferPacket | McpePacketName::DisconnectPacket
            ) {
                let name = raw.id;
                if self.absorb_boundary_packet(raw, name).await? {
                    return boundary_wakeup();
                }
                continue;
            }
            let decoded = decode_world_raw_with(raw, current_dimension, |raw| {
                self.stream.decode_raw_packet(raw)
            });
            match decoded {
                Ok(Some(event)) => return Ok(WorldIngress::Event(event)),
                Ok(None) => {}
                Err(ProtocolError::Session(error)) => {
                    if is_decode_error(&error) {
                        self.decode_errors = self.decode_errors.saturating_add(1);
                    }
                    return Err(ProtocolError::Session(error));
                }
                Err(error) => self.skip_or_fail_world(error)?,
            }
        }
    }

    /// Sends one packet through the encrypted play session.
    pub async fn send(&mut self, packet: Packet) -> Result<(), ProtocolError> {
        crate::codec::validate_packet(&packet)?;
        self.stream.send_packet(packet).await?;
        Ok(())
    }

    /// Starts a bounded, secret-safe packet-ID trace for native acceptance.
    pub fn begin_packet_id_trace(&mut self) {
        self.packet_id_trace.begin();
    }

    /// Cancels an armed trace when the triggering packet was not sent.
    pub fn cancel_packet_id_trace(&mut self) {
        self.packet_id_trace.cancel();
    }

    /// Drains packet IDs observed since the last drain without packet payloads.
    pub fn drain_packet_id_trace(&mut self) -> Option<PacketIdTraceSnapshot> {
        self.packet_id_trace.drain()
    }

    /// Number of receive-side decode/decompression failures observed in play.
    pub fn decode_error_count(&self) -> u64 {
        self.decode_errors
    }

    /// Whether login advertised cache support and this session owns a resolver.
    #[must_use]
    pub const fn blob_cache_enabled(&self) -> bool {
        self.blob_cache.is_some()
    }

    /// Secret-safe cache counters for acceptance evidence.
    #[must_use]
    pub fn blob_cache_stats(&self) -> BlobCacheStats {
        self.blob_cache
            .as_ref()
            .map_or_else(BlobCacheStats::default, BlobCacheResolver::stats)
    }

    /// Drops only session-scoped transactions; verified cache entries remain shared.
    pub fn reset_blob_cache_pending(&mut self) {
        if let Some(resolver) = self.blob_cache.as_mut() {
            resolver.reset_pending();
        }
        self.pending_blob_cache_delivery = None;
    }

    /// Arms a one-shot selective transaction rotation for the next raw
    /// LevelChunk/SubChunk candidate. Verified blobs and ready work survive.
    pub fn arm_blob_cache_reset_for_fast_transfer(&mut self) {
        if let Some(resolver) = self.blob_cache.as_mut() {
            resolver.arm_fast_transfer_reset();
        }
    }

    async fn recv_world_ingress_with_blob_cache(
        &mut self,
        current_dimension: i32,
    ) -> Result<WorldIngress, ProtocolError> {
        loop {
            if self
                .pending_blob_cache_delivery
                .as_ref()
                .is_some_and(|delivery| delivery.status_send_in_flight)
            {
                if let Err(error) = self.stream.drain_send().await {
                    self.reset_blob_cache_pending();
                    return Err(error.into());
                }
                self.pending_blob_cache_delivery
                    .as_mut()
                    .expect("pending cache delivery survives a successful drain")
                    .status_send_in_flight = false;
                continue;
            }
            if let Some(recovery) = self
                .blob_cache
                .as_mut()
                .expect("enabled path owns a resolver")
                .pop_recovery_ready()
            {
                return Ok(WorldIngress::Event(WorldEvent::ChunkResync(recovery)));
            }
            if let Some(status_packet) = self
                .pending_blob_cache_delivery
                .as_mut()
                .and_then(|delivery| delivery.status_packets.pop_front())
            {
                self.pending_blob_cache_delivery
                    .as_mut()
                    .expect("the status packet came from a pending cache delivery")
                    .status_send_in_flight = true;
                // SessionTransport retains an accepted frame until that exact frame flushes.
                // Transfer ownership before awaiting so cancellation cannot logically resend it.
                if let Err(error) = self.send(status_packet).await {
                    self.reset_blob_cache_pending();
                    return Err(error);
                }
                self.pending_blob_cache_delivery
                    .as_mut()
                    .expect("pending cache delivery survives a successful send")
                    .status_send_in_flight = false;
                continue;
            }
            if let Some(event) = self
                .pending_blob_cache_delivery
                .as_mut()
                .and_then(|delivery| delivery.events.pop_front())
            {
                return Ok(WorldIngress::Event(event));
            }
            self.pending_blob_cache_delivery = None;
            if let Some(ready) = self
                .blob_cache
                .as_mut()
                .expect("enabled path owns a resolver")
                .pop_ready_ingress()
            {
                let event = match ready {
                    ResolverReady::Packet(packet) => {
                        match into_world_event(packet, current_dimension) {
                            Ok(Some(WorldEvent::LevelChunk(mut event))) => {
                                let payload = Bytes::from(std::mem::take(&mut event.payload));
                                return Ok(WorldIngress::LevelChunk(event, payload));
                            }
                            Ok(Some(event)) => event,
                            Ok(None) => {
                                self.reset_blob_cache_pending();
                                continue;
                            }
                            Err(error) => {
                                self.skip_or_fail_world(error.into())?;
                                continue;
                            }
                        }
                    }
                    ResolverReady::WorldEvent(event) => event,
                    ResolverReady::LevelChunkBytes(event, payload) => {
                        return Ok(WorldIngress::LevelChunk(event, payload));
                    }
                };
                if matches!(event, WorldEvent::ChangeDimension(_)) {
                    self.reset_blob_cache_pending();
                }
                return Ok(WorldIngress::Event(event));
            }

            let resolver = self
                .blob_cache
                .as_mut()
                .expect("enabled path owns a resolver");
            if resolver.ordinary_lane_needs_drain() {
                resolver.unblock_ordinary_lane()?;
                continue;
            }

            let raw = match self.stream.recv_packet_raw().await {
                Ok(raw) => raw,
                Err(error) => return Err(self.fail_session(error)),
            };
            self.packet_id_trace.observe(raw.id);
            let packet_bytes = raw.inner_frame().len();
            let packet_name = raw.id;
            let raw = if packet_name == McpePacketName::LevelChunkPacket {
                raw.into_retention_bounded()
            } else {
                raw
            };

            if matches!(
                packet_name,
                McpePacketName::TransferPacket | McpePacketName::DisconnectPacket
            ) {
                if self.absorb_boundary_packet(raw, packet_name).await? {
                    return boundary_wakeup();
                }
                continue;
            }

            if matches!(
                packet_name,
                McpePacketName::LevelChunkPacket
                    | McpePacketName::SubChunkPacket
                    | McpePacketName::ClientCacheMissResponsePacket
            ) {
                if packet_name == McpePacketName::LevelChunkPacket {
                    let borrowed_raw = raw.clone();
                    let borrowed = match borrowed_raw.decode_borrowed() {
                        Ok(packet) => packet,
                        Err(error) => return Err(self.fail_session(error)),
                    };
                    let valentine::bedrock::version::v1_26_51::BorrowedMcpePacketData::LevelChunkPacket(view) = borrowed.data else {
                        unreachable!("LevelChunk packet ID decoded to another borrowed variant")
                    };
                    if !view.cache_enabled {
                        let (event, payload) =
                            match crate::world::normalize_borrowed_level_chunk(view) {
                                Ok(value) => value,
                                Err(error) => {
                                    self.skip_or_fail_world(error.into())?;
                                    continue;
                                }
                            };
                        let resolver = self
                            .blob_cache
                            .as_mut()
                            .expect("enabled path owns a resolver");
                        resolver.reset_pending_for_fast_transfer_candidate()?;
                        resolver.accept_level_chunk_bytes(event, payload, packet_bytes)?;
                        continue;
                    }
                }
                let packet = match self.stream.decode_raw_packet(raw) {
                    Ok(packet) => packet,
                    Err(error) => return Err(self.fail_session(error)),
                };
                reset_blob_cache_for_decoded_candidate(
                    self.blob_cache
                        .as_mut()
                        .expect("enabled path owns a resolver"),
                    &packet,
                )?;
                if let McpePacketData::ClientCacheMissResponsePacket(response) = packet.data {
                    if let Err(error) = self
                        .blob_cache
                        .as_mut()
                        .expect("enabled path owns a resolver")
                        .accept_miss_response(response)
                    {
                        return Err(error.into());
                    }
                    continue;
                }

                if is_cached_world_packet(&packet) {
                    let mut status = match self
                        .blob_cache
                        .as_mut()
                        .expect("enabled path owns a resolver")
                        .accept_cached_packet_with_size(packet, packet_bytes)
                    {
                        Ok(status) => status,
                        Err(error) => return Err(error.into()),
                    };
                    let recovery = status.take_recovery();
                    let admission = status.take_admission();
                    let mut events = VecDeque::with_capacity(2);
                    if let Some(admission) = admission {
                        events.push_back(WorldEvent::SubChunkReplyAdmission(admission));
                    }
                    if let Some(recovery) = recovery {
                        events.push_back(WorldEvent::ChunkResync(recovery));
                    }
                    self.pending_blob_cache_delivery = Some(PendingBlobCacheDelivery {
                        status_packets: status.into_packets().into_iter().map(Into::into).collect(),
                        status_send_in_flight: false,
                        events,
                    });
                    continue;
                }

                let event = match into_world_event(packet, current_dimension) {
                    Ok(event) => event,
                    Err(error) => {
                        self.skip_or_fail_world(error.into())?;
                        continue;
                    }
                };
                if let Some(event) = event
                    && let Err(error) = self
                        .blob_cache
                        .as_mut()
                        .expect("enabled path owns a resolver")
                        .accept_world_event(event, packet_bytes)
                {
                    return Err(error.into());
                }
                continue;
            }

            let decoded = decode_world_raw_with(raw, current_dimension, |raw| {
                self.stream.decode_raw_packet(raw)
            });
            match decoded {
                Ok(Some(event)) => {
                    if let Err(error) = self
                        .blob_cache
                        .as_mut()
                        .expect("enabled path owns a resolver")
                        .accept_world_event(event, packet_bytes)
                    {
                        return Err(error.into());
                    }
                }
                Ok(None) => {}
                Err(ProtocolError::Session(error)) => return Err(self.fail_session(error)),
                Err(error) => self.skip_or_fail_world(error)?,
            }
        }
    }

    fn fail_session(&mut self, error: JolyneError) -> ProtocolError {
        if is_decode_error(&error) {
            self.decode_errors = self.decode_errors.saturating_add(1);
        }
        self.reset_blob_cache_pending();
        ProtocolError::Session(error)
    }
}

/// Counts and skips a semantic world error without changing unrelated session state.
fn skip_semantic_world_error(
    error: ProtocolError,
    world_skips: &mut u64,
) -> Result<(), ProtocolError> {
    if matches!(
        error,
        ProtocolError::World(ref world) if !matches!(world, crate::WorldPacketError::Wire(_))
    ) {
        *world_skips = world_skips.saturating_add(1);
        Ok(())
    } else {
        Err(error)
    }
}

fn reset_blob_cache_for_decoded_candidate(
    resolver: &mut BlobCacheResolver,
    packet: &Packet,
) -> Result<bool, crate::BlobCacheError> {
    if matches!(
        &packet.data,
        McpePacketData::LevelChunkPacket(_) | McpePacketData::SubChunkPacket(_)
    ) {
        resolver.reset_pending_for_fast_transfer_candidate()
    } else {
        Ok(false)
    }
}

fn is_cached_world_packet(packet: &Packet) -> bool {
    match &packet.data {
        // 1.26.40 states cache participation with an explicit flag on both
        // packets rather than an optional hash list or an entry-type union.
        McpePacketData::LevelChunkPacket(packet) => packet.cache_enabled,
        McpePacketData::SubChunkPacket(packet) => packet.cache_enabled,
        _ => false,
    }
}

fn reset_cache_for_immediate_boundary(
    resolver: &mut BlobCacheResolver,
    packet: McpePacketName,
) -> Result<bool, crate::BlobCacheError> {
    match packet {
        McpePacketName::TransferPacket => {
            resolver.recover_pending()?;
            Ok(true)
        }
        McpePacketName::DisconnectPacket => {
            resolver.reset_pending();
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn is_decode_error(error: &JolyneError) -> bool {
    matches!(
        error,
        JolyneError::Decode(_)
            | JolyneError::PacketDecode { .. }
            | JolyneError::PacketTrailingBytes { .. }
            | JolyneError::Io(_)
            | JolyneError::Protocol(_)
    )
}

fn decode_world_raw_with(
    raw: RawPacket,
    current_dimension: i32,
    decode: impl FnOnce(RawPacket) -> Result<Packet, JolyneError>,
) -> Result<Option<WorldEvent>, ProtocolError> {
    if raw.id == McpePacketName::ItemRegistryPacket {
        // Login has already initialized this session's registry. Vanilla 1.26.50
        // ignores a repeated item registry once initialized, including an
        // empty/custom-only repeat. Decode the wire first so malformed repeats remain fatal;
        // neither the inventory ledger nor actor item store may be rebound here.
        decode(raw)?;
        return Ok(None);
    }
    if raw.id == McpePacketName::UpdateAbilitiesPacket {
        return crate::decode_abilities_update(raw.body())
            .map(WorldEvent::Abilities)
            .map(Some);
    }
    if raw.id == McpePacketName::CraftingDataPacket {
        let update = crate::decode_recipe_update(raw.body())
            .map_err(crate::world::WorldPacketError::from)?;
        return Ok(Some(WorldEvent::Inventory(crate::InventoryEvent::Recipes(
            update,
        ))));
    }
    if !matches!(
        raw.id,
        McpePacketName::TextPacket
            | McpePacketName::DeathInfoPacket
            | McpePacketName::CommandOutputPacket
            | McpePacketName::PlayStatusPacket
            | McpePacketName::SetHealthPacket
            | McpePacketName::BossEventPacket
            | McpePacketName::SetTitlePacket
            | McpePacketName::ModalFormRequestPacket
            | McpePacketName::ServerSettingsResponsePacket
            | McpePacketName::NpcDialoguePacket
            | McpePacketName::RemoveObjectivePacket
            | McpePacketName::SetDisplayObjectivePacket
            | McpePacketName::SetScorePacket
            | McpePacketName::ToastRequestPacket
            | McpePacketName::UpdateSoftEnumPacket
            | McpePacketName::AvailableCommandsPacket
            | McpePacketName::BiomeDefinitionListPacket
            | McpePacketName::AddPlayerPacket
            | McpePacketName::AddActorPacket
            | McpePacketName::RemoveActorPacket
            | McpePacketName::MoveActorAbsolutePacket
            | McpePacketName::MoveActorDeltaPacket
            | McpePacketName::SetActorDataPacket
            | McpePacketName::UpdateAttributesPacket
            | McpePacketName::ActorEventPacket
            | McpePacketName::AddItemActorPacket
            | McpePacketName::AvailableActorIdentifiersPacket
            | McpePacketName::TakeItemActorPacket
            | McpePacketName::PlayerListPacket
            | McpePacketName::PlayerSkinPacket
            | McpePacketName::MobEquipmentPacket
            | McpePacketName::MobArmorEquipmentPacket
            | McpePacketName::MobEffectPacket
            | McpePacketName::SetActorLinkPacket
            | McpePacketName::SyncActorPropertyPacket
            | McpePacketName::SetPlayerGameTypePacket
            | McpePacketName::UpdatePlayerGameTypePacket
            | McpePacketName::SetDefaultGameTypePacket
            | McpePacketName::InventoryContentPacket
            | McpePacketName::CreativeContentPacket
            | McpePacketName::InventorySlotPacket
            | McpePacketName::InventoryTransactionPacket
            | McpePacketName::PlayerHotbarPacket
            | McpePacketName::ItemStackResponsePacket
            | McpePacketName::ContainerOpenPacket
            | McpePacketName::ContainerClosePacket
            | McpePacketName::ContainerSetDataPacket
            | McpePacketName::PlayerEnchantOptionsPacket
            | McpePacketName::AnimatePacket
            | McpePacketName::AnimateEntityPacket
            | McpePacketName::LevelChunkPacket
            | McpePacketName::SubChunkPacket
            | McpePacketName::DimensionDataPacket
            | McpePacketName::UpdateBlockPacket
            | McpePacketName::UpdateBlockSyncedPacket
            | McpePacketName::UpdateSubChunkBlocksPacket
            | McpePacketName::BlockActorDataPacket
            | McpePacketName::BlockEventPacket
            | McpePacketName::ClientboundMapItemDataPacket
            | McpePacketName::OpenSignPacket
            | McpePacketName::ChunkRadiusUpdatedPacket
            | McpePacketName::NetworkChunkPublisherUpdatePacket
            | McpePacketName::ChangeDimensionPacket
            | McpePacketName::ShowCreditsPacket
            | McpePacketName::PlayerActionPacket
            | McpePacketName::RespawnPacket
            | McpePacketName::MovePlayerPacket
            | McpePacketName::CorrectPlayerMovePredictionPacket
            | McpePacketName::SetActorMotionPacket
            | McpePacketName::MovementEffectPacket
            | McpePacketName::NetworkStackLatencyPacket
            | McpePacketName::SetTimePacket
            | McpePacketName::SyncWorldClocksPacket
            | McpePacketName::GameRulesChangedPacket
            | McpePacketName::LevelEventPacket
            | McpePacketName::LevelEventGenericPacket
            | McpePacketName::SpawnParticleEffectPacket
            | McpePacketName::PlaySoundPacket
            | McpePacketName::StopSoundPacket
            | McpePacketName::LevelSoundEventPacket
            | McpePacketName::CameraPacket
            | McpePacketName::CameraShakePacket
            | McpePacketName::CameraInstructionPacket
            | McpePacketName::CameraPresetsPacket
            | McpePacketName::CameraSplinePacket
            | McpePacketName::CameraAimAssistPacket
            | McpePacketName::CameraAimAssistPresetsPacket
            | McpePacketName::CameraAimAssistActorPriorityPacket
            | McpePacketName::ScriptMessagePacket
            | McpePacketName::PrimitiveShapesPacket
    ) {
        return Ok(None);
    }
    if raw.id == McpePacketName::MoveActorAbsolutePacket {
        return Ok(Some(WorldEvent::Actor(
            crate::actor::normalize_move_entity_body(raw.body(), current_dimension)
                .map_err(crate::world::WorldPacketError::from)?,
        )));
    }
    crate::inventory::validate_raw_inventory_packet(&raw)
        .map_err(crate::world::WorldPacketError::from)?;
    crate::codec::validate_raw_ui_frame(raw.inner_frame()).map_err(demote_ui_semantic_rejection)?;
    if matches!(
        raw.id,
        McpePacketName::PlaySoundPacket
            | McpePacketName::StopSoundPacket
            | McpePacketName::LevelSoundEventPacket
    ) {
        let borrowed = raw.clone().decode_borrowed()?;
        crate::audio::validate_borrowed_audio_packet(&borrowed.data)?;
    }
    if raw.id == McpePacketName::MobEquipmentPacket
        && let Some(equipment) = raw_equipment::decode_empty_mob_equipment(&raw)?
    {
        return Ok(Some(WorldEvent::Equipment(equipment)));
    }
    if let Some(packet) = level_event_generic(&raw) {
        return Ok(into_world_event(packet, current_dimension)?);
    }
    let packet = decode(raw)?;
    Ok(into_world_event(packet, current_dimension)?)
}

/// LevelEventGeneric's event data is loose NBT tags filling the rest of the packet (as
/// gophertunnel documents); the generated decoder reads one rooted value and rejects the rest.
fn level_event_generic(raw: &RawPacket) -> Option<Packet> {
    use valentine::bedrock::codec::{BedrockCodec, Nbt, ZigZag32};
    use valentine::bedrock::version::v1_26_51::LevelEventGenericPacket;
    if raw.id != McpePacketName::LevelEventGenericPacket {
        return None;
    }
    let mut body = raw.body().clone();
    let event_id = ZigZag32::decode(&mut body, ()).ok()?.0;
    let data = McpePacketData::LevelEventGenericPacket(LevelEventGenericPacket {
        event_id,
        __ctd__: Nbt(body),
    });
    Some(Packet::new(raw.header, data))
}

/// Reclassifies the raw UI pre-validator's semantic rejections as skippable
/// world packets so a well-formed-but-odd UI packet (unknown text/score/soft-enum
/// discriminant, over-budget text/score/autocomplete counts) is skipped and
/// counted rather than tearing down the session. Genuine wire failures the same
/// validator can raise -- truncated varints, negative lengths, trailing bytes --
/// stay fatal, matching the sibling inventory pre-validator above.
fn demote_ui_semantic_rejection(error: ProtocolError) -> ProtocolError {
    match error {
        ProtocolError::Ui(ui) => ProtocolError::World(crate::world::WorldPacketError::Ui(ui)),
        other => other,
    }
}

#[cfg(test)]
mod block_event_tests;
#[cfg(test)]
mod dimension_ingress_tests;
#[cfg(test)]
mod experience_ingress_tests;
#[cfg(test)]
mod generic_event_tests;
#[cfg(test)]
mod motion_tests;
#[cfg(test)]
mod packet_trace_tests;
#[cfg(test)]
mod raw_inventory_provenance_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod wire_provenance_tests;

#[cfg(test)]
mod recipe_ingress_tests;

#[cfg(test)]
mod ability_ingress_tests;

#[cfg(test)]
mod game_mode_ingress_tests;

#[cfg(test)]
mod inventory_transaction_ingress_tests;

#[cfg(test)]
mod credits_ingress_tests;

#[cfg(test)]
mod block_sync_tests;

#[cfg(test)]
mod actor_identifier_ingress_tests;

#[cfg(test)]
mod primitive_shapes_ingress_tests;
