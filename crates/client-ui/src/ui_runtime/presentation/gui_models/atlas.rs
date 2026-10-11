//! Full-source texels for GUI geometry, without thumbnail projection or reduction.

use std::{collections::BTreeMap, sync::Arc};

use render_model::UiTexturePage;
use sha2::{Digest, Sha256};

use {super::super::UiPresentationError, ui::IconRef};

const SIDE: usize = render_model::UI_MODEL_ATLAS_SIDE as usize;
const GUTTER: usize = 1;

pub(super) type TextureKey = ([u16; 2], [u8; 32]);

pub(super) fn key(size: [u16; 2], pixels: &[u8]) -> TextureKey {
    (size, Sha256::digest(pixels).into())
}

/// Bounded shelf atlas. Identical sources share texels, never projected thumbnail pixels.
pub(super) struct Atlas {
    first: u16,
    limit: usize,
    pages: Vec<Vec<u8>>,
    cursor: [usize; 2],
    row: usize,
    refs: BTreeMap<TextureKey, IconRef>,
}

impl Atlas {
    pub(super) fn new(first: u16, limit: usize) -> Self {
        Self {
            first,
            limit,
            pages: Vec::new(),
            cursor: [0; 2],
            row: 0,
            refs: BTreeMap::new(),
        }
    }

    /// Places full-resolution texels. Each side keeps a one-texel edge gutter where the page has
    /// room; a side as long as the page fills it, its page border clamping instead.
    pub(super) fn insert(
        &mut self,
        size: [u16; 2],
        pixels: &[u8],
    ) -> Result<IconRef, UiPresentationError> {
        let [width, height] = size.map(usize::from);
        if width == 0
            || height == 0
            || width > SIDE
            || height > SIDE
            || pixels.len() != width * height * 4
        {
            return Err(UiPresentationError::InvalidFontTexture);
        }
        let key = key(size, pixels);
        if let Some(icon) = self.refs.get(&key) {
            return Ok(*icon);
        }
        let lead = [width, height].map(|side| if side + 2 * GUTTER <= SIDE { GUTTER } else { 0 });
        let padded = [width, height].map(|side| (side + 2 * GUTTER).min(SIDE));
        if self.cursor[0] + padded[0] > SIDE {
            self.cursor = [0, self.cursor[1] + self.row];
            self.row = 0;
        }
        if self.pages.is_empty() || self.cursor[1] + padded[1] > SIDE {
            if self.pages.len() == self.limit {
                return Err(UiPresentationError::InvalidFontTexture);
            }
            self.pages.push(vec![0; SIDE * SIDE * 4]);
            self.cursor = [0; 2];
            self.row = 0;
        }
        let image = self
            .pages
            .last_mut()
            .ok_or(UiPresentationError::InvalidFontTexture)?;
        for y in 0..padded[1] {
            let sy = y.saturating_sub(lead[1]).min(height - 1);
            for x in 0..padded[0] {
                let sx = x.saturating_sub(lead[0]).min(width - 1);
                let source = (sy * width + sx) * 4;
                let dest = ((self.cursor[1] + y) * SIDE + self.cursor[0] + x) * 4;
                image[dest..dest + 4].copy_from_slice(&pixels[source..source + 4]);
            }
        }
        let [left, top] = [0, 1].map(|axis| self.cursor[axis] + lead[axis]);
        let icon = IconRef {
            page: self
                .first
                .checked_add((self.pages.len() - 1) as u16)
                .ok_or(UiPresentationError::InvalidFontTexture)?,
            uv: [
                left as u16,
                top as u16,
                (left + width) as u16,
                (top + height) as u16,
            ],
            glint: false,
        };
        self.cursor[0] += padded[0];
        self.row = self.row.max(padded[1]);
        self.refs.insert(key, icon);
        Ok(icon)
    }

    /// Places full-resolution texels or box-filters by powers of two when size or budget requires it.
    /// Returns the stored region and size; returns None if even one texel cannot fit.
    pub(super) fn insert_fitted(
        &mut self,
        size: [u16; 2],
        pixels: &[u8],
        shrink: bool,
    ) -> Option<(IconRef, [u16; 2])> {
        let mut limit = SIDE as u32;
        loop {
            let fitted = render_model::fit_rgba_within(size[0], size[1], pixels, limit);
            let (size, pixels) = fitted
                .as_ref()
                .map_or((size, pixels), |(size, pixels)| (*size, pixels.as_slice()));
            if let Some(icon) = self.try_insert([(size, pixels)], |icons| icons.first().copied()) {
                return Some((icon, size));
            }
            let longest = u32::from(size[0].max(size[1]));
            if !shrink || longest <= 1 {
                return None;
            }
            limit = longest / 2;
        }
    }

    /// Inserts optional mesh textures, restoring atlas capacity when insertion or mesh building fails.
    pub(super) fn try_insert<'a, T>(
        &mut self,
        sources: impl IntoIterator<Item = ([u16; 2], &'a [u8])>,
        build: impl FnOnce(&[IconRef]) -> Option<T>,
    ) -> Option<T> {
        let (cursor, row, pages) = (self.cursor, self.row, self.pages.len());
        let mut added = Vec::new();
        let result = (|| {
            let mut icons = Vec::new();
            for (size, pixels) in sources {
                let key = key(size, pixels);
                let known = self.refs.contains_key(&key);
                icons.push(self.insert(size, pixels).ok()?);
                if !known {
                    added.push(key);
                }
            }
            build(&icons)
        })();
        if result.is_none() {
            self.cursor = cursor;
            self.row = row;
            self.pages.truncate(pages);
            for key in added {
                self.refs.remove(&key);
            }
        }
        result
    }

    /// Returns the pages already occupied by required model textures.
    pub(super) fn page_count(&self) -> usize {
        self.pages.len()
    }

    pub(super) fn finish(
        self,
    ) -> Result<(Vec<UiTexturePage>, BTreeMap<TextureKey, IconRef>), UiPresentationError> {
        let pages = self
            .pages
            .into_iter()
            .map(|pixels| {
                UiTexturePage::owned([SIDE as u32; 2], Arc::from(pixels))
                    .map_err(|_| UiPresentationError::InvalidFontTexture)
            })
            .collect::<Result<_, _>>()?;
        Ok((pages, self.refs))
    }
}
