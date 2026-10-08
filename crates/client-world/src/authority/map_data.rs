//! Map images assembled from server pixel updates, for framed maps.

use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
};

use protocol::{MAP_IMAGE_SIDE, MapDataEvent};

use super::WorldAuthority;

/// Working-set bound on retained map images; the least recently used map is
/// replaced, and a displayed map that was replaced is requested again.
pub const MAX_RETAINED_MAPS: usize = 64;

/// One map's 128x128 pixels, packed RGBA with red in the low byte; untouched pixels are zero.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MapImage {
    pub pixels: Vec<u32>,
    /// Changes on every applied update.
    pub revision: u64,
}

struct RetainedMap {
    image: MapImage,
    last_used: AtomicU64,
}

#[derive(Default)]
pub(super) struct MapImages {
    maps: BTreeMap<i64, RetainedMap>,
    clock: AtomicU64,
    replaced: u64,
    invalid_rectangles: u64,
}

impl MapImages {
    /// Advances the retained-map usage clock.
    fn tick(&self) -> u64 {
        self.clock.fetch_add(1, Ordering::Relaxed)
    }

    /// Reads a map and records its latest presentation use.
    fn get(&self, map_id: i64) -> Option<&MapImage> {
        let retained = self.maps.get(&map_id)?;
        retained.last_used.store(self.tick(), Ordering::Relaxed);
        Some(&retained.image)
    }

    /// Applies a validated rectangle while preserving the retained image bound.
    fn apply(&mut self, event: &MapDataEvent) {
        if event
            .start_x
            .checked_add(event.width)
            .is_none_or(|end| end > MAP_IMAGE_SIDE)
            || event
                .start_y
                .checked_add(event.height)
                .is_none_or(|end| end > MAP_IMAGE_SIDE)
        {
            self.invalid_rectangles = self.invalid_rectangles.saturating_add(1);
            if self.invalid_rectangles.is_power_of_two() {
                eprintln!(
                    "ignored invalid map rectangles: {}",
                    self.invalid_rectangles
                );
            }
            return;
        }
        let side = MAP_IMAGE_SIDE as usize;
        if !self.maps.contains_key(&event.map_id)
            && self.maps.len() >= MAX_RETAINED_MAPS
            && let Some(victim) = self
                .maps
                .iter()
                .min_by_key(|(_, retained)| retained.last_used.load(Ordering::Relaxed))
                .map(|(&id, _)| id)
        {
            self.maps.remove(&victim);
            self.replaced = self.replaced.saturating_add(1);
        }
        let now = self.tick();
        let retained = self
            .maps
            .entry(event.map_id)
            .or_insert_with(|| RetainedMap {
                image: MapImage {
                    pixels: vec![0; side * side],
                    revision: 0,
                },
                last_used: AtomicU64::new(now),
            });
        *retained.last_used.get_mut() = now;
        let image = &mut retained.image;
        let width = event.width as usize;
        for row in 0..event.height as usize {
            let target = (event.start_y as usize + row) * side + event.start_x as usize;
            let source = row * width;
            let (Some(destination), Some(pixels)) = (
                image.pixels.get_mut(target..target + width),
                event.pixels.get(source..source + width),
            ) else {
                continue;
            };
            destination.copy_from_slice(pixels);
        }
        image.revision = image.revision.wrapping_add(1);
    }
}

impl WorldAuthority {
    /// Applies one map update at its ordered world commit position.
    pub fn consume_map_data(&mut self, event: &MapDataEvent) {
        self.map_images.apply(event);
    }

    /// The assembled image of `map_id`, if any pixels have arrived; marks it recently used.
    #[must_use]
    pub fn map_image(&self, map_id: i64) -> Option<&MapImage> {
        self.map_images.get(map_id)
    }

    /// Retained maps replaced to admit a newer one.
    #[must_use]
    pub const fn replaced_maps(&self) -> u64 {
        self.map_images.replaced
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    /// Builds a small map rectangle independent of installed assets.
    fn event(map_id: i64, start_x: u32, width: u32, value: u32) -> MapDataEvent {
        MapDataEvent {
            map_id,
            start_x,
            start_y: 1,
            width,
            height: 2,
            pixels: Arc::from(vec![value; (width * 2) as usize]),
        }
    }

    #[test]
    fn review_map_rectangles_cannot_wrap_into_the_next_row() {
        let mut images = MapImages::default();
        images.apply(&event(3, 0, 1, 7));
        let original = images.maps[&3].image.clone();
        for start_x in [MAP_IMAGE_SIDE - 1, MAP_IMAGE_SIDE, u32::MAX] {
            images.apply(&event(3, start_x, 2, 9));
            assert_eq!(images.maps[&3].image, original);
        }
        let mut vertical = event(3, 0, 1, 9);
        vertical.start_y = MAP_IMAGE_SIDE;
        images.apply(&vertical);
        assert_eq!(images.maps[&3].image, original);
    }

    #[test]
    fn updates_patch_their_rectangle_and_bump_the_revision() {
        let mut images = MapImages::default();
        images.apply(&event(3, 4, 2, 0xAABBCCDD));
        images.apply(&event(3, 0, 1, 0x11));
        let image = &images.maps[&3].image;
        let side = MAP_IMAGE_SIDE as usize;
        assert_eq!(image.pixels[side + 4], 0xAABBCCDD);
        assert_eq!(image.pixels[2 * side + 5], 0xAABBCCDD);
        assert_eq!(image.pixels[side], 0x11);
        assert_eq!(image.pixels[0], 0);
        assert_eq!(image.revision, 2);
    }

    /// A full working set must replace its least recently used map, never refuse new ones.
    #[test]
    fn new_maps_past_the_budget_replace_the_least_recently_used_map() {
        let mut images = MapImages::default();
        for id in 0..MAX_RETAINED_MAPS as i64 {
            images.apply(&event(id, 0, 1, 1));
        }
        assert!(images.get(0).is_some());
        images.apply(&event(1_000, 0, 1, 1));
        assert!(images.maps.contains_key(&1_000));
        assert!(
            images.maps.contains_key(&0),
            "a displayed map stays resident"
        );
        assert!(!images.maps.contains_key(&1), "the stalest map is replaced");
        assert_eq!(images.maps.len(), MAX_RETAINED_MAPS);
        assert_eq!(images.replaced, 1);
        images.apply(&event(0, 0, 1, 2));
        assert_eq!(images.maps[&0].image.revision, 2);
    }
}
