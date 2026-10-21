//! Prepared screen artwork shares the bounded atlas without PNG encode/decode work.

use super::*;

/// Reserved texture for the host local player portrait, including its hat layer; it is its own namespace.
pub const LOCAL_PLAYER_HEAD: &str = "client-raster:local-player-head";

impl ServerAtlas {
    /// Updates the single host-owned local head without replacing other painters' rasters.
    pub fn set_local_player_head(&mut self, image: Option<&UiTexturePage>) {
        let previous = self.client_rasters.get(LOCAL_PLAYER_HEAD);
        if previous.map(|p| (p.identity(), p.dimensions()))
            == image.map(|p| (p.identity(), p.dimensions()))
        {
            return;
        }
        let images = image
            .map(|image| BTreeMap::from([(LOCAL_PLAYER_HEAD.to_owned(), image.clone())]))
            .unwrap_or_default();
        self.set_client_rasters(LOCAL_PLAYER_HEAD, &images);
    }

    /// Checks the shared tile and byte budget before a painter allocates a namespace's pixels.
    pub(crate) fn admits_client_rasters(
        &self,
        namespace: &str,
        count: usize,
        bytes: usize,
    ) -> bool {
        let (other_count, other_bytes) = self
            .client_rasters
            .iter()
            .filter(|(key, _)| !key.starts_with(namespace))
            .fold((0, 0), |(count, bytes), (_, image)| {
                (count + 1, bytes + image.pixels().len())
            });
        namespace.starts_with("client-raster:")
            && count <= MAX_EXTRA.saturating_sub(other_count)
            && bytes <= MAX_DECODED_BYTES.saturating_sub(other_bytes)
    }

    /// Replaces one namespace of client rasters while preserving other painters' artwork. Keys use the reserved `client-raster:` prefix;
    /// images must fit an atlas page and the shared decoded-image budget. Unchanged
    /// content does no work; changed residents keep their slot when dimensions match.
    pub fn set_client_rasters(
        &mut self,
        namespace: &str,
        images: &BTreeMap<String, UiTexturePage>,
    ) {
        if images.keys().any(|key| !key.starts_with(namespace))
            || !self.admits_client_rasters(
                namespace,
                images.len(),
                images.values().map(|p| p.pixels().len()).sum(),
            )
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
        if self
            .client_rasters
            .keys()
            .filter(|key| key.starts_with(namespace))
            .count()
            == images.len()
            && images.iter().all(|(key, image)| {
                self.client_rasters
                    .get(key)
                    .is_some_and(|old| same(old, image))
            })
        {
            return;
        }
        let metadata_changed = self
            .client_rasters
            .iter()
            .filter(|(key, _)| key.starts_with(namespace))
            .count()
            != images.len()
            || images.iter().any(|(key, image)| {
                self.client_rasters
                    .get(key)
                    .is_none_or(|old| old.dimensions() != image.dimensions())
            });
        let mut changed = Vec::new();
        for (key, old) in self
            .client_rasters
            .iter()
            .filter(|(key, _)| key.starts_with(namespace))
        {
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
        self.client_rasters
            .retain(|key, _| !key.starts_with(namespace));
        self.client_rasters.extend(
            images
                .iter()
                .map(|(key, image)| (key.clone(), image.clone())),
        );
        if metadata_changed {
            self.metadata_revision = self.metadata_revision.wrapping_add(1);
            self.metadata_dirty = true;
        }
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
        let roster_namespace = "client-raster:test";
        let mut atlas = ServerAtlas::new(&[], None, 1);
        let portrait = images(50).remove("client-raster:test").unwrap();
        atlas.set_local_player_head(Some(&portrait));
        atlas.set_client_rasters(roster_namespace, &images(20));
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
            atlas.set_client_rasters(roster_namespace, &roster);
        });
        assert_eq!(allocation.calls, 0);
        assert!(!atlas.take_dirty());
        atlas.set_client_rasters(roster_namespace, &BTreeMap::new());
        assert!(atlas.placement(LOCAL_PLAYER_HEAD).is_some());
        assert!(atlas.placement("client-raster:test").is_none());
        atlas.set_local_player_head(None);
        assert!(atlas.placement(LOCAL_PLAYER_HEAD).is_none());
    }

    #[test]
    fn replacement_updates_pixels_in_place_and_unchanged_content_does_no_upload() {
        let key = "client-raster:test";
        let mut atlas = ServerAtlas::new(&[], None, 1);
        atlas.set_client_rasters("client-raster:", &images(20));
        atlas.require([key]);
        let slot = atlas.placement(key).unwrap();
        atlas.take_dirty();
        let before = atlas.images()[0].identity();
        atlas.set_client_rasters("client-raster:", &images(20));
        assert!(!atlas.take_dirty());
        atlas.set_client_rasters("client-raster:", &images(90));
        assert!(atlas.take_dirty());
        assert_eq!(atlas.placement(key).unwrap().rect, slot.rect);
        assert_ne!(before, atlas.images()[0].identity());
        let offset = (slot.rect[1] as usize * PAGE_SIDE as usize + slot.rect[0] as usize) * 4;
        assert_eq!(&atlas.images()[0].pixels()[offset..offset + 4], &[90; 4]);
        atlas.set_client_rasters("client-raster:", &BTreeMap::new());
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
        atlas.set_client_rasters("client-raster:", &BTreeMap::from([(a.into(), raster(128))]));
        atlas.require([a]);
        atlas.set_client_rasters("client-raster:", &BTreeMap::from([(a.into(), raster(200))]));
        atlas.require([a]);
        let moved = atlas.placement(a).unwrap();
        atlas.set_client_rasters(
            "client-raster:",
            &BTreeMap::from([(a.into(), raster(200)), (b.into(), raster(200))]),
        );
        atlas.require([a, b]);
        assert_eq!(atlas.placement(a).unwrap().page, moved.page);
        assert_ne!(atlas.placement(b).unwrap().page, moved.page);
    }
    #[test]
    fn namespaces_share_the_existing_raster_admission_budget() {
        let mut atlas = ServerAtlas::new(&[], None, 1);
        let image = UiTexturePage::owned([1; 2], vec![255; 4].into()).unwrap();
        let first: BTreeMap<_, _> = (0..MAX_EXTRA)
            .map(|index| (format!("client-raster:first/{index}"), image.clone()))
            .collect();
        atlas.set_client_rasters("client-raster:first/", &first);
        atlas.set_client_rasters(
            "client-raster:second/",
            &BTreeMap::from([("client-raster:second/one".into(), image)]),
        );
        assert_eq!(atlas.client_rasters.len(), MAX_EXTRA);
        assert!(
            !atlas
                .client_rasters
                .contains_key("client-raster:second/one")
        );
    }

    #[test]
    fn replacing_one_namespace_preserves_other_painters_and_content_updates_keep_metadata() {
        let mut atlas = ServerAtlas::new(&[], None, 1);
        let raster = |key: &str, color| {
            BTreeMap::from([(
                key.to_owned(),
                UiTexturePage::owned([8; 2], vec![color; 8 * 8 * 4].into()).unwrap(),
            )])
        };
        let portrait = "client-raster:portraits/head";
        let mask = "client-raster:masks/row";
        atlas.set_client_rasters("client-raster:portraits/", &raster(portrait, 20));
        atlas.set_client_rasters("client-raster:masks/", &raster(mask, 90));
        atlas.require([portrait, mask]);
        let revision = atlas.metadata_revision();
        atlas.set_client_rasters("client-raster:masks/", &raster(mask, 120));
        assert_eq!(atlas.metadata_revision(), revision);
        assert!(atlas.placement(portrait).is_some());
        atlas.set_client_rasters("client-raster:masks/", &BTreeMap::new());
        assert!(atlas.placement(mask).is_none());
        assert!(atlas.placement(portrait).is_some());
    }
}
