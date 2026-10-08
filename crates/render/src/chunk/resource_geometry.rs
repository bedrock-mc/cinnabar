//! Complete mesh replacements share the atlas transaction's publication boundary.
use super::*;

impl ChunkRenderInstance {
    /// Replaces geometry while preserving the resident entity and its world identity.
    pub fn with_resource_mesh(
        mut self,
        mesh: ChunkMesh,
        biome: PackedBiomeRecord,
        tint_identity: ChunkBiomeTintIdentity,
    ) -> Self {
        self.biome = biome;
        self.tint_identity = tint_identity;
        self.cube_layout = mesh.cube_layout();
        let (cubes, cube_light, models, model_light, draws, transparent, liquids, liquid_light) =
            mesh.into_streams();
        self.cube_quads = cubes.into();
        self.cube_lighting = cube_light.into();
        self.model_refs = models.into();
        self.model_lighting = model_light.into();
        self.model_draw_refs = draws.into();
        self.transparent_model_draw_refs = transparent.into();
        self.depth_liquid_start = liquids
            .iter()
            .position(|quad| quad.is_depth_writing())
            .map(|i| i as u32);
        self.has_depth_liquid = self.depth_liquid_start.is_some();
        self.has_transparent_liquid = liquids.first().is_some_and(|quad| !quad.is_depth_writing());
        self.liquid_quads = liquids.into();
        self.liquid_lighting = liquid_light.into();
        self.token = None;
        self.publication_permit = None;
        self
    }
}

impl ChunkRenderQueue {
    /// Drops old meshes while preserving queued world removals.
    pub fn discard_resource_work(&mut self) {
        for (key, pending) in self.pending.drain() {
            if let Some(generation) = pending.previous_generation {
                self.render_manifest.insert(key, generation);
            } else {
                self.render_manifest.remove(&key);
            }
        }
        self.pending_bytes = 0;
    }
}
