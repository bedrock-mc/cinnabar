//! Same-tick item admission through the real bounded session command queue.

use gameplay::{
    item_use::{ItemUseRuntime, UseFrame, admit_on_tick, classify},
    melee::SwingTracker,
    mining::FrozenMiningSelection,
    test_support::survival_mining::{completed, evidence},
};
use protocol::{NetworkItemStack, VerifiedNetworkItemStack};

/// Supplies an admitted bow press against the current unsent tick.
fn use_frame(tick: u64) -> UseFrame {
    let stack = NetworkItemStack {
        network_id: 2,
        count: 1,
        stack_network_id: 9,
        ..NetworkItemStack::empty()
    };
    UseFrame {
        tick,
        now_millis: 1_000,
        position: completed(tick).position,
        held: true,
        selection: Some(FrozenMiningSelection {
            slot: 0,
            item: VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest).unwrap(),
        }),
        air_use: classify("minecraft:bow", false, 0, None),
        ready: true,
        creative: false,
        inventory_revision: Some(1),
        charge_projectile: None,
        press_consumed: false,
    }
}

#[test]
fn admitted_item_transaction_precedes_the_same_tick_start_using_input() {
    let (network, mut captured) = crate::runtime::network::NetworkHandle::stub_capturing_packets();
    let mut ticker = network.movement_ticker();
    ticker.reset(7, 100, completed(101).position);
    ticker.set_source(gameplay::movement::MovementSource::Physics);
    ticker.enqueue_completed_physics(completed(101)).unwrap();
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    let mut swings = SwingTracker::default();
    admit_on_tick(
        &mut runtime,
        &mut swings,
        &mut ticker,
        &use_frame(101),
        42,
        6,
        |packets| network.send_inventory_packets(packets),
    );
    assert!(runtime.is_using());
    crate::movement::flush_player_auth_inputs_guarded(
        &mut ticker,
        1,
        Some(evidence()),
        |identity, packet, guard| network.send_physics_packet(identity, packet, guard),
    )
    .unwrap();
    let packets = captured.drain();
    assert_eq!(
        packets
            .iter()
            .map(|packet| format!("{:?}", packet.header.id))
            .collect::<Vec<_>>(),
        ["InventoryTransactionPacket", "PlayerAuthInputPacket"],
    );
    let sample = protocol::player_auth_input_trace_sample(&packets[1]).unwrap();
    assert_eq!(sample.tick, 101);
    assert!(sample.flag_names.contains(&"StartUsingItem"));
}
