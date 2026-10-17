//! Applied empty meshes remain resident even though they need no draw entity.
use bevy::prelude::Resource;
use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use world::SubChunkKey;

#[derive(Default)]
struct Coverage {
    keys: Mutex<HashSet<SubChunkKey>>,
    revision: AtomicU64,
}

#[derive(Resource, Clone, Default)]
pub(crate) struct ChunkResidentCoverage(Arc<Coverage>);

impl ChunkResidentCoverage {
    /// Updates known resident coverage and advances its revision when the keys change.
    pub(crate) fn set(&self, key: SubChunkKey, present: bool) {
        let mut keys = self
            .0
            .keys
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let changed = if present {
            keys.insert(key)
        } else {
            keys.remove(&key)
        };
        if changed {
            self.0.revision.fetch_add(1, Ordering::Release);
        }
    }
    /// Retires all known resident coverage when the owning world is cleared.
    pub(crate) fn clear(&self) {
        let mut keys = self
            .0
            .keys
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if !keys.is_empty() {
            keys.clear();
            self.0.revision.fetch_add(1, Ordering::Release);
        }
    }
    /// Returns the revision of the retained resident coverage.
    pub(crate) fn revision(&self) -> u64 {
        self.0.revision.load(Ordering::Acquire)
    }
    /// Reads the known resident keys while holding their shared storage lock.
    pub(crate) fn with_keys<T>(&self, read: impl FnOnce(&HashSet<SubChunkKey>) -> T) -> T {
        read(
            &self
                .0
                .keys
                .lock()
                .unwrap_or_else(|poison| poison.into_inner()),
        )
    }
}

impl bevy::render::extract_resource::ExtractResource<bevy::render::RenderApp>
    for ChunkResidentCoverage
{
    type Source = Self;
    /// Shares resident coverage with the render world without copying its key set.
    fn extract_resource(source: &Self) -> Self {
        source.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_chunk_admission_is_retained_and_shared_without_rebuilds() {
        let coverage = ChunkResidentCoverage::default();
        let extracted = coverage.clone();
        let key = SubChunkKey::new(0, 2, 4, -3);
        assert_eq!(coverage.revision(), 0);
        coverage.set(key, true);
        let revision = coverage.revision();
        assert!(extracted.with_keys(|keys| keys.contains(&key)));
        coverage.set(key, true);
        assert_eq!(coverage.revision(), revision);
        coverage.set(key, false);
        assert!(extracted.with_keys(HashSet::is_empty));
        let revision = coverage.revision();
        coverage.clear();
        assert_eq!(coverage.revision(), revision);
    }
    #[test]
    fn applied_empty_mesh_removal_and_session_reset_update_coverage() {
        use crate::chunk::ChunkRenderPlugin;
        use bevy::prelude::*;
        use render_scene::{ChunkRenderQueue, ChunkUploadPriority};
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(ChunkRenderPlugin::new(1));
        let key = SubChunkKey::new(0, 1, 2, 3);
        let coverage = app.world().resource::<ChunkResidentCoverage>().clone();
        app.world_mut()
            .resource_mut::<ChunkRenderQueue>()
            .try_insert(
                key,
                meshing::ChunkMesh::default(),
                ChunkUploadPriority::new(0.0),
            )
            .unwrap();
        assert!(
            !coverage.with_keys(|keys| keys.contains(&key)),
            "queued geometry is not yet resident"
        );
        app.update();
        assert!(
            coverage.with_keys(|keys| keys.contains(&key)),
            "applied empty meshes remain known air"
        );
        let revision = coverage.revision();
        app.update();
        assert_eq!(coverage.revision(), revision);
        app.world_mut()
            .resource_mut::<ChunkRenderQueue>()
            .try_remove(key)
            .unwrap();
        app.update();
        assert!(!coverage.with_keys(|keys| keys.contains(&key)));
        app.world_mut()
            .resource_mut::<ChunkRenderQueue>()
            .try_insert(
                key,
                meshing::ChunkMesh::default(),
                ChunkUploadPriority::new(0.0),
            )
            .unwrap();
        app.update();
        app.world_mut()
            .resource_mut::<ChunkRenderQueue>()
            .reset_session();
        app.update();
        assert!(
            coverage.with_keys(HashSet::is_empty),
            "world reset drops all prior dimensions"
        );
    }
}
