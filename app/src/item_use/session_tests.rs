//! Same-tick item admission through the real bounded session command queue.

use crate::runtime::network::resource_packs::PackApplication;
use client_runtime::network::NetworkHandle;
use gameplay::{
    item_use::{ItemUseRuntime, UseFrame, admit_on_tick, fixtures::classify},
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
        riptide: None,
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
    let (network, mut captured) = NetworkHandle::<PackApplication>::stub_capturing_packets();
    let mut ticker = client_interaction::movement::MovementTicker::with_epoch_publisher(
        network.physics_epoch_publisher(),
    );
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
    gameplay::movement::flush_player_auth_inputs_guarded(
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
            .map(|packet| format!("{:?}", packet.kind()))
            .collect::<Vec<_>>(),
        ["InventoryTransactionPacket", "PlayerAuthInputPacket"],
    );
    let sample = protocol::player_auth_input_trace_sample(&packets[1]).unwrap();
    assert_eq!(sample.tick, 101);
    assert!(sample.flag_names.contains(&"StartUsingItem"));
}

#[test]
fn riptide_release_follows_completed_inputs_and_precedes_the_launch_input() {
    let (network, mut captured) = NetworkHandle::<PackApplication>::stub_capturing_packets();
    let mut ticker = client_interaction::movement::MovementTicker::with_epoch_publisher(
        network.physics_epoch_publisher(),
    );
    ticker.reset(7, 100, completed(101).position);
    ticker.set_source(gameplay::movement::MovementSource::Physics);
    ticker.enqueue_completed_physics(completed(101)).unwrap();
    let mut runtime = ItemUseRuntime::default();
    let mut swings = SwingTracker::default();
    let mut view = client_presentation::local_player::LocalViewPose::default();
    let trident = |tick, held| UseFrame {
        held,
        riptide: Some(gameplay::item_use::RiptideUse {
            level: 3,
            wet: true,
            mounted: false,
            usable: true,
        }),
        air_use: classify("minecraft:trident", false, 0, None),
        ..use_frame(tick)
    };
    runtime.observe_press(true);
    super::admit_with_action_aim(
        &mut runtime,
        &mut swings,
        &mut ticker,
        &mut view,
        &trident(101, true),
        42,
        6,
        None,
        &network,
    );
    let flush = |ticker: &mut gameplay::movement::MovementTicker, budget| {
        let mut written = Vec::new();
        gameplay::movement::flush_player_auth_inputs_guarded(
            ticker,
            budget,
            Some(evidence()),
            |identity, packet, guard| {
                written.push(identity);
                network.send_physics_packet(identity, packet, guard)
            },
        )
        .unwrap();
        for identity in written {
            assert!(ticker.acknowledge_physics_send(identity));
        }
    };
    flush(&mut ticker, 1);
    captured.drain();
    for tick in 102..=111 {
        ticker.enqueue_completed_physics(completed(tick)).unwrap();
    }
    super::admit_with_action_aim(
        &mut runtime,
        &mut swings,
        &mut ticker,
        &mut view,
        &trident(111, false),
        42,
        6,
        None,
        &network,
    );
    assert_eq!(runtime.take_riptide_launch(), Some(3));
    assert!(captured.drain().is_empty());
    let mut launch = completed(112);
    launch.processed.spin_started = true;
    launch.processed.spinning = true;
    ticker.enqueue_completed_physics(launch).unwrap();
    flush(&mut ticker, 11);
    let preceding = captured.drain();
    assert_eq!(preceding.len(), 10);
    assert_eq!(
        protocol::player_auth_input_trace_sample(&preceding[9])
            .unwrap()
            .tick,
        111
    );
    ticker.send_held_release(|_| Err(gameplay::BatchSendError::Full));
    flush(&mut ticker, 1);
    assert!(captured.drain().is_empty());
    ticker.send_held_release(|packets| network.send_inventory_packets(packets));
    flush(&mut ticker, 1);
    let packets = captured.drain();
    assert_eq!(packets.len(), 2);
    assert!(protocol::is_aim_assist_rotation_action(&packets[0]));
    let launch = protocol::player_auth_input_trace_sample(&packets[1]).unwrap();
    assert_eq!(launch.tick, 112);
    assert!(launch.flag_names.contains(&"StartSpinAttack"));
}

#[test]
fn aim_assisted_release_follows_the_input_that_carries_its_facing() {
    let (network, mut captured) = NetworkHandle::<PackApplication>::stub_capturing_packets();
    let mut ticker = client_interaction::movement::MovementTicker::with_epoch_publisher(
        network.physics_epoch_publisher(),
    );
    ticker.reset(7, 100, completed(101).position);
    ticker.set_source(gameplay::movement::MovementSource::Physics);
    ticker.enqueue_completed_physics(completed(101)).unwrap();
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    let mut swings = SwingTracker::default();
    let mut view = client_presentation::local_player::LocalViewPose::default();
    // Writes one input and acknowledges it as the socket pump does.
    let flush = |ticker: &mut gameplay::movement::MovementTicker| {
        let mut written = None;
        gameplay::movement::flush_player_auth_inputs_guarded(
            ticker,
            1,
            Some(evidence()),
            |identity, packet, guard| {
                written = Some(identity);
                network.send_physics_packet(identity, packet, guard)
            },
        )
        .unwrap();
        assert!(ticker.acknowledge_physics_send(written.unwrap()));
    };
    let rotation = bevy::prelude::Quat::from_euler(bevy::prelude::EulerRot::YXZ, 0.5, -0.3, 0.0);
    super::admit_with_action_aim(
        &mut runtime,
        &mut swings,
        &mut ticker,
        &mut view,
        &use_frame(101),
        42,
        6,
        Some(rotation),
        &network,
    );
    flush(&mut ticker);
    captured.drain();

    ticker.enqueue_completed_physics(completed(102)).unwrap();
    let release = UseFrame {
        held: false,
        ..use_frame(102)
    };
    super::admit_with_action_aim(
        &mut runtime,
        &mut swings,
        &mut ticker,
        &mut view,
        &release,
        42,
        6,
        Some(rotation),
        &network,
    );
    ticker.send_held_release(|packets| network.send_inventory_packets(packets));
    assert!(
        captured.drain().is_empty(),
        "the release waits for its tick's input"
    );
    flush(&mut ticker);
    ticker.send_held_release(|packets| network.send_inventory_packets(packets));
    let packets = captured.drain();
    assert_eq!(
        packets
            .iter()
            .map(|packet| format!("{:?}", packet.kind()))
            .collect::<Vec<_>>(),
        ["PlayerAuthInputPacket", "InventoryTransactionPacket"],
    );
    assert!(protocol::is_aim_assist_rotation_action(&packets[1]));
    let sample = protocol::player_auth_input_trace_sample(&packets[0]).unwrap();
    assert_eq!(sample.tick, 102);
    assert!((sample.pitch - 0.3_f32.to_degrees()).abs() < 1e-3);
    assert!((sample.yaw - (180.0 - 0.5_f32.to_degrees())).abs() < 1e-3);
    assert!(!ticker.has_held_release());
    assert_eq!(view.rotation(), rotation);
}
