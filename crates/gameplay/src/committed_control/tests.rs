use super::*;
use crate::movement::MovementSource;
use crate::test_support::movement_speed_attribute as attribute;
use client_world::ResolvedServerPosition;

mod local_actor_movement;

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
    assert!(speed.apply(7, 1, 0, attribute(0.25, 0.25, None)));
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
            vehicle_angular_velocity: None,
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

/// A delayed boost whose replay fails stays in the boost timeline for later replays.
#[test]
fn a_failed_boost_replay_keeps_the_boost_for_later_replays() {
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
    let glide = crate::movement::MovementBoost::Glide;
    let boosted: Vec<u64> = (100..110)
        .filter(|tick| physics.boosted(glide, *tick))
        .collect();
    assert_eq!(boosted, [103, 104]);
    assert_eq!(
        physics.record_server_boost(glide, 102, crate::movement::BoostSpan::Ticks(2)),
        Some(102),
        "the unreplayed ticks still differ from the timeline"
    );
}

/// Open air that counts collision queries, so tests can tell how often history replays.
struct CountingAir(std::cell::Cell<usize>);

impl CollisionWorld for CountingAir {
    fn collision_boxes(
        &self,
        _: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        self.0.set(self.0.get() + 1);
        Ok(sim::CollisionQuery::synthetic(Vec::new()))
    }
}

/// A delayed update that changes movement and liquid speeds replays history once.
#[test]
fn a_combined_delayed_speed_update_replays_once() {
    let replay_queries = |underwater: Option<f64>| {
        let (mut movement, mut physics, mut effects, mut speed) = owners();
        for _ in 0..4 {
            let frame = physics.advance(
                std::time::Duration::from_millis(50),
                sim::MovementInput::default(),
                &OpenAir,
            );
            assert_eq!(frame.completed_ticks, 1, "{:?}", frame.blocked);
        }
        let world = CountingAir(std::cell::Cell::new(0));
        CommittedGameplayState {
            movement: &mut movement,
            physics: &mut physics,
            effects: &mut effects,
            speed: &mut speed,
            session_generation: 7,
            dimension: 0,
            dimension_transfer_active: false,
        }
        .apply(
            CommittedControlEvent::LocalMovementSpeed {
                sequence: 9,
                dimension: 0,
                movement: Some(attribute(0.2, 0.2, None)),
                underwater,
                lava: None,
                air_drag_modifier: None,
                tick: 102,
            },
            &world,
            |_| {},
        );
        world.0.get()
    };
    let movement_only = replay_queries(None);
    assert!(movement_only > 0, "the delayed movement speed replays");
    assert_eq!(replay_queries(Some(0.05)), movement_only);
}

/// A Speed amplifier update refreshes the effective attribute without a sprint edge.
#[test]
fn speed_effect_updates_refresh_current_while_sprint_is_held() {
    use protocol::{ActorAttribute, ActorAttributeModifier, ActorEffectAction, ActorEffectEvent};
    use std::sync::Arc;

    let (mut movement, mut physics, mut effects, mut speed) = owners();
    let sprint_factor = sim::SPRINT_SPEED_MULTIPLIER as f32;
    let packet = ActorAttribute {
        name: Arc::from(protocol_ids::Attribute::Movement.wire_name()),
        min: 0.0,
        max: f32::MAX,
        current: 0.1 * 1.6 * sprint_factor,
        default: Some(0.1),
        modifiers: Arc::from([
            ActorAttributeModifier {
                id: Arc::from(protocol_ids::SPEED_EFFECT_MODIFIER_ID),
                name: Arc::from("speed"),
                amount: 0.6,
                operation: 2,
                operand: 2,
                serializable: false,
            },
            ActorAttributeModifier {
                id: Arc::from(protocol_ids::SPRINT_SPEED_MODIFIER_ID),
                name: Arc::from("sprint"),
                amount: sprint_factor - 1.0,
                operation: 2,
                operand: 2,
                serializable: false,
            },
        ]),
    };
    assert!(speed.apply(
        7,
        1,
        0,
        client_world::MovementSpeedAttribute::from_attribute(&packet).unwrap()
    ));
    speed.adopt_server_sprinting(Some(true));
    for (sequence, amplifier, factor) in [(2, 1, 1.4), (3, 3, 1.8), (4, 1, 1.8)] {
        let event = ActorEffectEvent {
            dimension: 0,
            actor_runtime_id: 42,
            action: ActorEffectAction::Update,
            effect_id: protocol_ids::Effect::Speed.wire_value(),
            amplifier,
            particles: false,
            ambient: false,
            duration_ticks: 600,
            tick: 100,
        };
        CommittedGameplayState {
            movement: &mut movement,
            physics: &mut physics,
            effects: &mut effects,
            speed: &mut speed,
            session_generation: 7,
            dimension: 0,
            dimension_transfer_active: false,
        }
        .apply(
            CommittedControlEvent::LocalMovementEffect { sequence, event },
            &NoQueries,
            |_| {},
        );
        speed.set_sprinting(true);
        let expected = 0.1 * factor * sprint_factor;
        assert!(
            (speed.current().unwrap() as f32 - expected).abs() < 1e-7,
            "amplifier {amplifier}: {:?}, expected {expected}",
            speed.current()
        );
    }
}

/// A vehicle correction that snaps the rider resets presentation like any correction; one
/// already applied changes nothing.
#[test]
fn an_applied_vehicle_correction_resets_presentation() {
    let (mut movement, mut physics, mut effects, mut speed) = owners();
    physics.steer_completed(
        crate::movement::vehicle::SteeredVehicle {
            runtime_id: 7,
            unique_id: -7,
            body: sim::BoatBody {
                width: 1.4,
                height: 0.455,
                height_offset: 0.375,
            },
            position: [0.5, 2.0, 0.5],
            yaw: 0.0,
            velocity: [0.0; 3],
            seat: [0.0, -0.2, 0.0],
            revision: 0,
        },
        100,
    );
    let control = CommittedControlEvent::VehicleMovementCorrection {
        sequence: 1,
        correction: protocol::PlayerMovementCorrectionEvent {
            position: [4.5, 2.0, 0.5],
            delta: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            subject: protocol::MovementCorrectionSubject::Vehicle,
            on_ground: false,
            tick: 101,
            vehicle_angular_velocity: None,
        },
        vehicle_runtime_id: 7,
    };
    let mut apply = || {
        CommittedGameplayState {
            movement: &mut movement,
            physics: &mut physics,
            effects: &mut effects,
            speed: &mut speed,
            session_generation: 7,
            dimension: 0,
            dimension_transfer_active: false,
        }
        .apply(control, &NoQueries, |_| {})
    };
    assert_eq!(
        apply(),
        ControlDisposition::Spatial(SpatialReset::Correction)
    );
    assert_eq!(apply(), ControlDisposition::Handled);
}
