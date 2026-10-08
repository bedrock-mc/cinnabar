//! Full-source texels for GUI geometry, without thumbnail projection or reduction.

use std::{collections::BTreeMap, sync::Arc};

use render_model::UiTexturePage;
use sha2::{Digest, Sha256};

use super::super::{IconRef, UiPresentationError};

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

    pub(super) fn insert(
        &mut self,
        size: [u16; 2],
        pixels: &[u8],
    ) -> Result<IconRef, UiPresentationError> {
        let [width, height] = size.map(usize::from);
        if width == 0
            || height == 0
            || width + 2 * GUTTER > SIDE
            || height + 2 * GUTTER > SIDE
            || pixels.len() != width * height * 4
        {
            return Err(UiPresentationError::InvalidFontTexture);
        }
        let key = key(size, pixels);
        if let Some(icon) = self.refs.get(&key) {
            return Ok(*icon);
        }
        let padded = [width + 2 * GUTTER, height + 2 * GUTTER];
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
            let sy = y.saturating_sub(GUTTER).min(height - 1);
            for x in 0..padded[0] {
                let sx = x.saturating_sub(GUTTER).min(width - 1);
                let source = (sy * width + sx) * 4;
                let dest = ((self.cursor[1] + y) * SIDE + self.cursor[0] + x) * 4;
                image[dest..dest + 4].copy_from_slice(&pixels[source..source + 4]);
            }
        }
        let [left, top] = self.cursor.map(|value| value + GUTTER);
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
