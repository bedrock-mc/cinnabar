//! Flat palette samples of small block volumes, with shared block-local geometry.

use std::collections::{BTreeSet, HashMap};

use world::{ChunkKey, SUB_CHUNK_SIDE, SubChunkKey};

use super::{Aabb, BlockPhysics, BlockPhysicsFacts, PaletteWorld, Vec3, WorldQueryError};
use assets::block_shapes::SelectionContext;

/// Layer slot without block storage; every layer of an unknown cell holds it.
pub const TERRAIN_NO_LAYER: u16 = u16::MAX;
/// Shape index of the known-empty shape.
pub const TERRAIN_EMPTY_SHAPE: u32 = 0;
/// Coordinates up to this magnitude survive the native single-precision shape origin.
const EXACT_COORDINATE: i32 = 1 << f32::MANTISSA_DIGITS;

/// Geometry kinds a sample resolves; unrequested kinds cost nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TerrainGeometry {
    pub collision: bool,
    pub selection: bool,
}

/// Shape of one state or slot: an index into the sample's shapes, or unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerrainShape {
    Unavailable,
    Known(u32),
}

/// How a palette state's geometry is found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerrainShapeBinding {
    NotRequested,
    /// The same shape at every occurrence.
    Fixed(TerrainShape),
    /// Every occurrence has a [`TerrainShapeOverride`].
    PerCell,
}

/// One distinct runtime ID; physics is absent without registry metadata.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerrainState {
    pub runtime_id: u32,
    pub physics: Option<BlockPhysicsFacts>,
    pub collision: TerrainShapeBinding,
    pub selection: TerrainShapeBinding,
}

/// Position-dependent geometry of one layer slot; `None` keeps the state's binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerrainShapeOverride {
    pub slot: u32,
    pub collision: Option<TerrainShape>,
    pub selection: Option<TerrainShape>,
}

/// A column read by the sample, with its revision when loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerrainColumn {
    pub x: i32,
    pub z: i32,
    pub revision: Option<u64>,
}

/// A sampled volume. Cell `((x * size_z) + z) * size_y + y`, relative to the minimum, has layer
/// `l` at `states[l * cells + cell]`, an index into `palette` or [`TERRAIN_NO_LAYER`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TerrainSample {
    pub layer_count: u8,
    /// Bit `cell % 64` of word `cell / 64` is set for known cells.
    pub loaded: Vec<u64>,
    pub states: Vec<u16>,
    pub palette: Vec<TerrainState>,
    /// Shape `s` is boxes `shape_offsets[s]..shape_offsets[s + 1]`; shape 0 is empty.
    pub shape_offsets: Vec<u32>,
    /// Six values per block-local box: minimum then maximum x, y and z.
    pub shape_boxes: Vec<f64>,
    /// Sorted by slot.
    pub overrides: Vec<TerrainShapeOverride>,
    /// Every column the sample depends on, sorted by x then z.
    pub columns: Vec<TerrainColumn>,
}

/// A sample that would exceed its representable or admitted size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerrainSampleError {
    OutOfRange,
    TooManyStates,
    TooManyBoxes,
}

/// Reusable scratch for repeated samples; output vectors are owned by each sample.
#[derive(Default)]
pub struct TerrainSampler {
    palette: HashMap<u32, u16>,
    order: Vec<u32>,
    remap: Vec<u16>,
    shapes: HashMap<Box<[u64]>, u32>,
    boxes: Vec<Aabb>,
    key: Vec<u64>,
    columns: BTreeSet<(i32, i32)>,
}

/// Static or per-occurrence geometry of one runtime ID, decided once per sample.
#[derive(Clone, Copy)]
struct Rule {
    collision: bool,
    selection: bool,
}

impl TerrainSampler {
    /// Samples `size` blocks from `min`; at most `max_boxes` distinct boxes are emitted.
    /// The exclusive maximum must fit in `i32`.
    pub fn sample(
        &mut self,
        world: &PaletteWorld<'_>,
        min: [i32; 3],
        size: [u8; 3],
        geometry: TerrainGeometry,
        max_boxes: usize,
    ) -> Result<TerrainSample, TerrainSampleError> {
        self.palette.clear();
        self.order.clear();
        self.shapes.clear();
        self.columns.clear();
        let [sx, sy, sz] = size.map(usize::from);
        let cells = sx * sy * sz;
        let mut max = min;
        for axis in 0..3 {
            max[axis] = min[axis]
                .checked_add(i32::from(size[axis]))
                .ok_or(TerrainSampleError::OutOfRange)?;
        }
        let sections = sections(world, min, max);
        let layer_count = sections
            .iter()
            .filter_map(|(_, sub, loaded)| loaded.then_some(sub.as_ref()))
            .map(|sub| sub.map_or(1, |sub| sub.storages().len().max(1)))
            .max()
            .unwrap_or(1);
        let mut sample = TerrainSample {
            layer_count: layer_count as u8,
            loaded: vec![0; cells.div_ceil(64)],
            states: vec![TERRAIN_NO_LAYER; layer_count * cells],
            ..Default::default()
        };
        let index = |x: i32, y: i32, z: i32| {
            (((x - min[0]) as usize * sz) + (z - min[2]) as usize) * sy + (y - min[1]) as usize
        };
        let mut column = [0_u16; SUB_CHUNK_SIDE];
        for (key, sub, loaded) in &sections {
            self.columns.insert((key.x, key.z));
            if !loaded {
                continue;
            }
            let origin = [key.x, key.y, key.z].map(|value| value * SUB_CHUNK_SIDE as i32);
            let low = [0, 1, 2].map(|axis| min[axis].max(origin[axis]));
            let high = [0, 1, 2]
                .map(|axis| max[axis].min(origin[axis].saturating_add(SUB_CHUNK_SIDE as i32)));
            let run = (high[1] - low[1]) as usize;
            for x in low[0]..high[0] {
                for z in low[2]..high[2] {
                    let first = index(x, low[1], z);
                    for cell in first..first + run {
                        sample.loaded[cell / 64] |= 1 << (cell % 64);
                    }
                }
            }
            let storages = sub.as_ref().map_or(&[][..], |sub| sub.storages());
            if storages.is_empty() {
                let air = self.state(world.registry.air_runtime_id)?;
                for x in low[0]..high[0] {
                    for z in low[2]..high[2] {
                        let first = index(x, low[1], z);
                        sample.states[first..first + run].fill(air);
                    }
                }
                continue;
            }
            for (layer, storage) in storages.iter().enumerate() {
                let plane = &mut sample.states[layer * cells..(layer + 1) * cells];
                let values = storage.palette().values();
                self.remap.clear();
                self.remap.resize(values.len(), TERRAIN_NO_LAYER);
                if let Some(runtime_id) = storage.uniform_runtime_id() {
                    let state = self.state(runtime_id)?;
                    for x in low[0]..high[0] {
                        for z in low[2]..high[2] {
                            let first = index(x, low[1], z);
                            plane[first..first + run].fill(state);
                        }
                    }
                    continue;
                }
                let y_low = (low[1] - origin[1]) as usize;
                for x in low[0]..high[0] {
                    for z in low[2]..high[2] {
                        storage.column_palette_indices(
                            (x - origin[0]) as u8,
                            (z - origin[2]) as u8,
                            &mut column,
                        );
                        let first = index(x, low[1], z);
                        for (slot, &entry) in plane[first..first + run]
                            .iter_mut()
                            .zip(&column[y_low..y_low + run])
                        {
                            *slot = match self.remap.get(usize::from(entry)).copied() {
                                Some(TERRAIN_NO_LAYER) => {
                                    let state = self.state(values[usize::from(entry)])?;
                                    self.remap[usize::from(entry)] = state;
                                    state
                                }
                                Some(state) => state,
                                None => TERRAIN_NO_LAYER,
                            };
                        }
                    }
                }
            }
        }
        sample.palette = self
            .order
            .iter()
            .map(|&runtime_id| TerrainState {
                runtime_id,
                physics: world.registry.movement_facts(runtime_id),
                collision: TerrainShapeBinding::NotRequested,
                selection: TerrainShapeBinding::NotRequested,
            })
            .collect();
        if geometry.collision || geometry.selection {
            self.resolve_geometry(world, &mut sample, min, max, geometry, max_boxes)?;
        }
        sample.columns = self
            .columns
            .iter()
            .map(|&(x, z)| TerrainColumn {
                x,
                z,
                revision: world
                    .store
                    .collision_revision(ChunkKey::new(world.dimension, x, z))
                    .map(|revision| revision.revision),
            })
            .collect();
        Ok(sample)
    }

    /// Interns a runtime ID into the response palette.
    fn state(&mut self, runtime_id: u32) -> Result<u16, TerrainSampleError> {
        if let Some(&index) = self.palette.get(&runtime_id) {
            return Ok(index);
        }
        let index = u16::try_from(self.order.len())
            .ok()
            .filter(|&index| index != TERRAIN_NO_LAYER)
            .ok_or(TerrainSampleError::TooManyStates)?;
        self.palette.insert(runtime_id, index);
        self.order.push(runtime_id);
        Ok(index)
    }

    /// Binds static geometry per state, then overrides every positional occurrence.
    fn resolve_geometry(
        &mut self,
        world: &PaletteWorld<'_>,
        sample: &mut TerrainSample,
        min: [i32; 3],
        max: [i32; 3],
        geometry: TerrainGeometry,
        max_boxes: usize,
    ) -> Result<(), TerrainSampleError> {
        sample.shape_offsets.extend([0, 0]);
        self.shapes.insert(Box::new([]), TERRAIN_EMPTY_SHAPE);
        let exact = [0, 1, 2].into_iter().all(|axis| {
            min[axis].unsigned_abs() <= EXACT_COORDINATE as u32
                && max[axis].unsigned_abs() <= EXACT_COORDINATE as u32
        });
        for index in 0..sample.palette.len() {
            let physics = world.registry.physics(sample.palette[index].runtime_id);
            let rule = physics.map_or(Rule::fixed(), |physics| Rule::of(world, physics, exact));
            let collision = physics.map(|physics| &*physics.shapes);
            let selection = physics.map(|physics| static_selection(world, physics));
            let collision = self.bind(
                sample,
                geometry.collision,
                rule.collision,
                collision,
                max_boxes,
            )?;
            let selection = self.bind(
                sample,
                geometry.selection,
                rule.selection,
                selection,
                max_boxes,
            )?;
            sample.palette[index].collision = collision;
            sample.palette[index].selection = selection;
        }
        let positional = |binding| binding == TerrainShapeBinding::PerCell;
        if !sample
            .palette
            .iter()
            .any(|state| positional(state.collision) || positional(state.selection))
        {
            return Ok(());
        }
        let [sy, sz] = [1, 2].map(|axis| (max[axis] - min[axis]) as usize);
        let cells = sample.states.len() / usize::from(sample.layer_count);
        for slot in 0..sample.states.len() {
            let index = sample.states[slot];
            let Some(state) = sample.palette.get(usize::from(index)).copied() else {
                continue;
            };
            if !positional(state.collision) && !positional(state.selection) {
                continue;
            }
            let cell = slot % cells;
            let position = [
                min[0] + (cell / (sy * sz)) as i32,
                min[1] + (cell % sy) as i32,
                min[2] + ((cell / sy) % sz) as i32,
            ];
            let physics = world
                .registry
                .physics(state.runtime_id)
                .expect("positional bindings have physics");
            let collision = if positional(state.collision) {
                let shapes = world.block_collision_shapes(position, physics, unit(position));
                Some(self.positional(sample, position, physics, shapes, max_boxes)?)
            } else {
                None
            };
            let selection = if positional(state.selection) {
                let shapes = self.selection_at(world, position, physics);
                Some(self.positional(sample, position, physics, shapes, max_boxes)?)
            } else {
                None
            };
            sample.overrides.push(TerrainShapeOverride {
                slot: slot as u32,
                collision,
                selection,
            });
        }
        Ok(())
    }

    /// Fixes static geometry for a state, or defers positional geometry to its occurrences.
    fn bind(
        &mut self,
        sample: &mut TerrainSample,
        requested: bool,
        positional: bool,
        shapes: Option<&[Aabb]>,
        max_boxes: usize,
    ) -> Result<TerrainShapeBinding, TerrainSampleError> {
        Ok(if !requested {
            TerrainShapeBinding::NotRequested
        } else if positional {
            TerrainShapeBinding::PerCell
        } else if let Some(shapes) = shapes {
            self.boxes.clear();
            self.boxes.extend_from_slice(shapes);
            TerrainShapeBinding::Fixed(self.intern(sample, max_boxes)?)
        } else {
            TerrainShapeBinding::Fixed(TerrainShape::Unavailable)
        })
    }

    /// Resolves interaction geometry while recording every column it reads.
    fn selection_at<'a>(
        &mut self,
        world: &PaletteWorld<'_>,
        position: [i32; 3],
        physics: &'a BlockPhysics,
    ) -> Result<std::borrow::Cow<'a, [Aabb]>, WorldQueryError> {
        if !world.may_pick(physics) {
            return Ok(std::borrow::Cow::Borrowed(&[]));
        }
        if physics.pick_shapes.is_none() {
            return world.block_collision_shapes(position, physics, unit(position));
        }
        if let Some(owner) = world.actor_selection_owner(position, physics)? {
            // An owner's actor geometry is only versioned while its column is loaded.
            let chunk = ChunkKey::new(world.dimension, owner[0] >> 4, owner[2] >> 4);
            self.columns.insert((chunk.x, chunk.z));
            world
                .store
                .collision_revision(chunk)
                .ok_or(WorldQueryError::UnloadedChunk(chunk))?;
        }
        let columns = &mut self.columns;
        world.selection_shapes_with_lookup(position, physics, |neighbour| {
            columns.insert((neighbour[0] >> 4, neighbour[2] >> 4));
            world.primary_runtime_id(neighbour)
        })
    }

    /// Translates resolved shapes to block-local bounds and interns them.
    fn positional(
        &mut self,
        sample: &mut TerrainSample,
        position: [i32; 3],
        physics: &BlockPhysics,
        shapes: Result<std::borrow::Cow<'_, [Aabb]>, WorldQueryError>,
        max_boxes: usize,
    ) -> Result<TerrainShape, TerrainSampleError> {
        let Ok(shapes) = shapes else {
            return Ok(TerrainShape::Unavailable);
        };
        let origin = Vec3::new(
            f64::from(position[0]),
            f64::from(position[1]),
            f64::from(position[2]),
        );
        let offset = physics.shape_offset(position) - origin;
        self.boxes.clear();
        self.boxes
            .extend(shapes.iter().map(|shape| shape.translated(offset)));
        self.intern(sample, max_boxes)
    }

    /// Deduplicates `self.boxes` by exact bit pattern.
    fn intern(
        &mut self,
        sample: &mut TerrainSample,
        max_boxes: usize,
    ) -> Result<TerrainShape, TerrainSampleError> {
        self.key.clear();
        for shape in &self.boxes {
            self.key.extend(
                [
                    shape.min.x,
                    shape.min.y,
                    shape.min.z,
                    shape.max.x,
                    shape.max.y,
                    shape.max.z,
                ]
                .map(f64::to_bits),
            );
        }
        if let Some(&index) = self.shapes.get(self.key.as_slice()) {
            return Ok(TerrainShape::Known(index));
        }
        if sample.shape_boxes.len() / 6 + self.boxes.len() > max_boxes {
            return Err(TerrainSampleError::TooManyBoxes);
        }
        let index = (sample.shape_offsets.len() - 1) as u32;
        sample
            .shape_boxes
            .extend(self.key.iter().map(|&bits| f64::from_bits(bits)));
        sample
            .shape_offsets
            .push((sample.shape_boxes.len() / 6) as u32);
        self.shapes.insert(self.key.as_slice().into(), index);
        Ok(TerrainShape::Known(index))
    }
}

impl Rule {
    const fn fixed() -> Self {
        Self {
            collision: false,
            selection: false,
        }
    }

    /// Geometry depends on position through offsets, paired doors, neighbours or actors.
    fn of(world: &PaletteWorld<'_>, physics: &BlockPhysics, exact: bool) -> Self {
        let offset = !exact || physics.random_offset.is_some();
        let contextual = match physics.pick_shapes {
            Some(_) => physics.selection_context != SelectionContext::Static,
            None => physics.door.is_some(),
        };
        Self {
            collision: offset || physics.door.is_some(),
            selection: world.may_pick(physics) && (offset || contextual),
        }
    }
}

/// Selection shapes of a state without positional rules.
fn static_selection<'a>(world: &PaletteWorld<'_>, physics: &'a BlockPhysics) -> &'a [Aabb] {
    if world.may_pick(physics) {
        physics.pick_shapes.as_deref().unwrap_or(&physics.shapes)
    } else {
        &[]
    }
}

/// The block's own unit cell, which every door resolution intersects.
fn unit(position: [i32; 3]) -> Aabb {
    let origin = Vec3::new(
        f64::from(position[0]),
        f64::from(position[1]),
        f64::from(position[2]),
    );
    Aabb::new(origin, origin + Vec3::ONE)
}

/// Every section the volume touches, acquired once, with its authority.
fn sections(
    world: &PaletteWorld<'_>,
    min: [i32; 3],
    max: [i32; 3],
) -> Vec<(SubChunkKey, Option<std::sync::Arc<world::SubChunk>>, bool)> {
    let mut sections = Vec::new();
    for x in min[0] >> 4..=(max[0] - 1) >> 4 {
        for z in min[2] >> 4..=(max[2] - 1) >> 4 {
            for y in min[1] >> 4..=(max[1] - 1) >> 4 {
                let key = SubChunkKey::new(world.dimension, x, y, z);
                let loaded = world.store.is_sub_chunk_loaded(key);
                sections.push((
                    key,
                    loaded.then(|| world.store.sub_chunk(key)).flatten(),
                    loaded,
                ));
            }
        }
    }
    sections
}

#[cfg(test)]
mod tests;
