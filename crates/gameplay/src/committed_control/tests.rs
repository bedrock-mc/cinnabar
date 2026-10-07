use super::*;
use crate::movement::MovementSource;
use client_world::ResolvedServerPosition;

/// A world that rejects any unintended spatial query during non-replay controls.
struct NoQueries;

impl CollisionWorld for NoQueries {
    /// These fixtures only apply live impulses and unconditional anchor snaps.
    fn collision_boxes(
        &self,
        _: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        panic!("control queried collisions without a retained replay");
    }
}

/// Creates one authorized owner at the initial server anchor.
fn owners() -> (
    MovementTicker,
    LocalPhysicsController,
    LocalMovementEffectTimeline,
    LocalMovementSpeedAuthority,
) {
    let position = [0.5, 2.620_01, 0.5];
    let mut movement = MovementTicker::default();
    movement.reset(7, 100, position);
    movement.set_source(MovementSource::Physics);
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(position, 100, true);
    let mut effects = LocalMovementEffectTimeline::default();
    effects.begin_session(7);
    let mut speed = LocalMovementSpeedAuthority::default();
    speed.begin_session(7, 0);
    (movement, physics, effects, speed)
}

#[test]
fn dimension_observations_bracket_the_authoritative_snap_and_clear_old_speed() {
    let (mut movement, mut physics, mut effects, mut speed) = owners();
    assert!(speed.apply(7, 1, 0, 0.25, None));
    let position = [10.0, 80.0, 20.0];
    let control = CommittedControlEvent::ChangeDimension {
        sequence: 1,
        change: protocol::ChangeDimensionEvent {
            dimension: 1,
            position,
            ..Default::default()
        },
        resolved: ResolvedServerPosition {
            position,
            surface_anchor: None,
        },
    };
    let mut observations = Vec::new();
    let disposition = CommittedGameplayState {
        movement: &mut movement,
        physics: &mut physics,
        effects: &mut effects,
        speed: &mut speed,
        session_generation: 7,
        dimension: 1,
        dimension_transfer_active: true,
    }
    .apply(control, &NoQueries, |observation| {
        observations.push(observation)
    });
    assert_eq!(
        disposition,
        ControlDisposition::Spatial(SpatialReset::Dimension)
    );
    assert!(matches!(observations.as_slice(), [
        ControlObservation::BeforeSpatial(observed),
        ControlObservation::Dimension,
        ControlObservation::Correction { outcome: PhysicsCorrectionOutcome::Snapped { .. }, position: target, .. },
    ] if *observed == control && *target == position));
    assert_eq!(speed.current(), None);
    assert_eq!(physics.network_position(), Some(position));
    assert_eq!(movement.completed_tick(), 100);
    assert!(movement.physics_is_authorized());
    assert!(movement.can_advance_physics_frame());
}

#[test]
fn dimension_destination_and_ready_respawn_preserve_the_global_input_clock() {
    let position = [10.0, 80.0, 20.0];
    let resolved = ResolvedServerPosition {
        position,
        surface_anchor: None,
    };
    let controls = [
        CommittedControlEvent::MovePlayer {
            sequence: 1,
            source_cohort: None,
            movement: protocol::MovePlayerEvent {
                runtime_id: 42,
                position,
                yaw: 0.0,
                pitch: 0.0,
                on_ground: false,
                teleported: true,
                source_tick: 0,
                ..Default::default()
            },
            resolved,
        },
        CommittedControlEvent::Respawn {
            sequence: 2,
            respawn: protocol::RespawnEvent {
                position,
                state: 1,
                runtime_entity_id: 0,
            },
            resolved,
        },
    ];
    for control in controls {
        let (mut movement, mut physics, mut effects, mut speed) = owners();
        let disposition = CommittedGameplayState {
            movement: &mut movement,
            physics: &mut physics,
            effects: &mut effects,
            speed: &mut speed,
            session_generation: 7,
            dimension: 1,
            dimension_transfer_active: true,
        }
        .apply(control, &NoQueries, |_| {});
        assert_eq!(
            disposition,
            ControlDisposition::Spatial(SpatialReset::Correction)
        );
        assert_eq!(movement.completed_tick(), 100);
        assert_eq!(physics.network_position(), Some(position));
    }
}

#[test]
fn motion_is_observed_without_a_frame_reset_and_only_updates_authorized_physics() {
    for authorized in [false, true] {
        let (mut movement, mut physics, mut effects, mut speed) = owners();
        if !authorized {
            movement.set_source(MovementSource::FreeCamera);
        }
        let motion = [0.25, 0.5, -0.125];
        let before = physics.state().unwrap().velocity;
        let mut observations = Vec::new();
        let disposition = CommittedGameplayState {
            movement: &mut movement,
            physics: &mut physics,
            effects: &mut effects,
            speed: &mut speed,
            session_generation: 7,
            dimension: 0,
            dimension_transfer_active: false,
        }
        .apply(
            CommittedControlEvent::LocalActorMotion {
                sequence: 9,
                event: protocol::ActorMotionEvent {
                    actor_runtime_id: 42,
                    motion,
                    tick: 0,
                },
            },
            &NoQueries,
            |observation| observations.push(observation),
        );
        assert_eq!(disposition, ControlDisposition::Handled);
        assert_eq!(observations, [ControlObservation::Knockback { motion }]);
        let expected = if authorized {
            sim::Vec3::new(0.25, 0.5, -0.125)
        } else {
            before
        };
        assert_eq!(physics.state().unwrap().velocity, expected);
    }
}

#[test]
fn a_correction_outside_retained_history_keeps_prediction_and_still_resets_the_frame() {
    let (mut movement, mut physics, mut effects, mut speed) = owners();
    let before = physics.network_position();
    let position = [99.0, 80.0, 99.0];
    let control = CommittedControlEvent::PlayerMovementCorrection {
        sequence: 4,
        correction: protocol::PlayerMovementCorrectionEvent {
            position,
            delta: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            subject: protocol::MovementCorrectionSubject::Player,
            on_ground: true,
            tick: 3,
        },
        resolved: ResolvedServerPosition {
            position,
            surface_anchor: None,
        },
    };
    let mut observations = Vec::new();
    let disposition = CommittedGameplayState {
        movement: &mut movement,
        physics: &mut physics,
        effects: &mut effects,
        speed: &mut speed,
        session_generation: 7,
        dimension: 0,
        dimension_transfer_active: false,
    }
    .apply(control, &NoQueries, |observation| {
        observations.push(observation)
    });
    assert_eq!(
        disposition,
        ControlDisposition::Spatial(SpatialReset::Correction)
    );
    assert_eq!(observations, [ControlObservation::BeforeSpatial(control)]);
    assert_eq!(physics.network_position(), before);
    assert!(movement.physics_is_authorized());
}

#[test]
fn world_clock_and_weather_cycle_controls_stay_with_the_environment_adapter() {
    let (mut movement, mut physics, mut effects, mut speed) = owners();
    let position = physics.network_position();
    for control in [
        CommittedControlEvent::WorldClocks {
            sequence: 1,
            update: protocol::WorldClockUpdateEvent::Sync(protocol::WorldClockState {
                id: protocol::OVERWORLD_CLOCK_ID,
                time: 42,
                paused: false,
            }),
        },
        CommittedControlEvent::WeatherCycle {
            sequence: 2,
            enabled: false,
        },
    ] {
        let disposition = CommittedGameplayState {
            movement: &mut movement,
            physics: &mut physics,
            effects: &mut effects,
            speed: &mut speed,
            session_generation: 7,
            dimension: 0,
            dimension_transfer_active: false,
        }
        .apply(control, &NoQueries, |_| {
            panic!("environment control emitted a spatial observation")
        });
        assert_eq!(disposition, ControlDisposition::Environment);
        assert_eq!(physics.network_position(), position);
        assert!(movement.physics_is_authorized());
    }
}

/// Open air that loads for prediction ticks.
struct OpenAir;

impl CollisionWorld for OpenAir {
    fn collision_boxes(
        &self,
        _: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        Ok(sim::CollisionQuery::synthetic(Vec::new()))
    }
}

/// Terrain that is no longer loaded, so a timeline replay cannot run.
struct Unloaded;

impl CollisionWorld for Unloaded {
    fn collision_boxes(
        &self,
        _: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        Err(sim::WorldQueryError::InvalidBounds)
    }

    fn block_physics(&self, _: [i32; 3]) -> Result<sim::BlockPhysicsSample, sim::WorldQueryError> {
        Err(sim::WorldQueryError::InvalidBounds)
    }
}

/// A delayed boost whose replay fails keeps its whole span for live ticks.
#[test]
fn a_failed_boost_replay_keeps_the_unapplied_span_live() {
    use crate::movement::MovementEffectSource;
    let (mut movement, mut physics, mut effects, mut speed) = owners();
    for _ in 0..4 {
        let frame = physics.advance(
            std::time::Duration::from_millis(50),
            sim::MovementInput::default(),
            &OpenAir,
        );
        assert_eq!(frame.completed_ticks, 1, "{:?}", frame.blocked);
    }
    let disposition = CommittedGameplayState {
        movement: &mut movement,
        physics: &mut physics,
        effects: &mut effects,
        speed: &mut speed,
        session_generation: 7,
        dimension: 0,
        dimension_transfer_active: false,
    }
    .apply(
        CommittedControlEvent::LocalMovementBoost {
            sequence: 9,
            event: protocol::MovementEffectEvent {
                actor_runtime_id: 42,
                kind: protocol::MovementEffectKind::GlideBoost,
                duration_ticks: 2,
                tick: 102,
            },
        },
        &Unloaded,
        |_| {},
    );
    assert_eq!(disposition, ControlDisposition::Handled);
    for _ in 0..2 {
        assert!(effects.snapshot().glide_boost);
        effects.commit_successful_tick();
    }
    assert!(!effects.snapshot().glide_boost);
}
