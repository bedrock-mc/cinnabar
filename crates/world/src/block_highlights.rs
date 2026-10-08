//! Bounded, read-only discovery of selected blocks in loaded terrain.
use crate::{BLOCKS_PER_SUB_CHUNK, ChunkStore, SUB_CHUNK_SIDE, SubChunk, SubChunkKey};
use std::{
    collections::{BinaryHeap, HashMap},
    sync::{Arc, Weak},
};

const PROBES_PER_UPDATE: usize = 128;
const SCANS_PER_UPDATE: usize = 8;
const CANDIDATES_PER_UPDATE: usize = 32_768;

struct Cached {
    source: Weak<SubChunk>,
    indices: Vec<u16>,
}

/// Incremental palette-aware cache. No chunk requests or world mutations are performed.
#[derive(Default)]
pub struct BlockHighlightScan {
    ids: Vec<u32>,
    lookup_ids: Vec<u32>,
    scope: Option<(i32, [i32; 3], u32)>,
    keys: Vec<SubChunkKey>,
    cursor: usize,
    cached: HashMap<SubChunkKey, Cached>,
    nearest: BinaryHeap<Candidate>,
    output: Vec<[i32; 3]>,
    previous_position: Option<[f32; 3]>,
    previous_limit: usize,
    scans: u64,
}

#[derive(Clone, Copy, Debug)]
struct Candidate {
    distance: f32,
    position: [i32; 3],
}
impl PartialEq for Candidate {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}
impl Eq for Candidate {}
impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.distance
            .total_cmp(&other.distance)
            .then(self.position.cmp(&other.position))
    }
}

impl BlockHighlightScan {
    /// Drops cached terrain and visible results when the owning session ends.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Reuses unchanged input; scans at most eight packed subchunks per call.
    /// Callers must bound range before querying this loaded-terrain cache.
    pub fn update(
        &mut self,
        store: &ChunkStore,
        dimension: i32,
        position: [f32; 3],
        range: f32,
        ids: &[u32],
        limit: usize,
    ) -> &[[i32; 3]] {
        if !position.iter().all(|v| v.is_finite())
            || !range.is_finite()
            || range < 1.0
            || ids.is_empty()
            || limit == 0
        {
            self.clear();
            return &self.output;
        }
        let side = SUB_CHUNK_SIDE as i32;
        let cell = position.map(|v| (v.floor() as i32).div_euclid(side));
        let radius = (range / side as f32).ceil() as i32;
        let scope = (dimension, cell, range.to_bits());
        let mut changed = false;
        if self.ids != ids {
            self.ids.clear();
            self.ids.extend_from_slice(ids);
            self.lookup_ids.clone_from(&self.ids);
            self.lookup_ids.sort_unstable();
            self.lookup_ids.dedup();
            self.cached.clear();
            changed = true;
        }
        if self.scope != Some(scope) {
            self.scope = Some(scope);
            self.keys.clear();
            self.cursor = 0;
            for x in -radius..=radius {
                for y in -radius..=radius {
                    for z in -radius..=radius {
                        let Some([x, y, z]) = cell[0]
                            .checked_add(x)
                            .zip(cell[1].checked_add(y))
                            .zip(cell[2].checked_add(z))
                            .map(|((x, y), z)| [x, y, z])
                        else {
                            continue;
                        };
                        self.keys.push(SubChunkKey::new(dimension, x, y, z));
                    }
                }
            }
            self.keys.sort_unstable_by_key(|key| {
                let delta = [key.x - cell[0], key.y - cell[1], key.z - cell[2]];
                (
                    delta.iter().map(|v| i64::from(*v).pow(2)).sum::<i64>(),
                    *key,
                )
            });
            changed = true;
        }
        self.cached.retain(|key, cached| {
            let retained = key.dimension == dimension
                && [key.x - cell[0], key.y - cell[1], key.z - cell[2]]
                    .iter()
                    .all(|v| v.abs() <= radius)
                && cached
                    .source
                    .upgrade()
                    .zip(store.sub_chunk(*key))
                    .is_some_and(|(old, new)| Arc::ptr_eq(&old, &new));
            changed |= !retained;
            retained
        });
        let mut scanned = 0;
        for _ in 0..PROBES_PER_UPDATE.min(self.keys.len()) {
            let key = self.keys[self.cursor];
            self.cursor = (self.cursor + 1) % self.keys.len();
            if self.cached.contains_key(&key) {
                continue;
            }
            let Some(source) = store.sub_chunk(key) else {
                continue;
            };
            let matching = source.storages().first().is_some_and(|storage| {
                storage
                    .palette()
                    .values()
                    .iter()
                    .any(|id| self.lookup_ids.binary_search(id).is_ok())
            });
            let mut indices = Vec::new();
            if matching {
                if scanned == SCANS_PER_UPDATE {
                    self.cursor = (self.cursor + self.keys.len() - 1) % self.keys.len();
                    break;
                }
                scanned += 1;
                self.scans += 1;
                for index in 0..BLOCKS_PER_SUB_CHUNK {
                    let [x, y, z] = local_position(index as u16);
                    if source
                        .runtime_id(0, x, y, z)
                        .is_some_and(|id| self.lookup_ids.binary_search(&id).is_ok())
                    {
                        indices.push(index as u16);
                    }
                }
            }
            self.cached.insert(
                key,
                Cached {
                    source: Arc::downgrade(&source),
                    indices,
                },
            );
            changed = true;
        }
        if !changed && self.previous_position == Some(position) && self.previous_limit == limit {
            return &self.output;
        }
        self.previous_position = Some(position);
        self.previous_limit = limit;
        self.nearest.clear();
        let mut visited = 0;
        'keys: for key in &self.keys {
            let Some(cached) = self.cached.get(key) else {
                continue;
            };
            for index in &cached.indices {
                if visited == CANDIDATES_PER_UPDATE {
                    break 'keys;
                }
                visited += 1;
                let local = local_position(*index);
                let point = [key.x, key.y, key.z].map(|v| i64::from(v) * i64::from(side));
                let Some(point) = point[0]
                    .checked_add(i64::from(local[0]))
                    .zip(point[1].checked_add(i64::from(local[1])))
                    .zip(point[2].checked_add(i64::from(local[2])))
                    .and_then(|((x, y), z)| {
                        Some([
                            i32::try_from(x).ok()?,
                            i32::try_from(y).ok()?,
                            i32::try_from(z).ok()?,
                        ])
                    })
                else {
                    continue;
                };
                let distance = point
                    .iter()
                    .zip(position)
                    .map(|(v, p)| (*v as f32 + 0.5 - p).powi(2))
                    .sum::<f32>();
                if distance > range * range {
                    continue;
                }
                let candidate = Candidate {
                    position: point,
                    distance,
                };
                if self.nearest.len() < limit {
                    self.nearest.push(candidate);
                } else if self.nearest.peek().is_some_and(|worst| candidate < *worst) {
                    self.nearest.pop();
                    self.nearest.push(candidate);
                }
            }
        }
        self.output.clear();
        self.output
            .extend(self.nearest.iter().map(|candidate| candidate.position));
        self.output.sort_unstable();
        &self.output
    }
}

fn local_position(index: u16) -> [u8; 3] {
    let side = SUB_CHUNK_SIDE as u16;
    [
        (index / (side * side)) as u8,
        (index % side) as u8,
        ((index / side) % side) as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlockUpdate, RawBlockIds};
    fn fixture(id: u8) -> (ChunkStore, SubChunkKey) {
        let mut store = ChunkStore::new();
        let key = SubChunkKey::new(0, -1, -1, -1);
        store
            .apply_sub_chunk(key, &[8, 1, 1, id * 2], &RawBlockIds { air: 0 })
            .unwrap();
        store.mark_sub_chunk_loaded(key).unwrap();
        (store, key)
    }
    fn complete(scan: &mut BlockHighlightScan, store: &ChunkStore) {
        for _ in 0..30 {
            scan.update(store, 0, [-1.0, -1.0, -1.0], 24.0, &[7], 1024);
        }
    }
    #[test]
    fn palette_miss_is_cached_without_block_scans() {
        let (store, _) = fixture(2);
        let mut scan = BlockHighlightScan::default();
        complete(&mut scan, &store);
        assert_eq!(scan.scans, 0);
        assert!(scan.output.is_empty());
        assert_eq!(scan.cached.len(), 1);
    }

    #[test]
    fn received_sparse_terrain_is_discovered_before_collision_completeness() {
        let mut store = ChunkStore::new();
        let key = SubChunkKey::new(0, -1, -1, -1);
        store
            .apply_sub_chunk(key, &[8, 1, 1, 4], &RawBlockIds { air: 0 })
            .unwrap();
        store
            .update_sub_chunk_blocks(key, &[BlockUpdate::new(15, 15, 15, 0, 7)], 0)
            .unwrap();
        assert!(!store.is_sub_chunk_loaded(key));
        assert!(store.sub_chunk(key).is_some());
        let mut scan = BlockHighlightScan::default();
        complete(&mut scan, &store);
        assert_eq!(scan.output, vec![[-1, -1, -1]]);
    }
    #[test]
    fn mutation_negative_coordinates_and_unload() {
        let (mut store, key) = fixture(2);
        store
            .update_sub_chunk_blocks(key, &[BlockUpdate::new(15, 15, 15, 0, 7)], 0)
            .unwrap();
        let mut scan = BlockHighlightScan::default();
        complete(&mut scan, &store);
        assert_eq!(scan.output, vec![[-1, -1, -1]]);
        let scans = scan.scans;
        complete(&mut scan, &store);
        assert_eq!(scan.scans, scans);
        store
            .update_sub_chunk_blocks(key, &[BlockUpdate::new(15, 15, 15, 0, 2)], 0)
            .unwrap();
        complete(&mut scan, &store);
        assert!(scan.output.is_empty());
        store.detach_chunks(&std::collections::BTreeSet::from([key.chunk()]));
        complete(&mut scan, &store);
        assert!(scan.cached.is_empty());
        assert!(scan.output.is_empty());
    }
    #[test]
    fn nearest_limit_range_and_dimension_clear() {
        let (store, _) = fixture(7);
        let mut scan = BlockHighlightScan::default();
        for _ in 0..30 {
            scan.update(&store, 0, [-0.5, -0.5, -0.5], 24.0, &[7], 2);
        }
        assert_eq!(scan.output.len(), 2);
        assert!(scan.output.contains(&[-1, -1, -1]));
        scan.update(&store, 1, [-0.5, -0.5, -0.5], 24.0, &[7], 2);
        assert!(scan.output.is_empty());
    }

    #[test]
    fn dense_loaded_terrain_is_incremental_and_output_is_bounded() {
        let mut store = ChunkStore::new();
        for x in -1..=1 {
            for y in -1..=1 {
                for z in -1..=1 {
                    let key = SubChunkKey::new(0, x, y, z);
                    store
                        .apply_sub_chunk(key, &[8, 1, 1, 14], &RawBlockIds { air: 0 })
                        .unwrap();
                    store.mark_sub_chunk_loaded(key).unwrap();
                }
            }
        }
        let mut scan = BlockHighlightScan::default();
        let points = scan.update(&store, 0, [0.0; 3], 64.0, &[7], 1024);
        assert_eq!(points.len(), 1024);
        assert_eq!(scan.scans, SCANS_PER_UPDATE as u64);
        let before = scan.scans;
        scan.update(&store, 0, [0.0; 3], 64.0, &[7], 1024);
        assert!(scan.scans - before <= SCANS_PER_UPDATE as u64);
    }
}
