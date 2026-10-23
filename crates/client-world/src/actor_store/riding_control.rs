//! Passenger order used to select a locally controlled vehicle.

use super::*;

/// One passenger's vehicle and position in the arrival order.
#[derive(Debug, Clone, Copy)]
pub(super) struct RiderLink {
    pub vehicle: i64,
    /// Arrival order while pending, then the accepted passenger position.
    pub order: i128,
    pub pending: bool,
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
        if link.pending {
            return false;
        }
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
                .filter(|(_, other)| other.vehicle == link.vehicle && !other.pending)
                .min_by_key(|(_, other)| other.order)
                .is_some_and(|(&id, _)| id == rider)
    }
}

impl ActorStore {
    /// Retains links while deferring passenger insertion until earlier identities are known.
    pub(super) fn apply_link_inner(&mut self, event: ActorLinkEvent) -> ActorApplyResult {
        match event.link_type {
            ActorLinkType::Unknown(_) => ActorApplyResult::Updated,
            ActorLinkType::Remove => {
                if self.ridden_unique_id(event.rider_unique_id) == Some(event.ridden_unique_id) {
                    self.rider_to_ridden.remove(&event.rider_unique_id);
                    self.resolve_vehicle_links(event.ridden_unique_id);
                }
                ActorApplyResult::Updated
            }
            ActorLinkType::Rider | ActorLinkType::Passenger => {
                let previous_vehicle = self.ridden_unique_id(event.rider_unique_id);
                if previous_vehicle == Some(event.ridden_unique_id) {
                    return ActorApplyResult::Updated;
                }
                if previous_vehicle.is_none() && self.rider_to_ridden.len() >= self.max_actor_links
                {
                    return ActorApplyResult::CapacityRejected;
                }
                self.rider_to_ridden.insert(
                    event.rider_unique_id,
                    RiderLink {
                        vehicle: event.ridden_unique_id,
                        order: i128::from(self.next_link_order),
                        pending: true,
                    },
                );
                self.next_link_order = self.next_link_order.saturating_add(1);
                if let Some(vehicle) = previous_vehicle {
                    self.resolve_vehicle_links(vehicle);
                }
                self.resolve_vehicle_links(event.ridden_unique_id);
                ActorApplyResult::Updated
            }
        }
    }

    /// Resolves the pending vehicle suffix when this rider's actor becomes available.
    pub(super) fn resolve_rider_links(&mut self, rider: i64) {
        if let Some(vehicle) = self.ridden_unique_id(rider) {
            self.resolve_vehicle_links(vehicle);
        }
    }

    /// Inserts known pending riders in arrival order without reordering accepted survivors.
    fn resolve_vehicle_links(&mut self, vehicle: i64) {
        let mut first = self
            .rider_to_ridden
            .iter()
            .filter(|(_, link)| link.vehicle == vehicle && !link.pending)
            .min_by_key(|(_, link)| link.order)
            .map(|(&id, link)| {
                (
                    link.order,
                    self.snapshot_by_unique(id)
                        .is_some_and(|actor| matches!(actor.kind, ActorKind::Player { .. })),
                )
            });
        let mut pending: Vec<_> = self
            .rider_to_ridden
            .iter()
            .filter(|(_, link)| link.vehicle == vehicle && link.pending)
            .map(|(&id, link)| (link.order, id))
            .collect();
        pending.sort_unstable();
        for (order, id) in pending {
            let Some(actor) = self.snapshot_by_unique(id) else {
                break;
            };
            let player = matches!(actor.kind, ActorKind::Player { .. });
            let order = match first {
                Some((first_order, false)) if player => first_order - 1,
                _ => order,
            };
            let link = self
                .rider_to_ridden
                .get_mut(&id)
                .expect("retained pending rider");
            link.order = order;
            link.pending = false;
            if first.is_none_or(|(first_order, _)| order < first_order) {
                first = Some((order, player));
            }
        }
    }

    /// Removes both riding directions and releases any suffix waiting on this rider.
    pub(super) fn remove_links_for(&mut self, unique_id: i64) {
        let previous_vehicle = self
            .rider_to_ridden
            .remove(&unique_id)
            .map(|link| link.vehicle);
        self.rider_to_ridden
            .retain(|_, link| link.vehicle != unique_id);
        if let Some(vehicle) = previous_vehicle {
            self.resolve_vehicle_links(vehicle);
        }
    }
}
