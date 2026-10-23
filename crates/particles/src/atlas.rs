//! CPU-side particle texture atlas: static particle textures shelf-packed once, plus a
//! bounded dynamic region for per-block terrain tiles uploaded on demand.

use std::{collections::HashMap, sync::Arc};

use assets::RuntimeParticleAssets;

/// Default atlas dimension; pack preparation may grow it within the device policy.
pub const ATLAS_SIDE: u32 = 1024;
/// Dynamic terrain slots retain the same row budget when the static atlas grows.
const DYNAMIC_ROWS: u32 = ATLAS_SIDE / 2;
#[cfg(test)]
const STATIC_ROWS: u32 = ATLAS_SIDE - DYNAMIC_ROWS;
pub const TILE_SLOT: u32 = 16;
/// Opaque white texel at the atlas origin, sampled by `ParticleAtlas::fallback`.
pub const FALLBACK_TEXEL: [u8; 4] = [255; 4];
#[cfg(test)]
const SLOTS_PER_ROW: u32 = ATLAS_SIDE / TILE_SLOT;
#[cfg(test)]
const SLOT_ROWS: u32 = (ATLAS_SIDE - STATIC_ROWS) / TILE_SLOT;
#[cfg(test)]
pub(super) const MAX_SLOTS: usize = (SLOTS_PER_ROW * SLOT_ROWS) as usize;

/// A texture's pixel rectangle in the atlas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    atlas_side: u32,
}

impl Placement {
    /// Normalized `(u, v, u_size, v_size)`.
    #[must_use]
    pub fn normalized(self) -> [f32; 4] {
        let side = self.atlas_side as f32;
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
    side: u32,
    pixels: Vec<u8>,
    base: Option<Arc<[u8]>>,
    patches: HashMap<usize, AtlasPatch>,
    seq: u64,
    placements: HashMap<Box<str>, Placement>,
    tiles: HashMap<u64, (Placement, u64)>,
    occupied: Vec<Option<u64>>,
    pinned: HashMap<usize, usize>,
    next_slot: usize,
    tick: u64,
}

impl Default for ParticleAtlas {
    fn default() -> Self {
        Self {
            side: ATLAS_SIDE,
            pixels: vec![0; (ATLAS_SIDE * ATLAS_SIDE * 4) as usize],
            base: None,
            patches: HashMap::new(),
            seq: 0,
            placements: HashMap::new(),
            tiles: HashMap::new(),
            occupied: Vec::new(),
            pinned: HashMap::new(),
            next_slot: 0,
            tick: 0,
        }
    }
}

impl ParticleAtlas {
    /// Packs every texture in the carrier; oversized ones that no longer fit are skipped.
    #[must_use]
    pub fn from_assets(assets: &RuntimeParticleAssets) -> Self {
        let order = ordered_textures(assets.textures());
        let policy = assets::texture::policy::device_policy();
        let max_side = policy.max_side();
        let mut side = ATLAS_SIDE;
        while !static_textures_fit(&order, side) {
            let next = side * 2;
            if next > max_side || policy.page_bytes(next, next).is_none() {
                break;
            }
            side = next;
        }
        let mut atlas = Self {
            side,
            pixels: vec![0; (side * side * 4) as usize],
            ..Self::default()
        };
        // A white texel at the origin backs untextured or missing-texture particles.
        atlas.blit(0, 0, 1, 1, &FALLBACK_TEXEL);
        atlas.placements.insert(
            "".into(),
            Placement {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
                atlas_side: side,
            },
        );
        let (mut x, mut y, mut shelf) = (2u32, 0u32, 1u32);
        for texture in order {
            let (w, h) = (texture.width, texture.height);
            if x + w + 1 > side {
                x = 0;
                y += shelf + 1;
                shelf = 0;
            }
            if y + h > atlas.static_rows() || w > side {
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
                    atlas_side: side,
                },
            );
            x += w + 1;
            shelf = shelf.max(h);
        }
        atlas.base = Some(Arc::from(atlas.pixels.as_slice()));
        atlas
    }

    /// Physical atlas dimension used by placements and the GPU upload.
    #[must_use]
    pub fn side(&self) -> u32 {
        self.side
    }

    /// Static sources end before the reserved terrain tile rows.
    fn static_rows(&self) -> u32 {
        self.side - DYNAMIC_ROWS
    }

    /// Dynamic slot count derived from the current atlas width and reserved rows.
    fn max_slots(&self) -> usize {
        ((self.side / TILE_SLOT) * (DYNAMIC_ROWS / TILE_SLOT)) as usize
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
            atlas_side: self.side,
        }
    }

    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Returns the dynamic slot covered by a tile placement; static textures have no pin.
    fn dynamic_slot(&self, placement: Placement) -> Option<usize> {
        (placement.y >= self.static_rows()
            && placement.x.is_multiple_of(TILE_SLOT)
            && (placement.y - self.static_rows()).is_multiple_of(TILE_SLOT)
            && placement.width.is_multiple_of(TILE_SLOT)
            && placement.height.is_multiple_of(TILE_SLOT))
        .then(|| {
            ((placement.y - self.static_rows()) / TILE_SLOT * (self.side / TILE_SLOT)
                + placement.x / TILE_SLOT) as usize
        })
    }

    /// Protects one emitter's placement, including owners sharing the same slot.
    pub(super) fn retain_placement(&mut self, placement: Placement) {
        if let Some(slot) = self.dynamic_slot(placement) {
            *self.pinned.entry(slot).or_default() += 1;
        }
    }

    /// Releases exactly one owner; a slot stays protected until its last owner leaves.
    pub(super) fn release_placement(&mut self, placement: Placement) {
        if let Some(slot) = self.dynamic_slot(placement) {
            let count = self.pinned.get_mut(&slot).expect("placement has an owner");
            *count -= 1;
            if *count == 0 {
                self.pinned.remove(&slot);
            }
        }
    }

    /// Releases all owners on disconnect while retaining the reusable tile cache.
    pub(super) fn clear_placements(&mut self) {
        self.pinned.clear();
    }

    /// Reports owners for comparison against an independent emitter walk.
    #[cfg(test)]
    pub(super) fn placement_references(&self, placement: Placement) -> usize {
        self.dynamic_slot(placement)
            .and_then(|slot| self.pinned.get(&slot).copied())
            .unwrap_or(0)
    }

    /// Returns the slot for a caller-keyed tile, uploading `pixels` (`size * size * 4`,
    /// nearest-resampled to the slot) on first use and recycling the least recent slot when full.
    pub fn tile(&mut self, key: u64, size: u32, pixels: &[u8]) -> Option<Placement> {
        let source_side = size as usize;
        if size == 0 || pixels.len() != source_side.checked_mul(source_side)?.checked_mul(4)? {
            return None;
        }
        if let Some((placement, used)) = self.tiles.get_mut(&key) {
            if placement.width != TILE_SLOT {
                return None;
            }
            self.tick = self.tick.wrapping_add(1);
            *used = self.tick;
            return Some(*placement);
        }
        let mut resampled = vec![0; (TILE_SLOT * TILE_SLOT * 4) as usize];
        for y in 0..TILE_SLOT as usize {
            for x in 0..TILE_SLOT as usize {
                let from = ((y * source_side / TILE_SLOT as usize) * source_side
                    + x * source_side / TILE_SLOT as usize)
                    * 4;
                let to = (y * TILE_SLOT as usize + x) * 4;
                resampled[to..to + 4].copy_from_slice(&pixels[from..from + 4]);
            }
        }
        self.sprite(key, TILE_SLOT, &resampled)
    }

    /// Retains original cosmetic flipbook texels in a contiguous square dynamic region.
    /// Live emitters pin the whole region; terrain tiles keep their historical 16-texel size.
    pub fn sprite(&mut self, key: u64, side: u32, pixels: &[u8]) -> Option<Placement> {
        if ![16, 32, 64, 128].contains(&side) || pixels.len() != (side * side * 4) as usize {
            return None;
        }
        self.tick = self.tick.wrapping_add(1);
        if let Some((placement, used)) = self.tiles.get_mut(&key) {
            if placement.width != side {
                return None;
            }
            *used = self.tick;
            return Some(*placement);
        }
        self.occupied.resize(self.max_slots(), None);
        let cells = side / TILE_SLOT;
        let columns = self.side / TILE_SLOT;
        let rows = DYNAMIC_ROWS / TILE_SLOT;
        let slot = if cells == 1 {
            let free = (self.next_slot..self.max_slots())
                .chain(0..self.next_slot)
                .find(|&slot| self.occupied[slot].is_none());
            if let Some(slot) = free {
                self.next_slot = (slot + 1) % self.max_slots();
                slot
            } else {
                let (placement, _) = self
                    .tiles
                    .values()
                    .filter(|(p, _)| !self.pinned.contains_key(&self.dynamic_slot(*p).unwrap()))
                    .min_by_key(|(_, used)| *used)?;
                self.dynamic_slot(*placement)?
            }
        } else {
            // Prefer free rectangles, then the rectangle whose newest evictable occupant is oldest.
            let mut best: Option<(u64, usize)> = None;
            for y in 0..=rows - cells {
                for x in 0..=columns - cells {
                    let mut newest = 0;
                    let mut blocked = false;
                    for dy in 0..cells {
                        for dx in 0..cells {
                            let at = ((y + dy) * columns + x + dx) as usize;
                            if let Some(owner) = self.occupied[at] {
                                let (p, age) = self.tiles[&owner];
                                blocked |= self.pinned.contains_key(&self.dynamic_slot(p).unwrap());
                                newest = newest.max(age);
                            }
                        }
                    }
                    if !blocked && best.is_none_or(|(age, _)| newest < age) {
                        best = Some((newest, (y * columns + x) as usize));
                    }
                }
            }
            best?.1
        };
        let mut placement = self.slot_placement(slot);
        placement.width = side;
        placement.height = side;
        let covered: Vec<_> = (0..cells)
            .flat_map(|y| (0..cells).map(move |x| slot + (y * columns + x) as usize))
            .collect();
        let victims: std::collections::HashSet<_> =
            covered.iter().filter_map(|&at| self.occupied[at]).collect();
        for victim in victims {
            let (old, _) = self.tiles.remove(&victim).unwrap();
            self.patches.remove(&self.dynamic_slot(old).unwrap());
            for cell in &mut self.occupied {
                if *cell == Some(victim) {
                    *cell = None;
                }
            }
        }
        for at in covered {
            self.occupied[at] = Some(key);
        }
        self.blit(placement.x, placement.y, side, side, pixels);
        self.seq += 1;
        self.patches.insert(
            slot,
            AtlasPatch {
                seq: self.seq,
                x: placement.x,
                y: placement.y,
                width: side,
                height: side,
                rgba8: Arc::from(pixels),
            },
        );
        self.tiles.insert(key, (placement, self.tick));
        Some(placement)
    }

    fn slot_placement(&self, slot: usize) -> Placement {
        let slot = slot as u32;
        let slots_per_row = self.side / TILE_SLOT;
        Placement {
            x: (slot % slots_per_row) * TILE_SLOT,
            y: self.static_rows() + (slot / slots_per_row) * TILE_SLOT,
            width: TILE_SLOT,
            height: TILE_SLOT,
            atlas_side: self.side,
        }
    }

    fn blit(&mut self, x: u32, y: u32, w: u32, h: u32, rgba: &[u8]) {
        for row in 0..h {
            let dest = (((y + row) * self.side + x) * 4) as usize;
            let src = (row * w * 4) as usize;
            self.pixels[dest..dest + (w * 4) as usize]
                .copy_from_slice(&rgba[src..src + (w * 4) as usize]);
        }
    }
}

/// Checks the complete static layout before optional artwork replaces retained sources.
pub fn particle_textures_fit<'a>(
    textures: impl IntoIterator<Item = &'a assets::ParticleTexture>,
    policy: assets::texture::policy::TexturePolicy,
) -> bool {
    let Some(exponent) = policy.max_side().checked_ilog2() else {
        return false;
    };
    let side = 1 << exponent;
    side >= ATLAS_SIDE
        && policy.page_bytes(side, side).is_some()
        && static_textures_fit(&ordered_textures(textures), side)
}

/// Keeps planning and actual packing in the same deterministic shelf order.
fn ordered_textures<'a>(
    textures: impl IntoIterator<Item = &'a assets::ParticleTexture>,
) -> Vec<&'a assets::ParticleTexture> {
    let mut order: Vec<_> = textures.into_iter().collect();
    order.sort_by(|a, b| (b.height, b.width, &a.path).cmp(&(a.height, a.width, &b.path)));
    order
}

/// Checks a shelf layout without allocating a larger atlas or copying source pixels.
fn static_textures_fit(textures: &[&assets::ParticleTexture], side: u32) -> bool {
    let (mut x, mut y, mut shelf) = (2u32, 0u32, 1u32);
    for texture in textures {
        let (width, height) = (texture.width, texture.height);
        if width == 0 || height == 0 || width > side || height > side - DYNAMIC_ROWS {
            return false;
        }
        if x + width + 1 > side {
            x = 0;
            y += shelf + 1;
            shelf = 0;
        }
        if y + height > side - DYNAMIC_ROWS {
            return false;
        }
        x += width + 1;
        shelf = shelf.max(height);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_admission_accounts_for_reserved_rows_and_combined_shelves() {
        let policy = assets::texture::policy::TexturePolicy::for_device(4096);
        let pixels: Arc<[u8]> = vec![255; 2049 * 2000 * 4].into();
        let textures = ["textures/a", "textures/b"].map(|path| assets::ParticleTexture {
            path: path.into(),
            width: 2049,
            height: 2000,
            rgba8: Arc::clone(&pixels),
        });
        assert!(particle_textures_fit(&textures[..1], policy));
        assert!(!particle_textures_fit(&textures, policy));
        let carrier = assets::encode_particle_catalog([1; 32], &textures[..1], &[]).unwrap();
        let assets = RuntimeParticleAssets::decode(&carrier).unwrap();
        assert!(
            ParticleAtlas::from_assets(&assets)
                .placement("textures/a")
                .is_some()
        );
    }

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
    fn shared_references_prevent_recycling_until_the_last_release() {
        let mut atlas = ParticleAtlas::default();
        let mut placements = Vec::new();
        for key in 1..=MAX_SLOTS as u64 {
            let placement = atlas.tile(key, 1, &[key as u8; 4]).unwrap();
            atlas.retain_placement(placement);
            placements.push(placement);
        }
        let first = placements[0];
        atlas.retain_placement(first);
        atlas.release_placement(first);
        let seq = atlas.patch_seq();
        assert!(atlas.tile(MAX_SLOTS as u64 + 1, 1, &[99; 4]).is_none());
        assert_eq!(atlas.patch_seq(), seq, "live pixels cannot be replaced");
        let at = ((first.y * atlas.side() + first.x) * 4) as usize;
        assert_eq!(&atlas.pixels()[at..at + 4], &[1; 4]);
        atlas.release_placement(first);
        assert_eq!(atlas.tile(MAX_SLOTS as u64 + 1, 1, &[99; 4]), Some(first));
        atlas.retain_placement(first);
        atlas.clear_placements();
        assert_eq!(
            atlas.tile(MAX_SLOTS as u64 + 2, 1, &[88; 4]),
            Some(placements[1])
        );
        assert!(atlas.pinned.is_empty());
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

    #[test]
    fn hd_particle_source_reaches_the_atlas_with_original_pixels_and_uvs() {
        let side = 2048;
        let pixels: Arc<[u8]> = (0..side * side)
            .flat_map(|i| [if i % 2 == 0 { 255 } else { 0 }, 73, 91, 255])
            .collect::<Vec<_>>()
            .into();
        let texture = assets::ParticleTexture {
            path: "textures/particle/particles".into(),
            width: side,
            height: side,
            rgba8: Arc::clone(&pixels),
        };
        let carrier = assets::encode_particle_catalog([1; 32], &[texture], &[]).unwrap();
        let assets = RuntimeParticleAssets::decode(&carrier).unwrap();
        let mut atlas = ParticleAtlas::from_assets(&assets);
        let placement = atlas.placement("textures/particle/particles").unwrap();
        assert_eq!([placement.width, placement.height], [side; 2]);
        assert_eq!(
            placement.normalized()[2..],
            [side as f32 / atlas.side() as f32; 2]
        );
        for row in [0, side - 1] {
            let start = (((placement.y + row) * atlas.side() + placement.x) * 4) as usize;
            let source = (row * side * 4) as usize;
            assert_eq!(
                &atlas.pixels()[start..start + (side * 4) as usize],
                &pixels[source..source + (side * 4) as usize]
            );
        }
        let tile = atlas.tile(1, 16, &[9; 16 * 16 * 4]).unwrap();
        assert!(tile.y >= placement.y + placement.height);
        assert!(atlas.base().len() <= assets::texture::policy::MAX_SUBSCRIBER_BYTES);
    }
}

#[cfg(test)]
mod sprite_tests {
    use super::*;

    #[test]
    fn cosmetic_regions_keep_every_texel_and_share_one_cached_patch() {
        let mut atlas = ParticleAtlas::default();
        let pixels: Vec<_> = (0..128 * 128)
            .flat_map(|i| [(i % 251) as u8, (i / 128) as u8, 91, 255])
            .collect();
        let p = atlas.sprite(42, 128, &pixels).unwrap();
        for row in 0..128 {
            let dest = (((p.y + row) * atlas.side + p.x) * 4) as usize;
            let source = (row * 128 * 4) as usize;
            assert_eq!(
                &atlas.pixels[dest..dest + 512],
                &pixels[source..source + 512]
            );
        }
        let seq = atlas.patch_seq();
        assert_eq!(atlas.sprite(42, 128, &pixels), Some(p));
        assert_eq!(atlas.patch_seq(), seq);
        assert_eq!(atlas.patches()[0].width, 128);
        assert!(atlas.sprite(42, 16, &[1; 1024]).is_none());
    }

    #[test]
    fn terrain_tiles_cannot_replace_any_part_of_a_shared_live_sprite() {
        let mut atlas = ParticleAtlas::default();
        let p = atlas.sprite(99999, 128, &vec![77; 128 * 128 * 4]).unwrap();
        atlas.retain_placement(p);
        atlas.retain_placement(p);
        atlas.release_placement(p);
        for key in 0..(MAX_SLOTS as u64 * 2) {
            atlas.tile(key, 1, &[key as u8; 4]).unwrap();
        }
        for row in 0..128 {
            let dest = (((p.y + row) * atlas.side + p.x) * 4) as usize;
            assert_eq!(&atlas.pixels[dest..dest + 512], &[77; 512]);
        }
        assert_eq!(atlas.placement_references(p), 1);
        atlas.release_placement(p);
        for key in 100000..100000 + MAX_SLOTS as u64 {
            atlas.tile(key, 1, &[1; 4]).unwrap();
        }
        assert!(!atlas.tiles.contains_key(&99999));
        assert_eq!(atlas.tiles.len(), MAX_SLOTS);
    }

    #[test]
    fn an_all_pinned_atlas_refuses_large_allocations_without_changing_pixels() {
        let mut atlas = ParticleAtlas::default();
        for key in 0..MAX_SLOTS as u64 {
            let p = atlas.tile(key, 1, &[3; 4]).unwrap();
            atlas.retain_placement(p);
        }
        let seq = atlas.patch_seq();
        assert!(atlas.sprite(99999, 128, &vec![4; 128 * 128 * 4]).is_none());
        assert_eq!(atlas.patch_seq(), seq);
        assert_eq!(atlas.tiles.len(), MAX_SLOTS);
    }
}
