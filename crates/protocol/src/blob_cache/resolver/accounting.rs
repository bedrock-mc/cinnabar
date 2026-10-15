use super::*;

impl BlobCacheResolver {
    pub(crate) fn retained_cached_bytes(&self) -> Result<usize, BlobCacheError> {
        self.debug_assert_running_totals();
        self.pending
            .capacity()
            .checked_mul(size_of::<(u64, PendingTransaction)>())
            .and_then(|bytes| {
                self.ready
                    .len()
                    .checked_mul(size_of::<(u64, ReadyTransaction)>())
                    .and_then(|ready| bytes.checked_add(ready))
            })
            .and_then(|bytes| {
                self.pending_order
                    .len()
                    .checked_mul(size_of::<u64>())
                    .and_then(|order| bytes.checked_add(order))
            })
            .and_then(|bytes| {
                self.resolved_pending
                    .len()
                    .checked_mul(size_of::<u64>())
                    .and_then(|resolved| bytes.checked_add(resolved))
            })
            .and_then(|bytes| {
                self.pending_by_hash
                    .capacity()
                    .checked_mul(size_of::<(u64, HashSet<u64>)>())
                    .and_then(|index| bytes.checked_add(index))
            })
            .and_then(|bytes| {
                self.pending_hash_set_capacity
                    .checked_mul(size_of::<u64>())
                    .and_then(|index| bytes.checked_add(index))
            })
            .and_then(|bytes| bytes.checked_add(self.pending_accounted_bytes))
            .and_then(|bytes| bytes.checked_add(self.ready_accounted_bytes))
            .ok_or(BlobCacheError::ByteCountOverflow)
    }

    /// Checks, in debug builds only, that the running totals equal a full walk of their
    /// collections; release builds trust the totals maintained at each mutation.
    pub(super) fn debug_assert_running_totals(&self) {
        debug_assert_eq!(
            self.pending_hash_set_capacity,
            self.pending_by_hash
                .values()
                .map(HashSet::capacity)
                .sum::<usize>()
        );
        debug_assert_eq!(
            self.pending_accounted_bytes,
            self.pending
                .values()
                .map(|transaction| transaction.accounted_bytes)
                .sum::<usize>()
        );
        debug_assert_eq!(
            self.ready_accounted_bytes,
            self.ready
                .values()
                .map(|transaction| transaction.accounted_bytes)
                .sum::<usize>()
        );
        debug_assert_eq!(
            self.retained_column_slots,
            self.pending
                .values()
                .map(|transaction| transaction.columns.len())
                .chain(
                    self.ready
                        .values()
                        .map(|transaction| transaction.columns.len())
                )
                .sum::<usize>()
        );
    }

    pub(crate) fn retained_ordinary_bytes(&self) -> Result<usize, BlobCacheError> {
        self.immediate_ready
            .len()
            .checked_mul(size_of::<(u64, ImmediateReady)>())
            .and_then(|bytes| {
                self.immediate_ready
                    .values()
                    .try_fold(bytes, |total, ready| {
                        total.checked_add(ready.accounted_bytes)
                    })
            })
            .ok_or(BlobCacheError::ByteCountOverflow)
    }

    pub(super) fn retained_reconstructed_bytes(&self) -> Result<usize, BlobCacheError> {
        self.debug_assert_running_totals();
        Ok(self.ready_accounted_bytes)
    }

    pub(super) fn refresh_pending_accounting(&mut self) -> Result<(), BlobCacheError> {
        if self.pending.is_empty() {
            self.pending = HashMap::new();
            self.pending_order = BTreeSet::new();
            self.resolved_pending = BTreeSet::new();
        }
        if self.pending_by_hash.is_empty() {
            self.pending_by_hash = HashMap::new();
        }
        self.stats.pending_bytes = self.retained_cached_bytes()?;
        self.stats.pending_transactions = self.pending.len();
        self.stats.retained_cached_transactions =
            self.pending.len().saturating_add(self.ready.len());
        self.stats.ordinary_ready_events = self.immediate_ready.len();
        self.stats.ordinary_ready_bytes = self.retained_ordinary_bytes()?;
        self.stats.recovery_ready_events = self.recovery_ready.len();
        self.stats.recovery_ready_bytes =
            self.recovery_ready
                .values()
                .try_fold(0_usize, |bytes, recovery| {
                    let ys_bytes = recovery
                        .requested_sub_chunk_ys
                        .as_ref()
                        .map_or(0, |ys| ys.len().saturating_mul(size_of::<i32>()));
                    bytes
                        .checked_add(size_of::<RecoveryReady>())
                        .and_then(|bytes| bytes.checked_add(ys_bytes))
                        .ok_or(BlobCacheError::ByteCountOverflow)
                })?;
        Ok(())
    }
}

/// Counts the decoded containers and inline bodies retained by a cached packet before resolution.
pub(super) fn cached_packet_retained_bytes(packet: &Packet) -> Result<usize, BlobCacheError> {
    let bytes = match &*packet.data {
        packets::Packet::LevelChunk(packet) => packet
            .cache_metadata
            .capacity()
            .checked_mul(size_of::<u64>())
            .and_then(|hash_bytes| {
                size_of::<LevelChunk>()
                    .checked_add(packet.serialized_chunk_data.len())
                    .and_then(|bytes| bytes.checked_add(hash_bytes))
            }),
        packets::Packet::SubChunk(packet) => packet
            .sub_chunk_data
            .capacity()
            .checked_mul(size_of::<SubChunkData>())
            .and_then(|bytes| bytes.checked_add(size_of::<SubChunk>()))
            .and_then(|bytes| {
                packet
                    .sub_chunk_data
                    .iter()
                    .try_fold(bytes, |total, entry| {
                        total.checked_add(
                            entry
                                .serialized_sub_chunk
                                .as_ref()
                                .map_or(0, bytes::Bytes::len),
                        )
                    })
            }),
        _ => return Err(BlobCacheError::NotCachedPacket),
    };
    bytes.ok_or(BlobCacheError::ByteCountOverflow)
}

impl BlobCacheResolver {
    /// Allows cancellation-retained prefetch only within the existing aggregate cached-byte lane.
    pub(crate) fn can_prefetch(&self, packet: &Packet, raw_bytes: usize) -> bool {
        if self.retained_cached_transaction_count() >= MAX_CLIENT_BLOB_PENDING_TRANSACTIONS {
            return false;
        }
        // The constructor reserves metadata before the first accounting refresh.
        let retained = if self.stats.pending_bytes == 0 {
            self.retained_cached_bytes().unwrap_or(usize::MAX)
        } else {
            self.stats.pending_bytes
        };
        cached_packet_retained_bytes(packet)
            .ok()
            .and_then(|bytes| {
                bytes
                    .max(raw_bytes)
                    .checked_add(size_of::<(Packet, usize)>())
            })
            .and_then(|bytes| bytes.checked_add(size_of::<packets::Packet>()))
            .and_then(|bytes| retained.checked_add(bytes))
            .is_some_and(|bytes| bytes <= MAX_CLIENT_BLOB_PENDING_BYTES)
    }
}
