use std::collections::VecDeque;

use bedrock_protocol_1_26_51::packets::{self, PacketId};
use bedrock_protocol_1_26_51::wire;
use bytes::Bytes;

use crate::blob_cache::ResolverReady;
use crate::packet::GameHeader;
use crate::packet_tap::PacketOutcome;
use crate::raw_packet::RawPacket;
use crate::session_transport::{Inbound, SessionTransport};
use crate::{
    BlobCacheResolver, BlobCacheStats, ClientBlobCache, LevelChunkEvent, Packet, ProtocolError,
    ServerDisconnectEvent, ServerTransferEvent, SessionError, WorldEvent, into_world_event,
};

mod blob_prefetch;
mod boundary;
mod cache_status;
#[cfg(test)]
mod cache_status_ingress_tests;
mod client_data;
mod game_data;
mod latency_probe;
mod outbound;
mod pack_handoff;
mod packet_trace;
mod raw_equipment;
pub(crate) mod session_join;
mod startup;
#[cfg(not(target_arch = "wasm32"))]
pub use session_join::NativeJoinPreparation;
pub use session_join::{JoinPreparation, SessionIo, SessionLogin};
mod unused_packets;
mod web_handoff;
use boundary::boundary_wakeup;
pub use web_handoff::WebSessionReader;

/// Bounds the drain that surfaces the core's Transfer or Disconnect behind a failed write.
pub const WRITE_FAILURE_DRAIN: std::time::Duration = std::time::Duration::from_secs(5);
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
pub struct PlaySession<R = bridge::FramedReader> {
    transport: SessionTransport<R>,
    pending_initialization: Option<u64>, // the runtime ID until loading completes
    terrain_before_spawn: bool,
    decode_errors: u64,
    world_skips: u64,
    unused_packets: u64,
    transfer_skips: u64,
    blob_cache: Option<BlobCacheResolver>,
    packet_id_trace: PacketIdTraceState,
    pending_blob_cache_events: VecDeque<WorldEvent>,
    cache_status: cache_status::SharedCacheStatus,
    pending_blob_prefetch: Option<(Packet, usize)>,
    server_disconnect: Option<ServerDisconnectEvent>,
    server_transfer: Option<ServerTransferEvent>,
    ingress_position: (u64, u32), // core frame and slot of the packet behind the last ingress
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

impl<R: bridge::FrameRead> PlaySession<R> {
    /// Buffers raw core frames while the world consumer is full. Decoding resumes in wire
    /// order when the caller receives again. Reading stops at the shared byte bound or the
    /// session's terminal message; cancelling this wait retains every complete frame read.
    pub async fn read_ahead(&mut self) {
        self.transport
            .read_ahead(crate::session_transport::MAX_READ_AHEAD_BYTES)
            .await;
    }

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
        transport: SessionTransport<R>,
        cache: Option<ClientBlobCache>,
        runtime_id: u64,
        terrain_before_spawn: bool,
    ) -> Self {
        Self {
            transport,
            pending_initialization: Some(runtime_id),
            terrain_before_spawn,
            decode_errors: 0,
            world_skips: 0,
            unused_packets: 0,
            transfer_skips: 0,
            blob_cache: cache.map(BlobCacheResolver::new),
            packet_id_trace: PacketIdTraceState::default(),
            pending_blob_cache_events: VecDeque::new(),
            cache_status: Default::default(),
            pending_blob_prefetch: None,
            server_disconnect: None,
            server_transfer: None,
            ingress_position: (0, 0),
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

    /// [`Self::skip_or_fail_world`] for the packet the transport delivered last, reporting
    /// whether it was skipped or ended the session.
    fn skip_or_fail_current(&mut self, error: ProtocolError) -> Result<(), ProtocolError> {
        let result = self.skip_or_fail_world(error);
        self.transport.reject_current(if result.is_ok() {
            PacketOutcome::Skipped
        } else {
            PacketOutcome::Failed
        });
        result
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
            if unused_packets::is_unused(raw.id) {
                self.discard_unused_packet(raw)?;
                continue;
            }
            let packet = raw.decode().map_err(|error| self.fail_session(error))?;
            if matches!(
                &*packet.data,
                packets::Packet::Transfer(_) | packets::Packet::Disconnect(_)
            ) {
                self.reset_blob_cache_pending();
            }
            self.transport.accept_current();
            return Ok(packet);
        }
    }

    /// Receives the next world-streaming event without decoding unrelated play packets.
    /// Latency probes are returned in order; the caller replies once earlier work is applied.
    pub async fn recv_world_event(
        &mut self,
        current_dimension: i32,
    ) -> Result<WorldEvent, ProtocolError> {
        Ok(self
            .recv_world_ingress(current_dimension)
            .await?
            .into_world_event())
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

    /// Settles accepted packets before the caller can pause world admission.
    async fn recv_world_ingress(
        &mut self,
        current_dimension: i32,
    ) -> Result<WorldIngress, ProtocolError> {
        let result = self.recv_world_ingress_inner(current_dimension).await;
        if result.is_ok() {
            self.transport.accept_current();
            if self.blob_cache.is_none() {
                self.ingress_position = self.delivered_position();
            }
        }
        result
    }

    /// Routes raw world packets through the enabled cache and decoding lanes.
    async fn recv_world_ingress_inner(
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
            if unused_packets::is_unused(raw.id) {
                self.discard_unused_packet(raw)?;
                continue;
            }
            if raw.id == PacketId::LevelChunk {
                let raw = raw.into_retention_bounded();
                self.transport.compact_current(&raw);
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
                        self.skip_or_fail_current(error.into())?;
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
            let decoded = decode_world_raw_with(raw, current_dimension, RawPacket::decode_parts);
            match decoded {
                Ok(Some(event)) => return Ok(WorldIngress::Event(event)),
                Ok(None) => {}
                Err(ProtocolError::Session(error)) => {
                    if error.0.is_decode() {
                        self.decode_errors = self.decode_errors.saturating_add(1);
                    }
                    if error.0.is_packet_failure() {
                        self.transport.reject_current(PacketOutcome::Failed);
                    }
                    return Err(ProtocolError::Session(error));
                }
                Err(error) => self.skip_or_fail_current(error)?,
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

    /// The latency-trace connection, core batch frame and in-frame index of the packet received
    /// last; see [`bridge::latency_trace`].
    #[must_use]
    pub fn latency_trace_position(&self) -> (u64, u64, u32) {
        let (conn, _, _) = self.transport.trace_position();
        (conn, self.ingress_position.0, self.ingress_position.1)
    }

    /// The core frame and slot of the packet the transport delivered last.
    fn delivered_position(&self) -> (u64, u32) {
        let (_, frame, index) = self.transport.trace_position();
        (frame, index)
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
        self.pending_blob_cache_events.clear();
        self.pending_blob_prefetch = None;
        self.cache_status.lock().expect("cache status lock").reset();
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
            if let Some(recovery) = self
                .blob_cache
                .as_mut()
                .expect("enabled path owns a resolver")
                .pop_recovery_ready()
            {
                // Recovery is the client's own resync request; no core packet produced it.
                self.ingress_position = (0, 0);
                return Ok(WorldIngress::Event(WorldEvent::ChunkResync(recovery)));
            }
            if let Some(event) = self.pending_blob_cache_events.pop_front() {
                // Delivery events come from the prefetched packet, read last.
                self.ingress_position = self.delivered_position();
                return Ok(WorldIngress::Event(event));
            }
            if let Some(ready) = self
                .blob_cache
                .as_mut()
                .expect("enabled path owns a resolver")
                .pop_ready_ingress()
            {
                self.ingress_position = self
                    .blob_cache
                    .as_ref()
                    .expect("enabled path owns a resolver")
                    .popped_trace_origin();
                let event = match ready {
                    ResolverReady::Packet(packet) => {
                        // A reconstructed LevelChunk hands its rebuilt payload on as is, as
                        // the uncached lane does, rather than through an owned copy.
                        let normalized = match *packet.data {
                            packets::Packet::LevelChunk(level_chunk) => {
                                match crate::world::normalize_level_chunk_payload(level_chunk) {
                                    Ok((event, payload)) => {
                                        return Ok(WorldIngress::LevelChunk(event, payload));
                                    }
                                    Err(error) => {
                                        self.skip_or_fail_world(error.into())?;
                                        continue;
                                    }
                                }
                            }
                            data => crate::world::dispatch(data, packet.header, current_dimension),
                        };
                        match normalized {
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

            if self.pending_blob_prefetch.is_some() {
                let status = self.cache_status.clone();
                blob_prefetch::prefetch_while_reading(
                    std::future::poll_fn(|cx| {
                        status.lock().expect("cache status lock").poll_capacity(cx)
                    }),
                    |cx| {
                        self.transport
                            .poll_read_ahead_one(crate::session_transport::MAX_READ_AHEAD_BYTES, cx)
                    },
                )
                .await;
                self.finish_blob_prefetch().await?;
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
            if bridge::latency_trace::active().is_some() {
                let origin = self.delivered_position();
                self.blob_cache
                    .as_mut()
                    .expect("enabled path owns a resolver")
                    .trace_origin(origin);
            }
            self.packet_id_trace.observe(raw.id);
            if unused_packets::is_unused(raw.id) {
                self.discard_unused_packet(raw)?;
                continue;
            }
            let packet_bytes = raw.inner_frame().len();
            let packet_name = raw.id;
            let raw = detach_cached_chunk(raw);
            self.transport.compact_current(&raw);

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
                            self.skip_or_fail_current(error.into())?;
                            continue;
                        }
                    };
                    let resolver = self
                        .blob_cache
                        .as_mut()
                        .expect("enabled path owns a resolver");
                    if resolver.reset_pending_for_fast_transfer_candidate()? {
                        self.cache_status.lock().expect("cache status lock").reset();
                    }
                    resolver.accept_level_chunk_bytes(event, payload, packet_bytes)?;
                    continue;
                }
                if reset_blob_cache_for_decoded_candidate(
                    self.blob_cache
                        .as_mut()
                        .expect("enabled path owns a resolver"),
                    &packet,
                )? {
                    self.cache_status.lock().expect("cache status lock").reset();
                }
                if let packets::Packet::ClientCacheMissResponse(response) = *packet.data {
                    match self
                        .blob_cache
                        .as_mut()
                        .expect("enabled path owns a resolver")
                        .accept_miss_response(response)
                    {
                        Ok(true) => {}
                        Ok(false) => self.transport.reject_current(PacketOutcome::Skipped),
                        Err(error) => {
                            self.transport.reject_current(PacketOutcome::Failed);
                            return Err(error.into());
                        }
                    }
                    continue;
                }

                if is_cached_world_packet(&packet) {
                    self.pending_blob_prefetch = Some((packet, packet_bytes));
                    continue;
                }

                let event = match into_world_event(packet, current_dimension) {
                    Ok(event) => event,
                    Err(error) => {
                        self.skip_or_fail_current(error.into())?;
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

            let decoded = decode_world_raw_with(raw, current_dimension, RawPacket::decode_parts);
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
                Err(error) => self.skip_or_fail_current(error)?,
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
                    self.skip_or_fail_current(semantic).map(|()| None)
                }
            },
        }
    }

    fn fail_session(&mut self, error: SessionError) -> ProtocolError {
        if error.0.is_decode() {
            self.decode_errors = self.decode_errors.saturating_add(1);
        }
        if error.0.is_packet_failure() {
            self.transport.reject_current(PacketOutcome::Failed);
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

/// Normalizes one raw world-lane packet; `decode` supplies the generic payload decode for the
/// packets no bounded raw decoder owns.
fn decode_world_raw_with(
    raw: RawPacket,
    current_dimension: i32,
    decode: impl FnOnce(RawPacket) -> Result<(GameHeader, packets::Packet), SessionError>,
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
    if raw.id == PacketId::UnlockedRecipes {
        let update = crate::decode_recipe_unlocks(raw.body())
            .map_err(crate::world::WorldPacketError::from)?;
        return Ok(Some(WorldEvent::Inventory(
            crate::InventoryEvent::RecipeUnlocks(update),
        )));
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
    if let Some((header, data)) = level_event_generic(&raw) {
        return Ok(crate::world::dispatch(data, header, current_dimension)?);
    }
    let (header, data) = decode(raw).map_err(crate::codec::world_decode_error)?;
    crate::codec::validate_decoded(&data).map_err(demote_ui_semantic_rejection)?;
    Ok(crate::world::dispatch(data, header, current_dimension)?)
}

/// Decodes a raw test packet as the play stream does.
#[cfg(test)]
fn decode_test_raw(raw: RawPacket) -> Result<(GameHeader, packets::Packet), SessionError> {
    raw.decode_parts()
}

/// LevelEventGeneric's event data is loose NBT tags filling the rest of the packet (as
/// gophertunnel documents); the generated decoder reads one rooted value and rejects the rest.
fn level_event_generic(raw: &RawPacket) -> Option<(GameHeader, packets::Packet)> {
    if raw.id != PacketId::LevelEventGeneric {
        return None;
    }
    let mut reader = wire::Reader::new(raw.body());
    let event_id = reader.read_zigzag_i32().ok()?;
    let data = packets::LevelEventGeneric {
        event_id: wire::ZigZag32(event_id),
        ctd: wire::NetworkNbt(raw.body().slice(reader.position()..)),
    };
    Some((raw.header, data.into()))
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
#[cfg(all(test, unix))]
mod packet_tap_ingress_tests;
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
