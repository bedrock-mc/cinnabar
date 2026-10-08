use std::collections::{BTreeMap, BTreeSet, VecDeque};

use assets::NetworkIdMode;
use meshing::{BlockClassifier, ChunkMesh, PackedBiomeRecord, mesh_sub_chunk_in_neighbourhood};
use world::{BlockUpdate, ChunkStore, MeshNeighbourhood, SubChunkKey};

use crate::{TerrainAssets, canonical, model::Arena};

const SIDE: i32 = 16;
const MAX_SUB_CHUNKS: usize = 4096;
const MAX_MESH_BYTES: u64 = 64 * 1024 * 1024;

/// Uses the native mesher's entire cube/model/liquid streams and 26-neighbour snapshot.
#[cfg(test)]
pub(super) fn prepare(
    arena: &Arena,
    assets: &TerrainAssets,
) -> Result<VecDeque<(SubChunkKey, ChunkMesh)>, String> {
    let scene = TerrainScene::new(arena, assets)?;
    mesh_keys(&scene.world, scene.keys.iter().copied(), assets, false)
}

pub(super) struct TerrainScene {
    arena_id: Option<String>,
    world: ChunkStore,
    keys: BTreeSet<SubChunkKey>,
    original: BTreeMap<[i32; 3], u32>,
    current: BTreeMap<[i32; 3], u32>,
    bounds: [i32; 6],
}
impl TerrainScene {
    pub(super) fn new(arena: &Arena, assets: &TerrainAssets) -> Result<Self, String> {
        let ids = canonical::palette_ids(&assets.canonical, &arena.palette)?;
        let mut batches = BTreeMap::<SubChunkKey, Vec<BlockUpdate>>::new();
        for &[x, y, z, palette] in &arena.blocks {
            let key = SubChunkKey::new(
                0,
                x.div_euclid(SIDE),
                y.div_euclid(SIDE),
                z.div_euclid(SIDE),
            );
            batches.entry(key).or_default().push(BlockUpdate::new(
                x.rem_euclid(SIDE) as u8,
                y.rem_euclid(SIDE) as u8,
                z.rem_euclid(SIDE) as u8,
                0,
                ids[palette as usize],
            ));
            if batches.len() > MAX_SUB_CHUNKS {
                return Err("arena exceeds the 4096-subchunk browser limit".into());
            }
        }
        let mut world = ChunkStore::new();
        let keys = batches.keys().copied().collect::<BTreeSet<_>>();
        for (key, updates) in batches {
            world
                .update_sub_chunk_blocks(key, &updates, assets.air)
                .map_err(|error| format!("invalid arena block batch: {error}"))?;
        }
        Ok(Self {
            arena_id: arena.id.clone(),
            world,
            keys,
            original: BTreeMap::new(),
            current: BTreeMap::new(),
            bounds: arena.bounds,
        })
    }
    /// Prepares a replacement for the current frame without publishing partial terrain.
    pub(super) fn from_frame(
        arena: &Arena,
        assets: &TerrainAssets,
        frame: Option<&crate::browser_model::Frame>,
    ) -> Result<Self, String> {
        let mut scene = Self::new(arena, assets)?;
        if let Some(frame) = frame
            && scene.matches_frame(frame)
        {
            scene.update_blocks(&frame.blocks, assets)?;
        }
        Ok(scene)
    }

    /// Descriptors without an identity retain the legacy active-arena behavior.
    pub(super) fn matches_frame(&self, frame: &crate::browser_model::Frame) -> bool {
        self.arena_id
            .as_ref()
            .is_none_or(|id| id == &frame.arena_id)
    }
    pub(super) fn initial(
        &self,
        assets: &TerrainAssets,
    ) -> Result<VecDeque<(SubChunkKey, ChunkMesh)>, String> {
        mesh_keys(&self.world, self.keys.iter().copied(), assets, false)
    }
    // Each frame owns a cumulative snapshot. Missing overrides restore the arena
    // baseline, so a backwards seek uses exactly the same incremental mesh path.
    pub(super) fn apply(
        &mut self,
        blocks: &[crate::browser_model::SceneBlock],
        assets: &TerrainAssets,
    ) -> Result<VecDeque<(SubChunkKey, ChunkMesh)>, String> {
        let affected = self.update_blocks(blocks, assets)?;
        mesh_keys(&self.world, affected, assets, true)
    }

    /// Updates cumulative overrides independently of mesh publication.
    fn update_blocks(
        &mut self,
        blocks: &[crate::browser_model::SceneBlock],
        assets: &TerrainAssets,
    ) -> Result<BTreeSet<SubChunkKey>, String> {
        let entries = blocks
            .iter()
            .map(|block| crate::model::PaletteEntry {
                name: block.name.clone(),
                states: block.states.clone(),
            })
            .collect::<Vec<_>>();
        let ids = canonical::palette_ids(&assets.canonical, &entries)?;
        let next = blocks
            .iter()
            .zip(ids)
            .map(|(block, id)| (block.position, id))
            .collect::<BTreeMap<_, _>>();
        let mut changed = BTreeMap::<SubChunkKey, Vec<BlockUpdate>>::new();
        for position in self
            .current
            .keys()
            .chain(next.keys())
            .copied()
            .collect::<BTreeSet<_>>()
        {
            if (0..3).any(|axis| {
                position[axis] < self.bounds[axis] || position[axis] > self.bounds[axis + 3]
            }) {
                return Err("replay block lies outside its arena".into());
            }
            let [x, y, z] = position;
            let key = SubChunkKey::new(
                0,
                x.div_euclid(SIDE),
                y.div_euclid(SIDE),
                z.div_euclid(SIDE),
            );
            let coords = [x, y, z].map(|value| value.rem_euclid(SIDE) as u8);
            let original = *self.original.entry(position).or_insert_with(|| {
                self.world
                    .sub_chunk(key)
                    .and_then(|chunk| chunk.runtime_id(0, coords[0], coords[1], coords[2]))
                    .unwrap_or(assets.air)
            });
            let before = self.current.get(&position).copied().unwrap_or(original);
            let after = next.get(&position).copied().unwrap_or(original);
            if before != after {
                changed
                    .entry(key)
                    .or_default()
                    .push(BlockUpdate::new(coords[0], coords[1], coords[2], 0, after));
            }
        }
        let mut affected = BTreeSet::new();
        for (key, updates) in changed {
            self.world
                .update_sub_chunk_blocks(key, &updates, assets.air)
                .map_err(|error| error.to_string())?;
            self.keys.insert(key);
            affected.insert(key);
            for offset in MeshNeighbourhood::adjacent_offsets() {
                let adjacent = SubChunkKey::new(
                    0,
                    key.x + i32::from(offset[0]),
                    key.y + i32::from(offset[1]),
                    key.z + i32::from(offset[2]),
                );
                if self.keys.contains(&adjacent) {
                    affected.insert(adjacent);
                }
            }
        }
        self.current = next;
        Ok(affected)
    }
}
fn mesh_keys(
    world: &ChunkStore,
    keys: impl IntoIterator<Item = SubChunkKey>,
    assets: &TerrainAssets,
    include_empty: bool,
) -> Result<VecDeque<(SubChunkKey, ChunkMesh)>, String> {
    let mut meshes = VecDeque::new();
    let mut total_bytes = 0_u64;
    for key in keys {
        let Some(center) = world.sub_chunk(key) else {
            // ChunkStore retires an all-air subchunk. Publish an empty mesh
            // so the renderer also retires its previously visible geometry.
            if include_empty {
                meshes.push_back((key, ChunkMesh::default()));
            }
            continue;
        };
        let neighbours = MeshNeighbourhood::adjacent_offsets()
            .map(|offset| {
                let key = SubChunkKey::new(
                    0,
                    key.x + i32::from(offset[0]),
                    key.y + i32::from(offset[1]),
                    key.z + i32::from(offset[2]),
                );
                (offset, world.sub_chunk(key))
            })
            .collect::<Vec<_>>();
        let mut snapshot = MeshNeighbourhood::new(&center);
        for (offset, chunk) in &neighbours {
            if let Some(chunk) = chunk {
                let _ = snapshot.insert(*offset, chunk);
            }
        }
        let mesh = mesh_sub_chunk_in_neighbourhood(
            &BlockClassifier::new(assets.air),
            &assets.runtime,
            NetworkIdMode::Sequential,
            &snapshot,
        );
        total_bytes = total_bytes.saturating_add(meshing::mesh_output_byte_len(
            &mesh,
            &PackedBiomeRecord::fallback(),
        ));
        if total_bytes > MAX_MESH_BYTES {
            return Err("arena native mesh exceeds the 64 MiB browser budget".into());
        }
        if include_empty || !mesh.is_empty() {
            meshes.push_back((key, mesh));
        }
    }
    Ok(meshes)
}

/// Canonical BREG collision seeds against the current replay terrain. Arena
/// exports do not contain light volumes or fluid physics; those two queries
/// retain the renderer's existing preview behavior rather than claim parity.
#[cfg(target_arch = "wasm32")]
pub(super) struct SceneParticleWorld<'a> {
    scene: &'a TerrainScene,
    assets: &'a TerrainAssets,
}
#[cfg(target_arch = "wasm32")]
impl TerrainScene {
    pub(super) fn block_id(&self, position: [i32; 3], air: u32) -> u32 {
        let [x, y, z] = position;
        let key = SubChunkKey::new(
            0,
            x.div_euclid(SIDE),
            y.div_euclid(SIDE),
            z.div_euclid(SIDE),
        );
        self.world
            .sub_chunk(key)
            .and_then(|chunk| {
                chunk.runtime_id(
                    0,
                    x.rem_euclid(SIDE) as u8,
                    y.rem_euclid(SIDE) as u8,
                    z.rem_euclid(SIDE) as u8,
                )
            })
            .unwrap_or(air)
    }
    pub(super) fn particle_world<'a>(
        &'a self,
        assets: &'a TerrainAssets,
    ) -> SceneParticleWorld<'a> {
        SceneParticleWorld {
            scene: self,
            assets,
        }
    }
}
#[cfg(target_arch = "wasm32")]
impl particles::ParticleWorld for SceneParticleWorld<'_> {
    fn solid_boxes(&self, min: [f32; 3], max: [f32; 3], out: &mut Vec<[f32; 6]>) {
        if !min.iter().chain(max.iter()).all(|value| value.is_finite()) {
            return;
        }
        let low: [i32; 3] = std::array::from_fn(|axis| {
            (min[axis].floor() as i32).saturating_sub(self.assets.collision_halo[axis][1])
        });
        let high: [i32; 3] = std::array::from_fn(|axis| {
            (max[axis].floor() as i32).saturating_sub(self.assets.collision_halo[axis][0])
        });
        let cells = (0..3).try_fold(1u64, |count, axis| {
            count.checked_mul(u64::try_from(i64::from(high[axis]) - i64::from(low[axis]) + 1).ok()?)
        });
        if cells.is_none_or(|count| count > 4096) {
            return;
        }
        for x in low[0]..=high[0] {
            for y in low[1]..=high[1] {
                for z in low[2]..=high[2] {
                    let key = SubChunkKey::new(
                        0,
                        x.div_euclid(SIDE),
                        y.div_euclid(SIDE),
                        z.div_euclid(SIDE),
                    );
                    let Some(id) = self.scene.world.sub_chunk(key).and_then(|chunk| {
                        chunk.runtime_id(
                            0,
                            x.rem_euclid(SIDE) as u8,
                            y.rem_euclid(SIDE) as u8,
                            z.rem_euclid(SIDE) as u8,
                        )
                    }) else {
                        continue;
                    };
                    let Some(record) = self
                        .assets
                        .collision_records
                        .get(id as usize)
                        .filter(|record| record.sequential_id == id)
                    else {
                        continue;
                    };
                    for shape in &record.collision_seed.boxes {
                        let position = [x as f32, y as f32, z as f32];
                        let shape_min = [shape.min_x, shape.min_y, shape.min_z]
                            .map(|value| value as f32 / 100_000_000.0);
                        let shape_max = [shape.max_x, shape.max_y, shape.max_z]
                            .map(|value| value as f32 / 100_000_000.0);
                        let lower: [f32; 3] =
                            std::array::from_fn(|axis| position[axis] + shape_min[axis]);
                        let upper: [f32; 3] =
                            std::array::from_fn(|axis| position[axis] + shape_max[axis]);
                        if (0..3).all(|axis| lower[axis] <= max[axis] && upper[axis] >= min[axis]) {
                            out.push([lower[0], lower[1], lower[2], upper[0], upper[1], upper[2]]);
                        }
                    }
                }
            }
        }
    }
    fn light(&self, _block: [i32; 3]) -> (u8, u8) {
        (0, 15)
    }
    fn fluid(&self, _block: [i32; 3]) -> particles::Fluid {
        particles::Fluid::None
    }
}
