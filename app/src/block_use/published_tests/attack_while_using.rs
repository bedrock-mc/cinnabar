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

/// Holding Attack while eating starts no mining: neither on open ground ahead nor on the
/// block behind an actor whose pick the rejected press cleared.
#[test]
fn eating_ignores_held_attack_mining() {
    // The fixture block at [4, 2, 6] sits behind an actor at z 7.5; z -30 leaves the view clear.
    for actor_z in [-30.0, 7.5] {
        let (mut world, mut captured) = attack_fixture(actor_z);
        start_eating(&mut world);
        for tick in 102..105 {
            super::attack_progress_tests::buttons(&mut world, vec![1, 2]);
            world
                .run_system_cached(crate::melee::prepare_melee)
                .unwrap();
            world
                .run_system_cached(crate::melee::produce_attacks)
                .unwrap();
            super::attack_progress_tests::ticks(&mut world, tick, tick + 1);
        }
        assert_eq!(
            world
                .resource::<client_interaction::survival_mining::SurvivalMiningRuntime>()
                .destroying_target(),
            None,
            "held attack mined while eating (actor at z {actor_z})"
        );
        let packets = captured.drain();
        let start = protocol::BlockActionKind::StartDestroy.wire_value();
        assert!(
            packets
                .iter()
                .flat_map(protocol::test_support::packets::input_block_actions)
                .all(|action| action.0 != start),
            "held attack started a destroy while eating (actor at z {actor_z})"
        );
        let swing = protocol::swing_arm_packet(42, protocol::SwingSource::Mine).kind();
        assert!(
            packets.iter().all(|packet| packet.kind() != swing),
            "held attack swung while eating (actor at z {actor_z})"
        );
        assert!(
            world
                .resource::<client_interaction::item_use::ItemUseRuntime>()
                .is_using()
        );
    }
}

/// A slot change in the same frame ends the eat before Attack is judged, so the press stays
/// queued for the newly selected item (it waits for the selection to be announced) instead
/// of being dropped as an attack during item use.
#[test]
fn a_same_frame_slot_change_ends_the_eat_before_the_attack() {
    let (mut world, _captured) = attack_fixture(7.5);
    start_eating(&mut world);
    let selected = world
        .resource::<client_state::PlayerRuntime>()
        .selected_hotbar_slot()
        .unwrap();
    let mode = world
        .resource::<client_state::PlayerRuntime>()
        .facts
        .player_game_mode();
    world
        .resource_mut::<client_state::PlayerRuntime>()
        .inventory
        .queue_local_hotbar_selection((selected + 1) % 9, mode);
    attack(&mut world);
    assert!(
        !world
            .resource::<client_interaction::item_use::ItemUseRuntime>()
            .is_using(),
        "the slot change left the old item in use"
    );
    assert!(
        world.resource::<MeleeRuntime>().press_pending(),
        "the attack after the slot change was dropped"
    );
}
