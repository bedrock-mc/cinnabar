//! Block-entity texture atlas: the carrier's packed static pixels, then a map strip, then
//! pages of runtime-rasterized sign text canvases that grow with the distinct texts in range.

use std::{collections::BTreeMap, sync::Arc};

use assets::RuntimeBlockEntityAssets;

use super::mob::MobTexture;

/// Size of one text canvas cell in atlas pixels.
pub const TEXT_CELL: [u32; 2] = [96, 48];
const TEXT_COLUMNS: u32 = 10;
/// Rows of text cells added each time the text strip grows.
const TEXT_PAGE_ROWS: u32 = 10;
const TEXT_PAGE_HEIGHT: u32 = TEXT_CELL[1] * TEXT_PAGE_ROWS;
/// Text cells per page.
pub const TEXT_SLOT_COUNT: usize = (TEXT_COLUMNS * TEXT_PAGE_ROWS) as usize;
/// Memory safety cap on text pages (800 distinct texts, about 16 MB at a 1024-wide atlas).
const MAX_TEXT_PAGES: usize = 8;
/// Size of one map image cell: a full 128x128 map.
pub const MAP_CELL: [u32; 2] = [128, 128];
const MAP_COLUMNS: u32 = 8;
const MAP_ROWS: u32 = 2;
const MAP_STRIP_HEIGHT: u32 = MAP_CELL[1] * MAP_ROWS;

/// Height of the dynamic strips below the static atlas: maps, then `text_pages` text pages.
const fn dynamic_height(text_pages: u32) -> u32 {
    MAP_STRIP_HEIGHT + text_pages * TEXT_PAGE_HEIGHT
}

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
    text_pages: u32,
}

impl BlockEntityAtlas {
    #[must_use]
    pub fn from_assets(assets: &RuntimeBlockEntityAssets) -> Self {
        let [width, static_height] = assets.atlas_size();
        Self {
            size: [width, static_height + dynamic_height(1)],
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
            text_pages: 1,
        }
    }

    /// Text pages the atlas height currently covers.
    #[must_use]
    pub const fn text_pages(&self) -> u32 {
        self.text_pages
    }

    /// Extends the atlas to cover `pages` text pages; existing cells keep their pixel rects.
    pub fn set_text_pages(&mut self, pages: u32) {
        self.text_pages = pages;
        self.size[1] = self.static_height + dynamic_height(pages);
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
            y: (self.static_height + (slot / MAP_COLUMNS) * MAP_CELL[1]) as f32,
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
        self.size[1] = height + dynamic_height(self.text_pages);
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
            .filter(|slot| *slot < TEXT_COLUMNS * TEXT_PAGE_ROWS * self.text_pages)?;
        Some(AtlasRect {
            x: ((slot % TEXT_COLUMNS) * TEXT_CELL[0]) as f32,
            y: (self.static_height + MAP_STRIP_HEIGHT + (slot / TEXT_COLUMNS) * TEXT_CELL[1])
                as f32,
            width: TEXT_CELL[0] as f32,
            height: TEXT_CELL[1] as f32,
        })
    }
}

/// Least-recently-used allocation of equally sized RGBA cells in a strip `atlas_width` wide,
/// growing by whole pages so earlier pages are never reallocated or moved.
#[derive(Debug)]
struct CellPool {
    width: usize,
    cell: [usize; 2],
    columns: usize,
    rows_per_page: usize,
    max_pages: usize,
    pages: Vec<Vec<u8>>,
    keys: Vec<Option<u64>>,
    last_used: Vec<u64>,
    clock: u64,
    /// The first clock value of the current frame; slots used since then are never evicted.
    frame_start: u64,
    revision: u64,
    capped: bool,
}

impl CellPool {
    fn new(atlas_width: u32, cell: [u32; 2], columns: u32, rows: u32, max_pages: usize) -> Self {
        let mut pool = Self {
            width: atlas_width as usize,
            cell: cell.map(|value| value as usize),
            columns: columns as usize,
            rows_per_page: rows as usize,
            max_pages,
            pages: Vec::new(),
            keys: Vec::new(),
            last_used: Vec::new(),
            clock: 0,
            frame_start: 0,
            revision: 0,
            capped: false,
        };
        pool.add_page();
        pool
    }

    fn page_slots(&self) -> usize {
        self.columns * self.rows_per_page
    }

    fn add_page(&mut self) {
        self.pages
            .push(vec![0; self.width * self.cell[1] * self.rows_per_page * 4]);
        self.keys.resize(self.keys.len() + self.page_slots(), None);
        self.last_used.resize(self.keys.len(), 0);
    }

    /// The slot holding `key`, rasterizing `make` (a cell-sized RGBA8 canvas) on a miss into an
    /// empty slot, else the least recently used slot no rect of this frame refers to, else a
    /// new page. `None` when the canvas has the wrong size or the page cap is reached.
    fn slot(&mut self, key: u64, make: impl FnOnce() -> Vec<u8>) -> Option<usize> {
        self.clock += 1;
        if let Some(slot) = self.keys.iter().position(|entry| *entry == Some(key)) {
            self.last_used[slot] = self.clock;
            return Some(slot);
        }
        let free = self.keys.iter().position(Option::is_none).or_else(|| {
            (0..self.keys.len())
                .filter(|slot| self.last_used[*slot] < self.frame_start)
                .min_by_key(|slot| self.last_used[*slot])
        });
        let slot = match free {
            Some(slot) => slot,
            None if self.pages.len() < self.max_pages => {
                let slot = self.keys.len();
                self.add_page();
                slot
            }
            None => {
                if !self.capped {
                    self.capped = true;
                    bevy::log::warn!(
                        "block-entity canvas cap reached: {} cells of {:?} px are in use this frame; further canvases are not drawn",
                        self.keys.len(),
                        self.cell
                    );
                }
                return None;
            }
        };
        let canvas = make();
        let [cell_width, cell_height] = self.cell;
        if canvas.len() != cell_width * cell_height * 4 {
            return None;
        }
        let page_slots = self.page_slots();
        let page_slot = slot % page_slots;
        let (column, row) = (page_slot % self.columns, page_slot / self.columns);
        let page = &mut self.pages[slot / page_slots];
        for line in 0..cell_height {
            let target = ((row * cell_height + line) * self.width + column * cell_width) * 4;
            let source = line * cell_width * 4;
            page[target..target + cell_width * 4]
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
            text: CellPool::new(
                atlas_width,
                TEXT_CELL,
                TEXT_COLUMNS,
                TEXT_PAGE_ROWS,
                MAX_TEXT_PAGES,
            ),
            maps: CellPool::new(atlas_width, MAP_CELL, MAP_COLUMNS, MAP_ROWS, 1),
        }
    }

    /// Ends the frame whose rects were just submitted; their slots become evictable again.
    pub fn begin_frame(&mut self) {
        for pool in [&mut self.text, &mut self.maps] {
            pool.frame_start = pool.clock + 1;
        }
    }

    /// Text pages allocated so far; the atlas must cover them before their cells are drawn.
    #[must_use]
    pub fn text_pages(&self) -> u32 {
        self.text.pages.len() as u32
    }

    /// Changes whenever any strip pixel changes.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.text.revision.wrapping_add(self.maps.revision)
    }

    /// The map strip, then every text page, as one upload block.
    #[must_use]
    pub fn pixels(&self) -> Vec<u8> {
        let pages = self.maps.pages.iter().chain(&self.text.pages);
        let mut pixels = Vec::with_capacity(pages.clone().map(Vec::len).sum());
        for page in pages {
            pixels.extend_from_slice(page);
        }
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
        // Key 1 was touched first and is now the oldest; next frame a new key takes its slot.
        text.begin_frame();
        let replaced = text.text_slot(1_000, || canvas(9)).unwrap();
        assert_eq!(replaced, first);
        assert_eq!(text.pixels()[1024 * MAP_STRIP_HEIGHT as usize * 4], 9);
        assert!(text.text_slot(2_000, || vec![0; 3]).is_none());
    }

    /// More distinct texts than a page in one frame grow the strip instead of repainting
    /// rects already handed out; the page cap only bounds memory.
    #[test]
    fn a_full_frame_grows_a_page_instead_of_evicting_its_own_slots() {
        let mut text = DynamicCells::new(1024);
        for key in 0..TEXT_SLOT_COUNT as u64 {
            assert_eq!(text.text_slot(key, || canvas(1)), Some(key as usize));
        }
        assert_eq!(text.text_pages(), 1);
        let grown = text.text_slot(1_000, || canvas(2)).unwrap();
        assert_eq!(grown, TEXT_SLOT_COUNT);
        assert_eq!(text.text_pages(), 2);
        for key in 0..TEXT_SLOT_COUNT as u64 {
            assert_eq!(
                text.text_slot(key, || panic!("a hit must not rasterize")),
                Some(key as usize)
            );
        }
        let total = MAX_TEXT_PAGES * TEXT_SLOT_COUNT;
        for key in 2_000..2_000 + (total - TEXT_SLOT_COUNT - 1) as u64 {
            text.text_slot(key, || canvas(3)).unwrap();
        }
        assert_eq!(text.text_pages() as usize, MAX_TEXT_PAGES);
        assert!(text.text_slot(9_999, || canvas(4)).is_none());
        // Next frame the least recently used cell (key 1000's) is reclaimed instead of growing.
        text.begin_frame();
        assert_eq!(text.text_slot(9_999, || canvas(4)), Some(TEXT_SLOT_COUNT));
        assert_eq!(text.text_pages() as usize, MAX_TEXT_PAGES);
    }

    #[test]
    fn map_cells_lead_the_text_pages_and_share_the_revision() {
        let mut cells = DynamicCells::new(1024);
        let before = cells.revision();
        let slot = cells
            .map_slot(7, || vec![5; (MAP_CELL[0] * MAP_CELL[1] * 4) as usize])
            .unwrap();
        assert_eq!(slot, 0);
        assert_ne!(cells.revision(), before);
        // The map strip opens the upload block so growing text pages never move it.
        assert_eq!(cells.pixels()[0], 5);
        assert_eq!(cells.pixels().len(), 1024 * dynamic_height(1) as usize * 4);
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
        assert_eq!(atlas.size()[1], 5 + dynamic_height(1));
        assert_eq!(atlas.texel("mob/test", 1, 1), Some([9, 9, 9, 9]));
        assert_eq!(atlas.map_cell(0).unwrap().y, 5.0);
        assert_eq!(atlas.text_cell(0).unwrap().y, (5 + MAP_STRIP_HEIGHT) as f32);
        assert!(atlas.text_cell(TEXT_SLOT_COUNT).is_none());
        atlas.set_text_pages(2);
        assert_eq!(atlas.size()[1], 5 + dynamic_height(2));
        assert_eq!(atlas.text_cell(0).unwrap().y, (5 + MAP_STRIP_HEIGHT) as f32);
        assert!(atlas.text_cell(TEXT_SLOT_COUNT).is_some());
    }
}
