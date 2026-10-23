//! Passenger order used to select a locally controlled vehicle.

use super::*;

/// One passenger's vehicle and position in the arrival order.
#[derive(Debug, Clone, Copy)]
pub(super) struct RiderLink {
    pub vehicle: i64,
    pub order: i128,
}

impl ActorStore {
    /// Tests whether this player is the selected controller of its current vehicle.
    pub(crate) fn rider_controls_vehicle(&self, rider: i64) -> bool {
        if !self
            .snapshot_by_unique(rider)
            .is_some_and(|actor| matches!(actor.kind, ActorKind::Player { .. }))
        {
            return false;
        }
        let Some(link) = self.rider_to_ridden.get(&rider) else {
            return false;
        };
        let Some(mount) = self.snapshot_by_unique(link.vehicle) else {
            return false;
        };
        use assets::facts::entity::NativeEntityClass;
        let controllable = match mount.entity_fact.native_class {
            NativeEntityClass::Boat => true,
            NativeEntityClass::Horse => [
                protocol_ids::ActorFlag::Saddled,
                protocol_ids::ActorFlag::Tamed,
            ]
            .into_iter()
            .all(|flag| mount.flag(flag.wire_value())),
            _ => false,
        };
        controllable
            && self
                .rider_to_ridden
                .iter()
                .filter(|(_, other)| other.vehicle == link.vehicle)
                .min_by_key(|(_, other)| other.order)
                .is_some_and(|(&id, _)| id == rider)
    }
}
