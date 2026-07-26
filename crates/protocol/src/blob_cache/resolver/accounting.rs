use super::*;

impl BlobCacheResolver {
    pub(crate) fn retained_pending_bytes(&self) -> Result<usize, BlobCacheError> {
        self.retained_cached_bytes()?
            .checked_add(self.retained_immediate_bytes()?)
            .ok_or(BlobCacheError::ByteCountOverflow)
    }

    pub(super) fn retained_cached_bytes(&self) -> Result<usize, BlobCacheError> {
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
                self.column_barriers
                    .capacity()
                    .checked_mul(size_of::<(ColumnKey, BTreeSet<u64>)>())
                    .and_then(|columns| bytes.checked_add(columns))
            })
            .and_then(|bytes| {
                self.column_barriers.values().try_fold(bytes, |total, ids| {
                    ids.len()
                        .checked_mul(size_of::<u64>())
                        .and_then(|index| total.checked_add(index))
                })
            })
            .and_then(|bytes| {
                self.authorized_misses
                    .capacity()
                    .checked_mul(size_of::<(u64, usize)>())
                    .and_then(|authorized| bytes.checked_add(authorized))
            })
            .and_then(|bytes| {
                self.retired_authorized_misses
                    .capacity()
                    .checked_mul(size_of::<(u64, usize)>())
                    .and_then(|retired| bytes.checked_add(retired))
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

    pub(super) fn retained_immediate_bytes(&self) -> Result<usize, BlobCacheError> {
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
            .and_then(|bytes| {
                self.recovery_ready
                    .capacity()
                    .checked_mul(size_of::<ReadyRecovery>())
                    .and_then(|recovery| bytes.checked_add(recovery))
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
        if self.column_barriers.is_empty() {
            self.column_barriers = HashMap::new();
        }
        let pending_bytes = self.retained_pending_bytes()?;
        self.stats.pending_bytes = pending_bytes;
        self.stats.pending_transactions = self.pending.len().saturating_add(self.ready.len());
        if self.retained_cached_bytes()? > self.cache.limits.max_pending_bytes {
            return Err(BlobCacheError::TooManyPendingBytes {
                max: self.cache.limits.max_pending_bytes,
            });
        }
        if self.retained_immediate_bytes()? > self.cache.limits.max_pending_bytes {
            return Err(BlobCacheError::ImmediateReadyBytePressure {
                max: self.cache.limits.max_pending_bytes,
            });
        }
        Ok(())
    }
}
