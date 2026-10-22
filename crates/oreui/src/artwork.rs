//! Shipped OreUI artwork: the decoded images and where their pages sit in the texture array.

use std::sync::Arc;

use super::Originals;
use crate::images::OreUiImages;

impl Originals {
    /// The artwork `images` whose first page sits at texture page `page`.
    pub fn new(page: u16, images: OreUiImages) -> Self {
        let masks = images
            .sprites
            .iter()
            .filter_map(|(key, sprite)| {
                key.strip_prefix("@mask/")
                    .map(|key| (key.to_owned(), *sprite))
            })
            .collect();
        Self {
            page,
            images: images.clone(),
            masks,
            sprites: images.sprites,
            loading_frames: images.loading_frames,
            animations: images.animations,
        }
    }

    /// The texture page of the artwork's first page.
    pub fn page(&self) -> u16 {
        self.page
    }

    /// Whether `images` is this artwork: the same page pixels, sprites and animations.
    pub fn same_images(&self, images: &OreUiImages) -> bool {
        self.images.pages.len() == images.pages.len()
            && self
                .images
                .pages
                .iter()
                .zip(&images.pages)
                .all(|(old, new)| {
                    old.dimensions == new.dimensions && Arc::ptr_eq(&old.pixels, &new.pixels)
                })
            && self.images.sprites == images.sprites
            && self.images.animations == images.animations
            && self.images.loading_frames == images.loading_frames
    }

    /// The shipped sprite `key`.
    pub fn sprite(&self, key: &str) -> Option<crate::images::OreUiSprite> {
        self.sprites.get(key).copied()
    }
}
