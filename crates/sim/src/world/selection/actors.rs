use super::super::BlockPhysics;
use crate::{Aabb, PaletteWorld, Vec3, WorldQueryError};
use assets::block_shapes::{PISTON_DIRECTIONS, SelectionContext};
use std::borrow::Cow;
use world::{BlockEntityKey, ShapeActor};

impl PaletteWorld<'_> {
    /// Identifies the actor owner whose revision authorizes a contextual pick shape.
    pub(in crate::world) fn actor_selection_owner(
        &self,
        position: [i32; 3],
        physics: &BlockPhysics,
    ) -> Result<Option<[i32; 3]>, WorldQueryError> {
        match physics.selection_context {
            SelectionContext::PistonArm => {
                let Some(facing) = physics.actor_facing else {
                    return Ok(None);
                };
                let direction = PISTON_DIRECTIONS[usize::from(facing)];
                let mut owner = position;
                for axis in 0..3 {
                    owner[axis] = owner[axis]
                        .checked_sub(direction[axis])
                        .ok_or(WorldQueryError::CoordinateOutOfRange)?;
                }
                Ok(Some(owner))
            }
            SelectionContext::MovingBlock => Ok(self
                .moving_visual(position)
                .and_then(|(_, piston, _)| (piston != [0, -1, 0]).then_some(piston))),
            _ => Ok(None),
        }
    }

    /// Resolves actor-dependent boxes from typed metadata and compiled visual states.
    pub(super) fn actor_selection<'a>(
        &self,
        position: [i32; 3],
        physics: &'a BlockPhysics,
    ) -> Result<Cow<'a, [Aabb]>, WorldQueryError> {
        match physics.selection_context {
            SelectionContext::PistonArm => {
                let Some(facing) = physics.actor_facing else {
                    return Ok(Cow::Borrowed(&[]));
                };
                let direction = PISTON_DIRECTIONS[usize::from(facing)];
                let piston = self
                    .actor_selection_owner(position, physics)?
                    .expect("admitted piston facing has an owner");
                let Some(progress) = self.piston_progress(piston) else {
                    return Ok(Cow::Borrowed(&[]));
                };
                let offset = direction.map(|value| value as f32 * (progress - 1.0));
                Ok(Cow::Owned(
                    physics
                        .pick_shapes
                        .as_deref()
                        .unwrap_or(&[])
                        .iter()
                        .map(|shape| translated_native(*shape, offset))
                        .collect(),
                ))
            }
            SelectionContext::MovingBlock => {
                let Some((shape, piston, expanding)) = self.moving_visual(position) else {
                    if self.selection_actor(position).is_some() {
                        let _ = self.unsupported_actor_selections.try_update(
                            std::sync::atomic::Ordering::Relaxed,
                            std::sync::atomic::Ordering::Relaxed,
                            |count| Some(count.saturating_add(1)),
                        );
                    }
                    return Ok(Cow::Borrowed(&[]));
                };
                let mut offset = [0.0; 3];
                if piston != [0, -1, 0] {
                    let id = self.primary_runtime_id(piston)?;
                    if let Some(facing) = self
                        .registry
                        .physics(id)
                        .and_then(|state| state.actor_facing)
                        && let Some(progress) = self.piston_progress(piston)
                    {
                        offset = PISTON_DIRECTIONS[usize::from(facing)].map(|value| {
                            value as f32 * (progress - if expanding { 1.0 } else { 0.0 })
                        });
                    }
                }
                Ok(Cow::Owned(vec![translated_native(shape, offset)]))
            }
            _ => unreachable!("actor selection requires an actor context"),
        }
    }

    /// Counts unsupported actor geometry resolutions, including repeated ray halo visits.
    pub fn unsupported_actor_selection_count(&self) -> u32 {
        self.unsupported_actor_selections
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Reads selection metadata through the same column observation as palette cells.
    fn selection_actor(&self, position: [i32; 3]) -> Option<std::sync::Arc<world::BlockEntityNbt>> {
        self.record_cell_read(world::SubChunkKey::new(
            self.dimension,
            position[0] >> 4,
            position[1] >> 4,
            position[2] >> 4,
        ))
        .ok()?;
        self.store.block_entity(actor_key(self.dimension, position))
    }

    /// Resolves usable carried geometry before any piston-column dependency is required.
    fn moving_visual(&self, position: [i32; 3]) -> Option<(Aabb, [i32; 3], bool)> {
        let actor = self.selection_actor(position)?;
        let ShapeActor::Moving {
            state_identity,
            piston,
            expanding,
        } = actor.shape_actor()
        else {
            return None;
        };
        let id = self.registry.shape_state_ids.get(state_identity)?;
        let shape = self.registry.physics(*id)?.actor_visual?;
        Some((shape, *piston, *expanding))
    }

    /// Reads a finite piston progress value already validated during NBT ingestion.
    fn piston_progress(&self, position: [i32; 3]) -> Option<f32> {
        let actor = self.selection_actor(position)?;
        match actor.shape_actor() {
            ShapeActor::Piston { progress_bits } => Some(f32::from_bits(*progress_bits)),
            _ => None,
        }
    }
}

/// Creates the dimension-aware key for a block-actor snapshot.
fn actor_key(dimension: i32, [x, y, z]: [i32; 3]) -> BlockEntityKey {
    BlockEntityKey::new(dimension, x, y, z)
}

/// Adds actor offsets in native coordinate precision before widening the box.
fn translated_native(shape: Aabb, offset: [f32; 3]) -> Aabb {
    let min =
        std::array::from_fn::<_, 3, _>(|axis| f64::from(shape.min[axis] as f32 + offset[axis]));
    let max =
        std::array::from_fn::<_, 3, _>(|axis| f64::from(shape.max[axis] as f32 + offset[axis]));
    Aabb::new(
        Vec3::new(min[0], min[1], min[2]),
        Vec3::new(max[0], max[1], max[2]),
    )
}
