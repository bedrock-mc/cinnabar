//! Shelf allocation with reusable rectangles for replaced artwork.

use render_model::UiTexturePage;

use super::{GUTTER, PAGE_SIDE};

/// One reserved page: its pixels, shelf cursor, residents, and last use.
pub(super) struct Page {
    pub(super) pixels: Vec<u8>,
    pub(super) cursor: [u32; 3],
    pub(super) free: Vec<[u32; 4]>,
    pub(super) keys: Vec<String>,
    pub(super) used: u64,
    pub(super) image: UiTexturePage,
}

impl Page {
    /// Creates an empty page with a fresh shelf allocator.
    pub(super) fn blank() -> Option<Self> {
        let pixels = vec![0; (PAGE_SIDE * PAGE_SIDE * 4) as usize];
        let image = UiTexturePage::owned([PAGE_SIDE; 2], pixels.clone().into()).ok()?;
        Some(Self {
            pixels,
            cursor: [0; 3],
            free: Vec::new(),
            keys: Vec::new(),
            used: 0,
            image,
        })
    }

    /// Reuses a released rectangle before advancing the shelf cursor.
    pub(super) fn allocate(&mut self, size: [u32; 2]) -> Option<[u32; 2]> {
        let [width, height] = size.map(|side| side + GUTTER);
        if let Some(index) = self
            .free
            .iter()
            .rposition(|rect| rect[2] >= width && rect[3] >= height)
        {
            let [x, y, w, h] = self.free.swap_remove(index);
            if w > width {
                self.free.push([x + width, y, w - width, h]);
            }
            if h > height {
                self.free.push([x, y + height, width, h - height]);
            }
            return Some([x, y]);
        }
        let [mut x, mut y, mut shelf] = self.cursor;
        if x + size[0] > PAGE_SIDE {
            (x, y, shelf) = (0, y + shelf + GUTTER, 0);
        }
        if y + size[1] > PAGE_SIDE {
            return None;
        }
        self.cursor = [x + size[0] + GUTTER, y, shelf.max(size[1])];
        Some([x, y])
    }

    /// Releases a resident rectangle with its gutter, merging adjacent free space.
    pub(super) fn release(&mut self, rect: [u16; 4]) {
        let [x, y, w, h] = rect.map(u32::from);
        for row in y..y + h {
            let start = ((row * PAGE_SIDE + x) * 4) as usize;
            self.pixels[start..start + (w * 4) as usize].fill(0);
        }
        let mut rect = [x, y, w + GUTTER, h + GUTTER];
        let mut index = 0;
        while index < self.free.len() {
            let other = self.free[index];
            let horizontal = rect[1] == other[1]
                && rect[3] == other[3]
                && (rect[0] + rect[2] == other[0] || other[0] + other[2] == rect[0]);
            let vertical = rect[0] == other[0]
                && rect[2] == other[2]
                && (rect[1] + rect[3] == other[1] || other[1] + other[3] == rect[1]);
            if horizontal || vertical {
                self.free.swap_remove(index);
                rect = [
                    rect[0].min(other[0]),
                    rect[1].min(other[1]),
                    if horizontal {
                        rect[2] + other[2]
                    } else {
                        rect[2]
                    },
                    if vertical {
                        rect[3] + other[3]
                    } else {
                        rect[3]
                    },
                ];
                index = 0;
            } else {
                index += 1;
            }
        }
        self.free.push(rect);
    }
}
