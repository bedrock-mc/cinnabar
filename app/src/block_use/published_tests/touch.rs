use super::*;
use semantic_input::{DeviceFrame, InputMode, TouchContact, touch};

/// Publishes the touch Use button through the same router as physical contacts.
fn touch_use(world: &mut World, held: bool) -> semantic_input::ActionSnapshot {
    let snapshot = world
        .resource_mut::<crate::semantic_controls::SemanticInputRuntime>()
        .route_and_finalize(DeviceFrame {
            touches: if held {
                vec![TouchContact {
                    contact_id: 1,
                    activity_sequence: 0,
                    position: [0.8, 0.8],
                    delta: [0.0; 2],
                    hit_id: Some(touch::USE),
                }]
            } else {
                Vec::new()
            },
            ..Default::default()
        })
        .unwrap();
    world.insert_resource(client_state::SemanticInputSnapshot(
        semantic_input::finalized::FinalizedInput::from_finalized(snapshot.clone()),
    ));
    snapshot
}

#[test]
fn touch_use_places_a_block_and_release_ends_the_hold() {
    let (mut world, mut captured) =
        floor_fixture([4.5, 2.620_01, 8.32], Quat::from_rotation_x(-0.9601));
    let input = touch_use(&mut world, true);
    assert_eq!(input.input_mode, InputMode::Touch);
    assert!(input.phases[Action::Use as usize].pressed);
    assert!(input.ordered.is_none());

    world.run_system_cached(produce_block_use).unwrap();
    assert_eq!(transaction_targets(&mut captured), [[4, 0, 7]]);
    assert_eq!(
        world
            .resource::<BlockUseRuntime>()
            .last_success_destination(),
        Some([4, 1, 7])
    );

    let input = touch_use(&mut world, false);
    assert!(input.phases[Action::Use as usize].released);
    world.run_system_cached(produce_block_use).unwrap();
    assert!(transaction_targets(&mut captured).is_empty());
    assert_eq!(
        world
            .resource::<BlockUseRuntime>()
            .last_success_destination(),
        None
    );

    let input = touch_use(&mut world, true);
    assert!(input.phases[Action::Use as usize].pressed);
    world.run_system_cached(produce_block_use).unwrap();
    assert_eq!(transaction_targets(&mut captured), [[4, 1, 7]]);
}

#[test]
fn held_touch_use_repeats_without_another_press() {
    let (mut world, mut captured) =
        floor_fixture([4.5, 2.620_01, 8.32], Quat::from_rotation_x(-0.9601));
    touch_use(&mut world, true);
    world.run_system_cached(produce_block_use).unwrap();
    assert_eq!(transaction_targets(&mut captured), [[4, 0, 7]]);

    let mut targets = Vec::new();
    for _ in 0..8 {
        let input = touch_use(&mut world, true);
        assert!(!input.phases[Action::Use as usize].pressed);
        assert!(input.phases[Action::Use as usize].held);
        world
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_millis(50));
        world.run_system_cached(produce_block_use).unwrap();
        targets.extend(transaction_targets(&mut captured));
        world
            .run_system_cached(crate::movement::advance_local_physics)
            .unwrap();
    }
    assert_eq!(targets.first(), Some(&[4, 1, 7]));
}
