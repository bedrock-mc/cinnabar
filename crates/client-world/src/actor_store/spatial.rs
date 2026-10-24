//! Spatial queries for retained actor snapshots.

use super::*;
use protocol_ids::ActorDataKey;

/// Collision dimensions retain a small positive extent after server scaling.
pub(super) const MIN_COLLISION_DIMENSION: f32 = 0.005;

impl ActorSnapshot {
    /// Bounds for movement use constructor dimensions when the server omits them.
    pub(super) fn movement_collision_bounds(&self) -> Option<ActorCollisionBounds> {
        self.bounding_box()
    }

    /// Uses the synced collision flag when supplied, otherwise the default definition.
    #[must_use]
    pub fn is_collidable(&self) -> bool {
        let bit = ActorFlag::Collidable.wire_value();
        let key = if bit < 64 {
            ActorDataKey::Flags
        } else {
            ActorDataKey::FlagsTwo
        };
        match self.metadata.get(&key.wire_value()) {
            Some(ActorMetadataValue::Flags(_) | ActorMetadataValue::FlagsExtended(_)) => {
                self.flag(bit)
            }
            _ => self.entity_fact.collidable,
        }
    }

    /// Feet-anchored `(min, max)` box with server scale applied once.
    /// Omitted dimensions retain the actor definition's defaults independently.
    #[must_use]
    pub fn bounding_box(&self) -> Option<([f32; 3], [f32; 3])> {
        self.bounding_box_at(self.position)
    }

    /// [`Self::bounding_box`] with the feet at `position`.
    #[must_use]
    pub fn bounding_box_at(&self, position: [f32; 3]) -> Option<([f32; 3], [f32; 3])> {
        if let Some(painting) = &self.painting {
            use assets::painting::{PAINTING_HALF_DEPTH, PAINTING_HITBOX_INSET, motive};
            use protocol::PaintingDirection;
            let [width, height] = motive(&painting.motive)
                .size
                .map(|side| f32::from(side) / 32.0 - PAINTING_HITBOX_INSET);
            let half = match painting.direction {
                PaintingDirection::South | PaintingDirection::North => {
                    [width, height, PAINTING_HALF_DEPTH]
                }
                PaintingDirection::West | PaintingDirection::East => {
                    [PAINTING_HALF_DEPTH, height, width]
                }
            };
            return Some((
                std::array::from_fn(|axis| position[axis] - half[axis]),
                std::array::from_fn(|axis| position[axis] + half[axis]),
            ));
        }
        let defaults = self.entity_fact.collision.unwrap_or([
            DEFAULT_ACTOR_COLLISION_WIDTH,
            DEFAULT_ACTOR_COLLISION_HEIGHT,
        ]);
        let width = self.scaled_collision_dimension(ActorDataKey::Width, defaults[0])?;
        let height = self.scaled_collision_dimension(ActorDataKey::Height, defaults[1])?;
        let half_width = width * 0.5;
        let [x, y, z] = position;
        Some((
            [x - half_width, y, z - half_width],
            [x + half_width, y + height, z + half_width],
        ))
    }

    /// Applies server scale and the collision minimum to one dimension, independently of position.
    pub(super) fn scaled_collision_dimension(
        &self,
        key: ActorDataKey,
        default: f32,
    ) -> Option<f32> {
        let dimension = match self.metadata.get(&key.wire_value()) {
            Some(ActorMetadataValue::Float(value)) if value.is_finite() => *value,
            Some(ActorMetadataValue::Float(_)) => {
                matches!(self.kind, ActorKind::Player { .. }).then_some(default)?
            }
            _ => default,
        };
        let scale = match self.metadata.get(&ActorDataKey::Scale.wire_value()) {
            Some(ActorMetadataValue::Float(value)) => *value,
            _ => 1.0,
        };
        let scaled = dimension * scale;
        scaled
            .is_finite()
            .then_some(scaled.max(MIN_COLLISION_DIMENSION))
    }

    /// Interaction boxes at the actor's current pose.
    #[must_use]
    pub fn hit_boxes(&self) -> ActorHitBoxes<'_> {
        self.hit_boxes_at_pose(self.current_pose())
    }

    /// Interaction boxes with collision feet at `position`. Custom pivots are relative
    /// to the native actor position and are independent of render scale and rotation.
    #[must_use]
    pub fn hit_boxes_at(&self, mut position: [f32; 3]) -> ActorHitBoxes<'_> {
        let feet = position;
        position[1] += self.network_position_offset();
        self.hit_boxes_with_origin(feet, position)
    }

    /// Uses the exact native origin retained by a current or predicted actor pose.
    pub(crate) fn hit_boxes_at_pose(&self, pose: ActorPose) -> ActorHitBoxes<'_> {
        self.hit_boxes_with_origin(
            pose.position,
            pose.native_position(self.network_position_offset()),
        )
    }

    /// Anchors collision boxes at feet and custom boxes at the matching native origin.
    fn hit_boxes_with_origin(&self, feet: [f32; 3], native: [f32; 3]) -> ActorHitBoxes<'_> {
        let custom = self
            .hitboxes
            .as_ref()
            .map_or(&[][..], |state| state.boxes.as_slice());
        let fallback = if custom.is_empty() {
            self.bounding_box_at(feet)
        } else {
            None
        };
        ActorHitBoxes::new(custom, native, fallback)
    }

    /// Samples 0.66 of the body height above interpolated feet.
    /// Network position offsets have already been removed by the actor store.
    pub fn brightness_sample_position(&self, mut feet: [f32; 3]) -> [f32; 3] {
        if self.painting.is_some() {
            return feet;
        }
        if let Some((min, max)) = self.bounding_box() {
            feet[1] += 0.66 * (max[1] - min[1]);
        }
        feet
    }

    /// Restores the native actor-state origin from stored collision feet.
    pub fn native_position(&self) -> [f32; 3] {
        self.current_native_position()
    }

    /// Native actor-state origin at supplied retained feet, preserving this class's decoded offset.
    pub fn native_position_at(&self, mut position: [f32; 3]) -> Option<[f32; 3]> {
        position[1] += self.network_position_offset();
        position
            .iter()
            .all(|value| value.is_finite())
            .then_some(position)
    }

    /// Whether this actor is any minecart variant.
    pub(crate) fn is_minecart(&self) -> bool {
        self.entity_fact.native_class == NativeEntityClass::Minecart
    }

    /// Offset from stored collision feet to the native actor-state position.
    pub(crate) fn network_position_offset(&self) -> f32 {
        match &self.kind {
            ActorKind::Player { .. } => {
                if self.player_is_sleeping() {
                    SLEEPING_PLAYER_NETWORK_OFFSET
                } else {
                    PLAYER_NETWORK_OFFSET
                }
            }
            ActorKind::Entity { .. } => match self.entity_fact.native_class {
                NativeEntityClass::FallingBlock => self.falling_block_network_offset(),
                NativeEntityClass::Tnt => self.primed_tnt_network_offset(),
                _ => self.entity_fact.network_y_offset,
            },
        }
    }
}

impl ActorStore {
    /// Shares solid bounds in stable runtime order, excluding the mover and its current mount.
    pub(crate) fn movement_collider_snapshot(
        &self,
        mover: u64,
        rider: i64,
    ) -> std::sync::Arc<[ActorCollisionBounds]> {
        self.effect_members
            .collidable
            .snapshot(mover, self.ridden_unique_id(rider))
    }
}

#[cfg(test)]
#[path = "spatial_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "collision_tests.rs"]
mod collision_tests;
