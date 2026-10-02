//! Block-entity texture atlas: the carrier's packed static pixels plus a fixed strip
//! below them for runtime-rasterized sign text canvases.

use std::{collections::BTreeMap, sync::Arc};

use assets::RuntimeBlockEntityAssets;

use super::mob::MobTexture;

/// Size of one text canvas cell in atlas pixels.
pub const TEXT_CELL: [u32; 2] = [96, 48];
const TEXT_COLUMNS: u32 = 10;
const TEXT_ROWS: u32 = 10;
/// Size of one map image cell: a full 128x128 map.
pub const MAP_CELL: [u32; 2] = [128, 128];
const MAP_COLUMNS: u32 = 8;
const MAP_ROWS: u32 = 2;
const TEXT_STRIP_HEIGHT: u32 = TEXT_CELL[1] * TEXT_ROWS;
/// Height of the dynamic strips appended below the static atlas: text, then maps.
pub const DYNAMIC_STRIP_HEIGHT: u32 = TEXT_STRIP_HEIGHT + MAP_CELL[1] * MAP_ROWS;
pub const TEXT_SLOT_COUNT: usize = (TEXT_COLUMNS * TEXT_ROWS) as usize;

/// A pixel rect in the full atlas.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AtlasRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A pack texture placed in the atlas, with the size its model UVs were authored against.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextureRef {
    pub rect: AtlasRect,
    pub logical: [f32; 2],
}

impl TextureRef {
    /// Converts a `[u, v, width, height]` texel rect of the logical texture to atlas-pixel
    /// `[u0, v0, u1, v1]`, so a higher-resolution replacement texture still maps.
    #[must_use]
    pub fn rect_uv(&self, texels: [f32; 4]) -> [f32; 4] {
        let scale_x = self.rect.width / self.logical[0];
        let scale_y = self.rect.height / self.logical[1];
        [
            self.rect.x + texels[0] * scale_x,
            self.rect.y + texels[1] * scale_y,
            self.rect.x + (texels[0] + texels[2]) * scale_x,
            self.rect.y + (texels[1] + texels[3]) * scale_y,
        ]
    }
}

#[derive(Clone, Debug)]
pub struct BlockEntityAtlas {
    size: [u32; 2],
    static_height: u32,
    static_rgba8: Arc<[u8]>,
    placements: BTreeMap<Box<str>, AtlasRect>,
    identity: [u8; 32],
}

impl BlockEntityAtlas {
    #[must_use]
    pub fn from_assets(assets: &RuntimeBlockEntityAssets) -> Self {
        let [width, static_height] = assets.atlas_size();
        Self {
            size: [width, static_height + DYNAMIC_STRIP_HEIGHT],
            static_height,
            static_rgba8: Arc::clone(assets.atlas_rgba8()),
            placements: assets
                .placements()
                .iter()
                .map(|placement| {
                    (
                        placement.name.clone(),
                        AtlasRect {
                            x: placement.x as f32,
                            y: placement.y as f32,
                            width: placement.width as f32,
                            height: placement.height as f32,
                        },
                    )
                })
                .collect(),
            identity: assets.identity(),
        }
    }

    /// Full atlas size including the dynamic strip.
    #[must_use]
    pub const fn size(&self) -> [u32; 2] {
        self.size
    }

    #[must_use]
    pub const fn static_height(&self) -> u32 {
        self.static_height
    }

    #[must_use]
    pub fn static_rgba8(&self) -> &Arc<[u8]> {
        &self.static_rgba8
    }

    #[must_use]
    pub const fn identity(&self) -> [u8; 32] {
        self.identity
    }

    /// A packed pack texture by pack-relative path without extension.
    #[must_use]
    pub fn texture(&self, name: &str, logical: [f32; 2]) -> Option<TextureRef> {
        self.placements.get(name).map(|rect| TextureRef {
            rect: *rect,
            logical,
        })
    }

    /// The rect of dynamic map cell `slot`.
    #[must_use]
    pub fn map_cell(&self, slot: usize) -> Option<AtlasRect> {
        let slot = u32::try_from(slot)
            .ok()
            .filter(|slot| *slot < MAP_COLUMNS * MAP_ROWS)?;
        Some(AtlasRect {
            x: ((slot % MAP_COLUMNS) * MAP_CELL[0]) as f32,
            y: (self.static_height + TEXT_STRIP_HEIGHT + (slot / MAP_COLUMNS) * MAP_CELL[1]) as f32,
            width: MAP_CELL[0] as f32,
            height: MAP_CELL[1] as f32,
        })
    }

    /// Shelf-packs `textures` into new static rows below the packed ones, moving the dynamic
    /// strips down; textures wider than the atlas or already present are skipped.
    pub fn append_textures(&mut self, textures: &[MobTexture]) {
        let previous_count = self.placements.len();
        let width = self.size[0];
        let mut pixels = self.static_rgba8.to_vec();
        let (mut x, mut y, mut shelf) = (0u32, self.static_height, 0u32);
        for texture in textures {
            let expected = texture.width as usize * texture.height as usize * 4;
            if texture.width == 0
                || texture.width > width
                || texture.height == 0
                || texture.rgba8.len() != expected
                || self.placements.contains_key(texture.name.as_str())
            {
                continue;
            }
            if x + texture.width > width {
                x = 0;
                y += shelf + 1;
                shelf = 0;
            }
            let needed = (y + texture.height) as usize * width as usize * 4;
            if pixels.len() < needed {
                pixels.resize(needed, 0);
            }
            for row in 0..texture.height {
                let target = ((y + row) as usize * width as usize + x as usize) * 4;
                let source = row as usize * texture.width as usize * 4;
                let length = texture.width as usize * 4;
                pixels[target..target + length]
                    .copy_from_slice(&texture.rgba8[source..source + length]);
            }
            self.placements.insert(
                texture.name.as_str().into(),
                AtlasRect {
                    x: x as f32,
                    y: y as f32,
                    width: texture.width as f32,
                    height: texture.height as f32,
                },
            );
            x += texture.width + 1;
            shelf = shelf.max(texture.height);
        }
        let height = (pixels.len() / (width as usize * 4)) as u32;
        self.static_height = height;
        self.size[1] = height + DYNAMIC_STRIP_HEIGHT;
        self.static_rgba8 = Arc::from(pixels);
        if self.placements.len() != previous_count {
            use sha2::{Digest, Sha256};
            let mut hash = Sha256::new();
            hash.update(self.identity);
            hash.update(width.to_le_bytes());
            hash.update(height.to_le_bytes());
            hash.update(&self.static_rgba8);
            for (name, rect) in &self.placements {
                hash.update((name.len() as u64).to_le_bytes());
                hash.update(name.as_bytes());
                for value in [rect.x, rect.y, rect.width, rect.height] {
                    hash.update(value.to_bits().to_le_bytes());
                }
            }
            self.identity = hash.finalize().into();
        }
    }

    /// One RGBA8 texel of a packed texture, in the texture's own pixel coordinates.
    #[must_use]
    pub fn texel(&self, name: &str, x: u32, y: u32) -> Option<[u8; 4]> {
        let rect = self.placements.get(name)?;
        if x as f32 >= rect.width || y as f32 >= rect.height {
            return None;
        }
        let row = rect.y as u32 + y;
        let column = rect.x as u32 + x;
        let start = (row as usize * self.size[0] as usize + column as usize) * 4;
        self.static_rgba8.get(start..start + 4)?.try_into().ok()
    }

    /// The rect of dynamic text cell `slot`.
    #[must_use]
    pub fn text_cell(&self, slot: usize) -> Option<AtlasRect> {
        let slot = u32::try_from(slot)
            .ok()
            .filter(|slot| *slot < TEXT_COLUMNS * TEXT_ROWS)?;
        Some(AtlasRect {
            x: ((slot % TEXT_COLUMNS) * TEXT_CELL[0]) as f32,
            y: (self.static_height + (slot / TEXT_COLUMNS) * TEXT_CELL[1]) as f32,
            width: TEXT_CELL[0] as f32,
            height: TEXT_CELL[1] as f32,
        })
    }
}

/// Least-recently-used allocation of equally sized RGBA cells in a strip `atlas_width` wide.
#[derive(Debug)]
struct CellPool {
    width: usize,
    cell: [usize; 2],
    columns: usize,
    pixels: Vec<u8>,
    keys: Vec<Option<u64>>,
    last_used: Vec<u64>,
    clock: u64,
    revision: u64,
}

impl CellPool {
    fn new(atlas_width: u32, cell: [u32; 2], columns: u32, rows: u32) -> Self {
        let width = atlas_width as usize;
        let count = (columns * rows) as usize;
        Self {
            width,
            cell: cell.map(|value| value as usize),
            columns: columns as usize,
            pixels: vec![0; width * (cell[1] * rows) as usize * 4],
            keys: vec![None; count],
            last_used: vec![0; count],
            clock: 0,
            revision: 0,
        }
    }

    /// The slot holding `key`, rasterizing `make` (a cell-sized RGBA8 canvas) into the least
    /// recently used slot on a miss. `None` when the canvas has the wrong size.
    fn slot(&mut self, key: u64, make: impl FnOnce() -> Vec<u8>) -> Option<usize> {
        self.clock += 1;
        if let Some(slot) = self.keys.iter().position(|entry| *entry == Some(key)) {
            self.last_used[slot] = self.clock;
            return Some(slot);
        }
        let canvas = make();
        let [cell_width, cell_height] = self.cell;
        if canvas.len() != cell_width * cell_height * 4 {
            return None;
        }
        let slot = self
            .keys
            .iter()
            .position(Option::is_none)
            .or_else(|| (0..self.keys.len()).min_by_key(|slot| self.last_used[*slot]))?;
        let column = slot % self.columns;
        let row = slot / self.columns;
        for line in 0..cell_height {
            let target = ((row * cell_height + line) * self.width + column * cell_width) * 4;
            let source = line * cell_width * 4;
            self.pixels[target..target + cell_width * 4]
                .copy_from_slice(&canvas[source..source + cell_width * 4]);
        }
        self.keys[slot] = Some(key);
        self.last_used[slot] = self.clock;
        self.revision = self.revision.wrapping_add(1);
        Some(slot)
    }
}

/// The runtime-rasterized strips below the static atlas: sign text canvases, then map images.
#[derive(Debug)]
pub struct DynamicCells {
    text: CellPool,
    maps: CellPool,
}

impl DynamicCells {
    #[must_use]
    pub fn new(atlas_width: u32) -> Self {
        Self {
            text: CellPool::new(atlas_width, TEXT_CELL, TEXT_COLUMNS, TEXT_ROWS),
            maps: CellPool::new(atlas_width, MAP_CELL, MAP_COLUMNS, MAP_ROWS),
        }
    }

    /// Changes whenever any strip pixel changes.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.text.revision.wrapping_add(self.maps.revision)
    }

    /// Both strips' pixels, text first, as one upload block.
    #[must_use]
    pub fn pixels(&self) -> Vec<u8> {
        let mut pixels = Vec::with_capacity(self.text.pixels.len() + self.maps.pixels.len());
        pixels.extend_from_slice(&self.text.pixels);
        pixels.extend_from_slice(&self.maps.pixels);
        pixels
    }

    /// The text slot holding `key`; see [`CellPool::slot`].
    pub fn text_slot(&mut self, key: u64, make: impl FnOnce() -> Vec<u8>) -> Option<usize> {
        self.text.slot(key, make)
    }

    /// The map slot holding `key`; see [`CellPool::slot`].
    pub fn map_slot(&mut self, key: u64, make: impl FnOnce() -> Vec<u8>) -> Option<usize> {
        self.maps.slot(key, make)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas(value: u8) -> Vec<u8> {
        vec![value; (TEXT_CELL[0] * TEXT_CELL[1] * 4) as usize]
    }

    #[test]
    fn rect_uv_scales_with_texture_resolution() {
        let texture = TextureRef {
            rect: AtlasRect {
                x: 100.0,
                y: 200.0,
                width: 128.0,
                height: 128.0,
            },
            logical: [64.0, 64.0],
        };
        assert_eq!(
            texture.rect_uv([8.0, 4.0, 8.0, 8.0]),
            [116.0, 208.0, 132.0, 224.0]
        );
    }

    #[test]
    fn text_slots_reuse_hits_and_evict_least_recently_used() {
        let mut text = DynamicCells::new(1024);
        let first = text.text_slot(1, || canvas(1)).unwrap();
        assert_eq!(
            text.text_slot(1, || panic!("a hit must not rasterize")),
            Some(first)
        );
        assert_eq!(text.revision(), 1);
        for key in 2..=TEXT_SLOT_COUNT as u64 {
            text.text_slot(key, || canvas(key as u8)).unwrap();
        }
        // Key 1 was touched first and is now the oldest; a new key takes its slot.
        let replaced = text.text_slot(1_000, || canvas(9)).unwrap();
        assert_eq!(replaced, first);
        assert_eq!(text.pixels()[0], 9);
        assert!(text.text_slot(2_000, || vec![0; 3]).is_none());
    }

    #[test]
    fn map_cells_follow_the_text_strip_and_share_the_revision() {
        let mut cells = DynamicCells::new(1024);
        let before = cells.revision();
        let slot = cells
            .map_slot(7, || vec![5; (MAP_CELL[0] * MAP_CELL[1] * 4) as usize])
            .unwrap();
        assert_eq!(slot, 0);
        assert_ne!(cells.revision(), before);
        // The map cell's first pixel sits after the whole text strip in the upload block.
        let text_bytes = 1024 * TEXT_STRIP_HEIGHT as usize * 4;
        assert_eq!(cells.pixels()[text_bytes], 5);
        assert_eq!(
            cells.pixels().len(),
            1024 * DYNAMIC_STRIP_HEIGHT as usize * 4
        );
    }

    #[test]
    fn appended_textures_extend_the_static_rows_and_shift_the_strips() {
        let bytes =
            assets::encode_block_entity_catalog(b"{}", 8, 2, &[0u8; 8 * 2 * 4], &[]).unwrap();
        let mut atlas = BlockEntityAtlas::from_assets(
            &assets::RuntimeBlockEntityAssets::decode(&bytes).unwrap(),
        );
        let texture = MobTexture {
            name: "mob/test".into(),
            width: 4,
            height: 3,
            rgba8: Arc::from(vec![9u8; 4 * 3 * 4]),
        };
        atlas.append_textures(&[texture.clone(), texture]);
        let rect = atlas.texture("mob/test", [4.0, 3.0]).unwrap().rect;
        assert_eq!((rect.x, rect.y), (0.0, 2.0));
        assert_eq!(atlas.static_height(), 5);
        assert_eq!(atlas.size()[1], 5 + DYNAMIC_STRIP_HEIGHT);
        assert_eq!(atlas.texel("mob/test", 1, 1), Some([9, 9, 9, 9]));
        assert_eq!(atlas.text_cell(0).unwrap().y, 5.0);
    }
}
