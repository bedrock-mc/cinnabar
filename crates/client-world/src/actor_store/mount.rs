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

    /// The vehicle's boarded passengers in seat order: by each one's boarding key, so a
    /// player who boarded in front of a mob stays ahead of passengers who boarded later.
    fn passenger_order(&self, vehicle_unique_id: i64) -> Vec<i64> {
        let mut seats: Vec<(i64, i64)> = self
            .rider_to_ridden
            .iter()
            .filter(|(_, ridden)| **ridden == vehicle_unique_id)
            .filter_map(|(rider, _)| Some((*self.seat_keys.get(rider)?, *rider)))
            .collect();
        seats.sort_unstable();
        seats.into_iter().map(|(_, rider)| rider).collect()
    }

    /// Boards every linked rider whose rider and vehicle actors both exist, as vanilla adds a
    /// queued link's passenger once both are present. A player boarding a vehicle fronted by
    /// a non-player takes the front seat; anyone else boards at the back.
    pub(super) fn board_linked_riders(&mut self) {
        let mut waiting: Vec<(i64, i64)> = self
            .rider_to_ridden
            .iter()
            .filter(|(rider, _)| !self.seat_keys.contains_key(*rider))
            .map(|(rider, vehicle)| (*rider, *vehicle))
            .collect();
        waiting.sort_unstable();
        for (rider, vehicle) in waiting {
            if self.snapshot_by_unique(rider).is_none()
                || self.snapshot_by_unique(vehicle).is_none()
            {
                continue;
            }
            let front_is_mob = self
                .passenger_order(vehicle)
                .first()
                .is_some_and(|front| !self.is_player_unique(*front));
            self.boardings = self.boardings.saturating_add(1);
            let key = if self.is_player_unique(rider) && front_is_mob {
                -self.boardings
            } else {
                self.boardings
            };
            self.seat_keys.insert(rider, key);
        }
    }

    fn is_player_unique(&self, unique_id: i64) -> bool {
        self.snapshot_by_unique(unique_id)
            .is_some_and(|actor| matches!(actor.kind, ActorKind::Player { .. }))
    }

    /// Applies one riding link. Link types only add or remove; a new link boards its rider
    /// once both actors exist.
    pub(super) fn apply_link_inner(&mut self, event: ActorLinkEvent) -> ActorApplyResult {
        let rider = event.rider_unique_id;
        let vehicle = event.ridden_unique_id;
        match event.link_type {
            ActorLinkType::Unknown(_) => ActorApplyResult::Updated,
            ActorLinkType::Remove => {
                if self.rider_to_ridden.get(&rider) == Some(&vehicle) {
                    self.rider_to_ridden.remove(&rider);
                    self.seat_keys.remove(&rider);
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
                    self.seat_keys.remove(&rider);
                    self.board_linked_riders();
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
        self.seat_keys.retain(|rider, _| riders.contains_key(rider));
    }

    pub(super) fn clear_links(&mut self) {
        self.rider_to_ridden.clear();
        self.seat_keys.clear();
    }
}
