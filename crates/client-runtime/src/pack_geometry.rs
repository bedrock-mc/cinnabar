//! Captures a stable resident set while the ordinary mesh handoff is paused.
use bevy::prelude::*;
use render_scene::{ChunkRenderInstance, ChunkTextureAssets, ChunkTextureReload};
use std::sync::{Arc, Mutex, mpsc};

type MeshReceiver = Mutex<mpsc::Receiver<Arc<[ChunkRenderInstance]>>>;

#[derive(Default)]
pub struct GeometryPreparation {
    pending: Option<(render_scene::ChunkTextureAssetIdentity, MeshReceiver)>,
    source_tint: Option<meshing::ChunkBiomeTintIdentity>,
}

impl GeometryPreparation {
    /// Prepares all dependent meshes before asking the renderer to stage the replacement.
    /// Returns whether the candidate's GPU completion may authorize publication.
    /// False keeps publication waiting while obsolete work retires or a snapshot is unavailable.
    pub fn request(
        &mut self,
        candidate: &ChunkTextureAssets,
        current: &ChunkTextureAssets,
        stream: Option<&chunk_pipeline::WorldStream>,
        chunks: &Query<&mut ChunkRenderInstance>,
        gpu: &ChunkTextureReload,
    ) -> Result<bool, String> {
        if let Some((identity, receiver)) = &self.pending {
            let tint_changed = *identity == candidate.identity()
                && stream.map(chunk_pipeline::WorldStream::biome_tint_identity) != self.source_tint;
            if *identity == candidate.identity() && !tint_changed {
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
                return Ok(true);
            }
            // Finish a submitted GPU build before reusing the candidate identity. Its result
            // belongs to the old biome snapshot and must never authorize publication.
            if tint_changed
                && gpu
                    .requested()
                    .is_some_and(|requested| requested.identity() == candidate.identity())
                && gpu.status(candidate.identity()).is_none()
            {
                return Ok(false);
            }
            // Dropping a receiver cannot cancel its build; finish it before starting another.
            if matches!(
                receiver
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .try_recv(),
                Err(mpsc::TryRecvError::Empty)
            ) {
                return Ok(false);
            }
            self.pending = None;
            if tint_changed {
                gpu.cancel_except(current.identity());
            }
        }
        let Some(stream) = stream else {
            gpu.request_geometry(
                candidate.clone(),
                chunks.iter().cloned().collect::<Vec<_>>().into(),
            );
            return Ok(true);
        };
        let instances: Vec<_> = chunks.iter().cloned().collect();
        if current.assets().has_same_geometry(candidate.assets())
            && current.assets().biome_assets() == candidate.assets().biome_assets()
        {
            gpu.request_geometry(candidate.clone(), instances.into());
            return Ok(true);
        }
        let old_tint = stream.biome_tint_identity();
        let tint = meshing::ChunkBiomeTintIdentity::new(
            old_tint.stream(),
            old_tint.revision()
                + u64::from(current.assets().biome_assets() != candidate.assets().biome_assets()),
        );
        let snapshot = {
            let _zone = bevy::log::info_span!("pack_apply.mesh_snapshot", chunks = instances.len())
                .entered();
            stream.resource_mesh_snapshot(instances.iter().map(ChunkRenderInstance::key))
        };
        let Some(snapshot) = snapshot else {
            return Ok(false);
        };
        gpu.hold_geometry();
        self.source_tint = Some(old_tint);
        let assets = candidate.assets().clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        self.pending = Some((candidate.identity(), Mutex::new(receiver)));
        std::thread::spawn(move || {
            let _zone =
                bevy::log::info_span!("pack_apply.remesh", chunks = instances.len()).entered();
            // Spread across the world cores, which the held mesh handoff leaves idle.
            let Some(meshes) = chunk_pipeline::on_idle_world_cores(|| snapshot.build(&assets))
            else {
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
        Ok(true)
    }

    /// Replaces the complete CPU set in the same frame that selects its staged GPU resources.
    pub fn publish(
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

    /// Commits the first biome definitions after recording a reload's earlier tint identity.
    fn changed_biomes() -> (chunk_pipeline::WorldStream, meshing::ChunkBiomeTintIdentity) {
        use client_world::ingestion::{BiomeDefinitionEvent, BiomeDefinitionsEvent, WorldEvent};
        let mut stream = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        });
        let before = stream.biome_tint_identity();
        stream
            .submit(
                1,
                WorldEvent::BiomeDefinitions(BiomeDefinitionsEvent {
                    definitions: Arc::from([BiomeDefinitionEvent {
                        biome_id: Some(0),
                        name: "example:reload".into(),
                        temperature: 0.8,
                        downfall: 0.4,
                        snow_foliage: 0.0,
                        max_snow_accumulation: None,
                        map_water_color: 0xff44_6688,
                    }]),
                }),
            )
            .unwrap();
        assert_ne!(before, stream.biome_tint_identity());
        (stream, before)
    }

    #[test]
    fn biome_change_waits_for_cpu_geometry_then_retries() {
        let (stream, before) = changed_biomes();
        let current = ChunkTextureAssets::default();
        let candidate = ChunkTextureAssets::with_revision(Arc::clone(current.assets()), 1);
        let (sender, receiver) = mpsc::sync_channel(1);
        let mut preparation = GeometryPreparation {
            pending: Some((candidate.identity(), Mutex::new(receiver))),
            source_tint: Some(before),
        };
        let mut world = World::new();
        let mut state =
            bevy::ecs::system::SystemState::<Query<&mut ChunkRenderInstance>>::new(&mut world);
        let chunks = state.get_mut(&mut world).unwrap();
        let gpu = ChunkTextureReload::default();
        preparation
            .request(&candidate, &current, Some(&stream), &chunks, &gpu)
            .expect("new biome definitions must not fail the pack reload");
        assert!(gpu.requested().is_none());
        sender.send(Arc::from([])).unwrap();
        preparation
            .request(&candidate, &current, Some(&stream), &chunks, &gpu)
            .expect("retired geometry must be replaced");
        assert!(gpu.geometry().is_some());
        assert!(gpu.status(candidate.identity()).is_none());
    }

    #[test]
    fn biome_change_retires_gpu_geometry_before_reusing_the_candidate() {
        for succeeded in [true, false] {
            let (stream, before) = changed_biomes();
            let current = ChunkTextureAssets::default();
            let candidate = ChunkTextureAssets::with_revision(Arc::clone(current.assets()), 1);
            let (sender, receiver) = mpsc::sync_channel(1);
            drop(sender);
            let mut preparation = GeometryPreparation {
                pending: Some((candidate.identity(), Mutex::new(receiver))),
                source_tint: Some(before),
            };
            let mut world = World::new();
            let mut state =
                bevy::ecs::system::SystemState::<Query<&mut ChunkRenderInstance>>::new(&mut world);
            let chunks = state.get_mut(&mut world).unwrap();
            let gpu = ChunkTextureReload::default();
            gpu.request_geometry(candidate.clone(), Arc::from([]));
            preparation
                .request(&candidate, &current, Some(&stream), &chunks, &gpu)
                .expect("an obsolete GPU build must wait without failing the reload");
            assert!(gpu.geometry_pending());
            gpu.finish(candidate.identity(), succeeded);
            preparation
                .request(&candidate, &current, Some(&stream), &chunks, &gpu)
                .expect("an obsolete GPU result must be replaced");
            assert!(gpu.geometry().is_some());
            assert!(gpu.status(candidate.identity()).is_none());
            gpu.finish(candidate.identity(), true);
            assert_eq!(gpu.status(candidate.identity()), Some(Ok(())));
        }
    }

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
        let chunks = state.get_mut(&mut world).unwrap();
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
