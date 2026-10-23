//! The boat the local player steers. The client predicts its pose, so server movement lands
//! on it only when the server teleports or forces the move; which boat that is follows the
//! current links.

use protocol::ActorMoveEvent;

use super::{ActorPose, ActorStore};

/// One tick's predicted pose of the vehicle the local player steers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalVehicleFeed {
    pub runtime_id: u64,
    /// The count of admitted server moves this prediction started from.
    pub server_moves: u64,
    /// Collision feet: the bottom centre of the vehicle's box.
    pub position: [f32; 3],
    pub yaw: f32,
    /// Displacement per tick.
    pub velocity: [f32; 3],
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct LocalVehicle {
    /// Server moves admitted for the predicted vehicle, which prediction must restart from.
    server_moves: u64,
}

impl ActorStore {
    /// The boat the local player currently steers, from the current links; the client
    /// predicts it while this holds.
    fn local_steered_boat(&self) -> Option<u64> {
        let local = self
            .actors
            .get(&self.remote_state_excluded_runtime_id?)?
            .unique_id;
        let vehicle = *self.rider_to_ridden.get(&local)?;
        if self.controlling_passenger(vehicle)? != local {
            return None;
        }
        let boat = self.snapshot_by_unique(vehicle)?;
        (boat.entity_fact.native_class == assets::facts::entity::NativeEntityClass::Boat)
            .then_some(boat.runtime_id)
    }

    /// Drives the predicted pose of the boat the local player steers. A feed for another
    /// actor, or one predicted before the latest server move, is ignored.
    pub(crate) fn sync_local_vehicle(&mut self, feed: Option<LocalVehicleFeed>) {
        let Some(feed) = feed.filter(|feed| {
            feed.position
                .iter()
                .chain(&feed.velocity)
                .chain(&[feed.yaw])
                .all(|value| value.is_finite())
        }) else {
            return;
        };
        if self.local_steered_boat() != Some(feed.runtime_id)
            || feed.server_moves != self.local_vehicle.server_moves
        {
            return;
        }
        let Some(actor) = self.actors.get_mut(&feed.runtime_id) else {
            return;
        };
        actor.received_pose = ActorPose {
            y_origin: Default::default(),
            position: feed.position,
            pitch: actor.received_pose.pitch,
            yaw: feed.yaw,
            head_yaw: feed.yaw,
        };
        actor.status.movement_interpolation = Default::default();
        // Each actor tick lands on the fed pose, as the local player's does.
        actor.interpolation_ticks_remaining = 0;
        let ticks_per_second = 1.0 / simulation_clock::TICK_DURATION.as_secs_f32();
        actor.velocity = feed.velocity.map(|axis| axis * ticks_per_second);
        actor.status.native_velocity = feed.velocity;
        self.effect_members.collidable.update(actor);
    }

    /// Whether a server move applies. Moves of the boat the local player steers apply only
    /// when they teleport or are forced, and then count as a restart for prediction.
    pub(super) fn admit_local_vehicle_move(&mut self, movement: &ActorMoveEvent) -> bool {
        if self.local_steered_boat() != Some(movement.runtime_id) {
            return true;
        }
        if !movement.teleported && !movement.force_local_move {
            return false;
        }
        self.local_vehicle.server_moves = self.local_vehicle.server_moves.wrapping_add(1);
        true
    }

    /// Counts server moves admitted for the predicted vehicle.
    pub(crate) const fn local_vehicle_server_moves(&self) -> u64 {
        self.local_vehicle.server_moves
    }
}
