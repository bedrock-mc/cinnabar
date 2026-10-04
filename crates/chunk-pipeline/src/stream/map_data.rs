//! Presentation access to authoritative map images.

use client_world::MapImage;
use client_world::ingestion::MapDataEvent;

use super::WorldStream;

impl WorldStream {
    /// Applies the map update at its ordered commit position.
    pub(super) fn consume_map_data(&mut self, event: &MapDataEvent) {
        self.authority.consume_map_data(event);
    }

    /// The assembled image of `map_id`, if any pixels have arrived; marks it recently used.
    #[must_use]
    pub fn map_image(&self, map_id: i64) -> Option<&MapImage> {
        self.authority.map_image(map_id)
    }

    /// Retained maps replaced to admit a newer one.
    #[must_use]
    pub const fn replaced_maps(&self) -> u64 {
        self.authority.replaced_maps()
    }
}
