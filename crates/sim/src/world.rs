use std::collections::BTreeMap;

use thiserror::Error;
use world::{ChunkKey, ChunkStore, SubChunkKey};

use crate::{Aabb, Vec3};

/// Runtime-ID keyed collision boxes in local block coordinates.
///
/// Empty entries are explicit passable blocks. Missing entries are unknown and
/// must stop prediction rather than silently becoming air or full cubes.
#[derive(Debug, Default)]
pub struct CollisionRegistry {
    blocks: BTreeMap<u32, BlockPhysics>,
}

#[derive(Debug)]
struct BlockPhysics {
    shapes: Box<[Aabb]>,
    friction: f64,
}

pub(crate) const DEFAULT_SURFACE_FRICTION: f64 = 0.6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RegistryError {
    #[error("runtime ID {runtime_id} has a non-finite or non-positive friction")]
    InvalidFriction { runtime_id: u32 },
    #[error("runtime ID {runtime_id} collision shape {shape_index} is non-finite or inverted")]
    InvalidShape { runtime_id: u32, shape_index: usize },
}

impl CollisionRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        runtime_id: u32,
        boxes: impl IntoIterator<Item = Aabb>,
    ) -> Result<(), RegistryError> {
        self.register_with_friction(runtime_id, boxes, DEFAULT_SURFACE_FRICTION)
    }

    pub fn register_with_friction(
        &mut self,
        runtime_id: u32,
        boxes: impl IntoIterator<Item = Aabb>,
        friction: f64,
    ) -> Result<(), RegistryError> {
        if !friction.is_finite() || friction <= 0.0 {
            return Err(RegistryError::InvalidFriction { runtime_id });
        }
        let shapes = boxes.into_iter().collect::<Vec<_>>();
        for (shape_index, shape) in shapes.iter().enumerate() {
            let coordinates = [
                shape.min.x,
                shape.min.y,
                shape.min.z,
                shape.max.x,
                shape.max.y,
                shape.max.z,
            ];
            if !coordinates.into_iter().all(f64::is_finite)
                || shape.min.x > shape.max.x
                || shape.min.y > shape.max.y
                || shape.min.z > shape.max.z
            {
                return Err(RegistryError::InvalidShape {
                    runtime_id,
                    shape_index,
                });
            }
        }
        self.blocks.insert(
            runtime_id,
            BlockPhysics {
                shapes: shapes.into_boxed_slice(),
                friction,
            },
        );
        Ok(())
    }

    fn boxes(&self, runtime_id: u32) -> Option<&[Aabb]> {
        self.blocks
            .get(&runtime_id)
            .map(|physics| physics.shapes.as_ref())
    }

    fn friction(&self, runtime_id: u32) -> Option<f64> {
        self.blocks.get(&runtime_id).map(|physics| physics.friction)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WorldQueryError {
    #[error("collision query contains non-finite or inverted bounds")]
    InvalidBounds,
    #[error("collision query coordinate is outside the i32 block range")]
    CoordinateOutOfRange,
    #[error("chunk {0:?} has not received a complete LevelChunk")]
    UnloadedChunk(ChunkKey),
    #[error("runtime ID {runtime_id} at {block:?} has no authoritative collision shape")]
    UnknownRuntimeId { runtime_id: u32, block: [i32; 3] },
}

pub trait CollisionWorld {
    fn collision_boxes(&self, query: Aabb) -> Result<Vec<Aabb>, WorldQueryError>;

    fn block_friction(&self, _block: [i32; 3]) -> Result<f64, WorldQueryError> {
        Ok(DEFAULT_SURFACE_FRICTION)
    }
}

/// Read-only collision adapter over the palette-packed world store.
pub struct PaletteWorld<'a> {
    store: &'a ChunkStore,
    registry: &'a CollisionRegistry,
    dimension: i32,
}

impl<'a> PaletteWorld<'a> {
    #[must_use]
    pub const fn new(
        store: &'a ChunkStore,
        registry: &'a CollisionRegistry,
        dimension: i32,
    ) -> Self {
        Self {
            store,
            registry,
            dimension,
        }
    }
}

impl CollisionWorld for PaletteWorld<'_> {
    fn collision_boxes(&self, query: Aabb) -> Result<Vec<Aabb>, WorldQueryError> {
        let coordinates = [
            query.min.x,
            query.min.y,
            query.min.z,
            query.max.x,
            query.max.y,
            query.max.z,
        ];
        if !coordinates.into_iter().all(f64::is_finite)
            || query.min.x > query.max.x
            || query.min.y > query.max.y
            || query.min.z > query.max.z
        {
            return Err(WorldQueryError::InvalidBounds);
        }
        if query.min == query.max {
            return Ok(Vec::new());
        }

        // Oomph/bedsim scans a one-block halo so collision shapes taller than
        // their block cell (fences/walls) are still found, then filters the
        // translated shapes against the original query.
        let grown = query.grown(1.0);
        let min = block_floor(grown.min)?;
        let max = block_ceil(grown.max)?;

        let min_chunk_x = min[0] >> 4;
        let max_chunk_x = max[0] >> 4;
        let min_chunk_z = min[2] >> 4;
        let max_chunk_z = max[2] >> 4;
        for chunk_x in min_chunk_x..=max_chunk_x {
            for chunk_z in min_chunk_z..=max_chunk_z {
                let key = ChunkKey::new(self.dimension, chunk_x, chunk_z);
                if !self.store.is_chunk_loaded(key) {
                    return Err(WorldQueryError::UnloadedChunk(key));
                }
            }
        }

        let mut result = Vec::new();
        for x in min[0]..=max[0] {
            for z in min[2]..=max[2] {
                for y in min[1]..=max[1] {
                    let key = SubChunkKey::new(self.dimension, x >> 4, y >> 4, z >> 4);
                    let Some(sub_chunk) = self.store.sub_chunk(key) else {
                        continue;
                    };
                    let local_x = x.rem_euclid(16) as u8;
                    let local_y = y.rem_euclid(16) as u8;
                    let local_z = z.rem_euclid(16) as u8;
                    for layer in 0..sub_chunk.storages().len() {
                        let runtime_id = sub_chunk
                            .runtime_id(layer, local_x, local_y, local_z)
                            .expect("validated palette storage resolves every local coordinate");
                        let shapes = self.registry.boxes(runtime_id).ok_or(
                            WorldQueryError::UnknownRuntimeId {
                                runtime_id,
                                block: [x, y, z],
                            },
                        )?;
                        let block_offset = Vec3::new(f64::from(x), f64::from(y), f64::from(z));
                        result.extend(
                            shapes
                                .iter()
                                .copied()
                                .map(|shape| shape.translated(block_offset))
                                .filter(|shape| shape.intersects(query)),
                        );
                    }
                }
            }
        }
        Ok(result)
    }

    fn block_friction(&self, block: [i32; 3]) -> Result<f64, WorldQueryError> {
        let [x, y, z] = block;
        let chunk = ChunkKey::new(self.dimension, x >> 4, z >> 4);
        if !self.store.is_chunk_loaded(chunk) {
            return Err(WorldQueryError::UnloadedChunk(chunk));
        }
        let key = SubChunkKey::new(self.dimension, x >> 4, y >> 4, z >> 4);
        let Some(sub_chunk) = self.store.sub_chunk(key) else {
            return Ok(DEFAULT_SURFACE_FRICTION);
        };
        let Some(runtime_id) = sub_chunk.runtime_id(
            0,
            x.rem_euclid(16) as u8,
            y.rem_euclid(16) as u8,
            z.rem_euclid(16) as u8,
        ) else {
            return Ok(DEFAULT_SURFACE_FRICTION);
        };
        self.registry
            .friction(runtime_id)
            .ok_or(WorldQueryError::UnknownRuntimeId { runtime_id, block })
    }
}

fn block_floor(value: Vec3) -> Result<[i32; 3], WorldQueryError> {
    convert_block_coords(value, f64::floor)
}

fn block_ceil(value: Vec3) -> Result<[i32; 3], WorldQueryError> {
    convert_block_coords(value, f64::ceil)
}

fn convert_block_coords(
    value: Vec3,
    round: impl Fn(f64) -> f64,
) -> Result<[i32; 3], WorldQueryError> {
    let values = [round(value.x), round(value.y), round(value.z)];
    if values
        .into_iter()
        .any(|value| value < f64::from(i32::MIN) || value > f64::from(i32::MAX))
    {
        return Err(WorldQueryError::CoordinateOutOfRange);
    }
    Ok(values.map(|value| value as i32))
}
