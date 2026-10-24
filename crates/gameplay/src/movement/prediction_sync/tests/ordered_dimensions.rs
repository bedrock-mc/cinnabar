use super::state_updates::{apply_controls, reported_box, write_dimensions};
use super::*;
use std::sync::Arc;

/// Reports a sparse width followed by accepted sneak, optionally replaying their tick.
fn width_then_sneak(observe_width: bool, replay: Option<bool>) -> [f32; 3] {
    let mut world = local_actor_authority();
    let mut physics = due_physics();
    apply_controls(&mut world, &mut physics);
    let anchor = physics
        .advance(
            simulation_clock::TICK_DURATION,
            sim::MovementInput::default(),
            &OpenWorld,
        )
        .samples
        .remove(0);
    if replay == Some(false) {
        physics.advance(
            simulation_clock::TICK_DURATION,
            sim::MovementInput::default(),
            &OpenWorld,
        );
    }
    write_dimensions(&mut world, &mut physics, 2, &[(ActorDataKey::Width, 0.8)]);
    if observe_width {
        assert_eq!(
            reported_box(&mut physics, &world),
            [1.0, 0.8, sim::PLAYER_HEIGHT as f32]
        );
    }
    world
        .apply_ordered_event(
            protocol::WorldEvent::Actor(protocol::ActorEvent::Metadata(
                protocol::ActorMetadataUpdateEvent {
                    dimension: 0,
                    runtime_id: 7,
                    metadata: Arc::from([protocol::ActorMetadata {
                        key: ActorDataKey::Flags.wire_value(),
                        value: ActorMetadataValue::Flags(1 << ActorFlag::Sneaking.wire_value()),
                    }]),
                    properties: Arc::from([]),
                    tick: 0,
                },
            )),
            Some(3),
        )
        .unwrap();
    apply_controls(&mut world, &mut physics);
    if replay != Some(false) {
        tick_sneak(&mut physics);
    }
    if replay.is_some() {
        let correction = physics
            .apply_correction(
                crate::movement::PhysicsAnchor {
                    network_position: anchor.position,
                    tick: anchor.tick,
                    on_ground: false,
                    velocity: Some(anchor.velocity),
                },
                crate::movement::PhysicsCorrectionMode::ReplayIfRetained,
                None,
                &OpenWorld,
            )
            .unwrap();
        assert!(matches!(
            correction.outcome,
            crate::movement::PhysicsCorrectionOutcome::Replayed {
                replayed_ticks: 1,
                ..
            }
        ));
        tick_sneak(&mut physics);
    }
    reported_box(&mut physics, &world)
}

/// Completes a matching local sneak without another authority update.
fn tick_sneak(physics: &mut LocalPhysicsController) {
    let result = physics.advance_with_context(
        simulation_clock::TICK_DURATION,
        sim::MovementInput {
            sneaking: true,
            ..Default::default()
        },
        crate::movement::PhysicsSampleContext::default(),
        &OpenWorld,
    );
    assert_eq!(result.completed_ticks, 1);
}

#[test]
fn sparse_dimension_arrivals_do_not_depend_on_sync_attempts() {
    let observed = width_then_sneak(true, None);
    assert_eq!(observed, [1.0, 0.8, sim::PLAYER_HEIGHT as f32]);
    assert_eq!(width_then_sneak(false, None), observed);
}

#[test]
fn sparse_dimension_arrivals_keep_their_order_through_replay() {
    for completed in [false, true] {
        let observed = width_then_sneak(true, Some(completed));
        assert_eq!(observed, [1.0, 0.8, sim::PLAYER_HEIGHT as f32]);
        assert_eq!(width_then_sneak(false, Some(completed)), observed);
    }
}
