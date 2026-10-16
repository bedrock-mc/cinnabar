//! Release sprint admission must not depend on the render frame's tick count.

use super::*;

/// Routes held forward and sprint input, optionally keeping the original item use active.
fn move_and_use(world: &mut World, held: bool) {
    let snapshot = world
        .resource_mut::<crate::semantic_controls::SemanticInputRuntime>()
        .route_and_finalize(semantic_input::DeviceFrame {
            keyboard_mouse: Some(semantic_input::KeyboardMouseFrame {
                keys: vec![0x1a, 0xe0],
                mouse_buttons: if held { vec![2] } else { Vec::new() },
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    assert!(snapshot.phases[semantic_input::Action::Sprint as usize].held);
    world.insert_resource(client_state::SemanticInputSnapshot(
        semantic_input::finalized::FinalizedInput::from_finalized(snapshot),
    ));
}

/// Advances the production runtime, retaining its actual tick samples in the movement ticker.
fn advance(world: &mut World, ticks: u32) {
    world
        .resource_mut::<Time<Real>>()
        .advance_by(simulation_clock::TICK_DURATION * ticks);
    world
        .run_system_cached(crate::movement::advance_local_physics)
        .unwrap();
}

/// Runs the same physical release and held controls with either one or two render frames.
fn release_ticks(
    batched: bool,
    release: bool,
) -> (Vec<bool>, Vec<protocol::PlayerAuthInputSnapshot>) {
    let (mut world, mut captured) = fixture();
    item_use_tests::hold_air_item(&mut world, "minecraft:trident", 1, riptide_data());
    assert!(
        world
            .resource_mut::<client_state::PlayerRuntime>()
            .facts
            .apply_hunger_attribute(&protocol::ActorAttribute {
                name: protocol_ids::Attribute::PlayerHunger.wire_name().into(),
                min: 0.0,
                max: 20.0,
                current: 20.0,
                default: Some(20.0),
                modifiers: [].into(),
            })
    );
    let origin = world
        .resource::<MovementTicker>()
        .newest_unsent_sample()
        .unwrap()
        .position;
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(origin, 100, false);
    physics.advance(
        simulation_clock::TICK_DURATION,
        sim::MovementInput::default(),
        &Water,
    );
    world.insert_resource(physics);
    flush_and_acknowledge(&mut world);
    produce_air_use(&mut world);
    for _ in 0..gameplay::item_use::RIPTIDE_CHARGE_TICKS {
        tick(&mut world);
        flush_and_acknowledge(&mut world);
        produce_air_use(&mut world);
    }
    captured.drain();
    assert!(
        world
            .resource::<client_interaction::item_use::ItemUseRuntime>()
            .is_using()
    );
    world.insert_resource(client_presentation::camera::AutoFly::new(false));
    #[cfg(feature = "acceptance")]
    world.insert_resource(AcceptanceRun::new(None, None, false, false));
    #[cfg(not(feature = "acceptance"))]
    world.init_resource::<AcceptanceRun>();
    world.init_resource::<client_interaction::movement::LocalMovementSpeedAuthority>();
    world.init_resource::<client_interaction::movement::LocomotionState>();
    move_and_use(&mut world, !release);
    produce_air_use(&mut world);
    assert_eq!(
        world.resource::<MovementTicker>().has_held_release(),
        release
    );
    if batched {
        advance(&mut world, 2);
    } else {
        advance(&mut world, 1);
        move_and_use(&mut world, !release);
        advance(&mut world, 1);
    }
    let mut snapshots = world.resource::<MovementTicker>().pending_snapshots();
    assert_eq!(snapshots.len(), 2, "both fixed ticks must reach the ticker");
    if release {
        flush_and_acknowledge(&mut world);
        world
            .resource_mut::<MovementTicker>()
            .send_held_release(|_| Err(gameplay::BatchSendError::Full));
        assert!(world.resource::<MovementTicker>().has_held_release());
    }
    move_and_use(&mut world, !release);
    advance(&mut world, 1);
    snapshots.push(
        *world
            .resource::<MovementTicker>()
            .pending_snapshots()
            .last()
            .unwrap(),
    );
    let sprinting = snapshots
        .iter()
        .map(|sample| {
            world
                .resource::<LocalPhysicsController>()
                .sample_at(sample.tick)
                .unwrap()
                .processed
                .sprinting
        })
        .collect();
    (sprinting, snapshots)
}

#[test]
fn riptide_release_sprint_is_independent_of_frame_batching() {
    let (batched_sprinting, batched) = release_ticks(true, true);
    let (split_sprinting, split) = release_ticks(false, true);
    assert_eq!(
        batched_sprinting, split_sprinting,
        "identical fixed ticks must admit the same held sprint"
    );
    assert_eq!(batched_sprinting, [false, true, true]);
    assert!(
        batched.iter().all(|sample| sample.flags.bits()
            & protocol::PlayerInputFlags::SPRINT_DOWN.bits()
            != 0)
    );
    assert_ne!(
        batched[1].flags.bits() & protocol::PlayerInputFlags::START_SPRINTING.bits(),
        0
    );
    assert_eq!(
        batched, split,
        "movement and input flags must not depend on render-frame grouping"
    );
    assert!(batched[0].move_vector[1] < batched[1].move_vector[1]);
    assert_eq!(
        batched
            .iter()
            .filter(|sample| sample.flags.bits()
                & protocol::PlayerInputFlags::START_SPIN_ATTACK.bits()
                != 0)
            .count(),
        1
    );
}

#[test]
fn riptide_active_use_keeps_sprint_blocked_in_both_frame_groupings() {
    let (batched_sprinting, batched) = release_ticks(true, false);
    let (split_sprinting, split) = release_ticks(false, false);
    assert_eq!(batched_sprinting, [false, false, false]);
    assert_eq!(batched_sprinting, split_sprinting);
    assert_eq!(batched, split);
    assert!(
        batched
            .iter()
            .all(|sample| sample.move_vector == batched[0].move_vector)
    );
}
