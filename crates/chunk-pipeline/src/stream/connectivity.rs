use super::*;

impl WorldStream {
    pub(super) fn set_connectivity(&mut self, key: SubChunkKey, value: Option<FaceConnectivity>) {
        let changed = match value {
            Some(value) => self.connectivity.insert(key, value) != Some(value),
            None => self.connectivity.remove(&key).is_some(),
        };
        if changed {
            self.bump_connectivity_generation();
        }
    }
    pub(super) fn bump_connectivity_generation(&mut self) {
        self.connectivity_generation = self.connectivity_generation.wrapping_add(1).max(1);
    }

    /// Lays the cave graph out for the retained view before it fills, so loading the view never
    /// re-lays it out part way, which would rebuild every cave traversal.
    pub(super) fn reserve_connectivity_for_view(&mut self) {
        let (Some(radius), Some(range)) = (
            self.retained_radius(),
            self.authority.dimension_range(self.current_dimension()),
        ) else {
            return;
        };
        self.connectivity.reserve_view(
            radius.max(0).unsigned_abs(),
            u32::try_from(range.sub_chunk_count).unwrap_or(u32::MAX),
        );
    }
}
