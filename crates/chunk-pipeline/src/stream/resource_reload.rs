use super::*;

/// Immutable neighbourhoods used to build a complete replacement away from the frame thread.
pub struct ResourceMeshSnapshot {
    chunks: Vec<(SubChunkKey, MeshSnapshot)>,
    classifier: BlockClassifier,
    mode: NetworkIdMode,
    definitions: Arc<[client_world::ingestion::BiomeDefinitionEvent]>,
}

impl ResourceMeshSnapshot {
    /// Rebuilds every captured mesh with one asset generation, in capture order. Each
    /// neighbourhood meshes independently, so the work spreads across the rayon pool.
    pub fn build(
        self,
        assets: &RuntimeAssets,
    ) -> Option<Vec<(SubChunkKey, ChunkMesh, PackedBiomeRecord)>> {
        use rayon::prelude::*;
        let live: Vec<_> = self
            .definitions
            .iter()
            .map(|definition| LiveBiomeDefinition {
                name: &definition.name,
                biome_id: definition.biome_id,
                temperature: definition.temperature,
                downfall: definition.downfall,
                snow_foliage: definition.snow_foliage,
                max_snow_accumulation: definition.max_snow_accumulation,
                map_water_argb: definition.map_water_color,
            })
            .collect();
        let resolved = assets.biome_assets().resolve_live(&live).ok()?;
        let (classifier, mode) = (self.classifier, self.mode);
        Some(
            self.chunks
                .into_par_iter()
                .map(|(key, snapshot)| {
                    let biome = pack_biome_record(&snapshot.biomes, &resolved);
                    let mesh = snapshot.mesh(classifier, assets, mode, &biome);
                    (key, mesh, biome)
                })
                .collect(),
        )
    }
}

impl WorldStream {
    /// The render choices captured by new mesh jobs and resource-pack snapshots.
    pub fn mesh_render_settings(&self) -> ::meshing::MeshRenderSettings {
        self.mesh_render_settings
    }

    /// Identifies the quality generation, including changes that return to an earlier value.
    pub fn mesh_render_revision(&self) -> u64 {
        self.mesh_render_revision
    }

    /// Rebuilds resident meshes when quality changes, cancelling old jobs and withdrawing
    /// queued results. Forced revisions reject late completions even after an A/B/A toggle.
    pub fn set_mesh_render_settings(
        &mut self,
        settings: ::meshing::MeshRenderSettings,
        now: Instant,
    ) -> bool {
        if self.mesh_render_settings == settings {
            return false;
        }
        self.mesh_render_settings = settings;
        self.mesh_policy_changed(now);
        true
    }

    /// Advances the mesh generation and withdraws work captured under the previous policy.
    fn mesh_policy_changed(&mut self, now: Instant) {
        self.mesh_render_revision += 1;
        self.invalidate_meshes(now);
        self.staged_mesh_completions.clear();
        self.staged_mesh_bytes = 0;
    }

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
        self.biome_records = None;
        self.authority.replace_runtime_assets(assets);
        self.reload_far_terrain_resources();
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
        self.retain_mesh_changes(|_| false);
        let now = Instant::now();
        let resident: Vec<_> = self.resident.iter().copied().collect();
        for key in resident {
            self.mark_dirty_exact(key, now);
        }
    }

    /// Meshes as Java does for a shader pack while `water_filled` (the sorted network values
    /// of the blocks Java fills with water: seagrass, kelp, bubble columns) is set: every leaf
    /// with its cutout, two-sided material, as Java's fancy leaves, and no water faces toward
    /// water-filled blocks. `None` restores Bedrock's opaque deep leaves and water faces.
    /// A change remeshes every resident sub-chunk.
    pub fn set_java_meshing(&mut self, water_filled: Option<Arc<[u32]>>, now: Instant) {
        if self.java_meshing == water_filled {
            return;
        }
        self.java_meshing = water_filled;
        self.mesh_policy_changed(now);
    }

    /// The water-filled network values of Java meshing ([`Self::set_java_meshing`]), if on.
    #[must_use]
    pub fn java_meshing(&self) -> Option<&[u32]> {
        self.java_meshing.as_deref()
    }
}
