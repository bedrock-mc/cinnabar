//! Eye replay follows the same accepted poses as body and input history.

use super::super::*;
use crate::item_use::{ItemUseRuntime, RIPTIDE_CHARGE_TICKS, RiptideUse, UseFrame, admit_on_tick};
use crate::melee::SwingTracker;
use crate::mining::FrozenMiningSelection;
use crate::movement::{
    MovementSource, MovementTicker, PhysicsCorrectionOutcome, flush_player_auth_inputs,
    reconcile_candidate_physics_correction,
};

#[derive(Clone, Copy, Debug)]
enum Pose {
    Standing,
    Sneaking,
    Swimming,
}

struct Pool(bool);

struct Unloaded;

impl CollisionWorld for Unloaded {
    /// Pauses movement with the same retryable terrain failure as an unloaded chunk.
    fn collision_boxes(
        &self,
        _: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        Err(sim::WorldQueryError::UnloadedChunk(world::ChunkKey::new(
            0, 0, 0,
        )))
    }

    /// Makes environment sampling unavailable along with collision data.
    fn block_physics(&self, _: [i32; 3]) -> Result<sim::BlockPhysicsSample, sim::WorldQueryError> {
        Err(sim::WorldQueryError::UnloadedChunk(world::ChunkKey::new(
            0, 0, 0,
        )))
    }
}

impl CollisionWorld for Pool {
    /// Keeps motion unconstrained while the liquid probes still read real block facts.
    fn collision_boxes(
        &self,
        _: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        Ok(sim::CollisionQuery::synthetic(Vec::new()))
    }

    /// Supplies a shallow surface or deep water for the retained swimming control.
    fn block_physics(
        &self,
        block: [i32; 3],
    ) -> Result<sim::BlockPhysicsSample, sim::WorldQueryError> {
        let water = self.0 || block[1] == 0;
        Ok(sim::BlockPhysicsSample {
            layers: Box::new([sim::BlockPhysicsFacts {
                friction: 0.6,
                horizontal_speed_factor: 1.0,
                vertical_speed_factor: 1.0,
                fluid_height_blocks: if water { 1.0 } else { 0.0 },
                flags: if water {
                    sim::BlockPhysicsFlags::WATER
                } else {
                    sim::BlockPhysicsFlags::default()
                },
                surface_response: sim::SurfaceResponse::None,
            }]),
            identity: sim::CollisionQuery::synthetic(()).identity,
        })
    }
}

/// Keeps swimming active and lets an existing crouch ease back toward standing after release.
fn controls(pose: Pose, after_release: usize) -> MovementInput {
    MovementInput {
        sneaking: matches!(pose, Pose::Sneaking) && after_release < 2,
        forward: if matches!(pose, Pose::Swimming) {
            1.0
        } else {
            0.0
        },
        sprinting: matches!(pose, Pose::Swimming),
        ..Default::default()
    }
}

/// Advances and admits exactly one successful fixed tick.
fn advance(
    physics: &mut LocalPhysicsController,
    ticker: &mut MovementTicker,
    input: MovementInput,
    world: &Pool,
) {
    let frame = physics.advance(simulation_clock::TICK_DURATION, input, world);
    assert!(frame.blocked.is_none(), "{:?}", frame.blocked);
    assert_eq!(frame.samples.len(), 1);
    ticker
        .enqueue_completed_physics(frame.samples[0].clone())
        .unwrap();
}

/// Stages a charged release behind its facing input, then optionally writes that input.
fn release(
    physics: &mut LocalPhysicsController,
    ticker: &mut MovementTicker,
    pose: Pose,
    written: bool,
    world: &Pool,
) {
    let stack = protocol::NetworkItemStack {
        network_id: 2,
        count: 1,
        stack_network_id: 9,
        ..protocol::NetworkItemStack::empty()
    };
    let selection = FrozenMiningSelection {
        slot: 0,
        item: protocol::VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest).unwrap(),
    };
    let mut runtime = ItemUseRuntime::default();
    let mut swings = SwingTracker::default();
    for index in 0..=RIPTIDE_CHARGE_TICKS {
        advance(physics, ticker, controls(pose, 0), world);
        let tick = ticker.completed_tick();
        let frame = UseFrame {
            riptide_facing: Default::default(),
            riptide: Some(RiptideUse {
                level: 3,
                wet: true,
                mounted: false,
                usable: true,
            }),
            tick,
            now_millis: tick * 50,
            position: physics.network_position().unwrap(),
            held: true,
            selection: Some(selection.clone()),
            air_use: crate::item_use::fixtures::classify("minecraft:trident", false, 0, None),
            ready: true,
            creative: false,
            inventory_revision: Some(1),
            charge_projectile: None,
            press_consumed: false,
        };
        runtime.observe_press(index == 0);
        admit_on_tick(&mut runtime, &mut swings, ticker, &frame, 42, 6, |_| Ok(()));
        if index == RIPTIDE_CHARGE_TICKS {
            let mut packets = Vec::new();
            admit_on_tick(
                &mut runtime,
                &mut swings,
                ticker,
                &UseFrame {
                    held: false,
                    ..frame
                },
                42,
                6,
                |batch| {
                    packets = batch;
                    Ok(())
                },
            );
            assert_eq!(packets.len(), 1);
            ticker.hold_release_after_tick(tick, packets);
            let launch = runtime.take_riptide_launch().unwrap();
            ticker.request_riptide(physics, launch.level, launch.rotation);
        }
    }
    if written {
        let mut receipts = Vec::new();
        flush_player_auth_inputs(
            ticker,
            crate::movement::OUTBOX_CAPACITY,
            Some(crate::test_support::survival_mining::evidence()),
            |identity, _| {
                receipts.push(identity);
                Ok::<_, ()>(())
            },
        )
        .unwrap();
        for receipt in receipts {
            assert!(ticker.acknowledge_physics_send(receipt));
        }
        ticker.send_held_release(|_| Err(crate::BatchSendError::Full));
    }
    assert!(ticker.has_held_release());
}

/// Compares cancellation against identical controls without a launch, or a retained valid release.
fn corrected_pair(
    pose: Pose,
    predicted: usize,
    anchor_offset: i64,
    written: bool,
) -> (LocalPhysicsController, LocalPhysicsController, u64, Pool) {
    let world = Pool(matches!(pose, Pose::Swimming));
    let origin = [0.0, PLAYER_NETWORK_OFFSET + 0.1, 0.0];
    let mut physics = LocalPhysicsController::default();
    physics.set_rewind_history_size(12);
    physics.reanchor_network_position(origin, 100, false);
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, origin);
    ticker.set_source(MovementSource::Physics);
    release(&mut physics, &mut ticker, pose, written, &world);
    if matches!(pose, Pose::Swimming) {
        assert_eq!(physics.mode(), sim::MovementMode::Swimming);
    }
    let facing = ticker.completed_tick();
    let mut equivalent = physics.clone();
    let mut equivalent_ticker = ticker.clone();
    if !written {
        equivalent.riptide_pending = None;
        // The equivalent trajectory never released, so it carries no release fence.
        equivalent_ticker.held_release = None;
    }
    for index in 0..predicted {
        advance(&mut physics, &mut ticker, controls(pose, index), &world);
        advance(
            &mut equivalent,
            &mut equivalent_ticker,
            controls(pose, index),
            &world,
        );
    }
    assert!(physics.state().unwrap().spin_attack_ticks.is_some());
    let tick = (facing as i64 + anchor_offset) as u64;
    let position = equivalent.sample_at(tick).unwrap().position;
    let before = physics.clone();
    let retained = written.then(|| physics.clone());
    for (ticker, physics) in [
        (&mut ticker, &mut physics),
        (&mut equivalent_ticker, &mut equivalent),
    ] {
        let outcome = reconcile_candidate_physics_correction(
            ticker,
            physics,
            position,
            tick,
            false,
            PhysicsCorrectionMode::ReplayIfRetained,
            &world,
        )
        .unwrap();
        assert!(matches!(outcome, PhysicsCorrectionOutcome::Replayed { .. }));
        assert_eq!(ticker.has_held_release(), written);
        if !written {
            ticker.send_held_release(|_| panic!("a discarded release cannot be sent"));
        }
    }
    assert_eq!(
        physics.state().unwrap().spin_attack_ticks.is_some(),
        written
    );
    // Vanilla's replay never ticks or copies back the live camera eye offset.
    assert_camera(&physics, &before);
    if let Some(retained) = retained {
        // The accepted network anchor rounds body coordinates through f32; pose offsets stay exact.
        assert_eye_history(&physics, &retained, facing, &world);
    }
    (physics, equivalent, facing, world)
}

/// Checks the actual camera interpolation relative to its independently corrected actor origin.
fn assert_camera(physics: &LocalPhysicsController, equivalent: &LocalPhysicsController) {
    for alpha in [0.0, 0.25, 0.5, 1.0] {
        assert_eq!(
            physics.eye_offset.height(alpha),
            equivalent.eye_offset.height(alpha),
            "camera eye differs at alpha {alpha}"
        );
        let mut actual = physics.clone();
        let mut expected = equivalent.clone();
        actual.accumulated_seconds = f64::from(alpha) * LOCAL_PHYSICS_TICK_SECONDS;
        expected.accumulated_seconds = actual.accumulated_seconds;
        let height = |controller: &LocalPhysicsController| {
            controller.render_eye_position().unwrap()[1]
                - controller.render_feet_position().unwrap()[1]
        };
        assert!((height(&actual) - height(&expected)).abs() < 0.000_01);
    }
}

/// Checks retained aim, both input copies and the resulting head-water probe at each tick.
fn assert_history(
    physics: &LocalPhysicsController,
    equivalent: &LocalPhysicsController,
    facing: u64,
    world: &Pool,
) {
    let end = physics.state().unwrap().tick;
    assert_eq!(
        physics
            .aim_poses_after(Some(facing - 2), end)
            .collect::<Vec<_>>(),
        equivalent
            .aim_poses_after(Some(facing - 2), end)
            .collect::<Vec<_>>(),
        "retained aim keeps the cancelled spin eye"
    );
    assert_eye_history(physics, equivalent, facing, world);
    assert_eq!(physics.state(), equivalent.state());
}

/// Compares pose offsets and liquid probes against the reference on the accepted body positions.
fn assert_eye_history(
    physics: &LocalPhysicsController,
    equivalent: &LocalPhysicsController,
    facing: u64,
    world: &Pool,
) {
    let end = physics.state().unwrap().tick;
    let expected_poses: Vec<_> = physics
        .sample_history
        .iter()
        .filter(|sample| sample.tick > facing - 2 && sample.tick <= end)
        .map(|sample| {
            let expected = equivalent
                .controller_history
                .iter()
                .find(|frame| frame.tick == sample.tick)
                .unwrap();
            let mut eye = sample.position;
            eye[1] += expected.eye_offset.height(1.0) - PLAYER_NETWORK_OFFSET;
            let orientation = equivalent
                .sample_at(sample.tick)
                .unwrap()
                .camera_orientation;
            (sample.tick, eye, orientation)
        })
        .collect();
    assert_eq!(
        physics
            .aim_poses_after(Some(facing - 2), end)
            .collect::<Vec<_>>(),
        expected_poses
    );
    for frame in &physics.controller_history {
        let expected = equivalent
            .controller_history
            .iter()
            .find(|other| other.tick == frame.tick)
            .unwrap();
        for alpha in [0.0, 0.25, 0.5, 1.0] {
            assert_eq!(
                frame.eye_offset.height(alpha),
                expected.eye_offset.height(alpha),
                "retained eye at tick {} and alpha {alpha}",
                frame.tick
            );
        }
        assert_eq!(
            frame.input.liquid_attach_height, expected.input.liquid_attach_height,
            "controller liquid attachment at {}",
            frame.tick
        );
        let actual_input = physics.history.input_at(frame.tick).unwrap();
        let expected_input = equivalent.history.input_at(frame.tick).unwrap();
        assert_eq!(
            actual_input.liquid_attach_height, expected_input.liquid_attach_height,
            "retained liquid attachment at {}",
            frame.tick
        );
        if let Some(previous) = physics.history.state_at(frame.tick - 1) {
            let expected_previous = equivalent.history.state_at(frame.tick - 1).unwrap();
            assert_eq!(
                sim::sample_water_head(
                    world,
                    previous.position,
                    actual_input.liquid_attach_height.unwrap()
                )
                .unwrap()
                .value,
                sim::sample_water_head(
                    world,
                    expected_previous.position,
                    expected_input.liquid_attach_height.unwrap()
                )
                .unwrap()
                .value
            );
        }
    }
}

/// Replay leaves the live camera eye blend, as vanilla ticks the eye offset
/// only on its temporary replay entity and copy-back leaves the live one;
/// live ticks then ease it toward the replayed pose.
#[test]
fn riptide_cancelled_release_replay_leaves_the_live_camera_eye() {
    for pose in [Pose::Standing, Pose::Sneaking, Pose::Swimming] {
        for predicted in [1, 4] {
            for offset in [-1, 0, predicted as i64] {
                // `corrected_pair` checks the camera against the pre-correction one.
                corrected_pair(pose, predicted, offset, false);
            }
        }
    }
}

#[test]
fn riptide_cancelled_release_rebuilds_retained_aim_and_liquid_heights() {
    for pose in [Pose::Standing, Pose::Sneaking, Pose::Swimming] {
        for predicted in [1, 4] {
            for offset in [-1, 0, predicted as i64] {
                let (physics, equivalent, facing, world) =
                    corrected_pair(pose, predicted, offset, false);
                assert_history(&physics, &equivalent, facing, &world);
            }
        }
    }
}

#[test]
fn riptide_written_release_replay_retains_eye_and_liquid_history() {
    for pose in [Pose::Standing, Pose::Sneaking, Pose::Swimming] {
        let (physics, equivalent, facing, world) = corrected_pair(pose, 4, 0, true);
        assert_camera(&physics, &equivalent);
        assert_history(&physics, &equivalent, facing, &world);
    }
}

#[test]
fn riptide_eye_replay_preserves_nonzero_anchors_and_repeated_rewinds() {
    let (mut physics, mut equivalent, facing, world) = corrected_pair(Pose::Sneaking, 4, 0, false);
    let oldest = physics.sample_history.front().unwrap().tick;
    assert!(
        oldest > 101,
        "the initial neutral eye is outside the retained window"
    );
    for tick in [oldest, physics.state().unwrap().tick, facing, oldest] {
        let (live, equivalent_live) = (physics.clone(), equivalent.clone());
        physics.replay_retained_from(tick, &world).unwrap();
        equivalent.replay_retained_from(tick, &world).unwrap();
        assert_camera(&physics, &live);
        assert_camera(&equivalent, &equivalent_live);
        assert_history(&physics, &equivalent, facing, &world);
    }
}

#[test]
fn riptide_eye_replay_preserves_a_collision_pause() {
    let (mut physics, _, facing, world) = corrected_pair(Pose::Sneaking, 4, 0, false);
    let frame = physics.advance(
        simulation_clock::TICK_DURATION,
        controls(Pose::Sneaking, 4),
        &Unloaded,
    );
    assert!(frame.blocked.is_some());
    assert!(frame.samples.is_empty());
    let held = physics.clone();
    for tick in [facing, physics.state().unwrap().tick, facing] {
        physics.replay_retained_from(tick, &world).unwrap();
        assert_camera(&physics, &held);
        assert_history(&physics, &held, facing, &world);
    }
    let mut expected = held;
    let mut ticker = MovementTicker::default();
    let mut expected_ticker = MovementTicker::default();
    for ticker in [&mut ticker, &mut expected_ticker] {
        ticker.reset(
            7,
            physics.state().unwrap().tick,
            physics.network_position().unwrap(),
        );
        ticker.set_source(MovementSource::Physics);
    }
    advance(
        &mut physics,
        &mut ticker,
        controls(Pose::Sneaking, 4),
        &world,
    );
    advance(
        &mut expected,
        &mut expected_ticker,
        controls(Pose::Sneaking, 4),
        &world,
    );
    assert_camera(&physics, &expected);
}

/// A timeline rewind whose preparatory riptide replay fails still schedules the vanilla sync.
#[test]
fn a_failed_riptide_preparation_keeps_the_prediction_sync() {
    let world = Pool(false);
    let origin = [0.0, PLAYER_NETWORK_OFFSET + 0.1, 0.0];
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(origin, 100, false);
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, origin);
    ticker.set_source(MovementSource::Physics);
    release(&mut physics, &mut ticker, Pose::Standing, false, &world);
    let facing = ticker.completed_tick();
    // The launch must be retained so discarding the unsent release replays history.
    for index in 0..2 {
        advance(
            &mut physics,
            &mut ticker,
            controls(Pose::Standing, index),
            &world,
        );
    }
    assert!(physics.state().unwrap().spin_attack_ticks.is_some());
    assert!(ticker.has_held_release());
    let scheduled = |physics: &LocalPhysicsController| {
        let mut countdown = physics.prediction_sync.clone();
        (0..400).any(|_| {
            countdown.tick();
            countdown.due()
        })
    };
    assert!(!scheduled(&physics));
    assert!(
        crate::movement::reconcile_timeline_rewind(&mut ticker, &mut physics, facing, &Unloaded)
            .is_err()
    );
    assert!(scheduled(&physics));
}

/// A rewind starts its first frame from the eye that frame started with live,
/// not from an earlier replay's rebuilt eye for the frame before it, so the
/// liquid attach height the frame used survives.
#[test]
fn a_later_rewind_starts_from_the_frame_entry_eye() {
    let world = Pool(false);
    let origin = [0.0, PLAYER_NETWORK_OFFSET + 0.1, 0.0];
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(origin, 100, false);
    let step = |physics: &mut LocalPhysicsController| {
        assert_eq!(
            physics
                .advance(
                    simulation_clock::TICK_DURATION,
                    MovementInput::default(),
                    &world
                )
                .samples
                .len(),
            1
        );
    };
    for _ in 0..3 {
        step(&mut physics);
    }
    let spin = client_world::MovementFlagUpdate {
        spinning: Some(true),
        ..Default::default()
    };
    physics.apply_server_movement_flags(101, spin);
    physics.replay_retained_from(101, &world).unwrap();
    step(&mut physics);
    let live = physics.history.input_at(104).unwrap().liquid_attach_height;
    physics.replay_retained_from(103, &world).unwrap();
    assert_eq!(
        physics.history.input_at(104).unwrap().liquid_attach_height,
        live
    );
}
