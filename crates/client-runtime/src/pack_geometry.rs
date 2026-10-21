//! Captures a stable resident set while the ordinary mesh handoff is paused.
use bevy::prelude::*;
use render_scene::{ChunkRenderInstance, ChunkTextureAssets, ChunkTextureReload};
use std::sync::{Arc, Mutex, mpsc};

type GeometrySource = (
    render_scene::ChunkTextureAssetIdentity,
    u64,
    Option<meshing::ChunkBiomeTintIdentity>,
    Option<u64>,
);

type MeshReceiver = Mutex<mpsc::Receiver<Arc<[ChunkRenderInstance]>>>;

#[derive(Default)]
pub struct GeometryPreparation {
    pending: Option<(GeometrySource, MeshReceiver)>,
}

impl GeometryPreparation {
    /// Prepares all dependent meshes before asking the renderer to stage the replacement.
    /// Returns whether the candidate's GPU completion may authorize publication.
    /// False keeps publication waiting while a current snapshot is unavailable.
    pub fn request(
        &mut self,
        candidate: &ChunkTextureAssets,
        current: &ChunkTextureAssets,
        stream: Option<&chunk_pipeline::WorldStream>,
        chunks: &Query<&mut ChunkRenderInstance>,
        gpu: &ChunkTextureReload,
    ) -> Result<bool, String> {
        let epoch = gpu.geometry_epoch();
        let source = (
            candidate.identity(),
            epoch,
            stream.map(chunk_pipeline::WorldStream::biome_tint_identity),
            stream.map(chunk_pipeline::WorldStream::mesh_render_revision),
        );
        if self
            .pending
            .as_ref()
            .is_some_and(|(previous, _)| *previous != source)
        {
            self.pending = None;
            gpu.cancel_except(current.binding_identity());
        }
        if let Some((_, receiver)) = &self.pending {
            match receiver
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .try_recv()
            {
                Ok(geometry) => gpu.request_geometry(epoch, candidate.clone(), geometry),
                Err(mpsc::TryRecvError::Disconnected)
                    if gpu.status(candidate.binding_identity()).is_none()
                        && gpu.geometry().is_none() =>
                {
                    return Err("Resource pack geometry worker stopped".into());
                }
                _ => {}
            }
            return Ok(true);
        }
        let Some(stream) = stream else {
            gpu.request_geometry(
                epoch,
                candidate.clone(),
                chunks.iter().cloned().collect::<Vec<_>>().into(),
            );
            return Ok(true);
        };
        let instances: Vec<_> = chunks.iter().cloned().collect();
        if current.assets().has_same_geometry(candidate.assets())
            && current.assets().biome_assets() == candidate.assets().biome_assets()
        {
            gpu.request_geometry(epoch, candidate.clone(), instances.into());
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
        gpu.hold_geometry(epoch);
        let assets = candidate.assets().clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        self.pending = Some((source, Mutex::new(receiver)));
        let work = move || {
            let _zone =
                bevy::log::info_span!("pack_apply.remesh", chunks = instances.len()).entered();
            // Spread across the world cores, which the held mesh handoff leaves idle.
            #[cfg(not(target_arch = "wasm32"))]
            let meshes = chunk_pipeline::on_idle_world_cores(|| snapshot.build(&assets));
            #[cfg(target_arch = "wasm32")]
            let meshes = snapshot.build(&assets);
            let Some(meshes) = meshes else {
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
        };
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn(work);
        #[cfg(target_arch = "wasm32")]
        crate::browser_jobs::geometry(work);
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
    fn stale_cpu_snapshot_never_blocks_the_new_session() {
        for completed in [false, true] {
            let (stream, _) = changed_biomes();
            let current = ChunkTextureAssets::default();
            let candidate = ChunkTextureAssets::with_revision(Arc::clone(current.assets()), 1);
            let (sender, receiver) = mpsc::sync_channel(1);
            let obsolete: Arc<[ChunkRenderInstance]> = Arc::from([]);
            if completed {
                sender.send(Arc::clone(&obsolete)).unwrap();
            }
            let mut preparation = GeometryPreparation {
                pending: Some((
                    (
                        candidate.identity(),
                        0,
                        Some(stream.biome_tint_identity()),
                        Some(stream.mesh_render_revision()),
                    ),
                    Mutex::new(receiver),
                )),
            };
            let mut world = World::new();
            let mut state =
                bevy::ecs::system::SystemState::<Query<&mut ChunkRenderInstance>>::new(&mut world);
            let chunks = state.get_mut(&mut world).unwrap();
            let gpu = ChunkTextureReload::default();
            gpu.hold_geometry(gpu.geometry_epoch());
            gpu.discard_geometry();
            preparation
                .request(&candidate, &current, Some(&stream), &chunks, &gpu)
                .unwrap();
            assert!(
                gpu.geometry().is_some(),
                "the new session must not wait for an obsolete worker"
            );
            assert!(!Arc::ptr_eq(&obsolete, &gpu.geometry().unwrap()));
            assert!(sender.send(Arc::from([])).is_err());
        }
    }

    #[test]
    fn biome_change_drops_cpu_geometry_without_waiting() {
        let (stream, before) = changed_biomes();
        let current = ChunkTextureAssets::default();
        let candidate = ChunkTextureAssets::with_revision(Arc::clone(current.assets()), 1);
        let (sender, receiver) = mpsc::sync_channel(1);
        let mut preparation = GeometryPreparation {
            pending: Some((
                (
                    candidate.identity(),
                    0,
                    Some(before),
                    Some(stream.mesh_render_revision()),
                ),
                Mutex::new(receiver),
            )),
        };
        let mut world = World::new();
        let mut state =
            bevy::ecs::system::SystemState::<Query<&mut ChunkRenderInstance>>::new(&mut world);
        let chunks = state.get_mut(&mut world).unwrap();
        let gpu = ChunkTextureReload::default();
        preparation
            .request(&candidate, &current, Some(&stream), &chunks, &gpu)
            .expect("new biome definitions must not fail the pack reload");
        assert!(sender.send(Arc::from([])).is_err());
        preparation
            .request(&candidate, &current, Some(&stream), &chunks, &gpu)
            .expect("retired geometry must be replaced");
        assert!(gpu.geometry().is_some());
        assert!(gpu.status(candidate.binding_identity()).is_none());
    }

    #[test]
    fn biome_change_replaces_gpu_geometry_without_waiting() {
        for succeeded in [true, false] {
            let (stream, before) = changed_biomes();
            let current = ChunkTextureAssets::default();
            let candidate = ChunkTextureAssets::with_revision(Arc::clone(current.assets()), 1);
            let (sender, receiver) = mpsc::sync_channel(1);
            drop(sender);
            let mut preparation = GeometryPreparation {
                pending: Some((
                    (
                        candidate.identity(),
                        0,
                        Some(before),
                        Some(stream.mesh_render_revision()),
                    ),
                    Mutex::new(receiver),
                )),
            };
            let mut world = World::new();
            let mut state =
                bevy::ecs::system::SystemState::<Query<&mut ChunkRenderInstance>>::new(&mut world);
            let chunks = state.get_mut(&mut world).unwrap();
            let gpu = ChunkTextureReload::default();
            gpu.request_geometry(gpu.geometry_epoch(), candidate.clone(), Arc::from([]));
            let obsolete = gpu.preparation().unwrap();
            preparation
                .request(&candidate, &current, Some(&stream), &chunks, &gpu)
                .expect("an obsolete GPU build must not block the reload");
            assert!(gpu.geometry_pending());
            assert!(!gpu.finish(&obsolete, succeeded));
            preparation
                .request(&candidate, &current, Some(&stream), &chunks, &gpu)
                .expect("an obsolete GPU result must be replaced");
            assert!(gpu.geometry().is_some());
            assert!(gpu.status(candidate.binding_identity()).is_none());
            assert!(!gpu.finish(&obsolete, true));
            gpu.finish(&gpu.preparation().unwrap(), true);
            assert_eq!(gpu.status(candidate.binding_identity()), Some(Ok(())));
        }
    }

    #[test]
    fn quality_round_trip_drops_cpu_geometry_without_waiting() {
        let (mut stream, _) = changed_biomes();
        let current = ChunkTextureAssets::default();
        let candidate = ChunkTextureAssets::with_revision(Arc::clone(current.assets()), 1);
        let (sender, receiver) = mpsc::sync_channel(1);
        let mut preparation = GeometryPreparation {
            pending: Some((
                (
                    candidate.identity(),
                    0,
                    Some(stream.biome_tint_identity()),
                    Some(stream.mesh_render_revision()),
                ),
                Mutex::new(receiver),
            )),
        };
        let original = stream.mesh_render_settings();
        let cheap = meshing::MeshRenderSettings {
            smooth_lighting: false,
            transparent_leaves: false,
        };
        let now = std::time::Instant::now();
        assert!(stream.set_mesh_render_settings(cheap, now));
        assert!(stream.set_mesh_render_settings(original, now));
        let mut world = World::new();
        let mut state =
            bevy::ecs::system::SystemState::<Query<&mut ChunkRenderInstance>>::new(&mut world);
        let chunks = state.get_mut(&mut world).unwrap();
        let gpu = ChunkTextureReload::default();
        preparation
            .request(&candidate, &current, Some(&stream), &chunks, &gpu)
            .unwrap();
        assert!(sender.send(Arc::from([])).is_err());
        assert!(gpu.geometry().is_some());
    }

    #[test]
    fn java_mode_round_trip_drops_pending_resource_snapshot_without_waiting() {
        let (mut stream, _) = changed_biomes();
        let current = ChunkTextureAssets::default();
        let candidate = ChunkTextureAssets::with_revision(Arc::clone(current.assets()), 1);
        let (sender, receiver) = mpsc::sync_channel(1);
        let mut preparation = GeometryPreparation {
            pending: Some((
                (
                    candidate.identity(),
                    0,
                    Some(stream.biome_tint_identity()),
                    Some(stream.mesh_render_revision()),
                ),
                Mutex::new(receiver),
            )),
        };
        let now = std::time::Instant::now();
        stream.set_java_meshing(Some(Arc::from([1_u32])), now);
        stream.set_java_meshing(None, now);
        let mut world = World::new();
        let mut state =
            bevy::ecs::system::SystemState::<Query<&mut ChunkRenderInstance>>::new(&mut world);
        let chunks = state.get_mut(&mut world).unwrap();
        let gpu = ChunkTextureReload::default();
        preparation
            .request(&candidate, &current, Some(&stream), &chunks, &gpu)
            .unwrap();
        assert!(sender.send(Arc::from([])).is_err());
        assert!(gpu.geometry().is_some());
    }

    #[test]
    fn superseded_geometry_drops_the_existing_worker() {
        let current = ChunkTextureAssets::default();
        let candidate = ChunkTextureAssets::with_revision(Arc::clone(current.assets()), 1);
        let (sender, receiver) = mpsc::sync_channel(1);
        let mut preparation = GeometryPreparation {
            pending: Some(((current.identity(), 0, None, None), Mutex::new(receiver))),
        };
        let mut world = World::new();
        let mut state =
            bevy::ecs::system::SystemState::<Query<&mut ChunkRenderInstance>>::new(&mut world);
        let chunks = state.get_mut(&mut world).unwrap();
        let gpu = ChunkTextureReload::default();
        preparation
            .request(&candidate, &current, None, &chunks, &gpu)
            .unwrap();
        assert!(sender.send(Arc::from([])).is_err());
        preparation
            .request(&candidate, &current, None, &chunks, &gpu)
            .unwrap();
        assert!(preparation.pending.is_none());
        assert!(gpu.geometry().is_some());
    }
}
