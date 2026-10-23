//! Client prediction of the boat the local player steers.
//!
//! Each tick the rider's move input paddles the boat locally and the rider sits on its seat
//! at the predicted pose. Inputs then report the boat's position and rotation, and a server
//! correction of the vehicle rewinds to the corrected tick and replays the recorded paddling.

use std::collections::VecDeque;

use sim::{BoatBody, BoatBuoyancy, BoatFloat, BoatState, CollisionWorld, WorldQueryError};

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

/// The outcome of an applied vehicle correction.
#[derive(Debug, Clone, PartialEq)]
pub struct VehicleReplay {
    /// The rider's seat feet on the corrected boat.
    pub seat: [f32; 3],
    /// The corrected tick and each replayed tick, as their inputs must now report them.
    pub samples: Vec<(u64, PredictedVehicleSample)>,
}

/// Squared position and velocity error the client accepts without replaying.
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
                history: VecDeque::new(),
            });
        }
        let Some(predicted) = self.predicted.as_mut() else {
            return Ok(None);
        };
        predicted.seat = vehicle.seat;
        predicted.state.body = vehicle.body;
        let power = sim::boat_paddle_power(move_vector);
        let wave_draw = next_draw(&mut self.wave_seed);
        let tick = sim::tick_boat(
            world,
            &predicted.state,
            power,
            BoatBuoyancy::Live { wave_draw },
        )?;
        let seat = seat_feet(&tick.state, predicted.seat);
        self.pending = Some(RecordedTick {
            tick: 0,
            power,
            float: tick.float,
            state: tick.state,
        });
        Ok(Some(seat))
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
        Some(predicted_sample(predicted.unique_id, &pending.state))
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
        let [x, _, z] = predicted.state.position;
        Some(client_world::LocalVehicleFeed {
            runtime_id: predicted.runtime_id,
            server_moves: predicted.server_moves,
            position: [x, predicted.state.feet_y(), z],
            yaw: predicted.state.yaw,
            velocity: predicted.state.velocity,
        })
    }

    /// Rewinds to a corrected tick and replays the recorded paddling after it. The replay
    /// commits only when every tick replays; otherwise the prediction is left unchanged.
    /// Returns `None` when the tick is not retained, already agrees, or cannot be replayed.
    pub(super) fn correct(
        &mut self,
        correction: &VehicleCorrection,
        world: &impl CollisionWorld,
    ) -> Option<VehicleReplay> {
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
        if agrees(&retained, correction) {
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
        let mut replayed = vec![state];
        for recorded in predicted.history.iter().skip(index + 1) {
            // Replay keeps each tick's recorded wave phase, as the live tick advanced it.
            state.wave_timer = recorded.state.wave_timer;
            state = sim::tick_boat(
                world,
                &state,
                recorded.power,
                BoatBuoyancy::Replay(recorded.float),
            )
            .ok()?
            .state;
            replayed.push(state);
        }
        let mut samples = Vec::with_capacity(replayed.len());
        for (recorded, state) in predicted.history.iter_mut().skip(index).zip(replayed) {
            recorded.state = state;
            samples.push((recorded.tick, predicted_sample(predicted.unique_id, &state)));
        }
        predicted.state = state;
        Some(VehicleReplay {
            seat: seat_feet(&state, predicted.seat),
            samples,
        })
    }
}

/// Whether a retained tick already matches a correction's position, motion, turn and ground.
fn agrees(retained: &BoatState, correction: &VehicleCorrection) -> bool {
    let squared_error =
        |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).map(|(a, b)| (a - b) * (a - b)).sum::<f32>();
    let angular_error = correction
        .angular_velocity
        .map_or(0.0, |velocity| (velocity - retained.angular_velocity).abs());
    squared_error(retained.position, correction.position) < POSITION_TOLERANCE_SQUARED
        && squared_error(retained.velocity, correction.velocity) < POSITION_TOLERANCE_SQUARED
        && angular_error < ANGULAR_VELOCITY_TOLERANCE
        && retained.yaw == correction.rotation[1]
        && retained.on_ground == correction.on_ground
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

impl super::MovementTicker {
    /// Rewrites the vehicle reported by inputs not yet admitted to the network after a
    /// vehicle correction replayed their ticks.
    pub fn reconcile_vehicle_inputs(&mut self, samples: &[(u64, PredictedVehicleSample)]) {
        let snapshots = self
            .outbox
            .iter_mut()
            .map(|queued| &mut queued.snapshot)
            .chain(
                self.pending_sends
                    .iter_mut()
                    .map(|pending| &mut pending.sample.snapshot),
            );
        for snapshot in snapshots {
            if snapshot.vehicle.is_some()
                && let Some((_, sample)) = samples.iter().find(|(tick, _)| *tick == snapshot.tick)
            {
                snapshot.vehicle = Some(sample.input());
            }
        }
    }
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
