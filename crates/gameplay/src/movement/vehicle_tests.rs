use super::*;
use sim::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery,
    SurfaceResponse, WorldQueryError,
};

/// Still water up to y = 63 over a floor at y = 58.
struct Lake;

/// A lake whose terrain beyond x = 100 is unloaded.
struct EdgeLake;

impl CollisionWorld for EdgeLake {
    fn ensure_movement_origin(&self, feet: sim::Vec3) -> Result<(), WorldQueryError> {
        if feet.x >= 100.0 {
            return Err(WorldQueryError::UnloadedChunk(world::ChunkKey {
                dimension: 0,
                x: 6,
                z: 0,
            }));
        }
        Ok(())
    }
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Lake.collision_boxes(query)
    }
    fn primary_is_air(
        &self,
        block: [i32; 3],
    ) -> Result<Option<CollisionQuery<bool>>, WorldQueryError> {
        Lake.primary_is_air(block)
    }
    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        Lake.block_physics(block)
    }
}

impl CollisionWorld for Lake {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        let mut boxes = Vec::new();
        for x in query.min.x.floor() as i32..=query.max.x.floor() as i32 {
            for y in query.min.y.floor() as i32..=query.max.y.floor().min(58.0) as i32 {
                for z in query.min.z.floor() as i32..=query.max.z.floor() as i32 {
                    let min = sim::Vec3::new(f64::from(x), f64::from(y), f64::from(z));
                    boxes.push(Aabb::new(min, min + sim::Vec3::ONE));
                }
            }
        }
        Ok(CollisionQuery::synthetic(boxes))
    }

    fn primary_is_air(
        &self,
        block: [i32; 3],
    ) -> Result<Option<CollisionQuery<bool>>, WorldQueryError> {
        Ok(Some(CollisionQuery::synthetic(block[1] > 62)))
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        let water = (59..=62).contains(&block[1]);
        Ok(BlockPhysicsSample {
            layers: Box::new([BlockPhysicsFacts {
                friction: 0.6,
                horizontal_speed_factor: 1.0,
                vertical_speed_factor: 1.0,
                fluid_height_blocks: if water { 8.0 / 9.0 } else { 0.0 },
                flags: if water {
                    BlockPhysicsFlags::WATER
                } else {
                    BlockPhysicsFlags::default()
                },
                surface_response: SurfaceResponse::None,
            }]),
            identity: CollisionQuery::synthetic(()).identity,
        })
    }
}

const FORWARD: [f32; 2] = [0.0, 1.0];
/// An oak boat's collision box and origin, as its entity facts give them.
const OAK: sim::BoatBody = sim::BoatBody {
    width: 1.4,
    height: 0.455,
    height_offset: 0.375,
};

fn boat() -> SteeredVehicle {
    SteeredVehicle {
        runtime_id: 7,
        unique_id: -7,
        body: OAK,
        position: [0.5, 62.6, 0.5],
        yaw: 0.0,
        seat: [0.0, -0.2, 0.0],
        server_moves: 0,
    }
}

/// Steps and commits `ticks` ticks from `first`, returning the last sample.
fn drive(
    prediction: &mut VehiclePrediction,
    vehicle: SteeredVehicle,
    first: u64,
    ticks: u64,
) -> PredictedVehicleSample {
    let mut sample = None;
    for tick in first..first + ticks {
        prediction
            .step(Some(vehicle), FORWARD, &Lake)
            .unwrap()
            .unwrap();
        sample = prediction.commit(tick, 32);
    }
    sample.unwrap()
}

#[test]
fn rowing_moves_the_boat_and_seats_the_rider_on_it() {
    let mut prediction = VehiclePrediction::default();
    let first = drive(&mut prediction, boat(), 1, 1);
    assert_eq!(first.unique_id, -7);
    let sample = drive(&mut prediction, boat(), 2, 20);
    assert!(sample.position[0] > first.position[0] + 1.0);
    assert!(sample.velocity[0] > 0.1);
    assert_eq!(sample.rotation, [0.0, 0.0]);

    let seat = prediction
        .step(Some(boat()), FORWARD, &Lake)
        .unwrap()
        .unwrap();
    let after = prediction.commit(22, 32).unwrap();
    assert_eq!(seat[0], after.position[0]);
    assert!((seat[1] - (after.position[1] - OAK.height_offset - 0.2)).abs() < 1.0e-6);

    let feed = prediction.feed().unwrap();
    assert_eq!(feed.runtime_id, 7);
    assert_eq!(feed.position[0], after.position[0]);
    assert!((feed.position[1] - (after.position[1] - OAK.height_offset)).abs() < 1.0e-6);
}

#[test]
fn a_correction_rewinds_to_its_tick_and_replays_the_later_paddling() {
    let mut predicted = VehiclePrediction::default();
    drive(&mut predicted, boat(), 1, 10);
    let mut reference = VehiclePrediction::default();
    let corrected_tick = drive(&mut reference, boat(), 1, 4);
    let shifted = VehicleCorrection {
        tick: 4,
        position: [
            corrected_tick.position[0] + 2.0,
            corrected_tick.position[1],
            corrected_tick.position[2],
        ],
        velocity: corrected_tick.velocity,
        rotation: corrected_tick.rotation,
        angular_velocity: None,
        on_ground: false,
    };
    let seat = predicted.correct(&shifted, &Lake).unwrap().seat;
    let before = drive(&mut reference, boat(), 5, 6);
    let after = predicted.feed().unwrap();
    assert!((after.position[0] - (before.position[0] + 2.0)).abs() < 1.0e-3);
    assert_eq!(seat[0], after.position[0]);

    // A correction that agrees, or names an unretained tick, changes nothing.
    assert_eq!(predicted.correct(&shifted, &Lake), None);
    assert_eq!(
        predicted.correct(
            &VehicleCorrection {
                tick: 99,
                ..shifted
            },
            &Lake
        ),
        None
    );
}

#[test]
fn a_server_teleport_restarts_prediction_from_the_new_pose() {
    let mut prediction = VehiclePrediction::default();
    drive(&mut prediction, boat(), 1, 20);
    let teleported = SteeredVehicle {
        position: [40.5, 62.6, 40.5],
        yaw: 90.0,
        server_moves: 1,
        ..boat()
    };
    let sample = drive(&mut prediction, teleported, 21, 1);
    assert!((sample.position[0] - 40.5).abs() < 0.1);
    assert!(sample.rotation[1] >= 90.0);
}

#[test]
fn leaving_the_boat_stops_predicting_it() {
    let mut prediction = VehiclePrediction::default();
    drive(&mut prediction, boat(), 1, 3);
    assert_eq!(prediction.step(None, FORWARD, &Lake), Ok(None));
    assert_eq!(prediction.commit(4, 32), None);
    assert_eq!(prediction.feed(), None);
}

#[test]
fn a_correction_replays_with_each_tick_s_recorded_wave_phase() {
    let mut predicted = VehiclePrediction::default();
    drive(&mut predicted, boat(), 1, 10);
    let before = predicted.feed().unwrap();
    let wave = predicted.predicted.as_ref().unwrap().state.wave_timer;
    let mut reference = VehiclePrediction::default();
    let at = drive(&mut reference, boat(), 1, 4);
    predicted
        .correct(
            &VehicleCorrection {
                tick: 4,
                position: [at.position[0] + 2.0, at.position[1], at.position[2]],
                velocity: at.velocity,
                rotation: at.rotation,
                angular_velocity: None,
                on_ground: false,
            },
            &Lake,
        )
        .unwrap();
    // A horizontal shift on still water leaves the bobbing exactly as predicted.
    let after = predicted.feed().unwrap();
    assert_eq!(after.position[1], before.position[1]);
    assert_eq!(predicted.predicted.as_ref().unwrap().state.wave_timer, wave);
}

#[test]
fn a_correction_that_changes_only_motion_or_ground_still_applies() {
    let mut predicted = VehiclePrediction::default();
    let at = drive(&mut predicted, boat(), 1, 6);
    let stopped = VehicleCorrection {
        tick: 6,
        position: at.position,
        velocity: [0.0; 3],
        rotation: at.rotation,
        angular_velocity: None,
        on_ground: false,
    };
    let replay = predicted.correct(&stopped, &Lake).unwrap();
    assert_eq!(
        replay.samples,
        vec![(
            6,
            PredictedVehicleSample {
                velocity: [0.0; 3],
                ..at
            }
        )]
    );
    let grounded = VehicleCorrection {
        on_ground: true,
        ..stopped
    };
    assert!(predicted.correct(&grounded, &Lake).is_some());
}

#[test]
fn a_replay_that_cannot_finish_leaves_the_prediction_unchanged() {
    let mut predicted = VehiclePrediction::default();
    let at = drive(&mut predicted, boat(), 1, 4);
    drive(&mut predicted, boat(), 5, 6);
    let before = predicted.feed().unwrap();
    let into_unloaded = VehicleCorrection {
        tick: 4,
        position: [150.0, at.position[1], at.position[2]],
        velocity: at.velocity,
        rotation: at.rotation,
        angular_velocity: None,
        on_ground: false,
    };
    assert_eq!(predicted.correct(&into_unloaded, &EdgeLake), None);
    assert_eq!(predicted.feed().unwrap(), before);
    let history = &predicted.predicted.as_ref().unwrap().history;
    assert!(
        history
            .iter()
            .all(|recorded| recorded.state.position[0] < 100.0)
    );
}

#[test]
fn a_boat_on_unloaded_terrain_fails_the_tick_instead_of_seating_the_rider_elsewhere() {
    let mut prediction = VehiclePrediction::default();
    let far = SteeredVehicle {
        position: [150.5, 62.6, 0.5],
        ..boat()
    };
    assert!(prediction.step(Some(far), FORWARD, &EdgeLake).is_err());
    assert_eq!(prediction.commit(1, 32), None);
}

#[test]
fn a_predicted_input_keeps_the_rider_position_and_its_unsent_boat_follows_corrections() {
    use crate::test_support::survival_mining::completed;
    let mut ticker = super::super::MovementTicker::default();
    ticker.reset(7, 100, completed(101).position);
    ticker.set_source(super::super::MovementSource::Physics);
    let boat = PredictedVehicleSample {
        unique_id: -7,
        position: [10.0, 62.375, 4.0],
        velocity: [0.3, 0.0, 0.0],
        rotation: [0.0, 90.0],
    };
    let mut sample = completed(101);
    sample.processed.vehicle = Some(boat);
    ticker.enqueue_completed_physics(sample.clone()).unwrap();
    let snapshot = ticker.pending_snapshots()[0];
    // Interactions read the rider; only the wire reports the boat.
    assert_eq!(snapshot.position, sample.position);
    assert_eq!(snapshot.vehicle, Some(boat.input()));
    let corrected = PredictedVehicleSample {
        position: [11.0, 62.375, 4.0],
        ..boat
    };
    ticker.reconcile_vehicle_inputs(&[(101, corrected)]);
    assert_eq!(
        ticker.pending_snapshots()[0].vehicle,
        Some(corrected.input())
    );
}
