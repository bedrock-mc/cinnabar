use super::*;

impl WorldStream {
    /// Proves a box hidden by exposed opaque faces in current uploaded terrain. An internal
    /// voxel cannot certify depth: its neighbour may have removed its face from the mesh.
    /// `drawn` must check section visibility; the caller must validate the camera's near clip.
    pub fn terrain_hides_box(
        &self,
        budget: &mut ::meshing::occlusion::OcclusionBudget,
        eye: [f32; 3],
        low: [f32; 3],
        high: [f32; 3],
        drawn: impl Fn([i32; 3], SubChunkKey) -> bool,
    ) -> bool {
        budget.hides_box(eye, low, high, |position, neighbour| {
            let Some((key, cube)) = self.occlusion_cell(position) else {
                return false;
            };
            self.is_mesh_clean(key)
                && self.applied_mesh_generations.contains_key(&key)
                && cube.is_opaque_cube(self.runtime_assets())
                && self
                    .occlusion_cell(neighbour)
                    .is_some_and(|(_, cell)| cell.is_empty())
                && drawn(position, key)
        })
    }

    /// Resolves one current cell, keeping authoritative air distinct from missing terrain.
    fn occlusion_cell(
        &self,
        position: [i32; 3],
    ) -> Option<(SubChunkKey, ::meshing::ResolvedContributors)> {
        let [x, y, z] = position.map(|axis| axis.div_euclid(world::SUB_CHUNK_SIDE as i32));
        let key = SubChunkKey::new(self.current_dimension(), x, y, z);
        let terrain = self.authority.terrain();
        let contributors = match terrain.sub_chunk(key) {
            Some(sub_chunk) => ::meshing::ContributorResolver::resolve_direct(
                self.classifier,
                self.runtime_assets(),
                self.network_id_mode(),
                &sub_chunk,
                position.map(|axis| axis.rem_euclid(world::SUB_CHUNK_SIDE as i32) as u8),
            ),
            None if terrain.is_sub_chunk_loaded(key) => ::meshing::ResolvedContributors::default(),
            None => return None,
        };
        Some((key, contributors))
    }
}
