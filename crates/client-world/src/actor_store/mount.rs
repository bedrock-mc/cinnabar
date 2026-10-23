use protocol::{ActorLinkEvent, ActorLinkType, ActorMetadataValue};
use protocol_ids::ActorDataKey;

use super::{ActorApplyResult, ActorKind, ActorSnapshot};

impl ActorSnapshot {
    /// Returns the horse-family behavior cached at admission.
    pub(crate) fn is_horse(&self) -> bool {
        self.entity_fact.horse
    }

    /// Returns the living-mount disposition cached at admission.
    pub fn is_known_living_mount(&self) -> bool {
        matches!(self.kind, ActorKind::Player { .. }) || self.entity_fact.living_mount
    }

    /// The passenger index whose input steers this vehicle; 0 unless the server sets one.
    fn controlling_seat_index(&self) -> usize {
        let index = match self
            .metadata
            .get(&ActorDataKey::ControllingSeatIndex.wire_value())
        {
            Some(ActorMetadataValue::Byte(value)) => i64::from(*value),
            Some(ActorMetadataValue::Short(value)) => i64::from(*value),
            Some(ActorMetadataValue::Int(value)) => i64::from(*value),
            Some(ActorMetadataValue::Long(value)) => *value,
            _ => 0,
        };
        usize::try_from(index).unwrap_or(usize::MAX)
    }
}

impl super::ActorStore {
    /// Reads equipment capabilities cached when the actor and catalog were admitted.
    pub(crate) fn mount_equipment(&self, runtime_id: u64) -> assets::entity::mount::MountEquipment {
        self.animation.mount_equipment(runtime_id)
    }

    /// The passenger in the vehicle's controlling seat, whose input steers it.
    pub(crate) fn controlling_passenger(&self, vehicle_unique_id: i64) -> Option<i64> {
        let vehicle = self.snapshot_by_unique(vehicle_unique_id)?;
        self.passenger_order(vehicle_unique_id)
            .get(vehicle.controlling_seat_index())
            .copied()
    }

    /// The vehicle's passengers in seat order, from its current links: arrival order, except
    /// that a player boarding a vehicle fronted by a non-player takes the front seat.
    fn passenger_order(&self, vehicle_unique_id: i64) -> Vec<i64> {
        let mut arrivals: Vec<(u64, i64)> = self
            .rider_to_ridden
            .iter()
            .filter(|(_, ridden)| **ridden == vehicle_unique_id)
            .map(|(rider, _)| (self.link_arrival.get(rider).copied().unwrap_or(0), *rider))
            .collect();
        arrivals.sort_unstable();
        let mut order: Vec<i64> = Vec::with_capacity(arrivals.len());
        for (_, rider) in arrivals {
            let front_is_mob = order
                .first()
                .is_some_and(|front| !self.is_player_unique(*front));
            if self.is_player_unique(rider) && front_is_mob {
                order.insert(0, rider);
            } else {
                order.push(rider);
            }
        }
        order
    }

    fn is_player_unique(&self, unique_id: i64) -> bool {
        self.snapshot_by_unique(unique_id)
            .is_some_and(|actor| matches!(actor.kind, ActorKind::Player { .. }))
    }

    /// Applies one riding link. Link types only add or remove; a new link records its
    /// arrival, which orders the vehicle's seats.
    pub(super) fn apply_link_inner(&mut self, event: ActorLinkEvent) -> ActorApplyResult {
        let rider = event.rider_unique_id;
        let vehicle = event.ridden_unique_id;
        match event.link_type {
            ActorLinkType::Unknown(_) => ActorApplyResult::Updated,
            ActorLinkType::Remove => {
                if self.rider_to_ridden.get(&rider) == Some(&vehicle) {
                    self.rider_to_ridden.remove(&rider);
                    self.link_arrival.remove(&rider);
                }
                ActorApplyResult::Updated
            }
            ActorLinkType::Rider | ActorLinkType::Passenger => {
                let previous = self.rider_to_ridden.get(&rider).copied();
                if previous.is_none() && self.rider_to_ridden.len() >= self.max_actor_links {
                    return ActorApplyResult::CapacityRejected;
                }
                if previous != Some(vehicle) {
                    self.rider_to_ridden.insert(rider, vehicle);
                    self.next_link_arrival = self.next_link_arrival.wrapping_add(1);
                    self.link_arrival.insert(rider, self.next_link_arrival);
                }
                ActorApplyResult::Updated
            }
        }
    }

    /// Drops every link that names this actor as rider or vehicle.
    pub(super) fn remove_links_for(&mut self, unique_id: i64) {
        self.rider_to_ridden.remove(&unique_id);
        self.rider_to_ridden
            .retain(|_, ridden_unique_id| *ridden_unique_id != unique_id);
        let riders = &self.rider_to_ridden;
        self.link_arrival
            .retain(|rider, _| riders.contains_key(rider));
    }

    pub(super) fn clear_links(&mut self) {
        self.rider_to_ridden.clear();
        self.link_arrival.clear();
    }
}
