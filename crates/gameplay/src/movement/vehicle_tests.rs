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
        velocity: [0.0; 3],
        seat: [0.0, -0.2, 0.0],
        revision: client_world::VehicleRevision { pose: 0, motion: 0 },
    }
}

/// Steps and commits ticks `first..first + ticks`, returning the last sample.
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
        sample = prediction.commit(tick);
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
    let after = prediction.commit(22).unwrap();
    assert_eq!(seat[0], after.position[0]);
    assert!((seat[1] - (after.position[1] - OAK.height_offset - 0.2)).abs() < 1.0e-6);

    let feed = prediction.feed(1.0).unwrap();
    assert_eq!(feed.runtime_id, 7);
    assert_eq!(feed.position[0], after.position[0]);
    assert!((feed.position[1] - (after.position[1] - OAK.height_offset)).abs() < 1.0e-6);
}

#[test]
fn a_server_teleport_restarts_prediction_from_the_new_pose() {
    let mut prediction = VehiclePrediction::default();
    drive(&mut prediction, boat(), 1, 20);
    let teleported = SteeredVehicle {
        position: [40.5, 62.6, 40.5],
        yaw: 90.0,
        revision: client_world::VehicleRevision { pose: 1, motion: 0 },
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
    assert_eq!(prediction.commit(4), None);
    assert_eq!(prediction.feed(1.0), None);
}

#[test]
fn a_boat_on_unloaded_terrain_fails_the_tick_instead_of_seating_the_rider_elsewhere() {
    let mut prediction = VehiclePrediction::default();
    let far = SteeredVehicle {
        position: [150.5, 62.6, 0.5],
        ..boat()
    };
    assert!(prediction.step(Some(far), FORWARD, &EdgeLake).is_err());
    assert_eq!(prediction.commit(4), None);
}

#[test]
fn a_predicted_input_keeps_the_rider_position_and_reports_the_boat_on_the_wire() {
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
}

#[test]
fn a_correction_restarts_prediction_from_the_newest_server_state() {
    let mut predicted = VehiclePrediction::default();
    let at = drive(&mut predicted, boat(), 1, 10);
    let correction = |tick, x: f32| VehicleCorrection {
        runtime_id: 7,
        revision: client_world::VehicleRevision { pose: 0, motion: 0 },
        tick,
        position: [x, at.position[1], at.position[2]],
        velocity: [0.0; 3],
        rotation: [0.0, 45.0],
        angular_velocity: Some(0.0),
        on_ground: false,
    };
    // A correction of a vehicle this session does not predict changes nothing.
    let other = VehicleCorrection {
        runtime_id: 8,
        ..correction(9, 2.0)
    };
    assert_eq!(predicted.correct(&other), None);
    let seat = predicted.correct(&correction(9, 2.0)).unwrap();
    assert_eq!(seat[0], 2.0);
    let feed = predicted.feed(1.0).unwrap();
    assert_eq!(
        (feed.position[0], feed.yaw, feed.velocity),
        (2.0, 45.0, [0.0; 3])
    );
    // An older correction arriving later does not undo the newer one.
    assert_eq!(predicted.correct(&correction(8, 5.0)), None);
    assert_eq!(predicted.feed(1.0).unwrap().position[0], 2.0);
    // Prediction continues from the corrected boat.
    let next = drive(&mut predicted, boat(), 11, 1);
    assert!((next.position[0] - 2.0).abs() < 0.5);
}

/// A correction arriving after a server change continues the session at that change's
/// revision, so the older change does not replace the corrected boat.
#[test]
fn a_correction_after_a_server_change_continues_at_its_revision() {
    let mut predicted = VehiclePrediction::default();
    let at = drive(&mut predicted, boat(), 1, 10);
    let correction = VehicleCorrection {
        runtime_id: 7,
        revision: client_world::VehicleRevision { pose: 1, motion: 0 },
        tick: 9,
        position: [2.0, at.position[1], at.position[2]],
        velocity: [0.0; 3],
        rotation: [0.0, 0.0],
        angular_velocity: None,
        on_ground: false,
    };
    predicted.correct(&correction).unwrap();
    assert_eq!(predicted.feed(1.0).unwrap().revision.pose, 1);
    let changed = SteeredVehicle {
        revision: client_world::VehicleRevision { pose: 1, motion: 0 },
        ..boat()
    };
    let next = drive(&mut predicted, changed, 11, 1);
    assert!((next.position[0] - 2.0).abs() < 0.5);
    // A correction from before that change is ignored.
    let older = VehicleCorrection {
        revision: client_world::VehicleRevision { pose: 0, motion: 0 },
        tick: 12,
        ..correction
    };
    assert_eq!(predicted.correct(&older), None);
}

#[test]
fn a_new_session_starts_from_the_boat_s_retained_motion() {
    let mut prediction = VehiclePrediction::default();
    let drifting = SteeredVehicle {
        velocity: [0.3, 0.0, 0.0],
        ..boat()
    };
    prediction.step(Some(drifting), [0.0; 2], &Lake).unwrap();
    let sample = prediction.commit(1).unwrap();
    // Without paddling, the boat keeps gliding instead of starting from a standstill.
    assert!(sample.position[0] > 0.6, "{sample:?}");
    assert!(sample.velocity[0] > 0.0);
}

#[test]
fn a_correction_from_an_earlier_session_is_ignored() {
    let mut predicted = VehiclePrediction::default();
    let at = drive(&mut predicted, boat(), 1, 10);
    let teleported = SteeredVehicle {
        position: [40.5, 62.6, 0.5],
        revision: client_world::VehicleRevision { pose: 1, motion: 0 },
        ..boat()
    };
    drive(&mut predicted, teleported, 11, 2);
    let before = predicted.feed(1.0).unwrap();
    let stale = VehicleCorrection {
        runtime_id: 7,
        revision: client_world::VehicleRevision { pose: 0, motion: 0 },
        tick: 8,
        position: at.position,
        velocity: [0.0; 3],
        rotation: at.rotation,
        angular_velocity: None,
        on_ground: false,
    };
    assert_eq!(predicted.correct(&stale), None);
    assert_eq!(predicted.feed(1.0).unwrap(), before);
}

/// The boat renders between its last two ticks on the rider's clock.
#[test]
fn the_fed_boat_renders_between_its_last_two_ticks() {
    let mut prediction = VehiclePrediction::default();
    assert_eq!(prediction.feed(0.5), None);
    for tick in 1..=6 {
        prediction.step(Some(boat()), [-1.0, 1.0], &Lake).unwrap();
        prediction.commit(tick);
    }
    let start = prediction.feed(0.0).unwrap();
    let end = prediction.feed(1.0).unwrap();
    let half = prediction.feed(0.5).unwrap();
    for axis in 0..3 {
        assert!((end.render_position[axis] - end.position[axis]).abs() < 1.0e-4);
    }
    assert!((end.render_yaw - end.yaw).abs() < 1.0e-4);
    assert_ne!(start.render_position, end.render_position);
    for axis in 0..3 {
        let midpoint = (start.render_position[axis] + end.render_position[axis]) / 2.0;
        assert!((half.render_position[axis] - midpoint).abs() < 1.0e-4);
    }
    assert!(start.render_yaw < half.render_yaw && half.render_yaw < end.render_yaw);
    // The latest tick's pose is fed whatever the render point.
    assert_eq!(half.position, end.position);
}

#[test]
fn the_rendered_heading_eases_through_the_current_tick() {
    let mut prediction = VehiclePrediction::default();
    assert_eq!(prediction.render_yaw(0.5), None);
    for tick in 1..=6 {
        prediction.step(Some(boat()), [-1.0, 0.0], &Lake).unwrap();
        prediction.commit(tick);
    }
    let session = prediction.session.as_ref().unwrap();
    let (previous, current) = (session.previous_yaw, session.state.yaw);
    assert!(current > previous);
    assert_eq!(prediction.render_yaw(0.0), Some(previous));
    assert!((prediction.render_yaw(1.0).unwrap() - current).abs() < 1.0e-4);
    let half = prediction.render_yaw(0.5).unwrap();
    assert!(previous < half && half < current);
}

/// Server motion that follows a correction in the same poll keeps the corrected pose and
/// takes the new motion, rather than restarting from the boat's uncorrected pose.
#[test]
fn server_motion_after_a_correction_keeps_the_corrected_pose() {
    let mut predicted = VehiclePrediction::default();
    let at = drive(&mut predicted, boat(), 1, 10);
    let correction = VehicleCorrection {
        runtime_id: 7,
        revision: client_world::VehicleRevision::default(),
        tick: 9,
        position: [2.0, at.position[1], at.position[2]],
        velocity: [0.0; 3],
        rotation: [0.0, 0.0],
        angular_velocity: None,
        on_ground: false,
    };
    predicted.correct(&correction).unwrap();
    let pushed = SteeredVehicle {
        revision: client_world::VehicleRevision { pose: 0, motion: 1 },
        velocity: [0.3, 0.0, 0.0],
        ..boat()
    };
    let next = drive(&mut predicted, pushed, 11, 1);
    assert!(next.position[0] > 2.0 && next.position[0] < 2.5, "{next:?}");
}
