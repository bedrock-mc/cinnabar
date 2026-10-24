//! Plans complete frame runs using layer room and the shared upload budget.

use super::super::{Builder, MIN_TILE};
use std::cmp::Reverse;

/// One contiguous portion of a run and the cost of adding it to its shard.
#[derive(Clone, Copy)]
pub(super) struct FramePlacement {
    pub(super) shard: usize,
    pub(super) tile: u32,
    pub(super) layer: u32,
    pub(super) count: usize,
    pub(super) upload_bytes: usize,
}

/// Available room and placement preference for one candidate shard.
struct Candidate {
    placement: FramePlacement,
    capacity: usize,
    priority: u8,
}

impl Builder<'_> {
    /// Reserves every base page before counting available overlay arrays.
    pub(in super::super) fn max_shards(&self) -> usize {
        assets::MAX_TEXTURE_PAGES.saturating_sub(self.overlay.base_texture_pages as usize)
    }

    /// Counts free layers across the device arrays, capped by the overlay format capacity.
    pub(super) fn remaining_layers(&self) -> usize {
        self.max_layers
            .saturating_mul(self.max_shards())
            .saturating_sub(self.sources.len())
    }

    /// Keeps a complete run together when possible, then splits only for real layer or byte limits.
    #[cfg(test)]
    pub(super) fn place_frames(&self, own: u32, frames: usize) -> Vec<FramePlacement> {
        self.place_frames_with_tiles(own, frames, &self.tiles)
    }

    /// Plans against reduced page sizes without changing any retained source or reference.
    pub(super) fn place_frames_with_tiles(
        &self,
        own: u32,
        frames: usize,
        page_tiles: &[u32],
    ) -> Vec<FramePlacement> {
        let mut page_counts = [0; assets::MAX_TEXTURE_PAGES];
        let counts = &mut page_counts[..self.max_shards()];
        for (shard, layers) in counts.iter_mut().enumerate() {
            *layers = self.shard_len(shard);
        }
        let mut tiles = page_tiles.to_vec();
        let mut upload_bytes = counts
            .iter()
            .enumerate()
            .map(|(shard, &layers)| {
                self.texture_policy
                    .terrain_bytes(tiles.get(shard).copied().unwrap_or(MIN_TILE), layers)
                    .unwrap_or(usize::MAX)
            })
            .fold(0usize, usize::saturating_add);
        let mut layer_room = self.remaining_layers();
        let mut remaining = frames;
        let mut placements = Vec::new();
        while remaining > 0 {
            let next = (tiles
                .len()
                .max(usize::from(counts.first().is_some_and(|&count| count > 0)))
                ..self.max_shards())
                .find(|&shard| counts[shard] < self.max_layers);
            let mut candidates = Vec::new();
            for (shard, &layers) in counts.iter().enumerate() {
                if layers >= self.max_layers
                    || (shard >= tiles.len() && layers == 0 && Some(shard) != next)
                {
                    continue;
                }
                let old_tile = tiles.get(shard).copied().unwrap_or(MIN_TILE);
                let tile = old_tile.max(own);
                let Some(old_cost) = self.texture_policy.terrain_bytes(old_tile, layers) else {
                    continue;
                };
                let Some(new_cost) = self.texture_policy.terrain_bytes(tile, layers) else {
                    continue;
                };
                let Some(layer_bytes) = self.texture_policy.terrain_bytes(tile, 1) else {
                    continue;
                };
                let recost = new_cost.saturating_sub(old_cost);
                let capacity = (self.max_layers - layers).min(layer_room).min(
                    self.texture_policy
                        .storage_bytes()
                        .saturating_sub(upload_bytes)
                        .saturating_sub(recost)
                        / layer_bytes,
                );
                if capacity == 0 {
                    continue;
                }
                let count = remaining.min(capacity);
                candidates.push(Candidate {
                    placement: FramePlacement {
                        shard,
                        tile,
                        layer: layers as u32,
                        count,
                        upload_bytes: recost + count * layer_bytes,
                    },
                    capacity,
                    priority: if (tiles.is_empty() && shard == 0)
                        || (shard < tiles.len() && old_tile >= own)
                    {
                        0
                    } else if shard >= tiles.len() {
                        1
                    } else {
                        2
                    },
                });
            }
            let selected = candidates
                .iter()
                .filter(|candidate| candidate.capacity >= remaining)
                .min_by_key(|candidate| {
                    (
                        candidate.priority,
                        candidate.placement.upload_bytes,
                        candidate.placement.shard,
                    )
                })
                .or_else(|| {
                    candidates.iter().min_by_key(|candidate| {
                        (
                            Reverse(candidate.capacity),
                            candidate.priority,
                            candidate.placement.upload_bytes,
                            candidate.placement.shard,
                        )
                    })
                });
            let Some(selected) = selected else {
                break;
            };
            let placement = selected.placement;
            tiles.resize(tiles.len().max(placement.shard + 1), MIN_TILE);
            tiles[placement.shard] = placement.tile;
            counts[placement.shard] += placement.count;
            upload_bytes += placement.upload_bytes;
            layer_room -= placement.count;
            remaining -= placement.count;
            placements.push(placement);
        }
        placements
    }
}
