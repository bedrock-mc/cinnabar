//! Player liquid-current impulse from the preceding pose, as vanilla applies it.

use std::collections::BTreeMap;

use super::{
    Aabb, CollisionQuery, PaletteWorld, Vec3, WorldCollisionIdentity, WorldQueryError, block_floor,
    flow::{self, LiquidCell, LiquidMaterial, NORMALIZATION_THRESHOLD},
    liquid_probe_bounds, validate_collision_query,
};
use crate::MAX_BLOCK_SAMPLES_PER_TICK;

const WATER_IMPULSE: f32 = 0.014;
const LAVA_IMPULSE: f32 = 0.0035;

struct Contact {
    material: LiquidMaterial,
    depths_known: bool,
    min: [i32; 3],
    max: [i32; 3],
    cells: Vec<([i32; 3], u8)>,
}

struct Samples<'a, 'world> {
    world: &'a PaletteWorld<'world>,
    liquids: BTreeMap<[i32; 3], Option<LiquidCell>>,
    identity: Option<WorldCollisionIdentity>,
}

impl Samples<'_, '_> {
    fn merge(&mut self, identity: &WorldCollisionIdentity) -> Result<(), WorldQueryError> {
        self.identity = Some(match self.identity.take() {
            Some(previous) => previous.merge(identity)?,
            None => identity.clone(),
        });
        Ok(())
    }

    fn liquid_at(&mut self, block: [i32; 3]) -> Result<Option<LiquidCell>, WorldQueryError> {
        if let Some(cell) = self.liquids.get(&block) {
            return Ok(*cell);
        }
        if self.liquids.len() == MAX_BLOCK_SAMPLES_PER_TICK {
            return Err(WorldQueryError::QueryExtentExceeded);
        }
        let sample = flow::liquid_at(self.world, block)?;
        self.merge(&sample.identity)?;
        self.liquids.insert(block, sample.value);
        Ok(sample.value)
    }

    fn contact(
        &mut self,
        aabb: Aabb,
        material: LiquidMaterial,
    ) -> Result<Contact, WorldQueryError> {
        let probe = liquid_probe_bounds(aabb, material == LiquidMaterial::Water);
        let min = block_floor(probe.min)?;
        // Vanilla's integer cell range uses floor(f32(upper + 1)) as its exclusive
        // end. Preserve that operation even where large coordinates round
        // the addition away rather than substituting floor(upper) + 1.
        let end = block_floor(Vec3::new(
            f64::from(probe.max.x as f32 + 1.0),
            f64::from(probe.max.y as f32 + 1.0),
            f64::from(probe.max.z as f32 + 1.0),
        ))?;
        let mut max = end;
        for value in &mut max {
            *value = value
                .checked_sub(1)
                .ok_or(WorldQueryError::CoordinateOutOfRange)?;
        }
        let mut cells = Vec::new();
        let mut depths_known = true;
        // Native gather order is Y, then Z, then X; retained sums are f32.
        for y in min[1]..end[1] {
            for z in min[2]..end[2] {
                for x in min[0]..end[0] {
                    let block = [x, y, z];
                    if let Some(cell) = self.liquid_at(block)?
                        && cell.material == material
                    {
                        depths_known &= cell.depth_known;
                        cells.push((block, cell.depth));
                    }
                }
            }
        }
        Ok(Contact {
            material,
            depths_known,
            min,
            max,
            cells,
        })
    }

    fn flow_admitted(&mut self, contact: &Contact) -> Result<Option<bool>, WorldQueryError> {
        if contact.cells.iter().any(|(_, depth)| *depth > 0) {
            return Ok(Some(true));
        }
        let mut unknown = false;
        for &(block, _) in &contact.cells {
            // The native entry flags select its outer Z-, X+, Z+, X- faces.
            for (axis, step) in [(2, -1), (0, 1), (2, 1), (0, -1)] {
                let edge = if step < 0 {
                    contact.min[axis]
                } else {
                    contact.max[axis]
                };
                if block[axis] != edge {
                    continue;
                }
                let mut neighbor = block;
                neighbor[axis] = neighbor[axis]
                    .checked_add(step)
                    .ok_or(WorldQueryError::CoordinateOutOfRange)?;
                if let Some(cell) = self.liquid_at(neighbor)?
                    && cell.material == contact.material
                {
                    if !cell.depth_known {
                        unknown = true;
                    } else if cell.depth != 0 {
                        return Ok(Some(true));
                    }
                }
            }
        }
        Ok((!unknown).then_some(false))
    }
}

pub(super) fn sample(
    world: &PaletteWorld<'_>,
    previous_pose: Aabb,
) -> Result<Option<CollisionQuery<Vec3>>, WorldQueryError> {
    validate_collision_query(previous_pose)?;
    let mut samples = Samples {
        world,
        liquids: BTreeMap::new(),
        identity: None,
    };
    let lava = samples.contact(previous_pose, LiquidMaterial::Lava)?;
    let water = samples.contact(previous_pose, LiquidMaterial::Water)?;
    // Vanilla selects lava when both material probes find contact.
    let contact = if lava.cells.is_empty() { water } else { lava };
    if !contact.depths_known {
        return Ok(None);
    }
    let mut value = Vec3::ZERO;
    let Some(admitted) = samples.flow_admitted(&contact)? else {
        return Ok(None);
    };
    if !contact.cells.is_empty() && admitted {
        let mut sum = [0.0_f32; 3];
        for (block, _) in contact.cells {
            let Some(flow) = flow::flow_at(world, block, contact.material)? else {
                // A provider without the native obstruction facts cannot
                // establish this impulse from collision geometry alone.
                return Ok(None);
            };
            samples.merge(&flow.identity)?;
            let components = [flow.value.x, flow.value.y, flow.value.z];
            for (total, component) in sum.iter_mut().zip(components) {
                *total += component as f32;
            }
        }
        let [x, y, z] = sum;
        // Vanilla sums (X² + Y²) + Z² in this order; f32 rounding depends on it.
        let length = (x * x + y * y + z * z).sqrt();
        if length >= NORMALIZATION_THRESHOLD {
            let scale = match contact.material {
                LiquidMaterial::Water => WATER_IMPULSE,
                LiquidMaterial::Lava => LAVA_IMPULSE,
            };
            value = Vec3::new(
                f64::from(x / length * scale),
                f64::from(y / length * scale),
                f64::from(z / length * scale),
            );
        }
    }
    Ok(Some(CollisionQuery {
        value,
        identity: match samples.identity {
            Some(identity) => identity,
            None => WorldCollisionIdentity::new(world.registry.identity(), [])?,
        },
    }))
}
