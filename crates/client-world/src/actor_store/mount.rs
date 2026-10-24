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
        self.passengers
            .get(&vehicle_unique_id)?
            .get(vehicle.controlling_seat_index())
            .copied()
    }

    /// Boards linked riders whose rider and vehicle actors both exist, as vanilla adds a
    /// queued link's passenger once both are present. Seats follow link arrival order, so a
    /// rider whose actor appears late, such as the local player on a ride restored at join,
    /// keeps its place ahead of riders linked after it. A player boarding a vehicle fronted by
    /// a non-player takes the front seat.
    pub(super) fn board_linked_riders(&mut self) {
        let mut waiting: Vec<(u64, i64, i64)> = self
            .link_arrivals
            .iter()
            .filter_map(|(&rider, &arrival)| {
                let vehicle = *self.rider_to_ridden.get(&rider)?;
                let seated = self
                    .passengers
                    .get(&vehicle)
                    .is_some_and(|seats| seats.contains(&rider));
                (!seated
                    && self.snapshot_by_unique(rider).is_some()
                    && self.snapshot_by_unique(vehicle).is_some())
                .then_some((arrival, rider, vehicle))
            })
            .collect();
        waiting.sort_unstable();
        for (arrival, rider, vehicle) in waiting {
            let rider_is_player = self.is_player_unique(rider);
            let seats = self.passengers.get(&vehicle).map_or(&[][..], Vec::as_slice);
            let front_is_mob = seats
                .first()
                .is_some_and(|front| !self.is_player_unique(*front));
            let index = if rider_is_player && front_is_mob {
                0
            } else {
                seats
                    .iter()
                    .position(|seated| self.link_arrivals.get(seated) > Some(&arrival))
                    .unwrap_or(seats.len())
            };
            self.passengers
                .entry(vehicle)
                .or_default()
                .insert(index, rider);
        }
    }

    /// Takes a rider off its vehicle's seats and out of the boarding order.
    fn unseat(&mut self, rider: i64, vehicle: i64) {
        self.link_arrivals.remove(&rider);
        if let Some(seats) = self.passengers.get_mut(&vehicle) {
            seats.retain(|&seated| seated != rider);
            if seats.is_empty() {
                self.passengers.remove(&vehicle);
            }
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
                    self.unseat(rider, vehicle);
                }
                ActorApplyResult::Updated
            }
            ActorLinkType::Rider | ActorLinkType::Passenger => {
                let previous = self.rider_to_ridden.get(&rider).copied();
                if previous.is_none() && self.rider_to_ridden.len() >= self.max_actor_links {
                    return ActorApplyResult::CapacityRejected;
                }
                if previous != Some(vehicle) {
                    if let Some(previous) = previous {
                        self.unseat(rider, previous);
                    }
                    self.rider_to_ridden.insert(rider, vehicle);
                    self.links_received = self.links_received.wrapping_add(1);
                    self.link_arrivals.insert(rider, self.links_received);
                    self.board_linked_riders();
                }
                ActorApplyResult::Updated
            }
        }
    }

    /// Drops every link that names this actor as rider or vehicle.
    pub(super) fn remove_links_for(&mut self, unique_id: i64) {
        if let Some(vehicle) = self.rider_to_ridden.remove(&unique_id) {
            self.unseat(unique_id, vehicle);
        }
        self.passengers.remove(&unique_id);
        self.rider_to_ridden
            .retain(|_, ridden_unique_id| *ridden_unique_id != unique_id);
        let riders = &self.rider_to_ridden;
        self.link_arrivals
            .retain(|rider, _| riders.contains_key(rider));
    }

    pub(super) fn clear_links(&mut self) {
        self.rider_to_ridden.clear();
        self.passengers.clear();
        self.link_arrivals.clear();
    }
}
