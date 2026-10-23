//! Client prediction of the boat the local player steers.
//!
//! Each tick the rider's move input paddles the boat locally and the rider sits on its seat
//! at the predicted pose. Inputs then report the boat's position and rotation, and a server
//! correction of the vehicle rewinds to the corrected tick and replays the recorded paddling.

use std::collections::VecDeque;

use sim::{BOAT_HEIGHT_OFFSET, BoatBuoyancy, BoatFloat, BoatState, CollisionWorld};

/// The boat the local player steers from its controlling seat.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SteeredVehicle {
    pub runtime_id: u64,
    pub unique_id: i64,
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

/// Squared position error the client accepts without replaying.
const POSITION_TOLERANCE_SQUARED: f32 = 1.0e-5;
/// Angular velocity error the client accepts without replaying.
const ANGULAR_VELOCITY_TOLERANCE: f32 = 1.0e-4;

#[derive(Debug, Clone, Copy)]
struct RecordedTick {
    tick: u64,
    power: [f32; 2],
    float: BoatFloat,
    state: BoatState,
}

#[derive(Debug, Clone)]
struct Predicted {
    runtime_id: u64,
    unique_id: i64,
    server_moves: u64,
    seat: [f32; 3],
    state: BoatState,
    history: VecDeque<RecordedTick>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct VehiclePrediction {
    predicted: Option<Predicted>,
    pending: Option<RecordedTick>,
    /// Xorshift state for the wave's big-wave draws.
    wave_seed: u32,
}

impl VehiclePrediction {
    /// Paddles the steered boat for the coming tick and returns the rider's seat feet.
    /// Returns `None`, predicting nothing, without a steered boat or when its terrain is
    /// unavailable.
    pub(super) fn step(
        &mut self,
        vehicle: Option<SteeredVehicle>,
        move_vector: [f32; 2],
        world: &impl CollisionWorld,
    ) -> Option<[f32; 3]> {
        self.pending = None;
        let Some(vehicle) = vehicle else {
            self.predicted = None;
            return None;
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
                state: BoatState::at_rest(vehicle.position, vehicle.yaw),
                history: VecDeque::new(),
            });
        }
        let predicted = self.predicted.as_mut()?;
        predicted.seat = vehicle.seat;
        let power = sim::boat_paddle_power(move_vector);
        let wave_draw = next_draw(&mut self.wave_seed);
        let tick = sim::tick_boat(
            world,
            &predicted.state,
            power,
            BoatBuoyancy::Live { wave_draw },
        )
        .ok()?;
        let seat = seat_feet(&tick.state, predicted.seat);
        self.pending = Some(RecordedTick {
            tick: 0,
            power,
            float: tick.float,
            state: tick.state,
        });
        Some(seat)
    }

    /// Keeps the stepped boat under the completed input tick, retaining `capacity` ticks.
    pub(super) fn commit(&mut self, tick: u64, capacity: usize) -> Option<PredictedVehicleSample> {
        let pending = self.pending.take()?;
        let predicted = self.predicted.as_mut()?;
        predicted.state = pending.state;
        while predicted.history.len() >= capacity.max(1) {
            predicted.history.pop_front();
        }
        predicted
            .history
            .push_back(RecordedTick { tick, ..pending });
        Some(PredictedVehicleSample {
            unique_id: predicted.unique_id,
            position: pending.state.position,
            velocity: pending.state.velocity,
            rotation: [0.0, pending.state.yaw],
        })
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
        let predicted = self.predicted.as_ref()?;
        predicted.history.back()?;
        let [x, y, z] = predicted.state.position;
        Some(client_world::LocalVehicleFeed {
            runtime_id: predicted.runtime_id,
            position: [x, y - BOAT_HEIGHT_OFFSET, z],
            yaw: predicted.state.yaw,
            velocity: predicted.state.velocity,
        })
    }

    /// Rewinds to a corrected tick and replays the recorded paddling after it. Returns the
    /// rider's new seat feet, or `None` when the tick is not retained or already agrees.
    pub(super) fn correct(
        &mut self,
        correction: &VehicleCorrection,
        world: &impl CollisionWorld,
    ) -> Option<[f32; 3]> {
        let predicted = self.predicted.as_mut()?;
        let finite = correction
            .position
            .iter()
            .chain(&correction.velocity)
            .chain(&correction.rotation)
            .chain(correction.angular_velocity.as_ref())
            .all(|value| value.is_finite());
        if correction.tick == 0 || !finite {
            return None;
        }
        let index = predicted
            .history
            .iter()
            .position(|recorded| recorded.tick == correction.tick)?;
        let retained = predicted.history[index].state;
        let position_error = retained
            .position
            .iter()
            .zip(correction.position)
            .map(|(predicted, corrected)| (predicted - corrected) * (predicted - corrected))
            .sum::<f32>();
        let angular_error = correction
            .angular_velocity
            .map_or(0.0, |velocity| (velocity - retained.angular_velocity).abs());
        if position_error < POSITION_TOLERANCE_SQUARED
            && angular_error < ANGULAR_VELOCITY_TOLERANCE
            && retained.yaw == correction.rotation[1]
        {
            return None;
        }
        let mut state = BoatState {
            position: correction.position,
            velocity: correction.velocity,
            yaw: correction.rotation[1],
            angular_velocity: correction
                .angular_velocity
                .unwrap_or(retained.angular_velocity),
            on_ground: correction.on_ground,
            ..retained
        };
        predicted.history[index].state = state;
        for recorded in predicted.history.iter_mut().skip(index + 1) {
            let Ok(tick) = sim::tick_boat(
                world,
                &state,
                recorded.power,
                BoatBuoyancy::Replay(recorded.float),
            ) else {
                break;
            };
            state = tick.state;
            recorded.state = state;
        }
        predicted.state = state;
        Some(seat_feet(&state, predicted.seat))
    }
}

/// Position, delta and vehicle an input reports: those of the predicted vehicle, if any.
pub(super) fn reported_motion(
    sample: &super::PhysicsMovementSample,
) -> ([f32; 3], [f32; 3], Option<protocol::PredictedVehicleInput>) {
    match sample.processed.vehicle {
        Some(vehicle) => (
            vehicle.position,
            vehicle.velocity,
            Some(protocol::PredictedVehicleInput {
                unique_id: vehicle.unique_id,
                rotation: vehicle.rotation,
            }),
        ),
        None => (sample.position, sample.velocity, None),
    }
}

/// Feet of a rider whose seat sits at `seat` in the boat's frame.
fn seat_feet(state: &BoatState, seat: [f32; 3]) -> [f32; 3] {
    let offset = client_world::seat_world_offset(seat, state.yaw);
    let [x, y, z] = state.position;
    [
        x + offset[0],
        y - BOAT_HEIGHT_OFFSET + offset[1],
        z + offset[2],
    ]
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
