use super::*;
use crate::{BlockUpdate, ChunkStore, RawBiomeIds, RawBlockIds, SubChunkKey};
use std::cell::Cell;

mod biome_only {
    use super::*;

    #[test]
    fn biome_changes_preserve_block_identity_and_report_only_mesh_changes() {
        for reuse in [false, true] {
            let mut store = ChunkStore::new();
            let key = ChunkKey::new(0, 0, 0);
            let ids = RawBlockIds { air: 0 };
            store
                .commit_level_chunk(key, decode(&payload(2, 0), &ids, None))
                .unwrap();
            let previous = store.chunk(key).unwrap().shared_sub_chunks();
            let revision = store.collision_revision(key);
            let replacement = decode(&payload(2, 1), &ids, reuse.then_some(previous.as_ref()));

            let applied = store.commit_level_chunk(key, replacement).unwrap();

            assert!(applied.block_changed.is_empty());
            assert_eq!(
                applied.changed,
                vec![
                    SubChunkKey::from_chunk(key, 0),
                    SubChunkKey::from_chunk(key, 1)
                ]
            );
            assert!(!applied.dirty.is_empty());
            assert_eq!(store.collision_revision(key), revision);
            for (y, section) in previous.iter() {
                assert!(Arc::ptr_eq(
                    section,
                    &store.sub_chunk(SubChunkKey::from_chunk(key, *y)).unwrap()
                ));
            }
        }
    }

    #[test]
    fn block_changes_are_rechecked_when_the_prepared_snapshot_is_stale() {
        let mut store = ChunkStore::new();
        let key = ChunkKey::new(0, 0, 0);
        let section = SubChunkKey::from_chunk(key, 0);
        let ids = RawBlockIds { air: 0 };
        store
            .commit_level_chunk(key, decode(&payload(2, 0), &ids, None))
            .unwrap();
        let previous = store.chunk(key).unwrap().shared_sub_chunks();
        let replacement = decode(&payload(2, 1), &ids, Some(previous.as_ref()));
        store
            .update_block(section, BlockUpdate::new(8, 8, 8, 0, 7), 0)
            .unwrap();

        let applied = store.commit_level_chunk(key, replacement).unwrap();

        assert_eq!(applied.block_changed, vec![section]);
        assert_eq!(
            applied.changed,
            vec![section, SubChunkKey::from_chunk(key, 1)]
        );
        assert_eq!(
            store.sub_chunk(section).unwrap().runtime_id(0, 8, 8, 8),
            Some(1)
        );
    }

    #[test]
    fn new_and_removed_stored_sections_remain_block_changes() {
        let mut store = ChunkStore::new();
        let key = ChunkKey::new(0, 0, 0);
        let keys = vec![
            SubChunkKey::from_chunk(key, 0),
            SubChunkKey::from_chunk(key, 1),
        ];
        let ids = RawBlockIds { air: 0 };
        let arrived = store
            .commit_level_chunk(key, decode(&payload(2, 0), &ids, None))
            .unwrap();
        assert_eq!(arrived.block_changed, keys);

        let removed = store
            .commit_level_chunk(key, DecodedLevelChunk::decode(0, 0, &[], &ids))
            .unwrap();
        assert_eq!(removed.block_changed, keys);
        assert_eq!(removed.changed, keys);
    }
}

struct CountingIds {
    calls: Cell<usize>,
    air: u32,
}

impl BlockIds for CountingIds {
    /// Counts palette resolution, which cached sections must skip completely.
    fn resolve(&self, id: u32) -> u32 {
        self.calls.set(self.calls.get() + 1);
        id
    }
    /// Supplies the fallback used by this immutable decoder.
    fn air(&self) -> u32 {
        self.air
    }
    /// Uses the canonical raw-id context so it cannot retain the mutable counter.
    fn inline_cache_context(&self) -> Option<Arc<dyn std::any::Any + Send + Sync>> {
        RawBlockIds { air: self.air }.inline_cache_context()
    }
    /// Delegates cache identity to the same raw-id policy as resolution.
    fn inline_cache_context_matches(&self, context: &(dyn std::any::Any + Send + Sync)) -> bool {
        RawBlockIds { air: self.air }.inline_cache_context_matches(context)
    }
}

/// Two complete uniform sections followed by the full column tail.
fn payload(second: u8, biome: u8) -> Vec<u8> {
    vec![
        9,
        1,
        0,
        1,
        2,
        9,
        1,
        1,
        1,
        second << 1,
        1,
        biome << 1,
        0xff,
        0,
    ]
}

/// Decodes the test column with optional resident blocks and independent biome resolution.
fn decode(
    bytes: &[u8],
    ids: &dyn BlockIds,
    previous: Option<&BTreeMap<i32, Arc<SubChunk>>>,
) -> DecodedLevelChunk {
    DecodedLevelChunk::decode_inline_reusing(
        ChunkKey::new(0, 0, 0),
        DimensionSlots {
            base_sub_chunk_y: 0,
            count: 2,
        },
        2,
        bytes,
        ids,
        &RawBiomeIds { default_biome: 0 },
        previous,
    )
}

/// Identical resends perform no palette resolution and preserve the exact resident Arcs.
#[test]
fn identical_sections_skip_decode_and_structural_comparison() {
    let ids = CountingIds {
        calls: Cell::new(0),
        air: 0,
    };
    let first = decode(&payload(2, 0), &ids, None);
    assert_eq!(first.decode_work().decoded_sections, 2);
    assert_eq!(ids.calls.replace(0), 2);
    let resent = decode(&payload(2, 0), &ids, Some(&first.sub_chunks));
    assert_eq!(
        resent.decode_work(),
        InlineDecodeWork {
            decoded_sections: 0,
            reused_sections: 2
        }
    );
    assert_eq!(ids.calls.get(), 0);
    for (y, section) in resent.sub_chunks() {
        assert!(Arc::ptr_eq(&section, &first.sub_chunks[&y]));
    }
}

/// A one-section edit decodes only that section, while an independent tail change still commits.
#[test]
fn changed_sections_and_biome_tail_still_decode() {
    let ids = CountingIds {
        calls: Cell::new(0),
        air: 0,
    };
    let first = decode(&payload(2, 0), &ids, None);
    ids.calls.set(0);
    let resent = decode(&payload(3, 1), &ids, Some(&first.sub_chunks));
    assert_eq!(
        resent.decode_work(),
        InlineDecodeWork {
            decoded_sections: 1,
            reused_sections: 1
        }
    );
    assert_eq!(ids.calls.get(), 1);
    assert!(Arc::ptr_eq(&first.sub_chunks[&0], &resent.sub_chunks[&0]));
    assert_eq!(resent.sub_chunks[&1].runtime_id(0, 0, 0, 0), Some(3));
    let key = ChunkKey::new(0, 0, 0);
    let mut store = ChunkStore::new();
    store.commit_level_chunk(key, first).unwrap();
    store.commit_level_chunk(key, resent).unwrap();
    assert_eq!(
        store.biome_id(SubChunkKey::from_chunk(key, 0), 0, 0, 0),
        Some(1)
    );
}

/// A local mutation clears its old wire identity so a server resend can restore the original value.
#[test]
fn mutations_and_changed_registry_contexts_invalidate_reuse() {
    let ids = CountingIds {
        calls: Cell::new(0),
        air: 0,
    };
    let mut first = decode(&payload(2, 0), &ids, None);
    let original = first.sub_chunks.clone();
    Arc::make_mut(first.sub_chunks.get_mut(&0).unwrap()).apply_block_updates(
        &[BlockUpdate {
            x: 0,
            y: 0,
            z: 0,
            layer: 0,
            runtime_id: 7,
        }],
        0,
    );
    assert_eq!(
        decode(&payload(2, 0), &ids, Some(&original))
            .decode_work()
            .decoded_sections,
        0
    );
    let resent = decode(&payload(2, 0), &ids, Some(&first.sub_chunks));
    assert_eq!(resent.decode_work().decoded_sections, 1);
    assert_eq!(resent.sub_chunks[&0].runtime_id(0, 0, 0, 0), Some(1));
    let different = RawBlockIds { air: 99 };
    assert_eq!(
        decode(&payload(2, 0), &different, Some(&resent.sub_chunks))
            .decode_work()
            .decoded_sections,
        2
    );
}

/// Warm mutation counts must not prevent structural reuse or keep a stale inline identity.
#[test]
fn edited_sections_refresh_identity_after_equal_resends() {
    let ids = RawBlockIds { air: 0 };
    let key = ChunkKey::new(0, 0, 0);
    let mut store = ChunkStore::new();
    store
        .commit_level_chunk(key, decode(&payload(2, 0), &ids, None))
        .unwrap();
    let original = store.chunk(key).unwrap().shared_sub_chunks();
    let section_key = SubChunkKey::from_chunk(key, 0);
    for (y, value) in [(0, 7), (1, 7), (0, 1), (1, 1)] {
        store
            .update_block(section_key, BlockUpdate::new(0, y, 0, 0, value), 0)
            .unwrap();
    }
    let edited = store.chunk(key).unwrap().shared_sub_chunks();
    assert!(!Arc::ptr_eq(&original[&0], &edited[&0]));
    assert!(original[&0].blocks_match(&edited[&0]));
    let decoded = decode(&payload(2, 0), &ids, Some(&edited));
    assert_eq!(decoded.decode_work().decoded_sections, 1);
    let applied = store.commit_level_chunk(key, decoded).unwrap();
    assert!(applied.changed.is_empty());
    assert!(applied.dirty.is_empty());
    let resident = store.chunk(key).unwrap().shared_sub_chunks();
    assert!(Arc::ptr_eq(&edited[&0], &resident[&0]));
    assert_eq!(
        decode(&payload(2, 0), &ids, Some(&resident))
            .decode_work()
            .reused_sections,
        2
    );

    let mut equivalent = vec![8, 1, 3];
    equivalent.extend(std::iter::repeat_n(0xaa, crate::palette::word_count(1) * 4));
    equivalent.extend([4, 2, 2]);
    equivalent.extend_from_slice(&payload(2, 0)[5..]);
    let decoded = decode(&equivalent, &ids, Some(&resident));
    assert_eq!(decoded.decode_work().decoded_sections, 1);
    assert!(
        store
            .commit_level_chunk(key, decoded)
            .unwrap()
            .dirty
            .is_empty()
    );
    let resident = store.chunk(key).unwrap().shared_sub_chunks();
    assert!(Arc::ptr_eq(&edited[&0], &resident[&0]));
    assert_eq!(
        decode(&equivalent, &ids, Some(&resident))
            .decode_work()
            .reused_sections,
        2
    );
    assert_eq!(
        decode(&equivalent, &RawBlockIds { air: 99 }, Some(&resident))
            .decode_work()
            .decoded_sections,
        2
    );

    let mut moved = resident.as_ref().clone();
    moved.insert(1, Arc::clone(&original[&0]));
    let mut wrong_y = payload(2, 0);
    wrong_y[7] = 0;
    wrong_y[9] = 2;
    let cached = decode(&wrong_y, &ids, Some(&moved));
    let ordinary = decode(&wrong_y, &ids, None);
    assert_eq!(cached, ordinary);
    assert!(cached.sub_chunk(1).is_none());
    assert_eq!(
        decode(&payload(2, 0), &ids, Some(&original))
            .decode_work()
            .reused_sections,
        2
    );
}

/// Sections loaded by request-mode replies acquire an inline identity without changing their Arcs.
#[test]
fn structural_reuse_retains_new_wire_hashes_for_later_resends() {
    let ids = RawBlockIds { air: 0 };
    let key = ChunkKey::new(0, 0, 0);
    let mut store = ChunkStore::new();
    for y in 0..2 {
        store
            .commit_sub_chunk(
                SubChunkKey::from_chunk(key, y),
                SubChunk::decode(&[9, 1, y as u8, 1, ((y + 1) << 1) as u8], &ids),
            )
            .unwrap();
    }
    let original = store.chunk(key).unwrap().shared_sub_chunks();
    let first = decode(&payload(2, 0), &ids, Some(&original));
    assert_eq!(first.decode_work().decoded_sections, 2);
    store.commit_level_chunk(key, first).unwrap();
    let resident = store.chunk(key).unwrap().shared_sub_chunks();
    assert!(Arc::ptr_eq(&original[&0], &resident[&0]));
    assert_eq!(
        decode(&payload(2, 0), &ids, Some(&resident))
            .decode_work()
            .decoded_sections,
        0
    );

    // A wider varint has the same block value but a different wire identity and boundary.
    let mut wider = payload(2, 0);
    wider.splice(4..5, [0x82, 0]);
    let changed_encoding = decode(&wider, &ids, Some(&resident));
    assert_eq!(changed_encoding.decode_work().decoded_sections, 1);
    assert!(
        store
            .commit_level_chunk(key, changed_encoding)
            .unwrap()
            .changed
            .is_empty()
    );
    let updated = store.chunk(key).unwrap().shared_sub_chunks();
    assert!(Arc::ptr_eq(&original[&0], &updated[&0]));
    assert_eq!(
        decode(&wider, &ids, Some(&updated))
            .decode_work()
            .decoded_sections,
        0
    );
}

/// Equivalent palette packing and wire headers retain the resident blocks without dirty meshes.
#[test]
fn equivalent_block_encodings_publish_no_change_and_refresh_the_cache() {
    let ids = RawBlockIds { air: 0 };
    let key = ChunkKey::new(0, 0, 0);
    let mut store = ChunkStore::new();
    store
        .commit_level_chunk(key, decode(&payload(2, 0), &ids, None))
        .unwrap();
    let original = store.chunk(key).unwrap().shared_sub_chunks();
    let original_revision = store.collision_revision(key);

    // Version eight omits Y, and a packed palette may repeat the same resolved value.
    let mut equivalent = vec![8, 1, 3];
    for _ in 0..crate::BLOCKS_PER_SUB_CHUNK.div_ceil(u32::BITS as usize) {
        equivalent.extend(0xaaaa_aaaa_u32.to_le_bytes());
    }
    equivalent.extend([4, 2, 2]);
    equivalent.extend_from_slice(&payload(2, 0)[5..]);
    let decoded = decode(&equivalent, &ids, Some(&original));
    assert_eq!(decoded.decode_work().decoded_sections, 1);
    let applied = store.commit_level_chunk(key, decoded).unwrap();
    assert!(applied.changed.is_empty());
    assert!(applied.dirty.is_empty());
    assert_eq!(store.collision_revision(key), original_revision);
    let resident = store.chunk(key).unwrap().shared_sub_chunks();
    assert!(Arc::ptr_eq(&original[&0], &resident[&0]));
    assert_eq!(
        decode(&equivalent, &ids, Some(&resident))
            .decode_work()
            .reused_sections,
        2
    );

    equivalent[3] ^= 1;
    let changed = decode(&equivalent, &ids, Some(&resident));
    // Duplicate palette entries still resolve to the same block after an index-only edit.
    assert!(
        store
            .commit_level_chunk(key, changed)
            .unwrap()
            .changed
            .is_empty()
    );
    equivalent[3 + crate::BLOCKS_PER_SUB_CHUNK.div_ceil(u32::BITS as usize) * 4 + 2] = 6;
    let changed = decode(&equivalent, &ids, Some(&resident));
    assert_eq!(
        store.commit_level_chunk(key, changed).unwrap().changed,
        vec![SubChunkKey::from_chunk(key, 0)]
    );
}

/// A cached section moved to another Y slot must still obey the ordinary misplaced-Y rule.
#[test]
fn reused_payloads_cannot_bypass_slot_y_validation() {
    let ids = RawBlockIds { air: 0 };
    let first = decode(&payload(2, 0), &ids, None);
    let mut moved = first.sub_chunks.clone();
    moved.insert(1, Arc::clone(&first.sub_chunks[&0]));
    let mut bytes = payload(2, 0);
    bytes[7] = 0;
    bytes[9] = 2;
    let ordinary = decode(&bytes, &ids, None);
    let cached = decode(&bytes, &ids, Some(&moved));
    assert_eq!(cached, ordinary);
    assert_eq!(cached.decode_work().reused_sections, 1);
    assert!(!cached.sub_chunks.contains_key(&1));
}

/// Cache transfers retain the current wire Y even when semantic reuse keeps an older header.
#[test]
fn equivalent_header_reuse_cannot_hide_a_misplaced_wire_y() {
    let ids = RawBlockIds { air: 0 };
    let key = ChunkKey::new(0, 0, 0);
    let mut bytes = payload(2, 0);
    bytes[0] = 8;
    bytes.remove(2);
    let mut store = ChunkStore::new();
    store
        .commit_level_chunk(key, decode(&bytes, &ids, None))
        .unwrap();
    let original = store.chunk(key).unwrap().shared_sub_chunks();
    let resent = decode(&payload(2, 0), &ids, Some(&original));
    assert!(
        store
            .commit_level_chunk(key, resent)
            .unwrap()
            .changed
            .is_empty()
    );
    let mut moved = store
        .chunk(key)
        .unwrap()
        .shared_sub_chunks()
        .as_ref()
        .clone();
    assert!(Arc::ptr_eq(&original[&0], &moved[&0]));
    assert_eq!(moved[&0].y_index(), None);
    moved.insert(1, Arc::clone(&original[&0]));
    let mut bytes = payload(2, 0);
    bytes[7] = 0;
    bytes[9] = 2;
    let ordinary = decode(&bytes, &ids, None);
    let cached = decode(&bytes, &ids, Some(&moved));
    assert!(
        cached
            .sub_chunk(0)
            .unwrap()
            .blocks_match(&ordinary.sub_chunk(0).unwrap())
    );
    assert_eq!(
        cached.block_bytes_consumed(),
        ordinary.block_bytes_consumed()
    );
    assert_eq!(cached.decode_work().reused_sections, 1);
    assert!(cached.sub_chunk(1).is_none());
}

/// A truncated palette varint must not turn into a hash hit after its missing byte arrives.
#[test]
fn appended_bytes_complete_truncated_sections_instead_of_reusing_them() {
    let ids = RawBlockIds { air: 0 };
    let first = decode(&[9, 1, 0, 1, 0x80], &ids, None);
    let resent = decode(&[9, 1, 0, 1, 0x80, 2], &ids, Some(&first.sub_chunks));
    assert_eq!(resent.decode_work().reused_sections, 0);
    assert_eq!(resent.sub_chunks[&0].runtime_id(0, 0, 0, 0), Some(128));
}

/// Cached reads retain the ordinary decoder's lenient cursor and section results across versions.
#[test]
fn cached_decode_matches_ordinary_reads_for_malformed_and_extended_payloads() {
    let ids = RawBlockIds { air: 0 };
    for version in 0..=12 {
        for header in [0, 1, 2, 3, 12, 32, 0xff] {
            for length in 0..12 {
                let mut bytes = vec![version, 2, 0, header];
                bytes.extend(std::iter::repeat_n(0x80, length));
                let first = decode(&bytes, &ids, None);
                bytes.extend([0, 9, 1, 1, 1, 4, 1, 0, 0xff, 0]);
                let ordinary = decode(&bytes, &ids, None);
                let cached = decode(&bytes, &ids, Some(&first.sub_chunks));
                assert_eq!(
                    cached.sub_chunks, ordinary.sub_chunks,
                    "version={version} header={header} length={length}"
                );
                assert_eq!(
                    cached.block_bytes_consumed(),
                    ordinary.block_bytes_consumed()
                );
                assert_eq!(cached.biomes, ordinary.biomes);
                assert_eq!(cached.block_entities, ordinary.block_entities);
            }
        }
    }
}
