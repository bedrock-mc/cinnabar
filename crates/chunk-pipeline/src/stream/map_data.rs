//! Presentation access to authoritative map images.

use client_world::MapImage;
use client_world::ingestion::MapDataEvent;

use super::WorldStream;

impl WorldStream {
    /// Applies the map update at its ordered commit position.
    pub(super) fn consume_map_data(&mut self, event: &MapDataEvent) {
        self.authority.consume_map_data(event);
    }

    /// The assembled image of `map_id`, if any pixels have arrived in this session.
    #[must_use]
    pub fn map_image(&self, map_id: i64) -> Option<&MapImage> {
        self.authority.map_image(map_id)
    }

    /// Whether the world session already owns this map or its pending request.
    #[must_use]
    pub fn map_is_known(&self, map_id: i64) -> bool {
        self.authority.map_is_known(map_id)
    }

    /// Commits an accepted outbound request to the same owner as received pixels.
    pub fn admit_map_request(&mut self, map_id: i64) {
        self.authority.admit_map_request(map_id);
    }
}
