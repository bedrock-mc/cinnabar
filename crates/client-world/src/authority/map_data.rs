//! Authoritative map images assembled from server pixel updates.

use std::collections::BTreeMap;

use protocol::{MAP_IMAGE_SIDE, MapDataEvent};

use super::WorldAuthority;

/// One map's 128x128 pixels, packed RGBA with red in the low byte; untouched pixels are zero.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MapImage {
    pub pixels: Vec<u32>,
    /// Changes on every applied update.
    pub revision: u64,
}

#[derive(Default)]
pub(super) struct MapImages {
    maps: BTreeMap<i64, MapImage>,
    replaced: u64,
    invalid_rectangles: u64,
}

impl MapImages {
    /// Reads the authoritative pixels retained for the current session.
    fn get(&self, map_id: i64) -> Option<&MapImage> {
        self.maps.get(&map_id)
    }

    /// Applies a validated rectangle without discarding any earlier map image.
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
                tracing::warn!(
                    "ignored invalid map rectangles: {}",
                    self.invalid_rectangles
                );
            }
            return;
        }
        let side = MAP_IMAGE_SIDE as usize;
        let image = self.maps.entry(event.map_id).or_insert_with(|| MapImage {
            pixels: vec![0; side * side],
            revision: 0,
        });
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

    /// The assembled image of `map_id`, if any pixels have arrived in this session.
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
    fn earlier_map_pixels_survive_more_than_one_working_set_of_other_maps() {
        let mut images = MapImages::default();
        images.apply(&event(266, 0, 1, 0x44332211));
        for id in 0..65 {
            images.apply(&event(id, 0, 1, 0x88776655));
        }
        assert_eq!(
            images
                .get(266)
                .map(|image| image.pixels[MAP_IMAGE_SIDE as usize]),
            Some(0x44332211),
            "an accepted map remains available without another request"
        );
    }

    #[test]
    fn review_map_rectangles_cannot_wrap_into_the_next_row() {
        let mut images = MapImages::default();
        images.apply(&event(3, 0, 1, 7));
        let original = images.maps[&3].clone();
        for start_x in [MAP_IMAGE_SIDE - 1, MAP_IMAGE_SIDE, u32::MAX] {
            images.apply(&event(3, start_x, 2, 9));
            assert_eq!(images.maps[&3], original);
        }
        let mut vertical = event(3, 0, 1, 9);
        vertical.start_y = MAP_IMAGE_SIDE;
        images.apply(&vertical);
        assert_eq!(images.maps[&3], original);
    }

    #[test]
    fn updates_patch_their_rectangle_and_bump_the_revision() {
        let mut images = MapImages::default();
        images.apply(&event(3, 4, 2, 0xAABBCCDD));
        images.apply(&event(3, 0, 1, 0x11));
        let image = &images.maps[&3];
        let side = MAP_IMAGE_SIDE as usize;
        assert_eq!(image.pixels[side + 4], 0xAABBCCDD);
        assert_eq!(image.pixels[2 * side + 5], 0xAABBCCDD);
        assert_eq!(image.pixels[side], 0x11);
        assert_eq!(image.pixels[0], 0);
        assert_eq!(image.revision, 2);
    }
}
