//! An item in use ignores the attack button, as vanilla does.
use super::attack_hit_position_tests::{attack, attack_fixture};
use super::*;
use protocol::test_support::transactions::observe_transaction;

/// Starts eating a golden apple with the use button still held.
fn start_eating(world: &mut World) {
    item_use_tests::hold_air_item(world, "minecraft:golden_apple", 2, [].into());
    let selection = client_interaction::block_use::verified_use_selection(
        world.resource::<client_state::PlayerRuntime>(),
        world.resource::<UiRuntimeResource>(),
    );
    let frame = gameplay::item_use::UseFrame {
        riptide: None,
        riptide_facing: Default::default(),
        tick: 101,
        now_millis: 1_000,
        position: [4.5, 2.620_01, 8.5],
        held: true,
        selection,
        air_use: gameplay::item_use::fixtures::classify(
            "minecraft:golden_apple",
            false,
            0,
            Some(32),
        ),
        ready: true,
        creative: false,
        inventory_revision: Some(1),
        charge_projectile: None,
        press_consumed: false,
    };
    let mut runtime = world.resource_mut::<client_interaction::item_use::ItemUseRuntime>();
    runtime.synchronize(7);
    runtime.observe_press(true);
    assert!(runtime.step(&frame).started);
}

#[test]
fn eating_ignores_an_attack_press_on_a_reachable_actor() {
    let (mut world, mut captured) = attack_fixture(7.5);
    start_eating(&mut world);
    attack(&mut world);
    let packets = captured.drain();
    let attacks = packets
        .iter()
        .filter_map(observe_transaction)
        .filter_map(|tx| tx.actor_use().cloned())
        .filter(|tx| tx.action == Some(protocol::ActorUseAction::Attack))
        .count();
    assert_eq!(attacks, 0, "an attack transaction interrupted the eat");
    let swing = protocol::swing_arm_packet(42, protocol::SwingSource::Attack).kind();
    assert!(
        packets.iter().all(|packet| packet.kind() != swing),
        "the attack press swung while eating"
    );
    assert!(
        world
            .resource::<client_interaction::item_use::ItemUseRuntime>()
            .is_using()
    );
}
