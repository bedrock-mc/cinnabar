//! Locomotion-mode wire edges: each start/stop flag rides exactly the tick the simulator changed mode.

use std::time::Duration;

use protocol::PlayerInputFlags;
use sim::{
    Aabb, CollisionQuery, CollisionWorld, MovementInput, MovementMode, Vec3, WorldQueryError,
};

use super::integration_tests::VersionedFloor;
use super::settle_tests::settled_sample;
use super::{
    HeldInput, LocalPhysicsController, ModeIntent, MovementSource, MovementTicker,
    PhysicsMovementSample, PhysicsSampleContext, RideKind, input_flags,
    reconcile_candidate_physics_correction,
};

const TICK: Duration = Duration::from_millis(50);

/// A render frame without a fixed tick must retain the flight toggle.
#[test]
fn flight_toggle_survives_a_frame_without_a_tick() {
    let mut physics = grounded_controller();
    let context = PhysicsSampleContext {
        mode_intent: ModeIntent {
            can_fly: true,
            fly_toggle: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let frame = physics.advance_with_context(
        Duration::from_millis(10),
        MovementInput::default(),
        context,
        &VersionedFloor(1),
    );
    assert_eq!(frame.completed_ticks, 0);
    let frame = physics.advance_with_context(
        Duration::from_millis(40),
        MovementInput::default(),
        PhysicsSampleContext {
            mode_intent: ModeIntent {
                fly_toggle: false,
                ..context.mode_intent
            },
            ..context
        },
        &VersionedFloor(1),
    );
    assert_eq!(frame.samples[0].processed.mode, MovementMode::Flying);
}

/// A spatial correction cannot cancel unacknowledged flight or erase a later server clear.
#[test]
fn in_session_snap_preserves_flight_and_server_ability_edges() {
    for server_flying in [false, true] {
        let mut physics = grounded_controller();
        let world = VersionedFloor(1);
        let mut ticker = MovementTicker::default();
        ticker.reset(
            1,
            physics.state().unwrap().tick,
            physics.network_position().unwrap(),
        );
        ticker.set_source(MovementSource::Physics);
        let input = MovementInput {
            jumping: true,
            ..Default::default()
        };
        let intent = ModeIntent {
            can_fly: true,
            server_flying,
            ..Default::default()
        };
        let flying = step(
            &mut physics,
            input,
            ModeIntent {
                fly_toggle: !server_flying,
                ..intent
            },
            &world,
        );
        assert_eq!(flying.processed.mode, MovementMode::Flying);
        ticker.enqueue_completed_physics(flying.clone()).unwrap();
        let started = ticker.pop_pending().unwrap().snapshot;
        assert!(has(started.flags, PlayerInputFlags::START_FLYING));
        reconcile_candidate_physics_correction(
            &mut ticker,
            &mut physics,
            flying.position,
            flying.tick,
            false,
            super::PhysicsCorrectionMode::Snap,
            &world,
        )
        .unwrap();
        assert_eq!(ticker.pending_count(), 0);
        // Consume the render delta preceding the correction before advancing a fresh tick.
        let discarded = physics.advance_with_context(
            Duration::ZERO,
            input,
            PhysicsSampleContext {
                mode_intent: intent,
                ..Default::default()
            },
            &world,
        );
        assert!(discarded.samples.is_empty());
        let continued = step(&mut physics, input, intent, &world);
        assert_eq!(continued.processed.mode, MovementMode::Flying);
        ticker.enqueue_completed_physics(continued).unwrap();
        let flags = ticker.pop_pending().unwrap().snapshot.flags;
        assert!(!has(flags, PlayerInputFlags::START_FLYING));
        assert!(!has(flags, PlayerInputFlags::STOP_FLYING));
        assert!(has(flags, PlayerInputFlags::WANT_UP));
        if server_flying {
            let stopped = step(
                &mut physics,
                input,
                ModeIntent {
                    server_flying: false,
                    ..intent
                },
                &world,
            );
            assert_eq!(stopped.processed.mode, MovementMode::Walking);
            ticker.enqueue_completed_physics(stopped).unwrap();
            let flags = ticker.pop_pending().unwrap().snapshot.flags;
            assert!(has(flags, PlayerInputFlags::STOP_FLYING));
            assert!(!has(flags, PlayerInputFlags::START_FLYING));
        }
    }
}

/// Floor top at y=1 plus a ceiling whose underside sits at the given height.
struct LowCeiling(f64);

impl CollisionWorld for LowCeiling {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        let mut base = VersionedFloor(1).collision_boxes(query)?;
        let ceiling = Aabb::new(
            Vec3::new(-64.0, self.0, -64.0),
            Vec3::new(64.0, self.0 + 1.0, 64.0),
        );
        if ceiling.intersects(query) {
            base.value.push(ceiling);
        }
        Ok(base)
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<sim::BlockPhysicsSample, WorldQueryError> {
        VersionedFloor(1).block_physics(block)
    }
}

fn grounded_controller() -> LocalPhysicsController {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    physics
}

/// Consumes the fresh-anchor depenetration probe on open ground so later ticks start clean.
fn settled_controller() -> LocalPhysicsController {
    let mut physics = grounded_controller();
    step(
        &mut physics,
        MovementInput::default(),
        ModeIntent::default(),
        &VersionedFloor(1),
    );
    physics
}

fn step(
    physics: &mut LocalPhysicsController,
    input: MovementInput,
    intent: ModeIntent,
    world: &impl CollisionWorld,
) -> PhysicsMovementSample {
    let frame = physics.advance_with_context(
        TICK,
        input,
        PhysicsSampleContext {
            mode_intent: intent,
            ..PhysicsSampleContext::default()
        },
        world,
    );
    assert!(frame.blocked.is_none(), "blocked: {:?}", frame.blocked);
    frame.samples.into_iter().next().expect("one tick")
}

fn has(flags: PlayerInputFlags, flag: PlayerInputFlags) -> bool {
    flags.bits() & flag.bits() != 0
}

#[test]
fn flight_start_ascend_and_stop_edges_follow_the_simulated_mode() {
    let mut physics = grounded_controller();
    let can_fly = ModeIntent {
        can_fly: true,
        ..ModeIntent::default()
    };
    let hold_jump = MovementInput {
        jumping: true,
        ..MovementInput::default()
    };
    let world = VersionedFloor(1);

    let start = step(
        &mut physics,
        hold_jump,
        ModeIntent {
            fly_toggle: true,
            ..can_fly
        },
        &world,
    );
    assert_eq!(start.processed.mode, MovementMode::Flying);
    let start_flags = input_flags(&start, HeldInput::default());
    assert!(has(start_flags, PlayerInputFlags::START_FLYING));
    assert!(has(start_flags, PlayerInputFlags::ASCEND));

    let cruise = step(&mut physics, hold_jump, can_fly, &world);
    assert_eq!(cruise.processed.mode, MovementMode::Flying);
    let cruise_flags = input_flags(&cruise, HeldInput::from(&start));
    assert!(!has(cruise_flags, PlayerInputFlags::START_FLYING));
    assert!(cruise.position[1] > start.position[1]);

    let stop = step(
        &mut physics,
        MovementInput::default(),
        ModeIntent {
            fly_toggle: true,
            ..can_fly
        },
        &world,
    );
    assert_eq!(stop.processed.mode, MovementMode::Walking);
    let stop_flags = input_flags(&stop, HeldInput::from(&cruise));
    assert!(has(stop_flags, PlayerInputFlags::STOP_FLYING));
    assert!(!has(stop_flags, PlayerInputFlags::ASCEND));
}

#[test]
fn keyboard_vertical_intents_reach_server_flight_controls() {
    // Current SendPlayerInputPacket 0x070fcfd0 emits processed up/down as
    // WantUp/WantDown. ServerMoveInputHandler 0x0998fe80 reconstructs them
    // directly; raw JumpDown/Ascend alone do not populate these control lanes.
    for (jumping, sneaking, expected) in [
        (false, false, 0),
        (true, false, 4),
        (false, true, 8),
        (true, true, 12),
    ] {
        let mut physics = grounded_controller();
        let sample = step(
            &mut physics,
            MovementInput {
                jumping,
                sneaking,
                ..Default::default()
            },
            ModeIntent {
                can_fly: true,
                fly_toggle: true,
                ..Default::default()
            },
            &VersionedFloor(1),
        );
        assert_eq!(sample.processed.mode, MovementMode::Flying);
        let flags = input_flags(&sample, HeldInput::default());
        let server_vertical_controls = (flags.bits() >> 14) & 12;
        assert_eq!(server_vertical_controls, expected);
        assert_eq!(has(flags, PlayerInputFlags::WANT_UP), jumping);
        assert_eq!(has(flags, PlayerInputFlags::WANT_DOWN), sneaking);
        let released = step(
            &mut physics,
            MovementInput::default(),
            ModeIntent {
                can_fly: true,
                ..Default::default()
            },
            &VersionedFloor(1),
        );
        let released_flags = input_flags(&released, HeldInput::from(&sample));
        assert_eq!((released_flags.bits() >> 14) & 12, 0);
    }
}

#[test]
fn flight_without_permission_never_starts() {
    let mut physics = grounded_controller();
    let sample = step(
        &mut physics,
        MovementInput::default(),
        ModeIntent {
            fly_toggle: true,
            ..ModeIntent::default()
        },
        &VersionedFloor(1),
    );
    assert_eq!(sample.processed.mode, MovementMode::Walking);
}

#[test]
fn server_clearing_flight_stops_an_airborne_player_with_jump_held() {
    let mut physics = grounded_controller();
    let input = MovementInput {
        jumping: true,
        ..Default::default()
    };
    let world = VersionedFloor(1);
    let flying = step(
        &mut physics,
        input,
        ModeIntent {
            can_fly: true,
            server_flying: true,
            ..Default::default()
        },
        &world,
    );
    assert_eq!(flying.processed.mode, MovementMode::Flying);
    assert!(!flying.grounded_after_tick);
    let stopped = step(
        &mut physics,
        input,
        ModeIntent {
            can_fly: true,
            server_flying: false,
            ..Default::default()
        },
        &world,
    );
    assert_eq!(stopped.processed.mode, MovementMode::Walking);
    assert!(has(
        input_flags(&stopped, HeldInput::from(&flying)),
        PlayerInputFlags::STOP_FLYING
    ));
}

#[test]
fn crawl_edges_fire_once_on_entry_and_once_on_exit() {
    let mut crawling = settled_sample(101, [0.0, 2.620_01, 0.0]);
    crawling.processed.mode = MovementMode::Crawling;
    let entry = input_flags(&crawling, HeldInput::default());
    assert!(has(entry, PlayerInputFlags::START_CRAWLING));

    let steady = input_flags(&crawling, HeldInput::from(&crawling));
    assert!(!has(steady, PlayerInputFlags::START_CRAWLING));
    assert!(!has(steady, PlayerInputFlags::STOP_CRAWLING));

    let standing = settled_sample(102, [0.0, 2.620_01, 0.0]);
    let exit = input_flags(&standing, HeldInput::from(&crawling));
    assert!(has(exit, PlayerInputFlags::STOP_CRAWLING));
}

#[test]
fn swim_and_glide_edges_pair_start_with_stop() {
    for (mode, start, stop) in [
        (
            MovementMode::Swimming,
            PlayerInputFlags::START_SWIMMING,
            PlayerInputFlags::STOP_SWIMMING,
        ),
        (
            MovementMode::Gliding,
            PlayerInputFlags::START_GLIDING,
            PlayerInputFlags::STOP_GLIDING,
        ),
    ] {
        let mut active = settled_sample(101, [0.0, 2.620_01, 0.0]);
        active.processed.mode = mode;
        assert!(has(input_flags(&active, HeldInput::default()), start));
        let idle = settled_sample(102, [0.0, 2.620_01, 0.0]);
        assert!(has(input_flags(&idle, HeldInput::from(&active)), stop));
    }
}

#[test]
fn a_ceiling_that_only_fits_a_sneak_forces_the_pose_and_persist_flag() {
    let mut physics = settled_controller();
    let world = LowCeiling(2.6);
    let first = step(
        &mut physics,
        MovementInput::default(),
        ModeIntent::default(),
        &world,
    );
    assert!(first.processed.forced_sneak && first.sneaking);
    let flags = input_flags(&first, HeldInput::default());
    assert!(has(flags, PlayerInputFlags::PERSIST_SNEAK));
    assert!(has(flags, PlayerInputFlags::SNEAKING));
    assert_eq!(first.processed.mode, MovementMode::Walking);
}

fn riding_sample(kind: RideKind, move_vector: [f32; 2]) -> PhysicsMovementSample {
    let mut sample = settled_sample(101, [0.0, 2.620_01, 0.0]);
    sample.processed.mode = MovementMode::Riding;
    sample.processed.ride = Some(kind);
    sample.move_vector = move_vector;
    sample
}

#[test]
fn boat_paddles_follow_steering_and_other_rides_never_paddle() {
    let paddles = |kind, vector| {
        let flags = input_flags(&riding_sample(kind, vector), HeldInput::default());
        (
            has(flags, PlayerInputFlags::PADDLING_LEFT),
            has(flags, PlayerInputFlags::PADDLING_RIGHT),
        )
    };
    assert_eq!(paddles(RideKind::Boat, [0.0, 1.0]), (true, true));
    assert_eq!(paddles(RideKind::Boat, [-1.0, 0.0]), (true, false));
    assert_eq!(paddles(RideKind::Boat, [1.0, 0.0]), (false, true));
    assert_eq!(paddles(RideKind::Boat, [0.0, 0.0]), (false, false));
    assert_eq!(paddles(RideKind::Horse, [0.0, 1.0]), (false, false));
}

#[test]
fn a_mounted_controller_streams_a_frozen_pose_with_steering_intact() {
    let mut physics = settled_controller();
    let rider = ModeIntent {
        ride: Some(RideKind::Horse),
        ..ModeIntent::default()
    };
    let before = physics.network_position().unwrap();
    let sample = step(
        &mut physics,
        MovementInput {
            forward: 1.0,
            sprinting: true,
            ..MovementInput::default()
        },
        rider,
        &VersionedFloor(1),
    );
    assert_eq!(sample.processed.mode, MovementMode::Riding);
    assert_eq!(sample.position, before);
    assert_eq!(sample.movement, [0.0; 3]);
    assert!(!sample.processed.sprinting);
    assert_eq!(sample.move_vector[1], 1.0);
}

#[test]
fn a_rider_follows_the_mount_seat_and_reports_the_seat_delta() {
    let mut physics = settled_controller();
    let before = physics.network_position().unwrap();
    let rider = ModeIntent {
        ride: Some(RideKind::Boat),
        ride_seat: Some([5.0, 3.0, 5.0]),
        ..ModeIntent::default()
    };
    let sample = step(
        &mut physics,
        MovementInput::default(),
        rider,
        &VersionedFloor(1),
    );
    assert_eq!(sample.position[0], 5.0);
    assert_eq!(sample.position[2], 5.0);
    assert!((sample.position[1] - (3.0 + protocol::PLAYER_NETWORK_OFFSET)).abs() < 1.0e-4);
    assert!((sample.movement[0] - (5.0 - before[0])).abs() < 1.0e-5);
    assert!(!has(
        input_flags(&sample, HeldInput::default()),
        PlayerInputFlags::JUMPING
    ));
}

#[test]
fn holding_right_sends_negative_wire_x_and_moves_right_of_facing() {
    let mut physics = settled_controller();
    let mut ticker = super::MovementTicker::default();
    ticker.reset(1, 101, [0.0, 2.620_01, 0.0]);
    ticker.set_source(super::MovementSource::Physics);
    // Yaw 0 faces +z, so the player's right is -x.
    let input = super::physics_movement_input([1.0, 0.0], 0.0, true, false, false, false, None);
    let sample = step(
        &mut physics,
        input,
        ModeIntent::default(),
        &VersionedFloor(1),
    );
    assert!(
        sample.position[0] < 0.0,
        "strafing right moves toward -x at yaw 0"
    );
    ticker.enqueue_completed_physics(sample).unwrap();
    let snapshot = ticker.pop_pending().unwrap().snapshot;
    assert!(snapshot.move_vector[0] < 0.0, "wire x is left-positive");
    assert!(has(snapshot.flags, PlayerInputFlags::RIGHT));
    assert!(!has(snapshot.flags, PlayerInputFlags::LEFT));
}

#[test]
fn a_repeated_takeoff_from_a_held_jump_signals_start_jumping() {
    let mut physics = settled_controller();
    let jump = MovementInput {
        jumping: true,
        ..MovementInput::default()
    };
    let mut previous = HeldInput::default();
    let mut takeoffs = 0;
    for _ in 0..60 {
        let sample = step(
            &mut physics,
            jump,
            ModeIntent::default(),
            &VersionedFloor(1),
        );
        let flags = input_flags(&sample, previous);
        if sample.processed.jump_initiated {
            takeoffs += 1;
            assert!(has(flags, PlayerInputFlags::START_JUMPING));
        }
        previous = HeldInput::from(&sample);
    }
    assert!(takeoffs >= 2, "a held jump repeats");
}

#[test]
fn rider_correction_replay_does_not_start_a_player_jump() {
    let mut physics = grounded_controller();
    let mut ticker = super::MovementTicker::default();
    ticker.reset(1, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(super::MovementSource::Physics);
    for _ in 0..3 {
        let sample = step(
            &mut physics,
            MovementInput {
                jumping: true,
                ..MovementInput::default()
            },
            ModeIntent {
                ride: Some(RideKind::Boat),
                ..ModeIntent::default()
            },
            &VersionedFloor(1),
        );
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    super::reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        [0.1, 2.620_01, 0.0],
        101,
        true,
        super::PhysicsCorrectionMode::ReplayIfRetained,
        &VersionedFloor(1),
    )
    .unwrap();
    for snapshot in ticker.pending_snapshots() {
        assert_eq!(
            snapshot.flags.bits() & PlayerInputFlags::START_JUMPING.bits(),
            0
        );
        assert_ne!(snapshot.flags.bits() & PlayerInputFlags::JUMPING.bits(), 0);
    }
}

struct PoolFloor(std::cell::Cell<bool>);

impl CollisionWorld for PoolFloor {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        VersionedFloor(1).collision_boxes(query)
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<sim::BlockPhysicsSample, WorldQueryError> {
        let mut sample = VersionedFloor(1).block_physics(block)?;
        if self.0.get() && block[1] >= 1 {
            sample.layers[0].flags = sim::BlockPhysicsFlags::from_bits(
                sample.layers[0].flags.bits() | sim::BlockPhysicsFlags::WATER.bits(),
            )
            .unwrap();
            sample.layers[0].fluid_height_blocks = 1.0;
        }
        Ok(sample)
    }

    fn primary_is_air(
        &self,
        block: [i32; 3],
    ) -> Result<Option<CollisionQuery<bool>>, WorldQueryError> {
        Ok(Some(CollisionQuery {
            value: !self.0.get() && block[1] >= 1,
            identity: self.block_physics(block)?.identity,
        }))
    }
}

#[test]
fn leaving_water_defers_held_ground_jump_until_native_swim_blend_finishes() {
    let world = PoolFloor(std::cell::Cell::new(true));
    let mut physics = grounded_controller();
    let swim = MovementInput {
        forward: 1.0,
        sprinting: true,
        ..MovementInput::default()
    };
    let mut previous = HeldInput::default();
    for _ in 0..12 {
        let sample = step(&mut physics, swim, ModeIntent::default(), &world);
        assert_eq!(sample.processed.mode, MovementMode::Swimming);
        previous = HeldInput::from(&sample);
    }
    assert_eq!(physics.state().unwrap().swim_amount, 1.0);
    world.0.set(false);
    // CurrentSwimAmount precedes SwimTrigger. The first dry tick still advances
    // the previous swimming flag; decay starts on the following tick.
    let stopped = step(
        &mut physics,
        MovementInput::default(),
        ModeIntent::default(),
        &world,
    );
    let stopped_flags = input_flags(&stopped, previous);
    assert!(has(stopped_flags, PlayerInputFlags::STOP_SWIMMING));
    assert_eq!(physics.state().unwrap().swim_amount, 1.0);
    previous = HeldInput::from(&stopped);
    let held_jump = MovementInput {
        jumping: true,
        ..MovementInput::default()
    };
    let mut stop_edges = 1;
    let mut takeoffs = 0;
    for tick in 0..10 {
        let sample = step(&mut physics, held_jump, ModeIntent::default(), &world);
        let flags = input_flags(&sample, previous);
        assert_eq!(sample.processed.mode, MovementMode::Walking);
        assert!(has(flags, PlayerInputFlags::WANT_UP));
        stop_edges += usize::from(has(flags, PlayerInputFlags::STOP_SWIMMING));
        takeoffs += usize::from(has(flags, PlayerInputFlags::START_JUMPING));
        assert_eq!(sample.processed.jump_initiated, tick == 9);
        previous = HeldInput::from(&sample);
    }
    assert_eq!((stop_edges, takeoffs), (1, 1));
}

#[test]
fn in_session_snap_preserves_swimming_blend_and_mode_edges() {
    let world = PoolFloor(std::cell::Cell::new(true));
    let mut physics = grounded_controller();
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 100, physics.network_position().unwrap());
    ticker.set_source(MovementSource::Physics);
    let input = MovementInput {
        forward: 1.0,
        sprinting: true,
        ..MovementInput::default()
    };
    let mut last = None;
    for _ in 0..3 {
        let sample = step(&mut physics, input, ModeIntent::default(), &world);
        ticker.enqueue_completed_physics(sample.clone()).unwrap();
        ticker.pop_pending().unwrap();
        last = Some(sample);
    }
    let last = last.unwrap();
    let blend = physics.state().unwrap().swim_amount;
    assert!(physics.state().unwrap().swim_pose_active);
    reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        last.position,
        last.tick,
        true,
        super::PhysicsCorrectionMode::Snap,
        &world,
    )
    .unwrap();
    assert_eq!(physics.state().unwrap().swim_amount, blend);
    assert!(physics.state().unwrap().swim_pose_active);
    physics.advance_with_context(
        Duration::ZERO,
        input,
        PhysicsSampleContext::default(),
        &world,
    );
    let next = step(&mut physics, input, ModeIntent::default(), &world);
    assert_eq!(next.processed.mode, MovementMode::Swimming);
    assert!(physics.state().unwrap().swim_amount > blend);
    ticker.enqueue_completed_physics(next).unwrap();
    let flags = ticker.pop_pending().unwrap().snapshot.flags;
    assert!(!has(flags, PlayerInputFlags::START_SWIMMING));
    assert!(!has(flags, PlayerInputFlags::STOP_SWIMMING));
}
