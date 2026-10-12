use std::collections::VecDeque;

use bedrock_protocol_1_26_51::packets::{self, PacketId};
use bedrock_protocol_1_26_51::wire;
use bytes::Bytes;

use crate::blob_cache::ResolverReady;
use crate::raw_packet::RawPacket;
use crate::session_transport::{Inbound, SessionTransport};
use crate::{
    BlobCacheResolver, BlobCacheStats, ClientBlobCache, LevelChunkEvent, Packet, ProtocolError,
    ServerDisconnectEvent, ServerTransferEvent, SessionError, WorldEvent, into_world_event,
};

mod boundary;
mod client_data;
mod game_data;
mod latency_probe;
mod outbound;
mod pack_handoff;
mod packet_trace;
mod raw_equipment;
pub(crate) mod session_join;
mod startup;
use boundary::boundary_wakeup;
pub use client_data::{
    ClientCape, ClientRandomId, ClientSkin, ClientSkinGeometry, GAME_VERSION, LoginSettings,
    PROTOCOL_VERSION, login_memory_tier,
};
pub use game_data::{GameData, StartupItems, StartupSettings};
pub use latency_probe::network_stack_latency_reply;
pub use outbound::PlayOutbound;
pub use pack_handoff::{ResourcePackArchive, ResourcePackContentKey, ResourcePackHandoff};
pub use packet_trace::PacketIdTraceSnapshot;
use packet_trace::PacketIdTraceState;
#[cfg(test)]
use packet_trace::{MAX_PACKET_ID_TRACE_ENTRIES, PACKET_ID_TRACE_DURATION};

/// Entry point for joining through the core.
pub struct LoginSequence;

/// An authenticated, spawned Bedrock session.
pub struct PlaySession {
    transport: SessionTransport,
    pending_initialization: Option<u64>, // the runtime ID until loading completes
    terrain_before_spawn: bool,
    decode_errors: u64,
    world_skips: u64,
    transfer_skips: u64,
    blob_cache: Option<BlobCacheResolver>,
    packet_id_trace: PacketIdTraceState,
    pending_blob_cache_delivery: Option<PendingBlobCacheDelivery>,
    server_disconnect: Option<ServerDisconnectEvent>,
    server_transfer: Option<ServerTransferEvent>,
    resource_packs: ResourcePackHandoff,
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

impl PlaySession {
    /// Sends loading-end and initialization once the client can present the world; later
    /// calls send nothing.
    pub async fn finish_loading(&mut self) -> Result<(), ProtocolError> {
        let packets = self.take_finish_loading_packets();
        self.transport.send(&packets).await
    }

    /// Takes the one-shot loading-end and initialization packets for a caller that batches them.
    fn take_finish_loading_packets(&mut self) -> Vec<Packet> {
        self.pending_initialization
            .take()
            .map(startup::loading_finished)
            .unwrap_or_default()
    }

    fn new(
        transport: SessionTransport,
        cache: Option<ClientBlobCache>,
        resource_packs: ResourcePackHandoff,
        runtime_id: u64,
        terrain_before_spawn: bool,
    ) -> Self {
        Self {
            transport,
            pending_initialization: Some(runtime_id),
            terrain_before_spawn,
            decode_errors: 0,
            world_skips: 0,
            transfer_skips: 0,
            blob_cache: cache.map(BlobCacheResolver::new),
            packet_id_trace: PacketIdTraceState::default(),
            pending_blob_cache_delivery: None,
            server_disconnect: None,
            server_transfer: None,
            resource_packs,
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
        std::mem::take(&mut self.resource_packs)
    }

    /// Whether the server sent terrain before login completed at PlayerSpawn.
    /// Servers such as Dragonfly stream none until the client reports initialized.
    #[must_use]
    pub fn terrain_before_spawn(&self) -> bool {
        self.terrain_before_spawn
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

    /// Receives one packet, counting malformed failures. The core's terminal transfer or
    /// disconnect is retained, as world ingress retains it, and ends the session.
    pub async fn recv(&mut self) -> Result<Packet, ProtocolError> {
        loop {
            let inbound = match self.transport.recv().await {
                Ok(inbound) => inbound,
                Err(error) => return Err(self.fail_session(error)),
            };
            let raw = match inbound {
                Inbound::Packet(raw) => raw,
                Inbound::Terminal(terminal) => {
                    self.reset_blob_cache_pending();
                    if self.absorb_terminal(terminal)? {
                        return boundary_wakeup();
                    }
                    continue;
                }
            };
            let packet = raw.decode().map_err(|error| self.fail_session(error))?;
            if matches!(
                &*packet.data,
                packets::Packet::Transfer(_) | packets::Packet::Disconnect(_)
            ) {
                self.reset_blob_cache_pending();
            }
            return Ok(packet);
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
            let raw = match self.transport.recv().await {
                Ok(Inbound::Packet(raw)) => raw,
                Ok(Inbound::Terminal(terminal)) => {
                    if self.absorb_terminal(terminal)? {
                        return boundary_wakeup();
                    }
                    continue;
                }
                Err(error) => {
                    if error.0.is_decode() {
                        self.decode_errors = self.decode_errors.saturating_add(1);
                    }
                    return Err(error.into());
                }
            };
            self.packet_id_trace.observe(raw.id);
            if raw.id == PacketId::LevelChunk {
                let raw = raw.into_retention_bounded();
                let packet = match self.decode_world_candidate(raw)? {
                    Some(packet) => packet,
                    None => continue,
                };
                let packets::Packet::LevelChunk(packet) = *packet.data else {
                    unreachable!("LevelChunk packet ID decoded to another variant")
                };
                match crate::world::normalize_level_chunk_payload(packet) {
                    Ok((event, payload)) => return Ok(WorldIngress::LevelChunk(event, payload)),
                    Err(error) => {
                        self.skip_or_fail_world(error.into())?;
                        continue;
                    }
                }
            }
            if matches!(raw.id, PacketId::Transfer | PacketId::Disconnect) {
                let name = raw.id;
                if self.absorb_boundary_packet(raw, name).await? {
                    return boundary_wakeup();
                }
                continue;
            }
            let decoded = decode_world_raw_with(raw, current_dimension, RawPacket::decode);
            match decoded {
                Ok(Some(event)) => return Ok(WorldIngress::Event(event)),
                Ok(None) => {}
                Err(ProtocolError::Session(error)) => {
                    if error.0.is_decode() {
                        self.decode_errors = self.decode_errors.saturating_add(1);
                    }
                    return Err(ProtocolError::Session(error));
                }
                Err(error) => self.skip_or_fail_world(error)?,
            }
        }
    }

    /// Sends one packet as its own batch behind every send accepted before it.
    pub async fn send(&mut self, packet: Packet) -> Result<(), ProtocolError> {
        crate::codec::validate_packet(&packet)?;
        self.transport.send(&[packet]).await
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
                if let Err(error) = self.transport.drain_send().await {
                    self.reset_blob_cache_pending();
                    return Err(error);
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

            let raw = match self.transport.recv().await {
                Ok(Inbound::Packet(raw)) => raw,
                Ok(Inbound::Terminal(terminal)) => {
                    if self.absorb_terminal(terminal)? {
                        return boundary_wakeup();
                    }
                    continue;
                }
                Err(error) => return Err(self.fail_session(error)),
            };
            self.packet_id_trace.observe(raw.id);
            let packet_bytes = raw.inner_frame().len();
            let packet_name = raw.id;
            let raw = detach_cached_chunk(raw);

            if matches!(packet_name, PacketId::Transfer | PacketId::Disconnect) {
                if self.absorb_boundary_packet(raw, packet_name).await? {
                    return boundary_wakeup();
                }
                continue;
            }

            if matches!(
                packet_name,
                PacketId::LevelChunk | PacketId::SubChunk | PacketId::ClientCacheMissResponse
            ) {
                let packet = match self.decode_world_candidate(raw)? {
                    Some(packet) => packet,
                    None => continue,
                };
                if let packets::Packet::LevelChunk(view) = &*packet.data
                    && !view.cache_enabled
                {
                    let packets::Packet::LevelChunk(view) = *packet.data else {
                        unreachable!("checked above")
                    };
                    let (event, payload) = match crate::world::normalize_level_chunk_payload(view) {
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
                reset_blob_cache_for_decoded_candidate(
                    self.blob_cache
                        .as_mut()
                        .expect("enabled path owns a resolver"),
                    &packet,
                )?;
                if let packets::Packet::ClientCacheMissResponse(response) = *packet.data {
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
                        status_packets: status
                            .into_packets()
                            .into_iter()
                            .map(Packet::new)
                            .collect(),
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

            let decoded = decode_world_raw_with(raw, current_dimension, RawPacket::decode);
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

    /// Decodes a chunk-lane or boundary packet: a body breaking only a schema or UI bound, or
    /// naming an unknown selector, is logged, counted and skipped (`None`); malformed wire ends
    /// the session.
    fn decode_world_candidate(&mut self, raw: RawPacket) -> Result<Option<Packet>, ProtocolError> {
        match raw.decode() {
            Ok(packet) => Ok(Some(packet)),
            Err(error) => match crate::codec::world_decode_error(error) {
                ProtocolError::Session(error) => Err(self.fail_session(error)),
                semantic => {
                    tracing::debug!(%semantic, "skipping a world packet the codec rejected");
                    self.skip_or_fail_world(semantic).map(|()| None)
                }
            },
        }
    }

    fn fail_session(&mut self, error: SessionError) -> ProtocolError {
        if error.0.is_decode() {
            self.decode_errors = self.decode_errors.saturating_add(1);
        }
        self.reset_blob_cache_pending();
        ProtocolError::from(error)
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
        &*packet.data,
        packets::Packet::LevelChunk(_) | packets::Packet::SubChunk(_)
    ) {
        resolver.reset_pending_for_fast_transfer_candidate()
    } else {
        Ok(false)
    }
}

/// Copies a LevelChunk or SubChunk entry out of its batch, so the payload slices the blob cache
/// retains, and charges by length, keep only that entry alive.
fn detach_cached_chunk(raw: RawPacket) -> RawPacket {
    if matches!(raw.id, PacketId::LevelChunk | PacketId::SubChunk) {
        raw.into_retention_bounded()
    } else {
        raw
    }
}

fn is_cached_world_packet(packet: &Packet) -> bool {
    match &*packet.data {
        // 1.26.40 states cache participation with an explicit flag on both
        // packets rather than an optional hash list or an entry-type union.
        packets::Packet::LevelChunk(packet) => packet.cache_enabled,
        packets::Packet::SubChunk(packet) => packet.cache_enabled,
        _ => false,
    }
}

fn reset_cache_for_immediate_boundary(
    resolver: &mut BlobCacheResolver,
    packet: PacketId,
) -> Result<bool, crate::BlobCacheError> {
    match packet {
        PacketId::Transfer => {
            resolver.recover_pending()?;
            Ok(true)
        }
        PacketId::Disconnect => {
            resolver.reset_pending();
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn decode_world_raw_with(
    raw: RawPacket,
    current_dimension: i32,
    decode: impl FnOnce(RawPacket) -> Result<Packet, SessionError>,
) -> Result<Option<WorldEvent>, ProtocolError> {
    if raw.id == PacketId::ItemRegistry {
        // Login has already initialized this session's registry. Vanilla 1.26.50
        // ignores a repeated item registry once initialized, including an
        // empty/custom-only repeat. Decode the wire first so malformed repeats remain fatal;
        // neither the inventory ledger nor actor item store may be rebound here.
        decode(raw).map_err(crate::codec::world_decode_error)?;
        return Ok(None);
    }
    if raw.id == PacketId::UpdateAbilities {
        return crate::decode_abilities_update(raw.body())
            .map(WorldEvent::Abilities)
            .map(Some);
    }
    if raw.id == PacketId::CraftingData {
        let update = crate::decode_recipe_update(raw.body())
            .map_err(crate::world::WorldPacketError::from)?;
        return Ok(Some(WorldEvent::Inventory(crate::InventoryEvent::Recipes(
            update,
        ))));
    }
    // Play decodes only the packets world normalization consumes; the rest stay undecoded.
    if !crate::world::routes_world_packet(raw.id) {
        return Ok(None);
    }
    if raw.id == PacketId::MoveActorAbsolute {
        return Ok(Some(WorldEvent::Actor(
            crate::actor::normalize_move_entity_body(raw.body(), current_dimension)
                .map_err(crate::world::WorldPacketError::from)?,
        )));
    }
    crate::inventory::validate_raw_inventory_packet(&raw)
        .map_err(crate::world::WorldPacketError::from)?;
    if raw.id == PacketId::MobEquipment
        && let Some(equipment) = raw_equipment::decode_empty_mob_equipment(&raw)?
    {
        return Ok(Some(WorldEvent::Equipment(equipment)));
    }
    if let Some(packet) = level_event_generic(&raw) {
        return Ok(into_world_event(packet, current_dimension)?);
    }
    let packet = decode(raw).map_err(crate::codec::world_decode_error)?;
    crate::codec::validate_decoded(&packet.data).map_err(demote_ui_semantic_rejection)?;
    Ok(into_world_event(packet, current_dimension)?)
}

/// Decodes a raw test packet as the play stream does.
#[cfg(test)]
fn decode_test_raw(raw: RawPacket) -> Result<Packet, SessionError> {
    raw.decode()
}

/// LevelEventGeneric's event data is loose NBT tags filling the rest of the packet (as
/// gophertunnel documents); the generated decoder reads one rooted value and rejects the rest.
fn level_event_generic(raw: &RawPacket) -> Option<Packet> {
    if raw.id != PacketId::LevelEventGeneric {
        return None;
    }
    let mut reader = wire::Reader::new(raw.body());
    let event_id = reader.read_zigzag_i32().ok()?;
    let data = packets::LevelEventGeneric {
        event_id: wire::ZigZag32(event_id),
        ctd: wire::NetworkNbt(raw.body().slice(reader.position()..)),
    };
    Some(Packet::from_parts(raw.header, data.into()))
}

/// Reclassifies the decoded-packet UI and audio checks' semantic rejections as skippable world
/// packets, so a well-formed but odd UI packet is skipped and counted rather than ending the
/// session.
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
