use super::*;

impl BlobCacheResolver {
    pub(crate) fn retained_cached_bytes(&self) -> Result<usize, BlobCacheError> {
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
                self.pending_by_hash.values().try_fold(bytes, |total, ids| {
                    ids.capacity()
                        .checked_mul(size_of::<u64>())
                        .and_then(|index| total.checked_add(index))
                })
            })
            .and_then(|bytes| {
                self.pending.iter().try_fold(bytes, |total, transaction| {
                    total.checked_add(transaction.1.accounted_bytes)
                })
            })
            .and_then(|bytes| {
                self.ready.iter().try_fold(bytes, |total, transaction| {
                    total.checked_add(transaction.1.accounted_bytes)
                })
            })
            .ok_or(BlobCacheError::ByteCountOverflow)
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
        self.stats.recovery_ready_bytes = self
            .recovery_ready
            .len()
            .checked_mul(size_of::<ChunkResyncEvent>())
            .ok_or(BlobCacheError::ByteCountOverflow)?;
        Ok(())
    }
}
