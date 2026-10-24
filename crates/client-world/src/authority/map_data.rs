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
    maps: BTreeMap<i64, Option<MapImage>>,
    invalid_rectangles: u64,
}

impl MapImages {
    /// Reads the authoritative pixels retained for the current session.
    fn get(&self, map_id: i64) -> Option<&MapImage> {
        self.maps.get(&map_id)?.as_ref()
    }

    /// Allocates an unpainted saved map once, preserving pixels already received.
    fn saved(&mut self, map_id: i64) -> &mut MapImage {
        self.maps
            .entry(map_id)
            .or_default()
            .get_or_insert_with(|| MapImage {
                pixels: vec![0; MAP_IMAGE_SIDE as usize * MAP_IMAGE_SIDE as usize],
                revision: 0,
            })
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
        for &id in event.initialized_ids.iter() {
            self.saved(id);
        }
        let image = self.saved(event.map_id);
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

    /// The assembled image of `map_id`, once saved map data has arrived in this session.
    #[must_use]
    pub fn map_image(&self, map_id: i64) -> Option<&MapImage> {
        self.map_images.get(map_id)
    }

    /// Whether this session has received the map or admitted a request for it.
    #[must_use]
    pub fn map_is_known(&self, map_id: i64) -> bool {
        self.map_images.maps.contains_key(&map_id)
    }

    /// Records an admitted request without replacing pixels already received.
    pub fn admit_map_request(&mut self, map_id: i64) {
        self.map_images.maps.entry(map_id).or_default();
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
            initialized_ids: Default::default(),
            start_x,
            start_y: 1,
            width,
            height: 2,
            pixels: Arc::from(vec![value; (width * 2) as usize]),
        }
    }

    #[test]
    fn initialization_saves_parent_maps_and_keeps_received_pixels() {
        let protocol::WorldEvent::MapData(creation) =
            protocol::test_support::packets::map_creation(&[266, 267], 3)
        else {
            panic!("map initialization must reach its data owner");
        };
        let mut images = MapImages::default();
        images.apply(&creation);
        assert!(
            images.get(266).is_some() && images.get(267).is_some(),
            "initialization creates saved data for every declared map"
        );
        images.apply(&event(267, 0, 1, 0x44332211));
        let painted = images.get(267).unwrap().pixels.clone();
        images.apply(&creation);
        assert_eq!(images.get(267).unwrap().pixels, painted);
    }

    #[test]
    fn saved_maps_can_exist_before_pixels_and_keep_later_updates() {
        let mut images = MapImages::default();
        images.maps.insert(3, None);
        assert!(images.get(3).is_none(), "a request alone has no saved data");
        let empty = MapDataEvent {
            map_id: 3,
            initialized_ids: Default::default(),
            start_x: 0,
            start_y: 0,
            width: 0,
            height: 0,
            pixels: Default::default(),
        };
        images.apply(&empty);
        assert!(
            images
                .get(3)
                .unwrap()
                .pixels
                .iter()
                .all(|pixel| *pixel == 0)
        );
        images.apply(&event(3, 0, 1, 0x44332211));
        let painted = images.get(3).unwrap().pixels.clone();
        images.apply(&empty);
        assert_eq!(images.get(3).unwrap().pixels, painted);
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
        let original = images.get(3).unwrap().clone();
        for start_x in [MAP_IMAGE_SIDE - 1, MAP_IMAGE_SIDE, u32::MAX] {
            images.apply(&event(3, start_x, 2, 9));
            assert_eq!(images.get(3), Some(&original));
        }
        let mut vertical = event(3, 0, 1, 9);
        vertical.start_y = MAP_IMAGE_SIDE;
        images.apply(&vertical);
        assert_eq!(images.get(3), Some(&original));
    }

    #[test]
    fn updates_patch_their_rectangle_and_bump_the_revision() {
        let mut images = MapImages::default();
        images.apply(&event(3, 4, 2, 0xAABBCCDD));
        images.apply(&event(3, 0, 1, 0x11));
        let image = images.get(3).unwrap();
        let side = MAP_IMAGE_SIDE as usize;
        assert_eq!(image.pixels[side + 4], 0xAABBCCDD);
        assert_eq!(image.pixels[2 * side + 5], 0xAABBCCDD);
        assert_eq!(image.pixels[side], 0x11);
        assert_eq!(image.pixels[0], 0);
        assert_eq!(image.revision, 2);
    }
}
