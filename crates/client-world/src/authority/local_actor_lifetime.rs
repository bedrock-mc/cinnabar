//! Publishes the complete authority for each replacement local actor.

use super::*;
use protocol_ids::Attribute;

impl WorldAuthority {
    /// Publishes complete authority once per source lifetime, including before its body exists.
    pub(super) fn publish_local_actor_replacement(&mut self, sequence: u64) {
        let revision = self.actors.local_actor_revision();
        if revision == self.published_local_actor_revision {
            return;
        }
        self.published_local_actor_revision = revision;
        let actor = self.actors.get(self.local_player_runtime_id);
        let flags = crate::MovementFlagUpdate::for_replacement(actor);
        let dimensions = actor.map_or_else(
            || crate::ExplicitDimensions::for_spawn(revision),
            |actor| actor.explicit_dimensions,
        );
        let attribute = |name: &str| actor.and_then(|actor| actor.attributes.get(name));
        let movement = attribute(Attribute::Movement.wire_name())
            .and_then(MovementSpeedAttribute::from_attribute);
        let underwater = attribute(Attribute::UnderwaterMovement.wire_name())
            .map(|value| f64::from(value.current));
        let lava =
            attribute(Attribute::LavaMovement.wire_name()).map(|value| f64::from(value.current));
        let air_drag_modifier = attribute(Attribute::AirDragModifier.wire_name())
            .map(|value| value.current)
            .filter(|value| value.is_finite());
        self.local_movement_speed = movement.map(|attribute| attribute.current);
        self.push_committed_control(CommittedControlEvent::LocalMovementFlags {
            sequence,
            tick: 0,
            flags,
            replacement: true,
            dimensions: Some(dimensions),
        });
        self.push_committed_control(CommittedControlEvent::LocalMovementSpeed {
            sequence,
            dimension: self.current_dimension,
            movement,
            movement_received: true,
            underwater,
            lava,
            air_drag_modifier,
            tick: 0,
        });
    }
}
