use super::*;

impl BlobCacheResolver {
    /// Validates the response before publishing its cache delta.
    pub(super) fn accept_miss_response_inner(
        &mut self,
        response: ClientCacheMissResponsePacket,
    ) -> Result<(), BlobCacheError> {
        let mut unique = Vec::<(u64, Vec<u8>)>::new();
        let mut positions = HashMap::<u64, usize>::new();
        for blob in response.missing_blobs {
            if !self.pending_by_hash.contains_key(&blob.blob_id) {
                return Err(BlobCacheError::UnsolicitedBlob(blob.blob_id));
            }
            if let Some(&index) = positions.get(&blob.blob_id) {
                if unique[index].1 != blob.blob_data {
                    return Err(BlobCacheError::ConflictingDuplicate(blob.blob_id));
                }
                continue;
            }
            // Deliberate security divergence from current vanilla: the public cache-poisoning
            // disclosure at https://gist.github.com/JustTalDevelops/1abfdae7ab7618af2ec82f709ffa93bb
            // reports that vanilla stopped validating this hash. Cinnabar keeps validation.
            let actual = client_blob_hash(&blob.blob_data);
            if actual != blob.blob_id {
                return Err(BlobCacheError::HashMismatch {
                    claimed: blob.blob_id,
                    actual,
                });
            }
            positions.insert(blob.blob_id, unique.len());
            unique.push((blob.blob_id, blob.blob_data));
        }

        let mut staged_additions = HashMap::<u64, usize>::new();
        for (hash, payload) in &unique {
            let Some(transactions) = self.pending_by_hash.get(hash) else {
                continue;
            };
            for sequence in transactions {
                let addition = staged_additions.entry(*sequence).or_default();
                *addition = addition.saturating_add(payload.len());
            }
        }
        let staged_excess = staged_additions
            .iter()
            .filter_map(|(sequence, addition)| {
                self.pending
                    .get(sequence)
                    .filter(|transaction| {
                        transaction.staged_bytes.saturating_add(*addition)
                            > MAX_CLIENT_BLOB_STAGED_BYTES_PER_TRANSACTION
                    })
                    .map(|_| *sequence)
            })
            .collect::<Vec<_>>();
        for sequence in staged_excess {
            self.record_staged_skip();
            self.abandon_pending_transaction(sequence)?;
        }
        for (sequence, addition) in staged_additions {
            if let Some(transaction) = self.pending.get_mut(&sequence) {
                transaction.staged_bytes = transaction.staged_bytes.saturating_add(addition);
                debug_assert!(
                    transaction.staged_bytes <= MAX_CLIENT_BLOB_STAGED_BYTES_PER_TRANSACTION
                );
            }
        }

        let evictions = {
            let mut store = self.cache.lock();
            let newly_admitted = validate_delta(&store, &unique)?;
            let available = MAX_CLIENT_BLOB_CACHE_ENTRIES.saturating_sub(store.entries.len());
            store.entries.reserve(newly_admitted.min(available));
            let before = store.entries.len();
            for (hash, payload) in &unique {
                insert_verified(&mut store, self.cache.limits, *hash, payload)?;
            }
            let expected_without_eviction = before.saturating_add(newly_admitted);
            expected_without_eviction.saturating_sub(store.entries.len())
        };
        self.stats.admitted_blobs = self
            .stats
            .admitted_blobs
            .saturating_add(u64::try_from(unique.len()).unwrap_or(u64::MAX));
        self.stats.evictions = self
            .stats
            .evictions
            .saturating_add(u64::try_from(evictions).unwrap_or(u64::MAX));
        for (hash, _) in &unique {
            self.resolve_hash(*hash)?;
        }
        self.refresh_pending_accounting()
    }
}

/// Checks conflicts and aggregate size before any response entry can be published.
fn validate_delta(store: &CacheStore, unique: &[(u64, Vec<u8>)]) -> Result<usize, BlobCacheError> {
    let mut total_bytes = store.total_bytes;
    let mut pinned_bytes = store.pinned_bytes;
    let mut new_entries = 0;
    for (hash, payload) in unique {
        if let Some(existing) = store.entries.get(hash) {
            if existing.payload.as_ref() != payload {
                return Err(BlobCacheError::ConflictingDuplicate(*hash));
            }
        } else {
            total_bytes = total_bytes
                .checked_add(payload.len())
                .ok_or(BlobCacheError::ByteCountOverflow)?;
            if store.pins.contains_key(hash) {
                pinned_bytes = pinned_bytes.saturating_add(payload.len());
                if pinned_bytes > MAX_CLIENT_BLOB_PINNED_BYTES {
                    return Err(BlobCacheError::PinnedPayloadPressure);
                }
            }
            new_entries += 1;
        }
    }
    let pinned_entries = store
        .entries
        .keys()
        .filter(|hash| store.pins.contains_key(hash))
        .count();
    if pinned_entries.saturating_add(new_entries) > MAX_CLIENT_BLOB_CACHE_ENTRIES {
        return Err(BlobCacheError::CacheEntryPressure);
    }
    Ok(new_entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Batch validation rejects a later conflict before admitting the earlier delta.
    #[test]
    fn response_delta_rejects_existing_conflict() {
        let mut store = CacheStore::default();
        insert_verified(&mut store, BlobCacheLimits::default(), 7, b"original").unwrap();
        let before_clock = store.clock;
        assert_eq!(
            validate_delta(&store, &[(8, b"new".to_vec()), (7, b"different".to_vec())]),
            Err(BlobCacheError::ConflictingDuplicate(7))
        );
        assert_eq!(store.entries.len(), 1);
        assert_eq!(store.clock, before_clock);
        assert_eq!(store.entries[&7].payload.as_ref(), b"original");
    }

    /// Aggregate overflow is checked before any mutation, including the cache clock.
    #[test]
    fn response_delta_rejects_aggregate_overflow() {
        let mut store = CacheStore {
            total_bytes: usize::MAX - 1,
            ..Default::default()
        };
        assert_eq!(
            validate_delta(&store, &[(1, vec![1]), (2, vec![2])]),
            Err(BlobCacheError::ByteCountOverflow)
        );
        assert!(store.entries.is_empty());
        assert_eq!(
            insert_verified(&mut store, BlobCacheLimits::default(), 3, &[1, 2]),
            Err(BlobCacheError::ByteCountOverflow)
        );
        assert_eq!(store.clock, 0);
    }
}
