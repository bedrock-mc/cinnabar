//! Captures a stable resident set while the ordinary mesh handoff is paused.
use bevy::prelude::*;
use render::{ChunkRenderInstance, ChunkTextureAssets, ChunkTextureReload};
use std::sync::{Arc, Mutex, mpsc};

type MeshReceiver = Mutex<mpsc::Receiver<Arc<[ChunkRenderInstance]>>>;

#[derive(Default)]
pub(super) struct GeometryPreparation {
    pending: Option<(render::ChunkTextureAssetIdentity, MeshReceiver)>,
    source_tint: Option<meshing::ChunkBiomeTintIdentity>,
}

impl GeometryPreparation {
    /// Prepares all dependent meshes before asking the renderer to stage the replacement.
    pub(super) fn request(
        &mut self,
        candidate: &ChunkTextureAssets,
        current: &ChunkTextureAssets,
        stream: Option<&chunk_pipeline::WorldStream>,
        chunks: &Query<&mut ChunkRenderInstance>,
        gpu: &ChunkTextureReload,
    ) -> Result<(), String> {
        if let Some((identity, receiver)) = &self.pending {
            if *identity == candidate.identity() {
                if stream.map(chunk_pipeline::WorldStream::biome_tint_identity) != self.source_tint
                {
                    return Err(
                        "World biomes changed while preparing resource packs; apply again".into(),
                    );
                }
                match receiver
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .try_recv()
                {
                    Ok(geometry) => gpu.request_geometry(candidate.clone(), geometry),
                    Err(mpsc::TryRecvError::Disconnected)
                        if gpu.status(candidate.identity()).is_none()
                            && gpu.geometry().is_none() =>
                    {
                        return Err("Resource pack geometry worker stopped".into());
                    }
                    _ => {}
                }
                return Ok(());
            }
            // Dropping a receiver cannot cancel its build; finish it before starting another.
            if matches!(
                receiver
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .try_recv(),
                Err(mpsc::TryRecvError::Empty)
            ) {
                return Ok(());
            }
            self.pending = None;
        }
        let Some(stream) = stream else {
            gpu.request_geometry(
                candidate.clone(),
                chunks.iter().cloned().collect::<Vec<_>>().into(),
            );
            return Ok(());
        };
        let instances: Vec<_> = chunks.iter().cloned().collect();
        if current.assets().has_same_geometry(candidate.assets())
            && current.assets().biome_assets() == candidate.assets().biome_assets()
        {
            gpu.request_geometry(candidate.clone(), instances.into());
            return Ok(());
        }
        let old_tint = stream.biome_tint_identity();
        let tint = meshing::ChunkBiomeTintIdentity::new(
            old_tint.stream(),
            old_tint.revision()
                + u64::from(current.assets().biome_assets() != candidate.assets().biome_assets()),
        );
        let Some(snapshot) =
            stream.resource_mesh_snapshot(instances.iter().map(ChunkRenderInstance::key))
        else {
            return Ok(());
        };
        gpu.hold_geometry();
        self.source_tint = Some(old_tint);
        let assets = candidate.assets().clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        self.pending = Some((candidate.identity(), Mutex::new(receiver)));
        std::thread::spawn(move || {
            let Some(meshes) = snapshot.build(&assets) else {
                return;
            };
            let mut meshes: std::collections::BTreeMap<_, _> = meshes
                .into_iter()
                .map(|(key, mesh, biome)| (key, (mesh, biome)))
                .collect();
            let instances = instances
                .into_iter()
                .map(|instance| {
                    let (mesh, biome) = meshes
                        .remove(&instance.key())
                        .expect("snapshot covers each resident instance");
                    instance.with_resource_mesh(mesh, biome, tint)
                })
                .collect::<Vec<_>>();
            let _ = sender.send(Arc::from(instances));
        });
        Ok(())
    }

    /// Replaces the complete CPU set in the same frame that selects its staged GPU resources.
    pub(super) fn publish(
        &mut self,
        chunks: &mut Query<&mut ChunkRenderInstance>,
        gpu: &ChunkTextureReload,
    ) -> bool {
        self.pending = None;
        let Some(geometry) = gpu.geometry() else {
            return false;
        };
        let by_key: std::collections::BTreeMap<_, _> = geometry
            .iter()
            .map(|instance| (instance.key(), instance))
            .collect();
        for mut instance in chunks.iter_mut() {
            if let Some(next) = by_key.get(&instance.key()) {
                *instance = (*next).clone();
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_superseded_geometry_waits_for_the_existing_worker() {
        let current = ChunkTextureAssets::default();
        let candidate = ChunkTextureAssets::with_revision(Arc::clone(current.assets()), 1);
        let (sender, receiver) = mpsc::sync_channel(1);
        let mut preparation = GeometryPreparation {
            pending: Some((current.identity(), Mutex::new(receiver))),
            source_tint: None,
        };
        let mut world = World::new();
        let mut state =
            bevy::ecs::system::SystemState::<Query<&mut ChunkRenderInstance>>::new(&mut world);
        let chunks = state.get_mut(&mut world);
        let gpu = ChunkTextureReload::default();
        preparation
            .request(&candidate, &current, None, &chunks, &gpu)
            .unwrap();
        assert_eq!(
            preparation.pending.as_ref().map(|(identity, _)| *identity),
            Some(current.identity())
        );
        assert!(gpu.geometry().is_none());
        sender.send(Arc::from([])).unwrap();
        preparation
            .request(&candidate, &current, None, &chunks, &gpu)
            .unwrap();
        assert!(preparation.pending.is_none());
        assert!(gpu.geometry().is_some());
    }
}
