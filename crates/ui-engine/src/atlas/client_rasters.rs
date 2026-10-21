//! Prepared screen artwork shares the bounded atlas without PNG encode/decode work.

use super::*;

/// Reserved texture for the host local player portrait, including its hat layer.
pub const LOCAL_PLAYER_HEAD: &str = "client-raster:local-player-head";

impl ServerAtlas {
    /// Replaces surface rasters while retaining the host local head. Keys use the
    /// reserved `client-raster:` prefix and fit an atlas page and decoded-image budget. Unchanged
    /// content does no work; changed residents keep their slot when dimensions match.
    pub fn set_client_rasters(&mut self, images: &BTreeMap<String, UiTexturePage>) {
        let surface_images = images
            .iter()
            .filter(|(key, _)| key.as_str() != LOCAL_PLAYER_HEAD);
        if self.client_rasters.len()
            == surface_images.clone().count()
                + usize::from(self.client_rasters.contains_key(LOCAL_PLAYER_HEAD))
            && surface_images.clone().all(|(key, image)| {
                self.client_rasters.get(key).is_some_and(|old| {
                    old.identity() == image.identity() && old.dimensions() == image.dimensions()
                })
            })
        {
            return;
        }
        let mut images = images.clone();
        images.remove(LOCAL_PLAYER_HEAD);
        if let Some(head) = self.client_rasters.get(LOCAL_PLAYER_HEAD) {
            images.insert(LOCAL_PLAYER_HEAD.into(), head.clone());
        }
        self.replace_client_rasters(&images);
    }

    /// Updates the single host-owned local head without replacing roster portraits.
    pub fn set_local_player_head(&mut self, image: Option<&UiTexturePage>) {
        let previous = self.client_rasters.get(LOCAL_PLAYER_HEAD);
        if previous.map(|p| (p.identity(), p.dimensions()))
            == image.map(|p| (p.identity(), p.dimensions()))
        {
            return;
        }
        let mut images = self.client_rasters.clone();
        images.remove(LOCAL_PLAYER_HEAD);
        if let Some(image) = image {
            images.insert(LOCAL_PLAYER_HEAD.into(), image.clone());
        }
        self.replace_client_rasters(&images);
    }

    /// Replaces admitted host rasters after each owning surface has contributed its images.
    fn replace_client_rasters(&mut self, images: &BTreeMap<String, UiTexturePage>) {
        if images.len() > MAX_EXTRA
            || images.values().map(|p| p.pixels().len()).sum::<usize>() > MAX_DECODED_BYTES
            || images.iter().any(|(key, image)| {
                !key.starts_with("client-raster:")
                    || image
                        .dimensions()
                        .into_iter()
                        .any(|side| side == 0 || side > PAGE_SIDE)
                    || image.pixels().len()
                        != image
                            .dimensions()
                            .into_iter()
                            .map(|v| v as usize)
                            .product::<usize>()
                            * 4
            })
        {
            return;
        }
        let same = |a: &UiTexturePage, b: &UiTexturePage| {
            a.identity() == b.identity() && a.dimensions() == b.dimensions()
        };
        if self.client_rasters.len() == images.len()
            && images.iter().all(|(key, image)| {
                self.client_rasters
                    .get(key)
                    .is_some_and(|old| same(old, image))
            })
        {
            return;
        }
        let mut changed = Vec::new();
        for (key, old) in &self.client_rasters {
            if images.get(key).is_some_and(|image| same(old, image)) {
                continue;
            }
            let Some(slot) = self.resident.get(key).copied() else {
                continue;
            };
            if let Some(image) = images
                .get(key)
                .filter(|image| image.dimensions() == old.dimensions())
            {
                let page = &mut self.pages[usize::from(slot.page)];
                let [x, y, width, height] = slot.rect.map(usize::from);
                for row in 0..height {
                    let offset = ((y + row) * PAGE_SIDE as usize + x) * 4;
                    page.pixels[offset..offset + width * 4]
                        .copy_from_slice(&image.pixels()[row * width * 4..(row + 1) * width * 4]);
                }
                changed.push(usize::from(slot.page));
            } else {
                self.resident.remove(key);
            }
        }
        changed.sort_unstable();
        changed.dedup();
        for index in changed {
            let page = &mut self.pages[index];
            page.image = UiTexturePage::owned([PAGE_SIDE; 2], page.pixels.clone().into())
                .expect("validated atlas page");
            self.images[index] = page.image.clone();
            self.dirty = true;
        }
        self.client_rasters.clone_from(images);
        self.metadata_revision = self.metadata_revision.wrapping_add(1);
        self.metadata_dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Makes one prepared portrait under a reserved key.
    fn images(color: u8) -> BTreeMap<String, UiTexturePage> {
        BTreeMap::from([(
            "client-raster:test".into(),
            UiTexturePage::owned([8; 2], vec![color; 8 * 8 * 4].into()).unwrap(),
        )])
    }

    #[test]
    fn local_player_head_survives_roster_replacement_and_clears_independently() {
        let mut atlas = ServerAtlas::new(&[], None, 1);
        let portrait = images(50).remove("client-raster:test").unwrap();
        atlas.set_local_player_head(Some(&portrait));
        atlas.set_client_rasters(&images(20));
        atlas.set_client_rasters(
            &[
                (
                    LOCAL_PLAYER_HEAD.into(),
                    images(90).remove("client-raster:test").unwrap(),
                ),
                (
                    "client-raster:test".into(),
                    images(20).remove("client-raster:test").unwrap(),
                ),
            ]
            .into(),
        );
        atlas.require([LOCAL_PLAYER_HEAD, "client-raster:test"]);
        assert_eq!(
            atlas.client_rasters[LOCAL_PLAYER_HEAD].identity(),
            portrait.identity()
        );
        assert!(atlas.placement(LOCAL_PLAYER_HEAD).is_some());
        assert!(atlas.placement("client-raster:test").is_some());
        let roster = images(20);
        atlas.take_dirty();
        let (_, allocation) = test_alloc::measure(|| {
            atlas.set_local_player_head(Some(&portrait));
            atlas.set_client_rasters(&roster);
        });
        assert_eq!(allocation.calls, 0);
        assert!(!atlas.take_dirty());
        atlas.set_client_rasters(&BTreeMap::new());
        assert!(atlas.placement(LOCAL_PLAYER_HEAD).is_some());
        assert!(atlas.placement("client-raster:test").is_none());
        atlas.set_local_player_head(None);
        assert!(atlas.placement(LOCAL_PLAYER_HEAD).is_none());
    }

    #[test]
    fn replacement_updates_pixels_in_place_and_unchanged_content_does_no_upload() {
        let key = "client-raster:test";
        let mut atlas = ServerAtlas::new(&[], None, 1);
        atlas.set_client_rasters(&images(20));
        atlas.require([key]);
        let slot = atlas.placement(key).unwrap();
        atlas.take_dirty();
        let before = atlas.images()[0].identity();
        atlas.set_client_rasters(&images(20));
        assert!(!atlas.take_dirty());
        atlas.set_client_rasters(&images(90));
        assert!(atlas.take_dirty());
        assert_eq!(atlas.placement(key).unwrap().rect, slot.rect);
        assert_ne!(before, atlas.images()[0].identity());
        let offset = (slot.rect[1] as usize * PAGE_SIDE as usize + slot.rect[0] as usize) * 4;
        assert_eq!(&atlas.images()[0].pixels()[offset..offset + 4], &[90; 4]);
        atlas.set_client_rasters(&BTreeMap::new());
        assert!(atlas.placement(key).is_none());
    }

    #[test]
    fn evicting_an_old_slot_preserves_a_resized_raster_on_another_page() {
        let a = "client-raster:a";
        let b = "client-raster:b";
        let raster = |side| {
            UiTexturePage::owned([side; 2], vec![255; (side * side * 4) as usize].into()).unwrap()
        };
        let mut atlas = ServerAtlas::new(&[], None, 2);
        atlas.set_client_rasters(&BTreeMap::from([(a.into(), raster(128))]));
        atlas.require([a]);
        atlas.set_client_rasters(&BTreeMap::from([(a.into(), raster(200))]));
        atlas.require([a]);
        let moved = atlas.placement(a).unwrap();
        atlas.set_client_rasters(&BTreeMap::from([
            (a.into(), raster(200)),
            (b.into(), raster(200)),
        ]));
        atlas.require([a, b]);
        assert_eq!(atlas.placement(a).unwrap().page, moved.page);
        assert_ne!(atlas.placement(b).unwrap().page, moved.page);
    }
}
