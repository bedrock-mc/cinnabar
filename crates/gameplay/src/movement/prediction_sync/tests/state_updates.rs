use super::*;
use crate::committed_control::CommittedGameplayState;
use std::sync::Arc;

/// Commits sparse dimensions through the same actor path as server metadata.
fn write_dimensions(
    world: &mut client_world::WorldAuthority,
    sequence: u64,
    fields: &[(ActorDataKey, f32)],
) {
    world
        .apply_ordered_event(
            protocol::WorldEvent::Actor(protocol::ActorEvent::Metadata(
                protocol::ActorMetadataUpdateEvent {
                    dimension: 0,
                    runtime_id: 7,
                    metadata: fields
                        .iter()
                        .map(|(key, value)| protocol::ActorMetadata {
                            key: key.wire_value(),
                            value: ActorMetadataValue::Float(*value),
                        })
                        .collect(),
                    properties: Arc::from([]),
                    tick: 0,
                },
            )),
            Some(sequence),
        )
        .unwrap();
}

/// Reads a due sync without consuming it, using the actual stored local actor.
fn reported_box(
    physics: &mut LocalPhysicsController,
    world: &client_world::WorldAuthority,
) -> [f32; 3] {
    let mut size = None;
    send_movement_prediction_sync(
        physics,
        &LocalActor(world.actor(7).unwrap().clone()),
        None,
        None,
        None,
        |packet| {
            size = protocol::test_support::packets::prediction_sync_bounding_box(&packet);
            false
        },
    );
    size.expect("due correction sync")
}

/// A width-only update preserves the last local height even if its width is unchanged.
#[test]
fn sync_sparse_width_preserves_pose_owned_height() {
    let height = sim::MovementMode::Walking.hitbox_height(true, sim::SneakProfile::Current) as f32;
    for scale in [1.0, 2.0] {
        for previous_height in [None, Some(1.5)] {
            let mut world = local_actor_authority();
            let mut physics = due_physics();
            if let Some(previous_height) = previous_height {
                write_dimensions(&mut world, 2, &[(ActorDataKey::Height, previous_height)]);
                assert_eq!(
                    reported_box(&mut physics, &world),
                    [1.0, 0.6, previous_height]
                );
            }
            physics.advance(
                simulation_clock::TICK_DURATION,
                sim::MovementInput {
                    sneaking: true,
                    ..Default::default()
                },
                &OpenWorld,
            );
            assert_eq!(reported_box(&mut physics, &world), [1.0, 0.6, height]);
            write_dimensions(
                &mut world,
                3,
                &[(ActorDataKey::Width, 0.6), (ActorDataKey::Scale, scale)],
            );
            assert_eq!(reported_box(&mut physics, &world), [scale, 0.6, height]);
            physics.advance(
                simulation_clock::TICK_DURATION,
                sim::MovementInput {
                    sneaking: true,
                    ..Default::default()
                },
                &OpenWorld,
            );
            assert_eq!(reported_box(&mut physics, &world), [scale, 0.6, height]);
        }
    }
}

/// A height-only update preserves width after a local resize replaced older metadata.
#[test]
fn sync_sparse_height_preserves_pose_owned_width() {
    let height = sim::MovementMode::Walking.hitbox_height(true, sim::SneakProfile::Current) as f32;
    let mut world = local_actor_authority();
    let mut physics = due_physics();
    write_dimensions(&mut world, 2, &[(ActorDataKey::Width, 0.8)]);
    assert_eq!(reported_box(&mut physics, &world), [1.0, 0.8, 1.8]);
    physics.advance(
        simulation_clock::TICK_DURATION,
        sim::MovementInput {
            sneaking: true,
            ..Default::default()
        },
        &OpenWorld,
    );
    assert_eq!(reported_box(&mut physics, &world), [1.0, 0.6, height]);
    write_dimensions(&mut world, 3, &[(ActorDataKey::Height, height)]);
    assert_eq!(reported_box(&mut physics, &world), [1.0, 0.6, height]);
}

/// Separate arrivals retain both supplied axes, including before their first local tick.
#[test]
fn sync_accumulates_sparse_dimensions_before_a_tick() {
    let mut world = local_actor_authority();
    let mut physics = due_physics();
    write_dimensions(&mut world, 2, &[(ActorDataKey::Width, 0.8)]);
    write_dimensions(&mut world, 3, &[(ActorDataKey::Height, 1.5)]);
    assert_eq!(reported_box(&mut physics, &world), [1.0, 0.8, 1.5]);
    physics.advance_with_context(
        simulation_clock::TICK_DURATION,
        sim::MovementInput::default(),
        crate::movement::PhysicsSampleContext {
            mode_intent: crate::movement::ModeIntent {
                explicit_dimensions: world.actor(7).unwrap().explicit_dimensions,
                ..Default::default()
            },
            ..Default::default()
        },
        &OpenWorld,
    );
    write_dimensions(&mut world, 4, &[(ActorDataKey::Scale, 2.0)]);
    assert_eq!(reported_box(&mut physics, &world), [2.0, 0.8, 1.5]);
}

/// Accepted replacement spawns cannot inherit the previous actor's reporting override.
#[test]
fn sync_actor_replacement_replaces_reported_speed() {
    let attribute = |current, default| protocol::ActorAttribute {
        name: Arc::from(Attribute::Movement.wire_name()),
        min: 0.0,
        max: 100.0,
        current,
        default,
        modifiers: Arc::from([]),
    };
    for attributes in [
        vec![attribute(0.2, Some(0.2))],
        vec![attribute(0.3, None)],
        Vec::new(),
    ] {
        let mut world = local_actor_authority();
        let mut physics = due_physics();
        let mut movement = crate::movement::MovementTicker::default();
        let mut effects = crate::movement::LocalMovementEffectTimeline::default();
        let mut speed = crate::movement::LocalMovementSpeedAuthority::default();
        speed.begin_session(1, 0);
        world.take_committed_controls();
        let expected = attributes.last().map_or(0.0, |attribute| attribute.current);
        let predictable = attributes
            .last()
            .and_then(client_world::MovementSpeedAttribute::from_attribute)
            .is_some();
        let replacement =
            protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(local_actor_spawn(attributes)));
        let initial = protocol::WorldEvent::Actor(protocol::ActorEvent::Attributes(
            protocol::ActorAttributesUpdateEvent {
                dimension: 0,
                runtime_id: 7,
                attributes: Arc::from([attribute(0.1, Some(0.1))]),
                tick: 0,
            },
        ));
        for (sequence, event) in [(2, initial), (3, replacement)] {
            world.apply_ordered_event(event, Some(sequence)).unwrap();
            for control in world.take_committed_controls() {
                CommittedGameplayState {
                    movement: &mut movement,
                    physics: &mut physics,
                    effects: &mut effects,
                    speed: &mut speed,
                    session_generation: 1,
                    dimension: 0,
                    dimension_transfer_active: false,
                }
                .apply(control, &OpenWorld, |_| {});
            }
        }
        assert_eq!(world.actor(7).unwrap().spawn_revision, 3);
        for sprinting in [false, true] {
            speed.set_sprinting(sprinting);
            let expected_speed = if sprinting && predictable {
                expected * sim::SPRINT_SPEED_MULTIPLIER as f32
            } else {
                expected
            };
            send_movement_prediction_sync(
                &mut physics,
                &LocalActor(world.actor(7).unwrap().clone()),
                None,
                speed.reported_current(),
                None,
                |packet| {
                    assert_eq!(
                        protocol::test_support::packets::prediction_sync_attributes(&packet)
                            .unwrap()[0],
                        expected_speed
                    );
                    false
                },
            );
        }
    }
}
