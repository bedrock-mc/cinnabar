//! Publishes admitted local movement authority and complete actor lifetimes.

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
        let speed = movement_attributes(sequence, self.current_dimension, 0, true, |name| {
            actor.and_then(|actor| actor.attributes.get(name))
        })
        .expect("replacement always publishes attribute authority");
        if let CommittedControlEvent::LocalMovementSpeed { movement, .. } = speed {
            self.local_movement_speed = movement.map(|attribute| attribute.current);
        }
        self.push_committed_control(CommittedControlEvent::LocalMovementFlags {
            sequence,
            tick: 0,
            flags,
            replacement: true,
            dimensions: Some(dimensions),
        });
        self.push_committed_control(speed);
    }

    /// Publishes only supplied attributes that remain in the admitted actor source.
    pub(super) fn publish_local_attribute_update(
        &mut self,
        sequence: u64,
        tick: u64,
        attributes: &[ActorAttribute],
    ) {
        let Some(actor) = self.actors.actor_authority(self.local_player_runtime_id) else {
            return;
        };
        let control = movement_attributes(sequence, self.current_dimension, tick, false, |name| {
            attributes
                .iter()
                .any(|attribute| attribute.name.as_ref() == name)
                .then(|| actor.attributes.get(name))
                .flatten()
        });
        if let Some(control) = control {
            if let CommittedControlEvent::LocalMovementSpeed {
                movement: Some(attribute),
                ..
            } = control
            {
                self.local_movement_speed = Some(attribute.current);
            }
            self.push_committed_control(control);
        }
    }
}

/// Projects accepted movement attributes through one shared sparse-or-replacement contract.
fn movement_attributes<'a>(
    sequence: u64,
    dimension: i32,
    tick: u64,
    replacement: bool,
    attribute: impl Fn(&str) -> Option<&'a ActorAttribute>,
) -> Option<CommittedControlEvent> {
    let received = attribute(Attribute::Movement.wire_name());
    let movement = received.and_then(MovementSpeedAttribute::from_attribute);
    let underwater =
        attribute(Attribute::UnderwaterMovement.wire_name()).map(|value| f64::from(value.current));
    let lava = attribute(Attribute::LavaMovement.wire_name()).map(|value| f64::from(value.current));
    let air_drag_modifier = attribute(Attribute::AirDragModifier.wire_name())
        .map(|value| value.current)
        .filter(|value| value.is_finite());
    (replacement
        || received.is_some()
        || underwater.is_some()
        || lava.is_some()
        || air_drag_modifier.is_some())
    .then_some(CommittedControlEvent::LocalMovementSpeed {
        sequence,
        dimension,
        movement,
        movement_received: replacement || received.is_some(),
        underwater,
        lava,
        air_drag_modifier,
        tick,
    })
}
