use super::super::*;

pub(in crate::stream) struct MeshSnapshot {
    pub(in crate::stream) center: Arc<SubChunk>,
    pub(in crate::stream) biomes: BiomeNeighbourhood,
    pub(in crate::stream) adjacent: [Option<Arc<SubChunk>>; 27],
    pub(in crate::stream) light_halo: MeshLightHalo,
}

pub(in crate::stream) type BiomeNeighbourhood =
    [Option<Arc<BiomeStorage>>; BIOME_NEIGHBOUR_SLOT_COUNT];

#[derive(Debug, Clone)]
pub(in crate::stream) struct MeshLightSlot {
    pub(in crate::stream) key: SubChunkKey,
    pub(in crate::stream) block_generation: u64,
    pub(in crate::stream) light_revision: u64,
    pub(in crate::stream) light: Arc<SubChunkLight>,
}

#[derive(Debug, Clone, Default)]
pub(in crate::stream) struct MeshLightHalo {
    pub(in crate::stream) center: Option<SubChunkKey>,
    pub(in crate::stream) slots: [Option<MeshLightSlot>; 27],
}

impl MeshLightHalo {
    pub(in crate::stream) fn sample_channels(&self, coordinate: [i32; 3]) -> [u8; 2] {
        let offset = coordinate.map(|value| value.div_euclid(16));
        if offset.into_iter().any(|value| !(-1..=1).contains(&value)) {
            return [0, 0];
        }
        let offset = offset.map(|value| value as i8);
        let local = coordinate.map(|value| value.rem_euclid(16) as u8);
        let Some(slot) = self.slots[mesh_offset_index(offset)].as_ref() else {
            return [0, 0];
        };
        [
            slot.light
                .get(LightChannel::Block, local[0], local[1], local[2])
                .unwrap_or(0),
            slot.light
                .get(LightChannel::Sky, local[0], local[1], local[2])
                .unwrap_or(0),
        ]
    }

    #[cfg(test)]
    pub(in crate::stream) fn occupied_slot_count(&self) -> usize {
        self.slots.iter().flatten().count()
    }
}

impl MeshLightSampler for MeshLightHalo {
    fn sample(&self, coordinate: [i32; 3]) -> MeshLightSample {
        let [block, sky] = self.sample_channels(coordinate);
        MeshLightSample::try_new(block, sky)
            .expect("world light storage exposes only bounded four-bit channels")
    }
}

pub(in crate::stream) fn pack_biome_record(
    storages: &BiomeNeighbourhood,
    resolved: &ResolvedBiomeTints,
) -> PackedBiomeRecord {
    PackedBiomeRecord::from_neighbourhood(storages, |raw_id| resolved.dense_index(raw_id))
}

impl MeshSnapshot {
    pub(in crate::stream) fn neighbourhood(&self) -> MeshNeighbourhood<'_> {
        let mut neighbourhood = MeshNeighbourhood::new(&self.center);
        for offset in MeshNeighbourhood::adjacent_offsets() {
            if let Some(sub_chunk) = self.adjacent[mesh_offset_index(offset)].as_deref() {
                let inserted = neighbourhood.insert(offset, sub_chunk);
                debug_assert!(inserted);
            }
        }
        neighbourhood
    }

    pub(in crate::stream) fn mesh(
        &self,
        classifier: BlockClassifier,
        runtime_assets: &RuntimeAssets,
        network_id_mode: NetworkIdMode,
    ) -> ChunkMesh {
        mesh_sub_chunk_in_neighbourhood_with_lighting(
            &classifier,
            runtime_assets,
            network_id_mode,
            &self.neighbourhood(),
            &self.light_halo,
        )
    }

    pub(in crate::stream) fn dependency_mask(
        &self,
        classifier: BlockClassifier,
        runtime_assets: &RuntimeAssets,
        network_id_mode: NetworkIdMode,
    ) -> MeshDependencyMask {
        mesh_dependency_mask(&classifier, runtime_assets, network_id_mode, &self.center)
    }
}

pub(in crate::stream) fn mesh_offset_index([x, y, z]: [i8; 3]) -> usize {
    (usize::from((x + 1) as u8) * 3 + usize::from((y + 1) as u8)) * 3 + usize::from((z + 1) as u8)
}
