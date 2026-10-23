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
    /// Network position and yaw the server last gave the boat; prediction starts there.
    pub position: [f32; 3],
    pub yaw: f32,
    /// The rider's seat relative to the boat's feet, in the boat's frame.
    pub seat: [f32; 3],
    /// Server teleports of the boat; a new one restarts prediction.
    pub server_moves: u64,
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
    pub tick: u64,
    /// Network position.
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    /// Pitch and yaw in degrees.
    pub rotation: [f32; 2],
    pub angular_velocity: Option<f32>,
    pub on_ground: bool,
}

#[derive(Debug, Clone)]
struct Predicted {
    runtime_id: u64,
    unique_id: i64,
    server_moves: u64,
    seat: [f32; 3],
    state: BoatState,
    /// At least one tick completed from this start.
    committed: bool,
    /// The newest correction applied; older ones are stale.
    corrected_tick: u64,
}

#[derive(Debug, Clone, Default)]
pub(super) struct VehiclePrediction {
    predicted: Option<Predicted>,
    pending: Option<BoatState>,
    /// Xorshift state for the wave's big-wave draws.
    wave_seed: u32,
}

impl VehiclePrediction {
    /// Paddles the steered boat for the coming tick and returns the rider's seat feet, or
    /// `None` without a steered boat. A boat whose terrain is unavailable fails the tick, so
    /// its input never silently reports the rider instead.
    pub(super) fn step(
        &mut self,
        vehicle: Option<SteeredVehicle>,
        move_vector: [f32; 2],
        world: &impl CollisionWorld,
    ) -> Result<Option<[f32; 3]>, WorldQueryError> {
        self.pending = None;
        let Some(vehicle) = vehicle else {
            self.predicted = None;
            return Ok(None);
        };
        let restart = self.predicted.as_ref().is_none_or(|predicted| {
            predicted.runtime_id != vehicle.runtime_id
                || predicted.unique_id != vehicle.unique_id
                || predicted.server_moves != vehicle.server_moves
        });
        if restart {
            self.predicted = Some(Predicted {
                runtime_id: vehicle.runtime_id,
                unique_id: vehicle.unique_id,
                server_moves: vehicle.server_moves,
                seat: vehicle.seat,
                state: BoatState::at_rest(vehicle.body, vehicle.position, vehicle.yaw),
                committed: false,
                corrected_tick: 0,
            });
        }
        let Some(predicted) = self.predicted.as_mut() else {
            return Ok(None);
        };
        predicted.seat = vehicle.seat;
        predicted.state.body = vehicle.body;
        let wave_draw = next_draw(&mut self.wave_seed);
        let tick = sim::tick_boat(
            world,
            &predicted.state,
            sim::boat_paddle_power(move_vector),
            BoatBuoyancy::Live { wave_draw },
        )?;
        self.pending = Some(tick.state);
        Ok(Some(seat_feet(&tick.state, predicted.seat)))
    }

    /// Keeps the stepped boat as the completed tick's state.
    pub(super) fn commit(&mut self) -> Option<PredictedVehicleSample> {
        let state = self.pending.take()?;
        let predicted = self.predicted.as_mut()?;
        predicted.state = state;
        predicted.committed = true;
        Some(predicted_sample(predicted.unique_id, &state))
    }

    /// Discards a stepped boat whose player tick failed.
    pub(super) fn discard_pending(&mut self) {
        self.pending = None;
    }

    /// Forgets the predicted boat.
    pub(super) fn reset(&mut self) {
        self.predicted = None;
        self.pending = None;
    }

    /// The predicted boat's latest pose for the actor store.
    pub(super) fn feed(&self) -> Option<client_world::LocalVehicleFeed> {
        let predicted = self
            .predicted
            .as_ref()
            .filter(|predicted| predicted.committed)?;
        let [x, _, z] = predicted.state.position;
        Some(client_world::LocalVehicleFeed {
            runtime_id: predicted.runtime_id,
            server_moves: predicted.server_moves,
            position: [x, predicted.state.feet_y(), z],
            yaw: predicted.state.yaw,
            velocity: predicted.state.velocity,
        })
    }

    /// Restarts prediction from a server correction's vehicle state and returns the rider's
    /// seat feet on it. Corrections no newer than one already applied, or with non-finite
    /// state, are ignored.
    pub(super) fn correct(&mut self, correction: &VehicleCorrection) -> Option<[f32; 3]> {
        let predicted = self.predicted.as_mut()?;
        let finite = correction
            .position
            .iter()
            .chain(&correction.velocity)
            .chain(&correction.rotation)
            .chain(correction.angular_velocity.as_ref())
            .all(|value| value.is_finite());
        if !finite || correction.tick <= predicted.corrected_tick {
            return None;
        }
        predicted.corrected_tick = correction.tick;
        predicted.state = BoatState {
            position: correction.position,
            velocity: correction.velocity,
            yaw: correction.rotation[1],
            angular_velocity: correction
                .angular_velocity
                .unwrap_or(predicted.state.angular_velocity),
            on_ground: correction.on_ground,
            ..predicted.state
        };
        self.pending = None;
        Some(seat_feet(&predicted.state, predicted.seat))
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
