//! Client prediction of the boat the local player steers.
//!
//! Each tick the rider's move input paddles the boat locally and the rider sits on its seat
//! at the predicted pose. Inputs then report the boat's position and rotation. A server
//! correction of the vehicle restarts prediction from the corrected state, as a teleport does;
//! vanilla instead rewinds and replays its recorded inputs, which is not implemented.

use sim::{BoatBody, BoatBuoyancy, BoatState, CollisionWorld, WorldQueryError};

/// The boat the local player steers from its controlling seat.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SteeredVehicle {
    pub runtime_id: u64,
    pub unique_id: i64,
    /// Collision box and position origin from the boat's actor facts.
    pub body: BoatBody,
    /// Network position, yaw and per-tick motion the boat retains; prediction starts there.
    pub position: [f32; 3],
    pub yaw: f32,
    pub velocity: [f32; 3],
    /// The rider's seat relative to the boat's feet, in the boat's frame.
    pub seat: [f32; 3],
    /// Server changes to the boat: a new pose revision restarts prediction, a new motion
    /// revision replaces its motion.
    pub revision: client_world::VehicleRevision,
}

/// The predicted vehicle reported in one tick's input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PredictedVehicleSample {
    pub unique_id: i64,
    /// Network position.
    pub position: [f32; 3],
    /// Displacement applied next tick.
    pub velocity: [f32; 3],
    /// Pitch and yaw in degrees.
    pub rotation: [f32; 2],
}

impl PredictedVehicleSample {
    /// The vehicle fields of the input that reports this tick.
    #[must_use]
    pub const fn input(&self) -> protocol::PredictedVehicleInput {
        protocol::PredictedVehicleInput {
            unique_id: self.unique_id,
            position: self.position,
            delta: self.velocity,
            rotation: self.rotation,
        }
    }
}

/// A server correction of the predicted vehicle at one input tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VehicleCorrection {
    /// The vehicle the player steered when the correction arrived.
    pub runtime_id: u64,
    /// The vehicle revision when the correction arrived; the corrected session continues
    /// from it, and only server changes after the correction apply on top.
    pub revision: client_world::VehicleRevision,
    pub tick: u64,
    /// Network position.
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    /// Pitch and yaw in degrees.
    pub rotation: [f32; 2],
    pub angular_velocity: Option<f32>,
    pub on_ground: bool,
}

/// Identifies one prediction session: the steered boat and the pose revision it started from.
/// A change of owner starts a new session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SessionOwner {
    runtime_id: u64,
    unique_id: i64,
    pose_revision: u64,
}

#[derive(Debug, Clone)]
struct Session {
    owner: SessionOwner,
    seat: [f32; 3],
    state: BoatState,
    /// The motion revision the session's velocity follows.
    motion_revision: u64,
    /// The yaw one tick before `state`, which rendering eases from.
    previous_yaw: f32,
    /// The network position one tick before `state`, which rendering eases from.
    previous_position: [f32; 3],
    /// Corrections at or before this tick belong to an earlier session or are already
    /// applied; `None` until the session's first tick completes.
    stale_through: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct VehiclePrediction {
    session: Option<Session>,
    pending: Option<BoatState>,
    /// Xorshift state for the wave's big-wave draws.
    wave_seed: u32,
}

impl VehiclePrediction {
    /// Paddles the steered boat for the coming tick and returns the rider's seat feet, or
    /// `None` without a steered boat. A new boat or server change starts a session from the
    /// boat's retained pose and motion. A boat whose terrain is unavailable fails the tick, so
    /// its input never silently reports the rider instead.
    pub(super) fn step(
        &mut self,
        vehicle: Option<SteeredVehicle>,
        move_vector: [f32; 2],
        world: &impl CollisionWorld,
    ) -> Result<Option<[f32; 3]>, WorldQueryError> {
        self.pending = None;
        let Some(vehicle) = vehicle else {
            self.session = None;
            return Ok(None);
        };
        let owner = SessionOwner {
            runtime_id: vehicle.runtime_id,
            unique_id: vehicle.unique_id,
            pose_revision: vehicle.revision.pose,
        };
        let session = match &mut self.session {
            Some(session) if session.owner == owner => {
                // New server motion replaces the predicted motion and keeps the pose.
                if session.motion_revision != vehicle.revision.motion {
                    session.motion_revision = vehicle.revision.motion;
                    session.state.velocity = vehicle.velocity;
                }
                session
            }
            slot => slot.insert(Session {
                owner,
                seat: vehicle.seat,
                state: BoatState {
                    velocity: vehicle.velocity,
                    ..BoatState::at_rest(vehicle.body, vehicle.position, vehicle.yaw)
                },
                motion_revision: vehicle.revision.motion,
                previous_yaw: vehicle.yaw,
                previous_position: vehicle.position,
                stale_through: None,
            }),
        };
        session.seat = vehicle.seat;
        session.state.body = vehicle.body;
        let wave_draw = next_draw(&mut self.wave_seed);
        let tick = sim::tick_boat(
            world,
            &session.state,
            sim::boat_paddle_power(move_vector),
            BoatBuoyancy::Live { wave_draw },
        )?;
        self.pending = Some(tick.state);
        Ok(Some(seat_feet(&tick.state, session.seat)))
    }

    /// Keeps the stepped boat as the state of the completed input tick.
    pub(super) fn commit(&mut self, tick: u64) -> Option<PredictedVehicleSample> {
        let state = self.pending.take()?;
        let session = self.session.as_mut()?;
        session.previous_yaw = session.state.yaw;
        session.previous_position = session.state.position;
        session.state = state;
        session.stale_through.get_or_insert(tick.saturating_sub(1));
        Some(predicted_sample(session.owner.unique_id, &state))
    }

    /// Renders the boat at its committed state until the next tick.
    pub(super) fn finish_interpolation(&mut self) {
        if let Some(session) = &mut self.session {
            session.previous_yaw = session.state.yaw;
            session.previous_position = session.state.position;
        }
    }

    /// Whether this tick stepped a predicted boat.
    pub(super) const fn stepped(&self) -> bool {
        self.pending.is_some()
    }

    /// Discards a stepped boat whose player tick failed.
    pub(super) fn discard_pending(&mut self) {
        self.pending = None;
    }

    /// Ends the session.
    pub(super) fn reset(&mut self) {
        self.session = None;
        self.pending = None;
    }

    /// The predicted boat's yaw `alpha` of the way through the current tick.
    pub(super) fn render_yaw(&self, alpha: f32) -> Option<f32> {
        let session = self
            .session
            .as_ref()
            .filter(|session| session.stale_through.is_some())?;
        let turn = (session.state.yaw - session.previous_yaw + 180.0).rem_euclid(360.0) - 180.0;
        Some(session.previous_yaw + turn * alpha.clamp(0.0, 1.0))
    }

    /// The predicted boat's latest pose for the actor store, and where it renders `alpha` of
    /// the way through the current tick, on the rider's clock.
    pub(super) fn feed(&self, alpha: f32) -> Option<client_world::LocalVehicleFeed> {
        let session = self
            .session
            .as_ref()
            .filter(|session| session.stale_through.is_some())?;
        let feet = |position: [f32; 3]| {
            [
                position[0],
                position[1] - session.state.body.height_offset,
                position[2],
            ]
        };
        let alpha = alpha.clamp(0.0, 1.0);
        let render = std::array::from_fn(|axis| {
            let from = session.previous_position[axis];
            from + (session.state.position[axis] - from) * alpha
        });
        Some(client_world::LocalVehicleFeed {
            runtime_id: session.owner.runtime_id,
            revision: client_world::VehicleRevision {
                pose: session.owner.pose_revision,
                motion: session.motion_revision,
            },
            position: feet(session.state.position),
            yaw: session.state.yaw,
            velocity: session.state.velocity,
            render_position: feet(render),
            render_yaw: self.render_yaw(alpha)?,
        })
    }

    /// Continues the session from a server correction's vehicle state and returns the rider's
    /// seat feet on it. A correction of another vehicle, from before this session or its
    /// revision, no newer than one already applied, or with non-finite state is ignored.
    pub(super) fn correct(&mut self, correction: &VehicleCorrection) -> Option<[f32; 3]> {
        let session = self.session.as_mut()?;
        let finite = correction
            .position
            .iter()
            .chain(&correction.velocity)
            .chain(&correction.rotation)
            .chain(correction.angular_velocity.as_ref())
            .all(|value| value.is_finite());
        if !finite
            || correction.runtime_id != session.owner.runtime_id
            || correction.revision.pose < session.owner.pose_revision
            || correction.tick <= session.stale_through?
        {
            return None;
        }
        // The correction is newer than any server change it arrived after; changes after it
        // still apply.
        session.owner.pose_revision = correction.revision.pose;
        session.motion_revision = correction.revision.motion;
        session.stale_through = Some(correction.tick);
        session.previous_yaw = correction.rotation[1];
        session.previous_position = correction.position;
        session.state = BoatState {
            position: correction.position,
            velocity: correction.velocity,
            yaw: correction.rotation[1],
            angular_velocity: correction
                .angular_velocity
                .unwrap_or(session.state.angular_velocity),
            on_ground: correction.on_ground,
            ..session.state
        };
        self.pending = None;
        Some(seat_feet(&session.state, session.seat))
    }
}

#[cfg(test)]
impl VehiclePrediction {
    /// Predicts `vehicle` from a session whose ticks through `completed` already ran.
    pub(crate) fn completed_session(vehicle: SteeredVehicle, completed: u64) -> Self {
        Self {
            session: Some(Session {
                owner: SessionOwner {
                    runtime_id: vehicle.runtime_id,
                    unique_id: vehicle.unique_id,
                    pose_revision: vehicle.revision.pose,
                },
                seat: vehicle.seat,
                state: BoatState::at_rest(vehicle.body, vehicle.position, vehicle.yaw),
                motion_revision: vehicle.revision.motion,
                previous_yaw: vehicle.yaw,
                previous_position: vehicle.position,
                stale_through: Some(completed),
            }),
            ..Self::default()
        }
    }
}

fn predicted_sample(unique_id: i64, state: &BoatState) -> PredictedVehicleSample {
    PredictedVehicleSample {
        unique_id,
        position: state.position,
        velocity: state.velocity,
        rotation: [0.0, state.yaw],
    }
}

/// Feet of a rider whose seat sits at `seat` in the boat's frame.
fn seat_feet(state: &BoatState, seat: [f32; 3]) -> [f32; 3] {
    let offset = client_world::seat_world_offset(seat, state.yaw);
    let [x, _, z] = state.position;
    [x + offset[0], state.feet_y() + offset[1], z + offset[2]]
}

/// A uniform draw in `[0, 1)` for the wave's big-wave chance.
fn next_draw(seed: &mut u32) -> f32 {
    let mut value = if *seed == 0 { 0x9e37_79b9 } else { *seed };
    value ^= value << 13;
    value ^= value >> 17;
    value ^= value << 5;
    *seed = value;
    (value >> 8) as f32 / (1_u32 << 24) as f32
}

#[cfg(test)]
#[path = "vehicle_tests.rs"]
mod tests;
