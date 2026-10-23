//! The boat the local player steers. The client predicts its pose, so server movement lands
//! on it only when the server teleports or forces the move; which boat that is follows the
//! current links. Each server change the prediction must restart from, an admitted move, new
//! motion or a respawn of the boat, advances the vehicle revision.

use protocol::ActorMoveEvent;

use super::{ActorPose, ActorStore};

/// One tick's predicted pose of the vehicle the local player steers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalVehicleFeed {
    pub runtime_id: u64,
    /// The vehicle revision this prediction started from.
    pub revision: u64,
    /// Collision feet: the bottom centre of the vehicle's box.
    pub position: [f32; 3],
    pub yaw: f32,
    /// Displacement per tick.
    pub velocity: [f32; 3],
    /// Where the boat renders this frame, on the rider's physics clock rather than the
    /// remote actors' clock, so it stays under its rider.
    pub render_position: [f32; 3],
    pub render_yaw: f32,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct LocalVehicle {
    /// Server changes to the predicted vehicle, which prediction must restart from.
    revision: u64,
    /// The vehicle the latest feed predicted. Its changes count even while the local player
    /// does not steer it, so a prediction resumed after regaining control restarts from them.
    predicted: Option<u64>,
    /// The accepted feed's render pose, which holds the boat's displayed pose through actor
    /// ticks.
    render: Option<(u64, ActorPose)>,
}

impl ActorStore {
    /// The boat the local player currently steers, from the current links; the client
    /// predicts it while this holds.
    pub(crate) fn local_steered_boat(&self) -> Option<u64> {
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
    /// actor, or one predicted before the latest server move, is ignored. Actor ticks land on
    /// the fed tick pose, which prediction restarts from, while the boat displays the fed
    /// render pose.
    pub(crate) fn sync_local_vehicle(&mut self, feed: Option<LocalVehicleFeed>) {
        self.local_vehicle.render = None;
        let Some(feed) = feed.filter(|feed| {
            feed.position
                .iter()
                .chain(&feed.velocity)
                .chain(&feed.render_position)
                .chain(&[feed.yaw, feed.render_yaw])
                .all(|value| value.is_finite())
        }) else {
            self.local_vehicle.predicted = None;
            return;
        };
        self.local_vehicle.predicted = Some(feed.runtime_id);
        if self.local_steered_boat() != Some(feed.runtime_id)
            || feed.revision != self.local_vehicle.revision
        {
            return;
        }
        let Some(actor) = self.actors.get_mut(&feed.runtime_id) else {
            return;
        };
        let pitch = actor.received_pose.pitch;
        let pose = |position, yaw| ActorPose {
            y_origin: Default::default(),
            position,
            pitch,
            yaw,
            head_yaw: yaw,
        };
        actor.received_pose = pose(feed.position, feed.yaw);
        actor.status.movement_interpolation = Default::default();
        actor.interpolation_ticks_remaining = 0;
        let ticks_per_second = 1.0 / simulation_clock::TICK_DURATION.as_secs_f32();
        actor.velocity = feed.velocity.map(|axis| axis * ticks_per_second);
        actor.status.native_velocity = feed.velocity;
        self.local_vehicle.render =
            Some((feed.runtime_id, pose(feed.render_position, feed.render_yaw)));
        self.show_local_vehicle_render();
    }

    /// The yaw the predicted boat `runtime_id` renders at this frame, from its latest feed.
    pub(super) fn local_vehicle_render_yaw(&self, runtime_id: u64) -> Option<f32> {
        self.local_vehicle
            .render
            .filter(|(predicted, _)| *predicted == runtime_id)
            .map(|(_, pose)| pose.yaw)
    }

    /// Holds the predicted boat at its fed render pose, whatever the actor clock's point
    /// between ticks.
    pub(super) fn show_local_vehicle_render(&mut self) {
        let Some((runtime_id, pose)) = self.local_vehicle.render else {
            return;
        };
        if let Some(actor) = self.actors.get_mut(&runtime_id) {
            actor.previous_pose = pose;
            actor.set_current_pose(pose);
            self.effect_members.collidable.update(actor);
        }
    }

    /// Whether a server move applies. Moves of the boat the local player steers and the
    /// client predicts apply only when they teleport or are forced; until prediction feeds
    /// it, the boat keeps its streamed movement.
    pub(super) fn admit_local_vehicle_move(&mut self, movement: &ActorMoveEvent) -> bool {
        if self.local_vehicle.predicted == Some(movement.runtime_id)
            && self.local_steered_boat() == Some(movement.runtime_id)
            && !movement.teleported
            && !movement.force_local_move
        {
            return false;
        }
        self.note_local_vehicle_change(movement.runtime_id);
        true
    }

    /// Advances the vehicle revision when the server changed the steered or last predicted
    /// boat, so prediction restarts from the change.
    pub(super) fn note_local_vehicle_change(&mut self, runtime_id: u64) {
        if self.local_vehicle.predicted == Some(runtime_id)
            || self.local_steered_boat() == Some(runtime_id)
        {
            self.local_vehicle.revision = self.local_vehicle.revision.wrapping_add(1);
            self.local_vehicle.render = None;
        }
    }

    /// Counts server changes to the predicted vehicle.
    pub(crate) const fn local_vehicle_revision(&self) -> u64 {
        self.local_vehicle.revision
    }
}
