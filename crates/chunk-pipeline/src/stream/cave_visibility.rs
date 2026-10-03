use super::*;

impl WorldStream {
    /// Reuses the caller's visibility buffers for the current connectivity graph.
    pub fn cave_visible_sub_chunks_into(
        &self,
        camera: SubChunkKey,
        scratch: &mut crate::CaveVisibilityScratch,
        visible: &mut HashSet<SubChunkKey>,
    ) {
        crate::culling::fill_visible(camera, &self.connectivity, scratch, visible);
    }
}
