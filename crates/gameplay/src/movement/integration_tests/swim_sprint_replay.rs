use super::*;

/// Water three blocks deep above the floor for `z < 3`; dry floor beyond.
struct SidePool;

impl SidePool {
    fn water(block: [i32; 3]) -> bool {
        (1..4).contains(&block[1]) && block[2] < 3
    }
}

impl CollisionWorld for SidePool {
    fn registry_identity(&self) -> CollisionRegistryIdentity {
        VersionedFloor(1).registry_identity()
    }

    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        VersionedFloor(1).collision_boxes(query)
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<sim::BlockPhysicsSample, WorldQueryError> {
        let mut sample = VersionedFloor(1).block_physics(block)?;
        if Self::water(block) {
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
            value: block[1] >= 1 && !Self::water(block),
            identity: self.block_physics(block)?.identity,
        }))
    }
}

const SWIM_TICKS: usize = 18;

/// Sprints into a held-jump swim start in water, which ends the sprint, then swims
/// unsprinted out through the side. Returns the swim-start tick and network
/// position; `on_time` applies a server sprint write right after it.
fn swim_out_of_the_side(
    physics: &mut LocalPhysicsController,
    ticker: &mut MovementTicker,
    on_time: Option<client_world::MovementFlagUpdate>,
) -> (u64, [f32; 3]) {
    physics.reanchor_network_position([0.5, 2.620_01, 2.4], 100, true);
    ticker.reset(7, 100, [0.5, 2.620_01, 2.4]);
    ticker.set_source(MovementSource::Physics);
    let mut tick = |physics: &mut LocalPhysicsController, input: MovementInput| {
        let frame = physics.advance(Duration::from_millis(50), input, &SidePool);
        assert!(frame.blocked.is_none(), "blocked: {:?}", frame.blocked);
        let sample = frame.samples[0].clone();
        ticker.enqueue_completed_physics(sample.clone()).unwrap();
        sample
    };
    let swim = MovementInput {
        forward: 1.0,
        movement_speed: Some(0.1),
        ..MovementInput::default()
    };
    tick(
        physics,
        MovementInput {
            jumping: true,
            ..MovementInput::default()
        },
    );
    let start = tick(
        physics,
        MovementInput {
            jumping: true,
            sprinting: true,
            ..swim
        },
    );
    assert_eq!(start.processed.mode, sim::MovementMode::Swimming);
    assert!(!start.processed.sprinting);
    if let Some(flags) = on_time {
        assert_eq!(physics.apply_server_movement_flags(start.tick, flags), None);
    }
    let modes: Vec<_> = (0..SWIM_TICKS)
        .map(|_| tick(physics, swim).processed.mode)
        .collect();
    assert_eq!(
        modes.last(),
        Some(&sim::MovementMode::Walking),
        "the swim leaves the water"
    );
    (start.tick, start.position)
}

/// A delayed server sprint write cannot outlast a liquid stop during replay.
/// Natively the flag is applied once at its tick: the swim keeps it in water, the
/// dry swimming tick stops it, and without a sprint control it stays stopped.
/// Replay must match the same write delivered on time, including walking speed,
/// and must not leave the stopped sprint pending for the live controls.
#[test]
fn a_replayed_server_sprint_ends_at_a_liquid_stop() {
    let sprint = client_world::MovementFlagUpdate {
        sprinting: Some(true),
        ..Default::default()
    };
    let mut on_time = LocalPhysicsController::default();
    swim_out_of_the_side(&mut on_time, &mut MovementTicker::default(), Some(sprint));

    let mut delayed = LocalPhysicsController::default();
    let mut ticker = MovementTicker::default();
    let (start, start_position) = swim_out_of_the_side(&mut delayed, &mut ticker, None);
    assert_eq!(delayed.latest_sneak_sprint(), Some((false, false)));
    let rewind = delayed.apply_server_movement_flags(start, sprint).unwrap();
    reconcile_timeline_rewind(&mut ticker, &mut delayed, rewind, &SidePool).unwrap();
    assert_eq!(delayed.state(), on_time.state());
    assert_eq!(delayed.latest_sneak_sprint(), Some((false, false)));
    assert_eq!(delayed.prediction_sync_posture(), Some((false, false)));
    assert_eq!(
        delayed
            .take_server_control_flags()
            .and_then(|flags| flags.sprinting),
        None
    );
    // The write stays retained: replaying again from before it reproduces it.
    reconcile_candidate_physics_correction(
        &mut ticker,
        &mut delayed,
        start_position,
        start,
        false,
        PhysicsCorrectionMode::ReplayIfRetained,
        &SidePool,
    )
    .unwrap();
    // The network anchor round-trips the feet through f32, so compare motion.
    let (replayed, expected) = (delayed.state().unwrap(), on_time.state().unwrap());
    assert_eq!(replayed.position, expected.position);
    assert_eq!(replayed.velocity, expected.velocity);
    assert_eq!(delayed.latest_sneak_sprint(), Some((false, false)));
}

/// A newer live sprint write survives the replay of an older delayed write that
/// a liquid stop ended, in either arrival order.
#[test]
fn a_newer_live_sprint_write_survives_an_older_stopped_replay() {
    let sprint = client_world::MovementFlagUpdate {
        sprinting: Some(true),
        ..Default::default()
    };
    for live_first in [false, true] {
        let mut physics = LocalPhysicsController::default();
        let mut ticker = MovementTicker::default();
        let (start, _) = swim_out_of_the_side(&mut physics, &mut ticker, None);
        let latest = physics.state().unwrap().tick;
        if live_first {
            assert_eq!(physics.apply_server_movement_flags(latest, sprint), None);
        }
        let rewind = physics.apply_server_movement_flags(start, sprint).unwrap();
        if !live_first {
            assert_eq!(physics.apply_server_movement_flags(latest, sprint), None);
        }
        reconcile_timeline_rewind(&mut ticker, &mut physics, rewind, &SidePool).unwrap();
        assert_eq!(
            physics
                .take_server_control_flags()
                .and_then(|flags| flags.sprinting),
            Some(true),
            "live first {live_first}"
        );
        // The controls adopt that sprint as their request; the next tick keeps it
        // without a held sprint control.
        let adopted = MovementInput {
            forward: 1.0,
            sprinting: true,
            ..MovementInput::default()
        };
        let frame = physics.advance(Duration::from_millis(50), adopted, &SidePool);
        assert!(
            frame.samples[0].processed.sprinting,
            "live first {live_first}"
        );
    }
}

/// Sampling controls without a fixed tick keeps newer live authority above delayed replays.
#[test]
fn live_sprint_ownership_survives_zero_tick_frames_before_a_stopped_replay() {
    let mut physics = LocalPhysicsController::default();
    let mut ticker = MovementTicker::default();
    let (start, _) = swim_out_of_the_side(&mut physics, &mut ticker, None);
    let latest = ticker.completed_tick();
    let sprint = client_world::MovementFlagUpdate {
        sprinting: Some(true),
        ..Default::default()
    };
    physics.apply_server_movement_flags(latest, sprint);
    let mut locals = super::super::LocomotionState::default();
    let mut speed = super::super::LocalMovementSpeedAuthority::default();
    let mut effects = super::super::LocalMovementEffectTimeline::default();
    speed.begin_session(7, 0);
    for _ in 0..2 {
        let mut frame = speed_frame(Default::default());
        frame.delta = Duration::ZERO;
        assert!(locals.advance(
            frame,
            &mut physics,
            &mut ticker,
            &mut effects,
            &mut speed,
            &SidePool,
        ));
    }
    let rewind = physics.apply_server_movement_flags(start, sprint).unwrap();
    for _ in 0..2 {
        reconcile_timeline_rewind(&mut ticker, &mut physics, rewind, &SidePool).unwrap();
        assert_eq!(physics.prediction_sync_posture(), Some((false, true)));
    }
    assert_eq!(ticker.completed_tick(), latest);
    assert!(locals.advance(
        speed_frame(Default::default()),
        &mut physics,
        &mut ticker,
        &mut effects,
        &mut speed,
        &SidePool,
    ));
    assert!(physics.sample_at(latest + 1).unwrap().processed.sprinting);
}

/// A replay result delivered to controls does not author a second server write.
#[test]
fn replayed_delayed_handoff_is_not_a_second_server_sprint() {
    let mut physics = LocalPhysicsController::default();
    let mut ticker = MovementTicker::default();
    let position = [0.5, 2.620_01, 5.0];
    physics.reanchor_network_position(position, 100, true);
    ticker.reset(7, 100, position);
    ticker.set_source(MovementSource::Physics);
    for jumping in [true, false, false] {
        let frame = physics.advance(
            Duration::from_millis(50),
            MovementInput {
                forward: 1.0,
                jumping,
                ..Default::default()
            },
            &SidePool,
        );
        ticker
            .enqueue_completed_physics(frame.samples[0].clone())
            .unwrap();
    }
    let anchor = 101;
    let rewind = physics
        .apply_server_movement_flags(anchor, flags(|flags| flags.sprinting = Some(true)))
        .unwrap();
    reconcile_timeline_rewind(&mut ticker, &mut physics, rewind, &SidePool).unwrap();
    assert_eq!(physics.latest_sneak_sprint(), Some((false, true)));
    let mut locals = super::super::LocomotionState::default();
    let mut speed = super::super::LocalMovementSpeedAuthority::default();
    let mut effects = super::super::LocalMovementEffectTimeline::default();
    speed.begin_session(7, 0);
    let mut frame = speed_frame(Default::default());
    frame.yaw = 0.0;
    assert!(locals.advance(
        frame,
        &mut physics,
        &mut ticker,
        &mut effects,
        &mut speed,
        &SidePool
    ));
    let latest = ticker.completed_tick();
    for _ in 0..2 {
        super::super::reconcile_physics_anchor(
            &mut ticker,
            &mut physics,
            super::super::PhysicsAnchor {
                network_position: [0.5, 2.620_01, 2.4],
                velocity: Some([0.0; 3]),
                tick: anchor,
                on_ground: true,
            },
            PhysicsCorrectionMode::ReplayIfRetained,
            &SidePool,
        )
        .unwrap();
        assert_eq!(
            physics
                .sample_at(anchor + 1)
                .unwrap()
                .input
                .server_sprinting,
            Some(true)
        );
        assert!(!physics.sample_at(latest).unwrap().processed.sprinting);
    }
}

/// A failed replay keeps the accepted flag visible until a live tick can record it.
#[test]
fn unrecorded_failed_replayed_sprint_still_overlays_the_completed_posture() {
    for position_correction in [false, true] {
        let (mut physics, mut ticker) = walked_physics(3);
        let rewind = physics
            .apply_server_movement_flags(101, flags(|flags| flags.sprinting = Some(true)))
            .unwrap();
        assert!(
            reconcile_timeline_rewind(&mut ticker, &mut physics, rewind, &VersionedFloor(2))
                .is_err()
        );
        assert_eq!(physics.prediction_sync_posture(), Some((false, true)));
        if position_correction {
            let sample = physics.sample_at(103).unwrap().clone();
            reconcile_candidate_physics_correction(
                &mut ticker,
                &mut physics,
                sample.position,
                sample.tick,
                sample.grounded_after_tick,
                PhysicsCorrectionMode::ReplayIfRetained,
                &VersionedFloor(1),
            )
            .unwrap();
        } else {
            let rewind = physics.queue_server_motion([0.1, 0.0, 0.0], 102).unwrap();
            reconcile_timeline_rewind(&mut ticker, &mut physics, rewind, &VersionedFloor(1))
                .unwrap();
        }
        assert_eq!(physics.latest_sneak_sprint(), Some((false, true)));
        assert_eq!(physics.prediction_sync_posture(), Some((false, true)));
    }
    let (mut physics, mut ticker) = walked_physics(1);
    let jump = physics.advance(
        Duration::from_millis(50),
        MovementInput {
            jumping: true,
            ..forward_physics_input()
        },
        &VersionedFloor(1),
    );
    ticker
        .enqueue_completed_physics(jump.samples[0].clone())
        .unwrap();
    ticker.hold_release_after_tick(102, Vec::new());
    ticker.request_riptide(&mut physics, 1, [0.0; 2]);
    let frame = physics.advance(
        Duration::from_millis(100),
        forward_physics_input(),
        &VersionedFloor(1),
    );
    for sample in frame.samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    assert!(physics.sample_at(103).unwrap().processed.spin_started);
    let rewind = physics
        .apply_server_movement_flags(101, flags(|flags| flags.sprinting = Some(true)))
        .unwrap();
    assert!(
        reconcile_timeline_rewind(&mut ticker, &mut physics, rewind, &VersionedFloor(2)).is_err()
    );
    let rewind = physics.queue_server_motion([0.1, 0.0, 0.0], 103).unwrap();
    reconcile_timeline_rewind(&mut ticker, &mut physics, rewind, &VersionedFloor(1)).unwrap();
    for sample in ticker.pending_snapshots() {
        assert_eq!(
            sample.position,
            physics.sample_at(sample.tick).unwrap().position,
            "tick {}",
            sample.tick
        );
    }
}

/// The first effect remains an operation even when earlier ticks had no attribute snapshot.
#[test]
fn unrecorded_first_speed_effect_uses_replayed_modifier_state() {
    for delayed in [false, true] {
        let position = [0.5, 2.620_01, 5.0];
        let mut physics = LocalPhysicsController::default();
        let mut ticker = MovementTicker::default();
        physics.reanchor_network_position(position, 100, true);
        ticker.reset(7, 100, position);
        ticker.set_source(MovementSource::Physics);
        let mut locals = super::super::LocomotionState::default();
        let mut speed = super::super::LocalMovementSpeedAuthority::default();
        let mut effects = super::super::LocalMovementEffectTimeline::default();
        speed.begin_session(7, 0);
        let correct = |physics: &mut LocalPhysicsController, ticker: &mut MovementTicker| {
            super::super::reconcile_physics_anchor(
                ticker,
                physics,
                super::super::PhysicsAnchor {
                    network_position: [0.5, 2.620_01, 2.4],
                    velocity: Some([0.0; 3]),
                    tick: 101,
                    on_ground: true,
                },
                PhysicsCorrectionMode::ReplayIfRetained,
                &SidePool,
            )
            .unwrap();
        };
        for index in 0..4 {
            if index == 3 {
                speed.apply_effect(
                    7,
                    1,
                    protocol::ActorEffectEvent {
                        dimension: 0,
                        actor_runtime_id: 42,
                        action: protocol::ActorEffectAction::Add,
                        effect_id: protocol_ids::Effect::Speed.wire_value(),
                        amplifier: 0,
                        particles: false,
                        ambient: false,
                        duration_ticks: 600,
                        tick: 0,
                    },
                );
            }
            let mut frame = speed_frame(semantic_input::ActionPhase {
                held: index == 0,
                ..Default::default()
            });
            frame.yaw = 0.0;
            frame.jump.held = index == 0;
            assert!(locals.advance(
                frame,
                &mut physics,
                &mut ticker,
                &mut effects,
                &mut speed,
                &SidePool
            ));
            if index == 0 && !delayed {
                correct(&mut physics, &mut ticker);
            }
        }
        if delayed {
            correct(&mut physics, &mut ticker);
        }
        let mut frame = speed_frame(Default::default());
        frame.delta = Duration::ZERO;
        assert!(locals.advance(
            frame,
            &mut physics,
            &mut ticker,
            &mut effects,
            &mut speed,
            &SidePool
        ));
        assert_eq!(physics.latest_sneak_sprint(), Some((false, false)));
        assert_eq!(
            speed.current(),
            Some(f64::from(0.1_f32 * 1.2)),
            "delayed {delayed}"
        );
        let mut frame = speed_frame(Default::default());
        frame.yaw = 0.0;
        assert!(locals.advance(
            frame,
            &mut physics,
            &mut ticker,
            &mut effects,
            &mut speed,
            &SidePool
        ));
        assert_eq!(physics.latest_sneak_sprint(), Some((false, false)));
        assert_eq!(speed.current(), Some(f64::from(0.1_f32 * 1.2)));
    }
}

/// A later live write applies in arrival order, even over a pending write stamped
/// in the future.
#[test]
fn a_later_live_sprint_stop_replaces_a_future_stamped_start() {
    let (mut physics, _) = walked_physics(2);
    let latest = physics.state().unwrap().tick;
    let flag = |sprinting| client_world::MovementFlagUpdate {
        sprinting: Some(sprinting),
        ..Default::default()
    };
    assert_eq!(
        physics.apply_server_movement_flags(latest + 5, flag(true)),
        None
    );
    assert_eq!(physics.apply_server_movement_flags(0, flag(false)), None);
    assert_eq!(
        physics
            .take_server_control_flags()
            .and_then(|flags| flags.sprinting),
        Some(false)
    );
}

/// The latest write applied to a tick decides it, as native applies corrections in
/// arrival order: an on-time server flag adopted on a tick does not mask a later
/// delayed write for the same tick, in either direction.
#[test]
fn a_later_sprint_write_replaces_an_earlier_tick_flag() {
    for (on_time, delayed) in [(false, true), (true, false)] {
        let (mut physics, mut ticker) = walked_physics(0);
        let mut ticks = Vec::new();
        for index in 0..4 {
            // The controls adopt an on-time sprint as their request.
            let input = MovementInput {
                sprinting: on_time && index >= 1,
                ..forward_physics_input()
            };
            let frame = physics.advance_with_context(
                Duration::from_millis(50),
                input,
                PhysicsSampleContext {
                    input: crate::movement::TickInput {
                        server_sprinting: (index == 1).then_some(on_time),
                        ..Default::default()
                    },
                    ..PhysicsSampleContext::default()
                },
                &VersionedFloor(1),
            );
            let sample = frame.samples[0].clone();
            ticker.enqueue_completed_physics(sample.clone()).unwrap();
            ticks.push(sample.tick);
        }
        assert_eq!(physics.latest_sneak_sprint(), Some((false, on_time)));
        let write = client_world::MovementFlagUpdate {
            sprinting: Some(delayed),
            ..Default::default()
        };
        let rewind = physics
            .apply_server_movement_flags(ticks[0], write)
            .unwrap();
        reconcile_timeline_rewind(&mut ticker, &mut physics, rewind, &VersionedFloor(1)).unwrap();
        assert_eq!(
            physics.latest_sneak_sprint(),
            Some((false, delayed)),
            "on time {on_time}"
        );
    }
}

/// An ordinary input-rejection stop also ends a replayed server sprint: walking
/// with too little forward input keeps the native stop request after the flag.
#[test]
fn a_replayed_server_sprint_ends_at_an_input_stop() {
    let (mut physics, mut ticker) = walked_physics(0);
    let slow = MovementInput {
        forward: 0.5,
        ..MovementInput::default()
    };
    let mut ticks = Vec::new();
    for _ in 0..4 {
        let frame = physics.advance(Duration::from_millis(50), slow, &VersionedFloor(1));
        let sample = frame.samples[0].clone();
        ticker.enqueue_completed_physics(sample.clone()).unwrap();
        ticks.push(sample.tick);
    }
    let sprint = client_world::MovementFlagUpdate {
        sprinting: Some(true),
        ..Default::default()
    };
    let rewind = physics
        .apply_server_movement_flags(ticks[0], sprint)
        .unwrap();
    reconcile_timeline_rewind(&mut ticker, &mut physics, rewind, &VersionedFloor(1)).unwrap();
    assert_eq!(physics.latest_sneak_sprint(), Some((false, false)));
    // The server set the flag before the stopped tick, so that tick sends the stop.
    let stopped = ticker
        .pending_samples()
        .into_iter()
        .find(|sample| sample.snapshot.tick == ticks[1])
        .unwrap();
    assert_ne!(
        stopped.snapshot.flags.bits() & PlayerInputFlags::STOP_SPRINTING.bits(),
        0
    );
    assert_eq!(
        physics
            .take_server_control_flags()
            .and_then(|flags| flags.sprinting),
        None
    );
}
