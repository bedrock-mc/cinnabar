use super::*;

pub(super) fn chunk_recovery(packet: &Packet) -> Option<ChunkResyncEvent> {
    match &packet.data {
        McpePacketData::LevelChunkPacket(packet) => Some(ChunkResyncEvent {
            dimension: dimension_id(&packet.dimension_id),
            x: packet.chunk_position.x,
            z: packet.chunk_position.z,
            requested_sub_chunks: None,
            requested_sub_chunk_ys: None,
        }),
        // Cached SubChunk packets are responses to scheduler-owned requests. Discarding the
        // response deliberately leaves that request outstanding, so its existing deadline and
        // bounded retry path performs recovery without a duplicate full-column request.
        McpePacketData::SubChunkPacket(_) => None,
        _ => None,
    }
}

pub(super) fn unadmitted_packet_recovery(packet: &PendingPacket) -> Vec<ChunkResyncEvent> {
    match packet {
        PendingPacket::LevelChunk(packet) => vec![ChunkResyncEvent {
            dimension: dimension_id(&packet.dimension_id),
            x: packet.chunk_position.x,
            z: packet.chunk_position.z,
            requested_sub_chunks: None,
            requested_sub_chunk_ys: None,
        }],
        // No admission reached the scheduler, so its existing deadline owns this retry.
        PendingPacket::SubChunk(_) => Vec::new(),
    }
}

pub(super) fn pending_packet_recovery(packet: &PendingPacket) -> Vec<ChunkResyncEvent> {
    match packet {
        PendingPacket::LevelChunk(packet) => vec![ChunkResyncEvent {
            dimension: dimension_id(&packet.dimension_id),
            x: packet.chunk_position.x,
            z: packet.chunk_position.z,
            requested_sub_chunks: None,
            requested_sub_chunk_ys: None,
        }],

        PendingPacket::SubChunk(packet) => {
            if !packet.cache_enabled {
                return Vec::new();
            }
            let mut columns = BTreeMap::<(i32, i32), BTreeSet<i32>>::new();
            for entry in &packet.sub_chunk_data {
                let offset = &entry.sub_chunk_pos_offset;
                columns
                    .entry((
                        packet
                            .center_pos
                            .subchunk_position_x
                            .saturating_add(i32::from(offset.subchunk_offset_x)),
                        packet
                            .center_pos
                            .subchunk_position_z
                            .saturating_add(i32::from(offset.subchunk_offset_z)),
                    ))
                    .or_default()
                    .insert(
                        packet
                            .center_pos
                            .subchunk_position_y
                            .saturating_add(i32::from(offset.subchunk_offset_y)),
                    );
            }
            columns
                .into_iter()
                .map(|((x, z), ys)| ChunkResyncEvent {
                    dimension: dimension_id(&packet.dimension_type),
                    x,
                    z,
                    requested_sub_chunks: None,
                    requested_sub_chunk_ys: Some(ys.into_iter().collect()),
                })
                .collect()
        }
    }
}

fn ready_value_recovery(value: &ResolverReady) -> Vec<ChunkResyncEvent> {
    let ResolverReady::Packet(packet) = value else {
        return Vec::new();
    };
    match &packet.data {
        McpePacketData::LevelChunkPacket(packet) => vec![ChunkResyncEvent {
            dimension: dimension_id(&packet.dimension_id),
            x: packet.chunk_position.x,
            z: packet.chunk_position.z,
            requested_sub_chunks: None,
            requested_sub_chunk_ys: None,
        }],
        // Cached and uncached replies are one entry type now, so both cases
        // collapse into a single traversal.
        McpePacketData::SubChunkPacket(packet) => sub_chunk_recoveries(
            dimension_id(&packet.dimension_type),
            [
                packet.center_pos.subchunk_position_x,
                packet.center_pos.subchunk_position_y,
                packet.center_pos.subchunk_position_z,
            ],
            packet.sub_chunk_data.iter().map(|entry| {
                [
                    entry.sub_chunk_pos_offset.subchunk_offset_x,
                    entry.sub_chunk_pos_offset.subchunk_offset_y,
                    entry.sub_chunk_pos_offset.subchunk_offset_z,
                ]
            }),
        ),
        _ => Vec::new(),
    }
}

fn sub_chunk_recoveries(
    dimension: i32,
    origin: [i32; 3],
    offsets: impl Iterator<Item = [i8; 3]>,
) -> Vec<ChunkResyncEvent> {
    let mut columns = BTreeMap::<(i32, i32), BTreeSet<i32>>::new();
    for [dx, dy, dz] in offsets {
        columns
            .entry((
                origin[0].saturating_add(i32::from(dx)),
                origin[2].saturating_add(i32::from(dz)),
            ))
            .or_default()
            .insert(origin[1].saturating_add(i32::from(dy)));
    }
    columns
        .into_iter()
        .map(|((x, z), ys)| ChunkResyncEvent {
            dimension,
            x,
            z,
            requested_sub_chunks: None,
            requested_sub_chunk_ys: Some(ys.into_iter().collect()),
        })
        .collect()
}

pub(super) fn cached_sub_chunk_admission(
    packet: &PendingPacket,
) -> Option<SubChunkReplyAdmissionEvent> {
    let PendingPacket::SubChunk(packet) = packet else {
        return None;
    };
    if !packet.cache_enabled {
        return None;
    }
    let positions = packet
        .sub_chunk_data
        .iter()
        .map(|entry| {
            let offset = &entry.sub_chunk_pos_offset;
            [
                packet
                    .center_pos
                    .subchunk_position_x
                    .saturating_add(i32::from(offset.subchunk_offset_x)),
                packet
                    .center_pos
                    .subchunk_position_y
                    .saturating_add(i32::from(offset.subchunk_offset_y)),
                packet
                    .center_pos
                    .subchunk_position_z
                    .saturating_add(i32::from(offset.subchunk_offset_z)),
            ]
        })
        .collect::<Vec<_>>();
    (!positions.is_empty()).then_some(SubChunkReplyAdmissionEvent {
        dimension: dimension_id(&packet.dimension_type),
        positions,
    })
}

impl BlobCacheResolver {
    /// Abandons retained cached work while preserving exact scheduler rollback events.
    pub fn recover_pending(&mut self) -> Result<(), BlobCacheError> {
        if !self.pending.is_empty() || !self.ready.is_empty() {
            self.stats.pending_resets = self.stats.pending_resets.saturating_add(1);
            self.recover_retained_cached_transactions()?;
        }
        self.immediate_ready = BTreeMap::new();
        self.stats.ordinary_ready_events = 0;
        self.stats.ordinary_ready_bytes = 0;
        Ok(())
    }

    pub(super) fn recover_retained_cached_transactions(&mut self) -> Result<(), BlobCacheError> {
        let recoveries = self
            .pending
            .values()
            .flat_map(|transaction| pending_packet_recovery(&transaction.packet))
            .chain(
                self.ready
                    .values()
                    .flat_map(|transaction| ready_value_recovery(&transaction.value)),
            )
            .collect::<Vec<_>>();
        for transaction in self.pending.drain().map(|(_, transaction)| transaction) {
            self.cache.unpin_all(&transaction.unique_hashes);
        }
        self.pending = HashMap::new();
        self.pending_order = BTreeSet::new();
        self.pending_by_hash = HashMap::new();
        self.resolved_pending = BTreeSet::new();
        self.ready = BTreeMap::new();
        self.enqueue_recoveries(recoveries);
        self.refresh_pending_accounting()
    }

    pub fn reset_pending(&mut self) {
        if !self.pending.is_empty() || !self.ready.is_empty() {
            self.stats.pending_resets = self.stats.pending_resets.saturating_add(1);
        }
        for transaction in self.pending.drain() {
            self.cache.unpin_all(&transaction.1.unique_hashes);
        }
        self.pending = HashMap::new();
        self.pending_order = BTreeSet::new();
        self.pending_by_hash = HashMap::new();
        self.resolved_pending = BTreeSet::new();
        self.ready = BTreeMap::new();
        self.immediate_ready = BTreeMap::new();
        self.recovery_ready = BTreeMap::new();
        self.fast_transfer_reset_armed = false;
        self.next_ready_sequence = 0;
        self.stats.pending_transactions = 0;
        self.stats.pending_bytes = 0;
        self.stats.retained_cached_transactions = 0;
        self.stats.ordinary_ready_events = 0;
        self.stats.ordinary_ready_bytes = 0;
        self.stats.recovery_ready_events = 0;
        self.stats.recovery_ready_bytes = 0;
    }

    pub(super) fn recover_skipped_miss_response(
        &mut self,
        hashes: &[u64],
    ) -> Result<(), BlobCacheError> {
        self.stats.skipped_miss_responses = self.stats.skipped_miss_responses.saturating_add(1);
        let sequences = hashes
            .iter()
            .filter_map(|hash| self.pending_by_hash.get(hash))
            .flatten()
            .copied()
            .collect::<BTreeSet<_>>();
        for sequence in sequences {
            self.abandon_pending_transaction(sequence)?;
        }
        self.refresh_pending_accounting()
    }

    pub(crate) fn pop_recovery_ready(&mut self) -> Option<ChunkResyncEvent> {
        let key = self.recovery_ready.first_key_value().map(|(key, _)| *key)?;
        let recovery = self
            .recovery_ready
            .remove(&key)
            .expect("first recovery key remains present");
        self.refresh_pending_accounting()
            .expect("retained recovery accounting cannot overflow after a pop");
        Some(recovery.into_event())
    }
}
