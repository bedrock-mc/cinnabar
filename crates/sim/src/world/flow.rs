//! Native liquid-cell direction (LiquidBlockBase::_getFlow, current 0x0395d2f0).

use std::collections::BTreeSet;

use super::{
    BlockPhysicsFlags, ChunkKey, CollisionQuery, CollisionRegistry, PaletteWorld, Vec3,
    WorldQueryError,
};

pub(super) const NORMALIZATION_THRESHOLD: f32 = 0.0001;
const FALLING_DOWNWARD_COMPONENT: f32 = -6.0;
// Facing::PLANAR at PE 0x15013e0c7; face masks are 1 << Facing.
const DIRECTIONS: [(usize, i32, u8); 4] = [(2, -1, 2), (0, 1, 5), (2, 1, 3), (0, -1, 4)];

/// Source-identified material and liquid detection facts, independent of meshes.
///
/// `blocked_faces` is BlockLiquidDetectionComponent's cache byte at Block+0xb9.
/// `allowed_faces` records the native directional virtual's admitted faces.
/// Missing registrations preserve an unknown result rather than infer these
/// facts from collision boxes or render face coverage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowBlockFacts {
    pub blocks_motion: bool,
    pub is_solid: bool,
    pub blocked_faces: u8,
    pub allowed_faces: u8,
    pub liquid_depth: Option<u8>,
}

impl CollisionRegistry {
    /// Retains independently established native flow facts for a runtime ID.
    pub fn set_flow_facts(&mut self, runtime_id: u32, facts: FlowBlockFacts) -> bool {
        let Some(block) = std::sync::Arc::make_mut(&mut self.blocks).get_mut(&runtime_id) else {
            return false;
        };
        block.flow = Some(facts);
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LiquidMaterial {
    Water,
    Lava,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct LiquidCell {
    pub(super) material: LiquidMaterial,
    pub(super) depth: u8,
    pub(super) depth_known: bool,
}

impl LiquidCell {
    fn effective_depth(self) -> i32 {
        if self.depth < 8 {
            i32::from(self.depth)
        } else {
            0
        }
    }
}

struct Samples<'a, 'world> {
    world: &'a PaletteWorld<'world>,
    chunks: BTreeSet<ChunkKey>,
}

impl Samples<'_, '_> {
    fn record(&mut self, [x, _, z]: [i32; 3]) {
        self.chunks
            .insert(ChunkKey::new(self.world.dimension, x >> 4, z >> 4));
    }

    fn liquid(&mut self, block: [i32; 3]) -> Result<Option<LiquidCell>, WorldQueryError> {
        self.record(block);
        let ids = self.world.runtime_ids_at(block)?;
        // getLiquidBlock (+0x28): extra layer unless it is air, then primary.
        let runtime_id = ids
            .get(1)
            .copied()
            .filter(|id| *id != self.world.registry.air_runtime_id)
            .unwrap_or(ids[0]);
        let physics = self
            .world
            .registry
            .physics(runtime_id)
            .ok_or(WorldQueryError::UnknownRuntimeId { runtime_id, block })?;
        let material = if physics.flags.contains(BlockPhysicsFlags::WATER) {
            LiquidMaterial::Water
        } else if physics.flags.contains(BlockPhysicsFlags::LAVA) {
            LiquidMaterial::Lava
        } else {
            return Ok(None);
        };
        // A water-like block without native LiquidDepth (including bubble
        // columns) keeps its material but provides no current authority.
        let depth = physics.flow.and_then(|facts| facts.liquid_depth);
        Ok(Some(LiquidCell {
            material,
            depth: depth.unwrap_or_default(),
            depth_known: depth.is_some(),
        }))
    }

    fn primary(&mut self, block: [i32; 3]) -> Result<Option<FlowBlockFacts>, WorldQueryError> {
        self.record(block);
        let runtime_id = self.world.primary_runtime_id(block)?;
        let physics = self
            .world
            .registry
            .physics(runtime_id)
            .ok_or(WorldQueryError::UnknownRuntimeId { runtime_id, block })?;
        Ok(physics.flow.or_else(|| {
            (runtime_id == self.world.registry.air_runtime_id).then_some(FlowBlockFacts {
                blocks_motion: false,
                is_solid: false,
                blocked_faces: 0,
                allowed_faces: 0x3f,
                liquid_depth: None,
            })
        }))
    }

    fn finish<T>(self, value: T) -> Result<CollisionQuery<T>, WorldQueryError> {
        Ok(CollisionQuery {
            value,
            identity: self.world.identity_for_chunks(self.chunks)?,
        })
    }
}

fn neighbor(mut block: [i32; 3], axis: usize, step: i32) -> Result<[i32; 3], WorldQueryError> {
    block[axis] = block[axis]
        .checked_add(step)
        .ok_or(WorldQueryError::CoordinateOutOfRange)?;
    Ok(block)
}

fn admits(facts: FlowBlockFacts, facing: u8) -> bool {
    let mask = 1 << facing;
    facts.blocked_faces & mask == 0 && facts.allowed_faces & mask != 0
}

fn normalize([x, y, z]: [f32; 3]) -> [f32; 3] {
    // Matching PE 14395d72d / 14395d793: (X² + Y²) + Z².
    let length = (x * x + y * y + z * z).sqrt();
    if length >= NORMALIZATION_THRESHOLD {
        [x / length, y / length, z / length]
    } else {
        [0.0; 3]
    }
}

pub(super) fn liquid_at(
    world: &PaletteWorld<'_>,
    block: [i32; 3],
) -> Result<CollisionQuery<Option<LiquidCell>>, WorldQueryError> {
    let mut samples = Samples {
        world,
        chunks: BTreeSet::new(),
    };
    let value = samples.liquid(block)?;
    samples.finish(value)
}

pub(super) fn flow_at(
    world: &PaletteWorld<'_>,
    block: [i32; 3],
    material: LiquidMaterial,
) -> Result<Option<CollisionQuery<Vec3>>, WorldQueryError> {
    let mut samples = Samples {
        world,
        chunks: BTreeSet::new(),
    };
    let cell = samples
        .liquid(block)?
        .filter(|cell| cell.material == material);
    if cell.is_some_and(|cell| !cell.depth_known) {
        return Ok(None);
    }
    let depth = cell.map_or(-1, LiquidCell::effective_depth);
    let mut flow = [0.0_f32; 3];
    for (axis, step, facing) in DIRECTIONS {
        let next = neighbor(block, axis, step)?;
        let other = samples
            .liquid(next)?
            .filter(|cell| cell.material == material);
        if let Some(other) = other {
            if !other.depth_known {
                return Ok(None);
            }
            let Some(other_facts) = samples.primary(next)? else {
                return Ok(None);
            };
            let Some(current_facts) = samples.primary(block)? else {
                return Ok(None);
            };
            if admits(other_facts, facing ^ 1) && admits(current_facts, facing) {
                flow[axis] += ((other.effective_depth() - depth) * step) as f32;
                continue;
            }
        }
        let obstruction = samples.primary(next)?;
        if obstruction.is_some_and(|facts| facts.blocks_motion) {
            continue;
        }
        let below = samples
            .liquid(neighbor(next, 1, -1)?)?
            .filter(|cell| cell.material == material);
        if let Some(below) = below {
            if obstruction.is_none() || !below.depth_known {
                return Ok(None);
            }
            flow[axis] += ((below.effective_depth() - depth + 8) * step) as f32;
        }
    }
    if cell.is_some_and(|cell| cell.depth >= 8) {
        let mut unknown = false;
        let mut solid = false;
        for (axis, step, _) in DIRECTIONS {
            let next = neighbor(block, axis, step)?;
            for probe in [next, neighbor(next, 1, 1)?] {
                match samples.primary(probe)? {
                    Some(facts) if facts.is_solid => {
                        solid = true;
                        break;
                    }
                    Some(_) => {}
                    None => unknown = true,
                }
            }
            if solid {
                break;
            }
        }
        if solid {
            flow = normalize(flow);
            flow[1] += FALLING_DOWNWARD_COMPONENT;
        } else if unknown {
            return Ok(None);
        }
    }
    let [x, y, z] = normalize(flow);
    Ok(Some(samples.finish(Vec3::new(
        f64::from(x),
        f64::from(y),
        f64::from(z),
    ))?))
}
