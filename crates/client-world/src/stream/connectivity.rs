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
}
