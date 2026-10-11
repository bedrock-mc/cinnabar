//! Runtime context for compiled selection facts.

use super::{Aabb, BlockPhysics, CollisionRegistry, PaletteWorld, WorldQueryError};
use std::borrow::Cow;
mod actors;
pub(super) mod halo;
#[cfg(test)]
mod tests;
use assets::block_shapes::{
    CompiledBlockShapes, OutlineContext, SelectionContext, ShapeBox, ShapeFlags,
};

impl CollisionRegistry {
    /// Projects compiled interaction data into a runtime ID without state interpretation.
    pub fn set_compiled_selection(&mut self, id: u32, shapes: &CompiledBlockShapes) -> bool {
        let Some(block) = std::sync::Arc::make_mut(&mut self.blocks).get_mut(&id) else {
            return false;
        };
        block.pick_shapes = shapes
            .door
            .is_none()
            .then(|| shapes.selection.iter().copied().map(box_to_aabb).collect());
        block.pick_rule = shapes.pick_rule;
        block.actor_facing = shapes.actor_facing;
        block.actor_visual = shapes.actor_visual.map(box_to_aabb);
        block.shape_flags = shapes.flags;
        block.selection_context = shapes.selection_context;
        block.contextual_selection = shapes
            .contextual_selection
            .iter()
            .copied()
            .map(box_to_aabb)
            .collect();
        block.outline = shapes.outline.map(box_to_aabb);
        block.connected_outline = shapes.connected_outline.map(box_to_aabb);
        block.outline_context = shapes.outline_context;
        self.selection_halo.include(shapes);
        std::sync::Arc::make_mut(&mut self.shape_state_ids).insert(shapes.state_identity, id);
        true
    }

    /// Attaches compiled outline boxes and their neighbour rule to a runtime ID.
    pub fn set_outline_shapes(
        &mut self,
        id: u32,
        outline: Option<Aabb>,
        connected: Option<Aabb>,
        context: OutlineContext,
    ) -> bool {
        let Some(block) = std::sync::Arc::make_mut(&mut self.blocks).get_mut(&id) else {
            return false;
        };
        block.outline = outline;
        block.connected_outline = connected;
        block.outline_context = context;
        true
    }
}

impl PaletteWorld<'_> {
    /// Resolves the visible outline independently of movement and interaction clipping.
    pub fn block_outline(
        &self,
        position: [i32; 3],
        id: u32,
    ) -> Result<Option<Aabb>, WorldQueryError> {
        let physics = self
            .registry
            .physics(id)
            .ok_or(WorldQueryError::UnknownRuntimeId {
                runtime_id: id,
                block: position,
            })?;
        self.block_outline_from_physics(position, physics)
    }

    /// Applies a neighbour rule to compiled boxes, then translates the selected outline.
    pub(super) fn block_outline_from_physics(
        &self,
        position: [i32; 3],
        physics: &BlockPhysics,
    ) -> Result<Option<Aabb>, WorldQueryError> {
        let mut outline = physics.outline;
        match physics.outline_context {
            OutlineContext::TwistingVines | OutlineContext::WeepingVines => {
                let mut neighbour = position;
                let direction = if physics.outline_context == OutlineContext::TwistingVines {
                    1
                } else {
                    -1
                };
                neighbour[1] = neighbour[1]
                    .checked_add(direction)
                    .ok_or(WorldQueryError::CoordinateOutOfRange)?;
                let id = self.primary_runtime_id(neighbour)?;
                if self
                    .registry
                    .physics(id)
                    .is_some_and(|other| other.outline_context == physics.outline_context)
                {
                    outline = physics.connected_outline;
                }
            }
            OutlineContext::Chorus => {
                if let Some(mut shape) = outline {
                    for axis in 0..3 {
                        for direction in [-1, 1] {
                            let mut neighbour = position;
                            neighbour[axis] = neighbour[axis]
                                .checked_add(direction)
                                .ok_or(WorldQueryError::CoordinateOutOfRange)?;
                            let id = self.primary_runtime_id(neighbour)?;
                            let connected = self.registry.physics(id).is_some_and(|other| {
                                other.shape_flags.contains(ShapeFlags::CHORUS_CONNECT)
                                    || (axis == 1
                                        && direction == -1
                                        && other.shape_flags.contains(ShapeFlags::END_STONE))
                            });
                            if connected {
                                if direction < 0 {
                                    shape.min[axis] = 0.0;
                                } else {
                                    shape.max[axis] = 1.0;
                                }
                            }
                        }
                    }
                    outline = Some(shape);
                }
            }
            OutlineContext::Static => {}
        }
        if outline.is_none() {
            let door = if physics.pick_shapes.is_none() {
                self.resolved_door_shape(position, physics)?
            } else {
                None
            };
            let shapes = match physics.pick_shapes.as_deref() {
                Some(_) => self.selection_shapes_at(position, physics)?,
                None => Cow::Borrowed(
                    door.as_ref()
                        .map_or(physics.shapes.as_ref(), std::slice::from_ref),
                ),
            };
            let Some((&first, rest)) = shapes.split_first() else {
                return Ok(None);
            };
            let mut bounds = first;
            for shape in rest {
                for axis in 0..3 {
                    bounds.min[axis] = bounds.min[axis].min(shape.min[axis]);
                    bounds.max[axis] = bounds.max[axis].max(shape.max[axis]);
                }
            }
            outline = Some(bounds);
        }
        Ok(outline.map(|shape| shape.translated(physics.shape_offset(position))))
    }

    /// Resolves a neighbour-dependent choice between compiled selection boxes.
    pub(super) fn selection_shapes_at<'a>(
        &self,
        position: [i32; 3],
        physics: &'a BlockPhysics,
    ) -> Result<Cow<'a, [Aabb]>, WorldQueryError> {
        self.selection_shapes_with_lookup(position, physics, |neighbour| {
            self.primary_runtime_id(neighbour)
        })
    }

    /// Resolves contextual boxes through the caller's palette lookup.
    pub(super) fn selection_shapes_with_lookup<'a>(
        &self,
        position: [i32; 3],
        physics: &'a BlockPhysics,
        mut lookup: impl FnMut([i32; 3]) -> Result<u32, WorldQueryError>,
    ) -> Result<Cow<'a, [Aabb]>, WorldQueryError> {
        match physics.selection_context {
            SelectionContext::PistonArm | SelectionContext::MovingBlock => {
                return self.actor_selection(position, physics);
            }
            SelectionContext::VineSupport => {
                let above =
                    self.neighbour_has_flag(position, 1, 1, ShapeFlags::VINE_SUPPORT, &mut lookup)?;
                let below = self.neighbour_has_flag(
                    position,
                    1,
                    -1,
                    ShapeFlags::VINE_SUPPORT,
                    &mut lookup,
                )?;
                let index = match (above, below) {
                    (true, true) => Some(2),
                    (true, false) => Some(0),
                    (false, true) => Some(1),
                    (false, false) => None,
                };
                return Ok(Cow::Borrowed(index.map_or(&[], |index| {
                    &physics.contextual_selection[index..=index]
                })));
            }
            SelectionContext::ScaffoldSupport
                if !self.neighbour_has_flag(
                    position,
                    1,
                    -1,
                    ShapeFlags::FULL_TOP_SUPPORT,
                    &mut lookup,
                )? =>
            {
                return Ok(Cow::Borrowed(&physics.contextual_selection));
            }
            _ => {}
        }
        if physics.selection_context == SelectionContext::PortalAxis {
            for direction in [-1, 1] {
                let mut neighbour = position;
                neighbour[0] = neighbour[0]
                    .checked_add(direction)
                    .ok_or(WorldQueryError::CoordinateOutOfRange)?;
                let id = lookup(neighbour)?;
                if self
                    .registry
                    .physics(id)
                    .is_some_and(|other| other.shape_flags.contains(ShapeFlags::PORTAL))
                {
                    return Ok(Cow::Borrowed(&physics.contextual_selection));
                }
            }
        }
        Ok(Cow::Borrowed(
            physics.pick_shapes.as_deref().unwrap_or(&physics.shapes),
        ))
    }

    /// Tests a precompiled property on one neighbouring primary block.
    fn neighbour_has_flag(
        &self,
        position: [i32; 3],
        axis: usize,
        direction: i32,
        flag: ShapeFlags,
        lookup: &mut impl FnMut([i32; 3]) -> Result<u32, WorldQueryError>,
    ) -> Result<bool, WorldQueryError> {
        let mut neighbour = position;
        neighbour[axis] = neighbour[axis]
            .checked_add(direction)
            .ok_or(WorldQueryError::CoordinateOutOfRange)?;
        let id = lookup(neighbour)?;
        Ok(self
            .registry
            .physics(id)
            .is_some_and(|other| other.shape_flags.contains(flag)))
    }

    /// Checks a compiled eligibility rule using this view's interaction context.
    pub(super) fn may_pick(&self, physics: &BlockPhysics) -> bool {
        physics.pick_rule.allows(
            self.pick_context,
            physics.flow.and_then(|flow| flow.liquid_depth),
        )
    }
}

/// Projects an asset-owned box into the simulation's coordinate type.
fn box_to_aabb(shape: ShapeBox) -> Aabb {
    Aabb::new(
        super::Vec3::new(shape.min[0], shape.min[1], shape.min[2]),
        super::Vec3::new(shape.max[0], shape.max[1], shape.max[2]),
    )
}
