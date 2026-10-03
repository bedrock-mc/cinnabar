use super::*;

/// Immutable neighbourhoods used to build a complete replacement away from the frame thread.
pub struct ResourceMeshSnapshot {
    chunks: Vec<(SubChunkKey, MeshSnapshot)>,
    classifier: BlockClassifier,
    mode: NetworkIdMode,
    definitions: Arc<[client_world::ingestion::BiomeDefinitionEvent]>,
}

impl ResourceMeshSnapshot {
    /// Rebuilds every captured mesh with one asset generation.
    pub fn build(
        self,
        assets: &RuntimeAssets,
    ) -> Option<Vec<(SubChunkKey, ChunkMesh, PackedBiomeRecord)>> {
        let live: Vec<_> = self
            .definitions
            .iter()
            .map(|definition| LiveBiomeDefinition {
                name: &definition.name,
                biome_id: definition.biome_id,
                temperature: definition.temperature,
                downfall: definition.downfall,
                map_water_argb: definition.map_water_color,
            })
            .collect();
        let resolved = assets.biome_assets().resolve_live(&live).ok()?;
        self.chunks
            .into_iter()
            .map(|(key, snapshot)| {
                Some((
                    key,
                    snapshot.mesh(self.classifier, assets, self.mode),
                    pack_biome_record(&snapshot.biomes, &resolved),
                ))
            })
            .collect()
    }
}

impl WorldStream {
    /// Captures resident render neighbourhoods only when their lighting is ready.
    pub fn resource_mesh_snapshot(
        &self,
        keys: impl IntoIterator<Item = SubChunkKey>,
    ) -> Option<ResourceMeshSnapshot> {
        let chunks = keys
            .into_iter()
            .map(|key| {
                Some((
                    key,
                    self.mesh_snapshot(
                        key,
                        self.authority.terrain().sub_chunk(key)?,
                        self.mesh_light_halo(key)?,
                    ),
                ))
            })
            .collect::<Option<Vec<_>>>()?;
        Some(ResourceMeshSnapshot {
            chunks,
            classifier: self.classifier,
            mode: self.authority.network_id_mode(),
            definitions: self.authority.biome_definitions().clone(),
        })
    }

    /// Cinnabar extension: replace visuals without resetting the live world's network sequence.
    pub fn reload_resource_assets(&mut self, assets: Arc<RuntimeAssets>) {
        if Arc::ptr_eq(self.authority.runtime_assets(), &assets) {
            return;
        }
        let geometry_changed = !self.authority.runtime_assets().has_same_geometry(&assets);
        let biomes_changed =
            self.authority.runtime_assets().biome_assets() != assets.biome_assets();
        self.authority.replace_runtime_assets(assets);
        if biomes_changed {
            self.apply_immediate(
                WorldEvent::BiomeDefinitions(client_world::ingestion::BiomeDefinitionsEvent {
                    definitions: self.authority.biome_definitions().clone(),
                }),
                None,
            );
        }
        if !geometry_changed && !biomes_changed {
            return;
        }
        self.mesh_changes.clear();
        let now = Instant::now();
        let resident: Vec<_> = self.resident.iter().copied().collect();
        for key in resident {
            self.mark_dirty_exact(key, now);
        }
    }
}
