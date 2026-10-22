//! Position corrections retain accepted uses while movement replaces its geometry.

use super::*;
use protocol::test_support::transactions::observe_transaction;

/// Corrects the current predicted tick without changing the session, dimension or life; the
/// replay still starts a new transport epoch.
fn correct_position(world: &mut World) {
    let before = world.resource::<MovementTicker>().reanchor_epoch();
    world.resource_scope(|world, mut movement: Mut<MovementTicker>| {
        let mut physics = world.resource_mut::<LocalPhysicsController>();
        let mut position = physics.network_position().unwrap();
        let tick = movement.completed_tick();
        position[0] += 0.25;
        gameplay::movement::reconcile_candidate_physics_correction(
            &mut movement,
            &mut physics,
            position,
            tick,
            false,
            gameplay::movement::PhysicsCorrectionMode::ReplayIfRetained,
            &Water,
        )
        .unwrap();
    });
    assert_ne!(world.resource::<MovementTicker>().reanchor_epoch(), before);
}

/// Verifies one release and its launch through both item phases and movement receipts.
fn finish_release(
    world: &mut World,
    captured: &mut client_session::CapturedPackets,
    launches: usize,
) -> Vec<protocol::Packet> {
    let mut sent = Vec::new();
    let mut launched = 0;
    for _ in 0..3 {
        launched += usize::from(tick(world).processed.spin_started);
        world
            .run_system_cached(crate::item_use::prepare_item_release)
            .unwrap();
        flush_and_acknowledge(world);
        world
            .run_system_cached(crate::item_use::produce_item_use)
            .unwrap();
        sent.extend(captured.drain());
    }
    assert_eq!(
        sent.iter()
            .filter_map(observe_transaction)
            .filter(|tx| tx.release().is_some())
            .count(),
        1,
        "the accepted use must release exactly once after correction"
    );
    assert!(!world.resource::<ItemUseRuntime>().is_using());
    assert!(!world.resource::<ItemUseRuntime>().has_pending_release());
    assert!(!world.resource::<MovementTicker>().has_held_release());
    assert_eq!(launched, launches);
    sent
}

#[test]
fn ordered_position_correction_preserves_an_active_charge() {
    let (mut world, mut captured) = respawn::charging_world(1);
    world.init_resource::<crate::melee::MeleeTarget>();
    ordered_frame(&mut world, Default::default(), vec![2]);
    let timing = world.resource::<ItemUseRuntime>().active_timing().unwrap();
    correct_position(&mut world);
    ordered_frame(&mut world, Default::default(), vec![2]);
    assert_eq!(
        world.resource::<ItemUseRuntime>().active_timing(),
        Some(timing)
    );

    let mut release = semantic_input::DesktopEvents::default();
    release.push(semantic_input::DesktopEvent::Button {
        control: semantic_input::PhysicalControl::MouseButton(2),
        pressed: false,
        modifiers: Default::default(),
    });
    ordered_frame(&mut world, release, Vec::new());
    finish_release(&mut world, &mut captured, 0);
}

#[test]
fn ordered_position_correction_preserves_a_pending_release() {
    let (mut world, mut captured) = observed_release();
    ordered_frame(&mut world, Default::default(), vec![2]);
    let timing = world.resource::<ItemUseRuntime>().active_timing().unwrap();
    let facing = world.resource::<ItemUseRuntime>().riptide_release_facing();
    correct_position(&mut world);
    ordered_frame(&mut world, Default::default(), vec![2]);
    let runtime = world.resource::<ItemUseRuntime>();
    assert!(runtime.has_pending_release());
    assert_eq!(runtime.active_timing(), Some(timing));
    assert_eq!(runtime.riptide_release_facing(), facing);
    let sent = finish_release(&mut world, &mut captured, 1);
    let facing = sent
        .iter()
        .position(|packet| {
            protocol::player_auth_input_trace_sample(packet)
                .is_some_and(|input| (input.yaw - 90.0).abs() < 1e-3)
        })
        .expect("the original release facing must still be carried");
    let release = sent
        .iter()
        .position(|packet| observe_transaction(packet).is_some_and(|tx| tx.release().is_some()))
        .unwrap();
    assert!(facing < release);
}
