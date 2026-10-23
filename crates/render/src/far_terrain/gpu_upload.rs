//! Tile uploads own their reserved storage until publication succeeds.

use super::*;

/// Returns a tile's slot and quad range unless its upload becomes resident.
pub(super) struct TileUpload<'a> {
    pub(super) gpu: &'a mut FarTerrainGpu,
    pub(super) slot: u32,
    pub(super) quads: Option<Range<u32>>,
}

impl TileUpload<'_> {
    /// Publishes the reservation only after every tile write succeeds.
    pub(super) fn commit(
        mut self,
        device: &RenderDevice,
        queue: &RenderQueue,
        mesh: &Arc<TileMesh>,
        bytes: u64,
    ) -> Result<(), crate::gpu_allocation::TerrainWriteError> {
        crate::gpu_allocation::terrain_write(device, "far terrain tile upload", bytes, || {
            self.gpu.upload(
                queue,
                mesh,
                self.slot,
                self.quads.as_ref().expect("reserved quads"),
            );
        })?;
        self.gpu.resident.insert(
            mesh.key,
            Resident {
                mesh: Arc::clone(mesh),
                slot: self.slot,
                quads: self.quads.take().expect("reserved quads"),
                visible: false,
            },
        );
        Ok(())
    }
}

impl Drop for TileUpload<'_> {
    /// Returns unpublished storage even if the checked upload exits early.
    fn drop(&mut self) {
        if let Some(quads) = self.quads.take() {
            self.gpu.ranges.release(quads);
            self.gpu.free_slots.push(self.slot);
        }
    }
}
