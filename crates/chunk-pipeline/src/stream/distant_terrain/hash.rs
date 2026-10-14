//! The 64-bit content hash of a chunk column that lets ingestion skip unchanged resends.
//!
//! It covers what a column is made of, its blocks and biomes, plus which of its sub-chunks
//! had solved light, so a column first sampled with the fallback light is sampled again
//! once its light is solved. Light values are left out: they depend on which neighbours
//! were loaded, so hashing them would sample unchanged columns again on every visit.

use lod::{classify::BlockClassifier, sampler::ColumnSnapshot};
use world::{PalettedStorage, SubChunkKey};
use xxhash_rust::xxh64::{Xxh64, xxh64};

/// Seeds the hash; changing how columns are sampled or hashed bumps it, so every stored
/// hash stops matching and each column is sampled again.
const CONTENT_HASH_VERSION: u64 = 3;

/// Hashes the blocks and biomes sampling a column reads: per stored sub-chunk its Y, each
/// block layer's packed indices with its palette as store materials, and the biome storage
/// at that Y. Palette values go through `classifier`, so equal blocks under different
/// session ids hash alike and blocks the assets resolve differently hash apart. Equal
/// content may still hash apart when the server packs it differently, which only costs a
/// resample.
pub(in crate::stream) fn block_hash(
    snapshot: &ColumnSnapshot<'_>,
    classifier: &mut impl BlockClassifier,
) -> u64 {
    let mut hasher = Xxh64::new(CONTENT_HASH_VERSION);
    let mut bytes = Vec::new();
    for (&y, sub_chunk) in snapshot.sub_chunks {
        bytes.clear();
        bytes.extend_from_slice(&y.to_le_bytes());
        let storages = sub_chunk.storages();
        bytes.extend_from_slice(&(storages.len() as u32).to_le_bytes());
        for storage in storages {
            let materials = storage
                .palette()
                .values()
                .iter()
                .map(|&value| classifier.classify(value).normalized().material.raw());
            append_storage(&mut bytes, storage, materials);
        }
        match snapshot.biomes.biome_storage(y) {
            Some(biomes) => {
                bytes.push(1);
                bytes.push(biomes.bits_per_index());
                let palette = biomes.palette().values();
                bytes.extend_from_slice(&(palette.len() as u32).to_le_bytes());
                for &biome in palette {
                    bytes.extend_from_slice(&biome.to_le_bytes());
                }
                for &word in biomes.packed_words() {
                    bytes.extend_from_slice(&word.to_le_bytes());
                }
            }
            None => bytes.push(0),
        }
        hasher.update(&bytes);
    }
    hasher.digest()
}

/// The content hash of a column whose blocks and biomes hash to `blocks`
/// ([`block_hash`]): it adds the Y of every sub-chunk of `snapshot` with solved light.
pub(in crate::stream) fn content_hash(blocks: u64, snapshot: &ColumnSnapshot<'_>) -> u64 {
    let mut lit = Vec::with_capacity(snapshot.sub_chunks.len() * 4);
    for &y in snapshot.sub_chunks.keys() {
        let key = SubChunkKey::from_chunk(snapshot.chunk, y);
        if snapshot
            .light
            .is_some_and(|light| light.light(key).is_some())
        {
            lit.extend_from_slice(&y.to_le_bytes());
        }
    }
    xxh64(&lit, blocks)
}

/// Appends one block layer: its index width, its palette as `materials` and its packed
/// words.
fn append_storage(
    bytes: &mut Vec<u8>,
    storage: &PalettedStorage,
    materials: impl ExactSizeIterator<Item = u32>,
) {
    bytes.push(storage.bits_per_index());
    bytes.extend_from_slice(&(materials.len() as u32).to_le_bytes());
    for material in materials {
        bytes.extend_from_slice(&material.to_le_bytes());
    }
    for &word in storage.packed_words() {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
}
