use std::collections::BTreeMap;

use assets::NetworkIdMode;
use meshing::{BlockClassifier, Neighbourhood, mesh_sub_chunk};
use world::{BlockUpdate, ChunkStore, SubChunkKey};

use crate::{geometry, materials, model::Arena};

const AIR: u32 = 0;
const SUB_CHUNK_SIDE: i32 = 16;
const MAX_SUB_CHUNKS: usize = 4096;
pub(super) const FLOATS_PER_VERTEX: usize = 9;
const MAX_VERTEX_FLOATS: usize = 2_000_000 * FLOATS_PER_VERTEX;

pub(super) fn mesh(arena: &Arena) -> Result<Vec<f32>, String> {
    let visual_ids = arena
        .palette
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            if assets::is_default_invisible_block(&entry.name) {
                AIR
            } else {
                index as u32
            }
        })
        .collect::<Vec<_>>();
    let mut batches = BTreeMap::<SubChunkKey, Vec<BlockUpdate>>::new();
    for &[x, y, z, palette] in &arena.blocks {
        let key = SubChunkKey::new(
            0,
            x.div_euclid(SUB_CHUNK_SIDE),
            y.div_euclid(SUB_CHUNK_SIDE),
            z.div_euclid(SUB_CHUNK_SIDE),
        );
        batches.entry(key).or_default().push(BlockUpdate::new(
            x.rem_euclid(SUB_CHUNK_SIDE) as u8,
            y.rem_euclid(SUB_CHUNK_SIDE) as u8,
            z.rem_euclid(SUB_CHUNK_SIDE) as u8,
            0,
            // Retain air updates so an invisible final value removes a prior visible block.
            visual_ids[palette as usize],
        ));
        if batches.len() > MAX_SUB_CHUNKS {
            return Err("arena exceeds the 4096-subchunk browser limit".into());
        }
    }
    let mut world = ChunkStore::new();
    let keys = batches.keys().copied().collect::<Vec<_>>();
    for (key, updates) in batches {
        world
            .update_sub_chunk_blocks(key, &updates, AIR)
            .map_err(|error| format!("invalid arena block batch: {error}"))?;
    }
    let (assets, colors) = materials::palette_assets(&arena.palette)?;
    let mut vertices = Vec::new();
    for key in keys {
        let Some(center) = world.sub_chunk(key) else {
            continue;
        };
        let neighbours = [
            world.sub_chunk(SubChunkKey::new(0, key.x - 1, key.y, key.z)),
            world.sub_chunk(SubChunkKey::new(0, key.x + 1, key.y, key.z)),
            world.sub_chunk(SubChunkKey::new(0, key.x, key.y - 1, key.z)),
            world.sub_chunk(SubChunkKey::new(0, key.x, key.y + 1, key.z)),
            world.sub_chunk(SubChunkKey::new(0, key.x, key.y, key.z - 1)),
            world.sub_chunk(SubChunkKey::new(0, key.x, key.y, key.z + 1)),
        ];
        let mut neighbourhood = Neighbourhood::empty();
        if let Some(chunk) = neighbours[0].as_deref() {
            neighbourhood = neighbourhood.with_negative_x(chunk);
        }
        if let Some(chunk) = neighbours[1].as_deref() {
            neighbourhood = neighbourhood.with_positive_x(chunk);
        }
        if let Some(chunk) = neighbours[2].as_deref() {
            neighbourhood = neighbourhood.with_negative_y(chunk);
        }
        if let Some(chunk) = neighbours[3].as_deref() {
            neighbourhood = neighbourhood.with_positive_y(chunk);
        }
        if let Some(chunk) = neighbours[4].as_deref() {
            neighbourhood = neighbourhood.with_negative_z(chunk);
        }
        if let Some(chunk) = neighbours[5].as_deref() {
            neighbourhood = neighbourhood.with_positive_z(chunk);
        }
        let mesh = mesh_sub_chunk(
            &BlockClassifier::new(AIR),
            &assets,
            NetworkIdMode::Sequential,
            &neighbourhood,
            &center,
        );
        let additional = mesh.quads().len() * 6 * FLOATS_PER_VERTEX;
        if vertices.len().saturating_add(additional) > MAX_VERTEX_FLOATS {
            return Err("arena geometry exceeds the two-million-vertex browser limit".into());
        }
        let origin = [key.x, key.y, key.z].map(|value| (value * SUB_CHUNK_SIDE) as f32);
        for quad in mesh.quads() {
            let color = colors
                .get(quad.material_id() as usize)
                .copied()
                .ok_or("arena mesh has an invalid material reference")?;
            geometry::append_quad(&mut vertices, quad, origin, color);
        }
    }
    Ok(vertices)
}
