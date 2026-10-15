use super::*;

mod lazy_compaction;
mod memory;

/// Column decoding agrees with per-block lookup at every width, including truncated words.
#[test]
fn column_indices_match_block_lookup() {
    for bits in SUPPORTED_BITS {
        let words = (0..word_count(bits))
            .map(|index| (index as u32).wrapping_mul(0x9e37_79b9))
            .take(word_count(bits).saturating_sub(3))
            .collect();
        let storage = PalettedStorage::new(bits, words, vec![0; 1 << bits.min(12)]);
        let mut column = [0; SUB_CHUNK_SIDE];
        for x in 0..16 {
            for z in 0..16 {
                storage.column_palette_indices(x, z, &mut column);
                for (y, &index) in column.iter().enumerate() {
                    let linear = (usize::from(x) << 8) | (usize::from(z) << 4) | y;
                    let expected = storage
                        .palette_index(linear)
                        .map_or(u16::MAX, |index| index as u16);
                    assert_eq!(index, expected, "bits={bits} x={x} z={z} y={y}");
                }
            }
        }
    }
}

/// Single edits must reproduce the preserved batch path's canonical storage.
#[test]
fn single_update_matches_batch_canonicalization() {
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut cases = vec![PalettedStorage::uniform(7), PalettedStorage::uniform(0)];
    for bits in PACKED_BITS {
        for palette_len in [1_usize, 2, 3, 9, 40, 256, 257, 300, BLOCKS_PER_SUB_CHUNK] {
            let palette_len = palette_len.min(1 << bits);
            // Duplicates, unused entries and garbage padding stay non-canonical.
            let palette = (0..palette_len).map(|_| (next() % 12) as u32).collect();
            let words = (0..word_count(bits)).map(|_| next() as u32).collect();
            let mut storage = PalettedStorage::new(bits, words, palette);
            storage.zero_indices_at_or_above(palette_len);
            cases.push(storage.clone());
            let mut single_owner = storage.clone();
            single_owner.apply_runtime_updates(&[(17, 99)]);
            cases.push(single_owner);
        }
    }
    for storage in cases {
        for _ in 0..24 {
            let linear = (next() % BLOCKS_PER_SUB_CHUNK as u64) as usize;
            let runtime_id = match next() % 4 {
                0 => 99,
                _ => (next() % 14) as u32,
            };
            let mut batch = storage.clone();
            let mut single = storage.clone();
            let batch_changed = batch.apply_runtime_updates(&[(linear, runtime_id)]);
            assert_eq!(
                single.apply_runtime_update(linear, runtime_id),
                batch_changed
            );
            assert_storage_matches(&single, &batch);
        }
    }
}

/// Builds packed fixtures without canonicalizing duplicates, unused entries or padding.
fn packed(bits: u8, values: Vec<u32>, index: impl Fn(usize) -> usize) -> PalettedStorage {
    let mut words = vec![0; word_count(bits)];
    for linear in 0..BLOCKS_PER_SUB_CHUNK {
        write_palette_index(&mut words, bits, linear, index(linear));
    }
    PalettedStorage::new(bits, words, values)
}

/// Checks representation, resolved cells, and every retained count against the packed data.
fn assert_storage_matches(actual: &PalettedStorage, expected: &PalettedStorage) {
    assert_eq!(actual.bits_per_index(), expected.bits_per_index());
    assert_eq!(actual.palette(), expected.palette());
    assert_eq!(actual.words(), expected.words());
    assert_eq!(actual, expected);
    let mut counts = vec![0_u16; actual.palette.len()];
    let mut runtime_counts = HashMap::<u32, usize>::new();
    for linear in 0..BLOCKS_PER_SUB_CHUNK {
        let x = (linear >> 8) as u8;
        let y = (linear & 15) as u8;
        let z = ((linear >> 4) & 15) as u8;
        let value = actual.runtime_id(x, y, z).unwrap();
        assert_eq!(Some(value), expected.runtime_id(x, y, z));
        counts[actual.palette_index(linear).unwrap()] += 1;
        *runtime_counts.entry(value).or_default() += 1;
    }
    if let Some(edits) = &actual.edits {
        let mut retained = vec![0_u16; actual.palette.len()];
        let mut physical_counts = vec![0_u16; edits.slots.len()];
        for linear in 0..BLOCKS_PER_SUB_CHUNK {
            physical_counts[actual.physical_index(linear).unwrap()] += 1;
        }
        for (slot, &count) in edits.slots.iter().zip(&physical_counts) {
            assert_eq!(slot.count, count);
            if count != 0 {
                retained[usize::from(slot.palette_index)] += count;
            }
        }
        assert_eq!(retained, counts);
        assert_eq!(
            retained.iter().map(|&n| usize::from(n)).sum::<usize>(),
            BLOCKS_PER_SUB_CHUNK
        );
        assert!(edits.slots.len() <= BLOCKS_PER_SUB_CHUNK);
        let mut aggregated = HashMap::<u32, usize>::new();
        for (&value, &count) in actual.palette.values.iter().zip(&retained) {
            *aggregated.entry(value).or_default() += usize::from(count);
        }
        assert_eq!(aggregated, runtime_counts);
    }
}

/// Compares one edit with the batch oracle, including the change result and retained counts.
fn compare_edit(
    single: &mut PalettedStorage,
    batch: &mut PalettedStorage,
    linear: usize,
    value: u32,
) {
    assert_eq!(
        single.apply_runtime_update(linear, value),
        batch.apply_runtime_updates(&[(linear, value)])
    );
    assert_storage_matches(single, batch);
}

/// Every packed width can patch repeated existing values without replacing any other word.
#[test]
fn same_width_edits_retain_words_and_snapshot_isolation() {
    for (previous, bits) in SUPPORTED_BITS.into_iter().zip(PACKED_BITS) {
        let len = (1 << previous) + 1;
        let mut storage = packed(bits, (0..len as u32).collect(), |i| i % len);
        let snapshot = storage.clone();
        let mut batch = storage.clone();
        let allocation = storage.words.as_ptr();
        let per_word = 32 / usize::from(bits);
        for linear in [0, per_word - 1, per_word, BLOCKS_PER_SUB_CHUNK - 1, 17, 18] {
            let before = storage.words.clone();
            let old_index = storage.palette_index(linear).unwrap();
            compare_edit(
                &mut storage,
                &mut batch,
                linear,
                ((old_index + 1) % len) as u32,
            );
            assert_eq!(storage.words.as_ptr(), allocation, "bits={bits}");
            for (index, (&old, &new)) in before.iter().zip(storage.words.iter()).enumerate() {
                if index != linear / per_word {
                    assert_eq!(old, new, "bits={bits} word={index}");
                }
            }
        }
        assert_storage_matches(
            &snapshot,
            &packed(bits, (0..len as u32).collect(), |i| i % len),
        );
        assert!(snapshot.edits.is_none());
        let mut clone = storage.clone();
        compare_edit(&mut clone, &mut batch, 31, len as u32 + 10);
        assert_ne!(clone, storage);
        assert_storage_matches(&storage, &storage.clone());
    }
}

/// Appending inside a width and repeatedly crossing each legal width boundary stays canonical.
#[test]
fn repeated_width_growth_shrink_and_same_width_append_match_batch() {
    for bits in SUPPORTED_BITS.into_iter().filter(|&bits| bits < 16) {
        let capacity = 1_usize << bits;
        let mut single = packed(bits, (0..capacity as u32).collect(), |i| i % capacity);
        let mut batch = single.clone();
        for _ in 0..4 {
            compare_edit(
                &mut single,
                &mut batch,
                BLOCKS_PER_SUB_CHUNK - 1,
                capacity as u32,
            );
            compare_edit(
                &mut single,
                &mut batch,
                BLOCKS_PER_SUB_CHUNK - 1,
                ((BLOCKS_PER_SUB_CHUNK - 1) % capacity) as u32,
            );
            assert_eq!(single.bits_per_index(), bits);
        }
    }
    for (previous, bits) in SUPPORTED_BITS
        .into_iter()
        .zip(PACKED_BITS)
        .filter(|&(_, bits)| bits > 1)
    {
        let len = (1_usize << previous) + 1;
        let mut single = packed(bits, (0..len as u32).collect(), |i| i % len);
        let mut batch = single.clone();
        let words = single.words.as_ptr();
        compare_edit(&mut single, &mut batch, 0, len as u32);
        assert_eq!(single.words.as_ptr(), words);
        assert_eq!(single.palette.values.last(), Some(&(len as u32)));
    }
}

/// Removing a middle or final entry remaps indices; inserting and deleting can return to uniform.
#[test]
fn last_use_and_mixed_batches_preserve_palette_order() {
    for removed in [1, 2] {
        for replacement in [0, 1, 2, 77] {
            let mut single = packed(2, vec![10, 11, 12], |i| if i < 2 { i + 1 } else { 0 });
            let mut batch = single.clone();
            compare_edit(
                &mut single,
                &mut batch,
                removed - 1,
                if replacement < 3 {
                    10 + replacement
                } else {
                    replacement
                },
            );
        }
    }
    let mut single = PalettedStorage::uniform(7);
    let mut batch = single.clone();
    for _ in 0..4 {
        compare_edit(&mut single, &mut batch, 17, 8);
        let updates = [(19, 9), (17, 42), (17, 7), (19, 8)];
        assert_eq!(
            single.apply_runtime_updates(&updates),
            batch.apply_runtime_updates(&updates)
        );
        assert_storage_matches(&single, &batch);
        compare_edit(&mut single, &mut batch, 19, 7);
        assert!(single.is_uniform());
        compare_edit(&mut single, &mut batch, 19, 7);
    }
    let updates = [(0, 99), (1, 9), (0, 8)];
    assert!(single.apply_runtime_updates(&updates));
    assert!(batch.apply_runtime_updates(&updates));
    assert_eq!(single.palette.values.as_ref(), &[7, 9, 8]);
    assert_storage_matches(&single, &batch);
    compare_edit(&mut single, &mut batch, 2, 9);
}

/// Noncanonical palettes, widths and padding remain untouched on no-ops and rebuild on change.
#[test]
fn noncanonical_inputs_cannot_take_a_word_patch() {
    let mut cases = vec![
        PalettedStorage::new(0, vec![u32::MAX], vec![7]),
        packed(0, vec![7, 8], |_| 0),
        packed(1, vec![7, 7], |i| i % 2),
        packed(2, vec![7, 8, 8, 9], |i| i % 3),
        packed(2, vec![7, 8, 9], |i| i % 2),
        packed(16, vec![7, 8], |i| i % 2),
    ];
    let mut extra_word = packed(1, vec![7, 8], |i| i % 2);
    let mut words = extra_word.words.to_vec();
    words.push(0);
    extra_word.words = words.into_boxed_slice();
    cases.push(extra_word);
    for (previous, bits) in SUPPORTED_BITS.into_iter().zip(PACKED_BITS) {
        let per_word = 32 / usize::from(bits);
        let len = (1 << previous) + 1;
        let mut storage = packed(bits, (0..len as u32).collect(), |i| i % len);
        let used_bits = per_word * usize::from(bits);
        if used_bits < 32 {
            storage.words[0] |= 1 << used_bits;
            cases.push(storage.clone());
        }
        let tail_bits = (BLOCKS_PER_SUB_CHUNK % per_word) * usize::from(bits);
        if tail_bits != 0 {
            *storage.words.last_mut().unwrap() |= 1 << tail_bits;
            cases.push(storage);
        }
    }
    for mut single in cases {
        let original = single.clone();
        let value = single.runtime_id(0, 0, 0).unwrap();
        for _ in 0..3 {
            assert!(!single.apply_runtime_update(0, value));
            assert_storage_matches(&single, &original);
        }
        assert!(single.canonical_counts().is_none());
        assert!(single.edits.is_none());
        assert_storage_matches(&single, &original);
        let mut batch = original;
        compare_edit(&mut single, &mut batch, 0, 99);
        assert!(single.edits.is_some());
    }
}

/// Decoder repair discards old counts and initializes them from repaired cells on the next edit.
#[test]
fn index_repair_invalidates_counts_without_changing_equality() {
    let mut storage = packed(2, vec![7, 8, 9], |i| if i < 2 { i + 1 } else { 0 });
    assert!(storage.apply_runtime_update(0, 10));
    assert!(storage.edits.is_some());
    // Repair must first resolve physical slots into the currently exposed logical order.
    let mut expected = PalettedStorage::new(
        storage.bits_per_index(),
        storage.words().to_vec(),
        storage.palette.values.to_vec(),
    );
    storage.zero_indices_at_or_above(2);
    expected.zero_indices_at_or_above(2);
    assert!(storage.edits.is_none());
    assert_storage_matches(&storage, &expected);
    compare_edit(&mut storage, &mut expected, 9, 8);
    assert!(storage.edits.is_some());

    let mut malformed = packed(2, vec![7, 8], |i| i % 4);
    assert!(malformed.canonical_counts().is_none());
    malformed.zero_indices_at_or_above(2);
    assert!(malformed.edits.is_none());
    for linear in 0..BLOCKS_PER_SUB_CHUNK {
        assert_eq!(
            malformed.palette_index(linear),
            Some(if linear % 4 == 1 { 1 } else { 0 })
        );
    }
}

/// Cache warmth is ignored while distinct palettes, words and widths remain structurally unequal.
#[test]
fn equality_ignores_only_derived_counts() {
    let cold = packed(1, vec![7, 8], |i| i % 2);
    let mut warm = cold.clone();
    warm.edits = warm.canonical_counts().map(PaletteEdits::new);
    assert!(warm.edits.is_some());
    assert_storage_matches(&warm, &cold);
    assert_ne!(warm, packed(1, vec![7, 8], |i| (i + 1) % 2));
    assert_ne!(warm, packed(1, vec![8, 7], |i| i % 2));
    assert_ne!(warm, packed(2, vec![7, 8], |i| i % 2));
}

/// Malformed internal storage retains the batch path's failure contract rather than patching it.
#[test]
fn malformed_storage_keeps_batch_fallback() {
    for mut single in [
        PalettedStorage::new(1, vec![0], vec![7, 8]),
        packed(2, vec![7, 8], |i| i % 4),
    ] {
        let mut batch = single.clone();
        let single_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            single.apply_runtime_update(0, 8)
        }));
        let batch_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            batch.apply_runtime_updates(&[(0, 8)])
        }));
        assert!(single_result.is_err());
        assert!(batch_result.is_err());
        assert_eq!(single, batch);
    }
}

/// Large unique palettes retain counts through both common edits and last-use replacements.
#[test]
fn large_palettes_keep_counts_across_single_and_batch_edits() {
    for len in [
        256,
        257,
        300,
        BLOCKS_PER_SUB_CHUNK / 2,
        BLOCKS_PER_SUB_CHUNK,
    ] {
        let mut single = packed(bits_for_palette_len(len), (0..len as u32).collect(), |i| {
            i % len
        });
        let mut batch = single.clone();
        for linear in [0, len / 2, len - 1, BLOCKS_PER_SUB_CHUNK - 1] {
            compare_edit(&mut single, &mut batch, linear, len as u32 + 1);
            compare_edit(&mut single, &mut batch, linear, 0);
            compare_edit(&mut single, &mut batch, linear, 0);
            let updates = [(linear, 17), (linear, 0), (19, len as u32 + 2)];
            assert_eq!(
                single.apply_runtime_updates(&updates),
                batch.apply_runtime_updates(&updates)
            );
            assert_storage_matches(&single, &batch);
        }
    }
}

mod batch_tests {
    use super::*;

    /// The position-by-position definition of a batch: each position's last update wins,
    /// then used values keep palette order and new ones follow in update order.
    fn reference_batch(storage: &PalettedStorage, updates: &[(usize, u32)]) -> PalettedStorage {
        let final_update = |linear: usize| {
            updates
                .iter()
                .rev()
                .find(|&&(candidate, _)| candidate == linear)
                .map(|&(_, value)| value)
        };
        let value_at = |linear: usize| {
            final_update(linear).unwrap_or_else(|| {
                storage
                    .palette
                    .get(storage.palette_index(linear).unwrap())
                    .unwrap()
            })
        };
        let finals: Vec<u32> = (0..BLOCKS_PER_SUB_CHUNK).map(value_at).collect();
        let mut values = Vec::new();
        for &value in storage.palette.values.iter() {
            if finals.contains(&value) && !values.contains(&value) {
                values.push(value);
            }
        }
        for &(linear, value) in updates {
            if final_update(linear) == Some(value) && !values.contains(&value) {
                values.push(value);
            }
        }
        let bits_per_index = bits_for_palette_len(values.len());
        let mut words = vec![0; word_count(bits_per_index)];
        for (linear, value) in finals.iter().enumerate() {
            let index = values
                .iter()
                .position(|candidate| candidate == value)
                .unwrap();
            write_palette_index(&mut words, bits_per_index, linear, index);
        }
        PalettedStorage::new(bits_per_index, words, values)
    }

    /// Batches with repeated, out-of-section and no-op positions match the reference.
    #[test]
    fn batch_updates_match_position_reference() {
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for bits in PACKED_BITS {
            for palette_len in [1_usize, 2, 5, 40] {
                let palette_len = palette_len.min(1 << bits);
                let palette = (0..palette_len).map(|_| (next() % 9) as u32).collect();
                let words = (0..word_count(bits)).map(|_| next() as u32).collect();
                let mut storage = PalettedStorage::new(bits, words, palette);
                storage.zero_indices_at_or_above(palette_len);
                for batch_len in [2_usize, 7, 300] {
                    let updates: Vec<_> = (0..batch_len)
                        .map(|_| {
                            let linear = match next() % 8 {
                                0 => BLOCKS_PER_SUB_CHUNK + (next() % 3) as usize,
                                1 => 5,
                                _ => (next() % BLOCKS_PER_SUB_CHUNK as u64) as usize,
                            };
                            (linear, (next() % 12) as u32)
                        })
                        .collect();
                    let expected = reference_batch(&storage, &updates);
                    let mut actual = storage.clone();
                    if actual.apply_runtime_updates(&updates) {
                        assert_eq!(actual, expected, "bits={bits} batch={batch_len}");
                    } else {
                        assert_eq!(actual, storage);
                        assert!((0..BLOCKS_PER_SUB_CHUNK).all(|linear| {
                            expected.runtime_id_at(linear) == storage.runtime_id_at(linear)
                        }));
                    }
                }
            }
        }
    }

    impl PalettedStorage {
        fn runtime_id_at(&self, linear: usize) -> Option<u32> {
            self.palette.get(self.palette_index(linear)?)
        }
    }
}
