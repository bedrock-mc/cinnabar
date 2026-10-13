//! Spatial queries for retained actor snapshots.

use super::*;

impl ActorSnapshot {
    /// Feet-anchored `(min, max)` box with server scale applied once.
    /// Omitted dimensions retain the generic actor defaults independently.
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
        let player = matches!(self.kind, ActorKind::Player { .. });
        let dimension = |key, default| match self.metadata.get(&key) {
            Some(ActorMetadataValue::Float(value)) if value.is_finite() && *value > 0.0 => {
                Some(*value)
            }
            Some(ActorMetadataValue::Float(_)) => player.then_some(default),
            _ => Some(default),
        };
        let scale = self.render_scale();
        let half_width = dimension(
            BOUNDING_BOX_WIDTH_METADATA_KEY,
            DEFAULT_ACTOR_COLLISION_WIDTH,
        )? * scale
            * 0.5;
        let height = dimension(
            BOUNDING_BOX_HEIGHT_METADATA_KEY,
            DEFAULT_ACTOR_COLLISION_HEIGHT,
        )? * scale;
        if !half_width.is_finite() || !height.is_finite() {
            return None;
        }
        let [x, y, z] = position;
        Some((
            [x - half_width, y, z - half_width],
            [x + half_width, y + height, z + half_width],
        ))
    }

    /// Interaction boxes at the actor's current pose.
    #[must_use]
    pub fn hit_boxes(&self) -> ActorHitBoxes<'_> {
        self.hit_boxes_at(self.position)
    }

    /// Interaction boxes with collision feet at `position`. Custom pivots are relative
    /// to the native actor position and are independent of render scale and rotation.
    #[must_use]
    pub fn hit_boxes_at(&self, mut position: [f32; 3]) -> ActorHitBoxes<'_> {
        let custom = self
            .hitboxes
            .as_ref()
            .map_or(&[][..], |state| state.boxes.as_slice());
        let fallback = if custom.is_empty() {
            self.bounding_box_at(position)
        } else {
            None
        };
        position[1] += self.network_position_offset();
        ActorHitBoxes::new(custom, position, fallback)
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
        let mut position = self.position;
        position[1] += self.network_position_offset();
        position
    }

    /// Native actor-state origin at supplied retained feet, preserving this class's decoded offset.
    pub fn native_position_at(&self, mut position: [f32; 3]) -> Option<[f32; 3]> {
        position[1] += self.network_position_offset();
        position
            .iter()
            .all(|value| value.is_finite())
            .then_some(position)
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
            ActorKind::Entity { identifier } => {
                let Some(path) = identifier.strip_prefix("minecraft:") else {
                    return 0.0;
                };
                match path {
                    "item" => ITEM_ACTOR_NETWORK_OFFSET,
                    "falling_block" => self.falling_block_network_offset(),
                    "tnt" => self.primed_tnt_network_offset(),
                    "minecart"
                    | "hopper_minecart"
                    | "tnt_minecart"
                    | "chest_minecart"
                    | "command_block_minecart" => MINECART_NETWORK_OFFSET,
                    "boat" => BOAT_NETWORK_OFFSET,
                    _ => 0.0,
                }
            }
        }
    }
}
