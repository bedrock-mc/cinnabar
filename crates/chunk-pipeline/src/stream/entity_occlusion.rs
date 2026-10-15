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
        let mut sections = OcclusionSections::new(self);
        budget.hides_box(eye, low, high, |position, neighbour| {
            let Some((key, cube)) = sections.cell(position) else {
                return false;
            };
            sections.mesh_current(key)
                && cube.is_opaque_cube(self.runtime_assets())
                && sections
                    .cell(neighbour)
                    .is_some_and(|(_, cell)| cell.is_empty())
                && drawn(position, key)
        })
    }
}

/// Section-level facts one occlusion probe reads for every cell it visits. Ray samples
/// step through neighbouring cells, so the last two sections and the last mesh check
/// answer most lookups without hashing; every cell is still resolved from its palette.
struct OcclusionSections<'a> {
    stream: &'a WorldStream,
    terrain: &'a ChunkStore,
    dimension: i32,
    /// Most recent first: the section's borrowed storage, and whether a missing one is
    /// authoritatively loaded air.
    recent: [Option<(SubChunkKey, Option<&'a SubChunk>, bool)>; 2],
    /// The last section whose uploaded mesh was checked, and whether it is current.
    mesh: Option<(SubChunkKey, bool)>,
}

impl<'a> OcclusionSections<'a> {
    fn new(stream: &'a WorldStream) -> Self {
        Self {
            stream,
            terrain: stream.authority.terrain(),
            dimension: stream.current_dimension(),
            recent: [None; 2],
            mesh: None,
        }
    }

    /// Resolves one current cell, keeping authoritative air distinct from missing terrain.
    fn cell(
        &mut self,
        position: [i32; 3],
    ) -> Option<(SubChunkKey, ::meshing::ResolvedContributors)> {
        let [x, y, z] = position.map(|axis| axis.div_euclid(world::SUB_CHUNK_SIDE as i32));
        let key = SubChunkKey::new(self.dimension, x, y, z);
        let contributors = match self.section(key) {
            (Some(sub_chunk), _) => ::meshing::ContributorResolver::resolve_direct(
                self.stream.classifier,
                self.stream.runtime_assets(),
                self.stream.network_id_mode(),
                sub_chunk,
                position.map(|axis| axis.rem_euclid(world::SUB_CHUNK_SIDE as i32) as u8),
            ),
            (None, true) => ::meshing::ResolvedContributors::default(),
            (None, false) => return None,
        };
        Some((key, contributors))
    }

    /// The section's storage and, when it has none, whether it is loaded.
    fn section(&mut self, key: SubChunkKey) -> (Option<&'a SubChunk>, bool) {
        if let Some(index) = self
            .recent
            .iter()
            .position(|entry| entry.is_some_and(|(recent, _, _)| recent == key))
        {
            let (_, sub_chunk, loaded) = self.recent[index].expect("matched entry is present");
            if index != 0 {
                self.recent.swap(0, index);
            }
            return (sub_chunk, loaded);
        }
        let sub_chunk = self.terrain.sub_chunk_ref(key);
        let loaded = sub_chunk.is_none() && self.terrain.is_sub_chunk_loaded(key);
        self.recent[1] = self.recent[0];
        self.recent[0] = Some((key, sub_chunk, loaded));
        (sub_chunk, loaded)
    }

    /// Whether the section's uploaded mesh is its current geometry.
    fn mesh_current(&mut self, key: SubChunkKey) -> bool {
        if let Some((checked, current)) = self.mesh
            && checked == key
        {
            return current;
        }
        let current = self.stream.is_mesh_clean(key)
            && self.stream.applied_mesh_generations.contains_key(&key);
        self.mesh = Some((key, current));
        current
    }
}
