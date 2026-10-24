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
        self.seated_passengers(vehicle_unique_id)
            .get(vehicle.controlling_seat_index())
            .copied()
    }

    /// The vehicle's passengers in seat order, the one ordering that seats, driver selection
    /// and fallback seats all read: its passenger list, filtered to riders whose actors exist.
    /// No passengers without the vehicle.
    pub(super) fn seated_passengers(&self, vehicle_unique_id: i64) -> Vec<i64> {
        if self.snapshot_by_unique(vehicle_unique_id).is_none() {
            return Vec::new();
        }
        self.vehicle_riders
            .get(&vehicle_unique_id)
            .into_iter()
            .flatten()
            .copied()
            .filter(|rider| self.snapshot_by_unique(*rider).is_some())
            .collect()
    }

    /// Whether the actor is a player: `None` while it is unknown, before its actor exists.
    /// The local player is known from the session start, as vanilla's always exists.
    fn known_player(&self, unique_id: i64) -> Option<bool> {
        if self.local_unique_id == Some(unique_id) {
            return Some(true);
        }
        self.snapshot_by_unique(unique_id)
            .map(|actor| matches!(actor.kind, ActorKind::Player { .. }))
    }

    /// Adds a linked rider to its vehicle's passenger list as vanilla's `Actor::addPassenger`
    /// does: a player added behind a front rider that is not a known player takes the front
    /// seat; any other rider goes to the back. A rider still unknown goes to the back until
    /// its actor appears.
    fn add_passenger(&mut self, rider: i64, vehicle: i64) {
        let front = self
            .vehicle_riders
            .get(&vehicle)
            .and_then(|riders| riders.first())
            .map(|front| self.known_player(*front));
        let to_front = self.known_player(rider) == Some(true)
            && front.is_some_and(|front| front != Some(true));
        let riders = self.vehicle_riders.entry(vehicle).or_default();
        if to_front {
            riders.insert(0, rider);
        } else {
            riders.push(rider);
        }
    }

    /// Adds a rider whose actor just appeared to its vehicle again, as vanilla adds a queued
    /// link's passenger once its actor exists. The local player was placed when linked.
    pub(super) fn add_appeared_passenger(&mut self, unique_id: i64) {
        let Some(&vehicle) = self.rider_to_ridden.get(&unique_id) else {
            return;
        };
        if self.local_unique_id == Some(unique_id) {
            return;
        }
        if let Some(riders) = self.vehicle_riders.get_mut(&vehicle) {
            riders.retain(|&linked| linked != unique_id);
        }
        self.add_passenger(unique_id, vehicle);
    }

    /// Unlinks a rider from its vehicle, keeping both indexes in step.
    fn unlink(&mut self, rider: i64) {
        let Some(vehicle) = self.rider_to_ridden.remove(&rider) else {
            return;
        };
        if let Some(riders) = self.vehicle_riders.get_mut(&vehicle) {
            riders.retain(|&linked| linked != rider);
            if riders.is_empty() {
                self.vehicle_riders.remove(&vehicle);
            }
        }
    }

    /// Applies one riding link. Link types only add or remove; a rider's seat follows from
    /// its place in the vehicle's link order once its actor exists.
    pub(super) fn apply_link_inner(&mut self, event: ActorLinkEvent) -> ActorApplyResult {
        let rider = event.rider_unique_id;
        let vehicle = event.ridden_unique_id;
        match event.link_type {
            ActorLinkType::Unknown(_) => ActorApplyResult::Updated,
            ActorLinkType::Remove => {
                if self.rider_to_ridden.get(&rider) == Some(&vehicle) {
                    self.unlink(rider);
                }
                ActorApplyResult::Updated
            }
            ActorLinkType::Rider | ActorLinkType::Passenger => {
                let previous = self.rider_to_ridden.get(&rider).copied();
                if previous.is_none() && self.rider_to_ridden.len() >= self.max_actor_links {
                    return ActorApplyResult::CapacityRejected;
                }
                if previous != Some(vehicle) {
                    self.unlink(rider);
                    self.rider_to_ridden.insert(rider, vehicle);
                    self.add_passenger(rider, vehicle);
                }
                ActorApplyResult::Updated
            }
        }
    }

    /// Drops every link that names this actor as rider or vehicle.
    pub(super) fn remove_links_for(&mut self, unique_id: i64) {
        self.unlink(unique_id);
        for rider in self.vehicle_riders.remove(&unique_id).unwrap_or_default() {
            self.rider_to_ridden.remove(&rider);
        }
    }

    pub(super) fn clear_links(&mut self) {
        self.rider_to_ridden.clear();
        self.vehicle_riders.clear();
    }
}
