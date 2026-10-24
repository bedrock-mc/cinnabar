use super::*;

#[cfg(test)]
thread_local! {
    static DELTA_PIN_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl BlobCacheResolver {
    /// Validates the response before publishing its cache delta.
    pub(super) fn accept_miss_response_inner(
        &mut self,
        response: ClientCacheMissResponse,
    ) -> Result<(), BlobCacheError> {
        let mut unique = Vec::<(u64, bytes::Bytes)>::new();
        let mut positions = HashMap::<u64, usize>::new();
        let mut unsolicited = None;
        for blob in response.missing_blobs {
            let (blob_id, blob_data) = (blob.blob_id.0, blob.blob_data);
            if !self.pending_by_hash.contains_key(&blob_id) && !self.cache.contains(blob_id) {
                // A retired transaction can leave a late blob beside still-needed payloads.
                // Ignore its body without discarding or recovering the other columns.
                unsolicited = Some(blob_id);
                continue;
            }
            if let Some(&index) = positions.get(&blob_id) {
                if unique[index].1 != blob_data {
                    return Err(BlobCacheError::ConflictingDuplicate(blob_id));
                }
                continue;
            }
            // Validate repeated payloads too, before publishing any part of the response.
            let actual = client_blob_hash(&blob_data);
            if actual != blob_id {
                return Err(BlobCacheError::HashMismatch {
                    claimed: blob_id,
                    actual,
                });
            }
            positions.insert(blob_id, unique.len());
            unique.push((blob_id, blob_data));
        }
        if let Some(hash) = unsolicited {
            if unique.is_empty() {
                return Err(BlobCacheError::UnsolicitedBlob(hash));
            }
            self.stats.miss_response_unsolicited =
                self.stats.miss_response_unsolicited.saturating_add(1);
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

        let mut admitted = Vec::with_capacity(unique.len());
        let evictions = {
            let mut store = self.cache.lock();
            let newly_admitted = validate_delta(&store, &unique)?;
            let available = MAX_CLIENT_BLOB_CACHE_ENTRIES.saturating_sub(store.entries.len());
            store.entries.reserve(newly_admitted.min(available));
            let before = store.entries.len();
            for (hash, payload) in &unique {
                insert_verified(&mut store, self.cache.limits, *hash, payload)?;
                if let Some(entry) = store.entries.get(hash) {
                    admitted.push((*hash, entry.payload.clone()));
                }
            }
            let expected_without_eviction = before.saturating_add(newly_admitted);
            expected_without_eviction.saturating_sub(store.entries.len())
        };
        for (hash, payload) in admitted {
            self.cache.persist(hash, payload);
        }
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
fn validate_delta(
    store: &CacheStore,
    unique: &[(u64, bytes::Bytes)],
) -> Result<usize, BlobCacheError> {
    let mut total_bytes = store.total_bytes;
    let mut pinned_bytes = store.pinned_bytes;
    let mut new_entries = 0;
    for (hash, payload) in unique {
        if let Some(existing) = store.entries.get(hash) {
            if existing.payload.as_ref() != &payload[..] {
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
    // Every resident pin belongs to the pin index, so its size proves ordinary responses fit
    // without inspecting bodies retained from earlier worlds. Missing pins only need an exact
    // intersection when this conservative bound exceeds capacity.
    if store.pins.len().saturating_add(new_entries) > MAX_CLIENT_BLOB_CACHE_ENTRIES {
        #[cfg(test)]
        DELTA_PIN_VISITS.with(|visits| visits.set(visits.get() + store.entries.len()));
        let pinned_entries = store
            .entries
            .keys()
            .filter(|hash| store.pins.contains_key(hash))
            .count();
        if pinned_entries.saturating_add(new_entries) > MAX_CLIENT_BLOB_CACHE_ENTRIES {
            return Err(BlobCacheError::CacheEntryPressure);
        }
    }
    Ok(new_entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small response does not inspect unrelated bodies retained from previous worlds.
    #[test]
    fn blob_cache_delta_validation_skips_unrelated_entries() {
        let mut store = CacheStore::default();
        for hash in 0..4_096 {
            insert_verified(&mut store, BlobCacheLimits::default(), hash, b"body").unwrap();
        }
        store.pins.insert(0, 2);
        store.pins.insert(8_192, 1);
        DELTA_PIN_VISITS.with(|visits| visits.set(0));
        assert_eq!(
            validate_delta(&store, &[(8_192, bytes::Bytes::from_static(b"new"))]),
            Ok(1)
        );
        DELTA_PIN_VISITS.with(|visits| assert_eq!(visits.get(), 0));
    }

    /// Unresolved pins cannot reject a response when its resident pinned bodies still fit.
    #[test]
    fn response_delta_counts_resident_pins_at_capacity() {
        let mut store = CacheStore::default();
        for hash in 0..MAX_CLIENT_BLOB_CACHE_ENTRIES as u64 {
            store.pins.insert(hash, 1);
        }
        store.pins.insert(0, 2);
        insert_verified(&mut store, BlobCacheLimits::default(), 0, b"resident").unwrap();
        let candidate = [(1, bytes::Bytes::from_static(b"new"))];
        assert_eq!(validate_delta(&store, &candidate), Ok(1));
        let entry = store.entries[&0].clone();
        for hash in 1..MAX_CLIENT_BLOB_CACHE_ENTRIES as u64 {
            store.entries.insert(hash, entry.clone());
        }
        assert_eq!(
            validate_delta(
                &store,
                &[(u64::MAX, bytes::Bytes::from_static(b"overflow"))]
            ),
            Err(BlobCacheError::CacheEntryPressure)
        );
    }

    /// The upper bound accepts exact capacity without inspecting any resident pin bodies.
    #[test]
    fn response_delta_accepts_exact_pin_capacity_without_scanning() {
        let mut store = CacheStore::default();
        for hash in 0..MAX_CLIENT_BLOB_CACHE_ENTRIES as u64 - 1 {
            store.pins.insert(hash, 1);
        }
        DELTA_PIN_VISITS.with(|visits| visits.set(0));
        assert_eq!(
            validate_delta(&store, &[(u64::MAX, bytes::Bytes::from_static(b"new"))]),
            Ok(1)
        );
        DELTA_PIN_VISITS.with(|visits| assert_eq!(visits.get(), 0));
    }

    /// Batch validation rejects a later conflict before admitting the earlier delta.
    #[test]
    fn response_delta_rejects_existing_conflict() {
        let mut store = CacheStore::default();
        insert_verified(&mut store, BlobCacheLimits::default(), 7, b"original").unwrap();
        let before_clock = store.clock;
        assert_eq!(
            validate_delta(
                &store,
                &[
                    (8, bytes::Bytes::from_static(b"new")),
                    (7, bytes::Bytes::from_static(b"different")),
                ]
            ),
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
            validate_delta(
                &store,
                &[
                    (1, bytes::Bytes::from_static(&[1])),
                    (2, bytes::Bytes::from_static(&[2])),
                ]
            ),
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
