//! Measures one distant terrain sample job without the store or the writer.

use std::sync::RwLock;

use lod::registry::MaterialRegistry;
use lod_store::store::SharedRegistry;

use super::*;

/// One resident column of mixed-palette sub-chunks with solved light, sampled as a Lod-lane
/// job samples it.
pub struct LodSampleFixture {
    stream: WorldStream,
    column: ChunkKey,
    registry: SharedRegistry,
}

impl LodSampleFixture {
    /// Installs a column of `sub_chunks` stacked sub-chunks from Y 0; setup is outside the
    /// timed region.
    pub fn new(sub_chunks: usize) -> Self {
        assert!(sub_chunks > 0);
        let mut stream = WorldStream::new(client_world::ingestion::WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [8.0; 3],
            world_spawn_position: [8; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        });
        let mut bytes = vec![9, 1, 0, 3];
        for _ in 0..128 {
            bytes.extend_from_slice(&0xaaaa_aaaa_u32.to_le_bytes());
        }
        bytes.extend([4, 0, 2]);
        let section = SubChunk::decode(&bytes, &world::RawBlockIds { air: 0 });
        let column = ChunkKey::new(0, 0, 0);
        for y in 0..sub_chunks as i32 {
            let key = SubChunkKey::from_chunk(column, y);
            stream
                .authority
                .commit_sub_chunk(key, section.clone())
                .unwrap();
            stream.resident.insert(key);
            let mut light = SubChunkLight::uniform(0, 15, 1).unwrap();
            light.set(LightChannel::Block, 8, 8, 8, 7).unwrap();
            stream.lighting.store.insert_resident(key, light);
        }
        Self {
            stream,
            column,
            registry: Arc::new(RwLock::new(MaterialRegistry::new())),
        }
    }

    /// Captures, hashes and samples the column with its surface signature; returns the
    /// content hash and the sampled sub-chunk count.
    pub fn sample(&self) -> (u64, usize) {
        self.stream
            .sample_lod_column_for_bench(self.column, &self.registry)
            .expect("the fixture column is keyed and resident")
    }
}
