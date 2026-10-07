//! Packs original images without changing their resolution or sampling.

use std::sync::Arc;

use super::{MAX_IMAGE_SIDE, OREUI_PAGE_SIDE, OreUiPage, OreUiSprite};

const GUTTER: u32 = 1;
pub(super) struct Pages {
    pages: Vec<([u32; 2], Vec<u8>)>,
    active: Option<usize>,
    bytes: usize,
    side: u32,
    byte_limit: usize,
    x: u32,
    y: u32,
    shelf: u32,
}

impl Default for Pages {
    fn default() -> Self {
        Self::new(OREUI_PAGE_SIDE, render_model::MAX_UI_TEXTURE_BYTES)
    }
}

impl Pages {
    pub(super) fn new(side: u32, byte_limit: usize) -> Self {
        Self {
            pages: Vec::new(),
            active: None,
            bytes: 0,
            side,
            byte_limit,
            x: 0,
            y: 0,
            shelf: 0,
        }
    }
    pub(super) fn insert(
        &mut self,
        pixels: &[u8],
        width: u32,
        height: u32,
    ) -> Result<OreUiSprite, String> {
        if width == 0 || height == 0 || width > MAX_IMAGE_SIDE || height > MAX_IMAGE_SIDE {
            return Err("OreUI raster exceeds the texture page dimensions".into());
        }
        if width > self.side || height > self.side {
            let page = self.add([width, height], pixels.to_vec())?;
            return Ok(OreUiSprite {
                page: page as u16,
                bounds: [0, 0, width as u16, height as u16],
            });
        }
        if self.x + width > self.side {
            self.x = 0;
            self.y += self.shelf + GUTTER;
            self.shelf = 0;
        }
        if self.active.is_none() || self.y + height > self.side {
            let page = self.add(
                [self.side; 2],
                vec![0; self.side as usize * self.side as usize * 4],
            )?;
            self.active = Some(page);
            (self.x, self.y, self.shelf) = (0, 0, 0);
        }
        let page = self.active.expect("packed page was allocated");
        let row = width as usize * 4;
        let side = self.side as usize;
        for line in 0..height as usize {
            let start = ((self.y as usize + line) * side + self.x as usize) * 4;
            self.pages[page].1[start..start + row]
                .copy_from_slice(&pixels[line * row..(line + 1) * row]);
        }
        let sprite = OreUiSprite {
            page: page as u16,
            bounds: [
                self.x as u16,
                self.y as u16,
                (self.x + width) as u16,
                (self.y + height) as u16,
            ],
        };
        self.x += width + GUTTER;
        self.shelf = self.shelf.max(height);
        Ok(sprite)
    }

    fn add(&mut self, dimensions: [u32; 2], pixels: Vec<u8>) -> Result<usize, String> {
        self.bytes += pixels.len();
        if self.bytes > self.byte_limit {
            return Err("OreUI artwork exceeds the texture budget".into());
        }
        let page = self.pages.len();
        self.pages.push((dimensions, pixels));
        Ok(page)
    }

    pub(super) fn finish(self) -> Vec<OreUiPage> {
        self.pages
            .into_iter()
            .map(|(dimensions, pixels)| OreUiPage {
                dimensions,
                pixels: Arc::from(pixels),
            })
            .collect()
    }
}
