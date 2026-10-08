//! CPU-side particle texture atlas: static particle textures shelf-packed once, plus a
//! bounded dynamic region for per-block terrain tiles uploaded on demand.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use assets::RuntimeParticleAssets;

pub const ATLAS_SIDE: u32 = 1024;
/// Rows below this belong to the dynamic tile region.
const STATIC_ROWS: u32 = 512;
pub const TILE_SLOT: u32 = 16;
const SLOTS_PER_ROW: u32 = ATLAS_SIDE / TILE_SLOT;
const SLOT_ROWS: u32 = (ATLAS_SIDE - STATIC_ROWS) / TILE_SLOT;
pub(super) const MAX_SLOTS: usize = (SLOTS_PER_ROW * SLOT_ROWS) as usize;

/// A texture's pixel rectangle in the atlas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Placement {
    /// Normalized `(u, v, u_size, v_size)`.
    #[must_use]
    pub fn normalized(self) -> [f32; 4] {
        let side = ATLAS_SIDE as f32;
        [
            self.x as f32 / side,
            self.y as f32 / side,
            self.width as f32 / side,
            self.height as f32 / side,
        ]
    }
}

/// One uploaded tile rectangle; the newest patch per slot supersedes older ones.
#[derive(Clone, Debug)]
pub struct AtlasPatch {
    pub seq: u64,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub rgba8: Arc<[u8]>,
}

pub struct ParticleAtlas {
    pixels: Vec<u8>,
    base: Option<Arc<[u8]>>,
    patches: HashMap<usize, AtlasPatch>,
    seq: u64,
    placements: HashMap<Box<str>, Placement>,
    tiles: HashMap<u64, (usize, u64)>,
    pinned: HashSet<usize>,
    next_slot: usize,
    tick: u64,
}

impl Default for ParticleAtlas {
    fn default() -> Self {
        Self {
            pixels: vec![0; (ATLAS_SIDE * ATLAS_SIDE * 4) as usize],
            base: None,
            patches: HashMap::new(),
            seq: 0,
            placements: HashMap::new(),
            tiles: HashMap::new(),
            pinned: HashSet::new(),
            next_slot: 0,
            tick: 0,
        }
    }
}

impl ParticleAtlas {
    /// Packs every texture in the carrier; oversized ones that no longer fit are skipped.
    #[must_use]
    pub fn from_assets(assets: &RuntimeParticleAssets) -> Self {
        let mut atlas = Self::default();
        // A white texel at the origin backs untextured or missing-texture particles.
        atlas.blit(0, 0, 1, 1, &[255, 255, 255, 255]);
        atlas.placements.insert(
            "".into(),
            Placement {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
        );
        let mut order: Vec<_> = assets.textures().iter().collect();
        order.sort_by(|a, b| (b.height, b.width, &a.path).cmp(&(a.height, a.width, &b.path)));
        let (mut x, mut y, mut shelf) = (2u32, 0u32, 1u32);
        for texture in order {
            let (w, h) = (texture.width, texture.height);
            if x + w + 1 > ATLAS_SIDE {
                x = 0;
                y += shelf + 1;
                shelf = 0;
            }
            if y + h > STATIC_ROWS || w > ATLAS_SIDE {
                continue;
            }
            atlas.blit(x, y, w, h, &texture.rgba8);
            atlas.placements.insert(
                texture.path.clone(),
                Placement {
                    x,
                    y,
                    width: w,
                    height: h,
                },
            );
            x += w + 1;
            shelf = shelf.max(h);
        }
        atlas.base = Some(Arc::from(atlas.pixels.as_slice()));
        atlas
    }

    /// The static-region snapshot uploaded once; the dynamic region starts empty.
    #[must_use]
    pub fn base(&mut self) -> Arc<[u8]> {
        Arc::clone(
            self.base
                .get_or_insert_with(|| Arc::from(vec![0u8; self.pixels.len()])),
        )
    }

    /// Bumps whenever a dynamic tile is written.
    #[must_use]
    pub fn patch_seq(&self) -> u64 {
        self.seq
    }

    /// Current tile patches, oldest first.
    #[must_use]
    pub fn patches(&self) -> Vec<AtlasPatch> {
        let mut list: Vec<_> = self.patches.values().cloned().collect();
        list.sort_by_key(|patch| patch.seq);
        list
    }

    #[must_use]
    pub fn placement(&self, path: &str) -> Option<Placement> {
        self.placements.get(path).copied()
    }

    /// The white-texel placement used when a texture is unavailable.
    #[must_use]
    pub fn fallback(&self) -> Placement {
        Placement {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        }
    }

    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Prevents recycling slots retained by emitters or their live particles.
    pub(super) fn set_live_placements(&mut self, placements: impl Iterator<Item = Placement>) {
        self.pinned.clear();
        for placement in placements {
            if placement.y >= STATIC_ROWS
                && placement.width == TILE_SLOT
                && placement.height == TILE_SLOT
            {
                let slot = ((placement.y - STATIC_ROWS) / TILE_SLOT * SLOTS_PER_ROW
                    + placement.x / TILE_SLOT) as usize;
                self.pinned.insert(slot);
            }
        }
    }

    /// Returns the slot for a caller-keyed tile, uploading `pixels` (`size * size * 4`,
    /// nearest-resampled to the slot) on first use and recycling the least recent slot when full.
    pub fn tile(&mut self, key: u64, size: u32, pixels: &[u8]) -> Option<Placement> {
        let source_side = size as usize;
        let bytes = source_side.checked_mul(source_side)?.checked_mul(4)?;
        if size == 0 || pixels.len() != bytes {
            return None;
        }
        self.tick += 1;
        let slot = if let Some(entry) = self.tiles.get_mut(&key) {
            entry.1 = self.tick;
            entry.0
        } else {
            let slot = if self.next_slot < MAX_SLOTS {
                self.next_slot += 1;
                self.next_slot - 1
            } else {
                let (&victim, &(slot, _)) = self
                    .tiles
                    .iter()
                    .filter(|(_, (slot, _))| !self.pinned.contains(slot))
                    .min_by_key(|(_, (_, used))| *used)?;
                self.tiles.remove(&victim);
                slot
            };
            let placement = self.slot_placement(slot);
            let mut resampled = vec![0u8; (TILE_SLOT * TILE_SLOT * 4) as usize];
            for ty in 0..TILE_SLOT {
                for tx in 0..TILE_SLOT {
                    let sx = tx as usize * source_side / TILE_SLOT as usize;
                    let sy = ty as usize * source_side / TILE_SLOT as usize;
                    let from = (sy * source_side + sx) * 4;
                    let to = ((ty * TILE_SLOT + tx) * 4) as usize;
                    resampled[to..to + 4].copy_from_slice(&pixels[from..from + 4]);
                }
            }
            self.blit(placement.x, placement.y, TILE_SLOT, TILE_SLOT, &resampled);
            self.seq += 1;
            self.patches.insert(
                slot,
                AtlasPatch {
                    seq: self.seq,
                    x: placement.x,
                    y: placement.y,
                    width: TILE_SLOT,
                    height: TILE_SLOT,
                    rgba8: Arc::from(resampled),
                },
            );
            self.tiles.insert(key, (slot, self.tick));
            slot
        };
        Some(self.slot_placement(slot))
    }

    fn slot_placement(&self, slot: usize) -> Placement {
        let slot = slot as u32;
        Placement {
            x: (slot % SLOTS_PER_ROW) * TILE_SLOT,
            y: STATIC_ROWS + (slot / SLOTS_PER_ROW) * TILE_SLOT,
            width: TILE_SLOT,
            height: TILE_SLOT,
        }
    }

    fn blit(&mut self, x: u32, y: u32, w: u32, h: u32, rgba: &[u8]) {
        for row in 0..h {
            let dest = (((y + row) * ATLAS_SIDE + x) * 4) as usize;
            let src = (row * w * 4) as usize;
            self.pixels[dest..dest + (w * 4) as usize]
                .copy_from_slice(&rgba[src..src + (w * 4) as usize]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_are_cached_by_key_and_recycled_when_full() {
        let mut atlas = ParticleAtlas::default();
        let pixels = vec![9u8; 16 * 16 * 4];
        let first = atlas.tile(1, 16, &pixels).unwrap();
        let generation = atlas.patch_seq();
        assert_eq!(atlas.tile(1, 16, &pixels), Some(first));
        assert_eq!(atlas.patch_seq(), generation);
        for key in 2..=(MAX_SLOTS as u64 + 1) {
            assert!(atlas.tile(key, 16, &pixels).is_some());
        }
        assert_eq!(atlas.tiles.len(), MAX_SLOTS);
    }

    #[test]
    fn patches_keep_only_the_newest_write_per_slot() {
        let mut atlas = ParticleAtlas::default();
        let pixels = vec![1u8; 16 * 16 * 4];
        atlas.tile(1, 16, &pixels).unwrap();
        atlas.tile(2, 16, &pixels).unwrap();
        atlas.tile(1, 16, &pixels).unwrap();
        let patches = atlas.patches();
        assert_eq!(patches.len(), 2);
        assert!(patches[0].seq < patches[1].seq);
        assert_eq!(atlas.patch_seq(), 2);
    }

    #[test]
    fn rejects_mismatched_tile_buffers() {
        let mut atlas = ParticleAtlas::default();
        assert!(atlas.tile(1, 16, &[0; 12]).is_none());
        assert!(atlas.tile(1, 0, &[]).is_none());
    }

    #[test]
    fn larger_tiles_resample_to_the_slot() {
        let mut atlas = ParticleAtlas::default();
        let mut pixels = vec![0u8; 32 * 32 * 4];
        pixels[..4].copy_from_slice(&[1, 2, 3, 4]);
        let placement = atlas.tile(7, 32, &pixels).unwrap();
        let at = ((placement.y * ATLAS_SIDE + placement.x) * 4) as usize;
        assert_eq!(&atlas.pixels()[at..at + 4], &[1, 2, 3, 4]);
    }
    #[test]
    fn review_render_particle_tile_rejects_overflowing_dimensions() {
        assert!(ParticleAtlas::default().tile(1, 65536, &[]).is_none());
    }
}
