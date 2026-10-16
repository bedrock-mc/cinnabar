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

mod assisted;
mod snap;

/// Supplies the selected stack shared by use frames and prediction checks.
fn held_stack() -> NetworkItemStack {
    NetworkItemStack {
        network_id: 2,
        count: 1,
        stack_network_id: 9,
        ..NetworkItemStack::empty()
    }
}

/// Supplies an admitted bow press against the current unsent tick.
fn use_frame(tick: u64) -> UseFrame {
    let stack = held_stack();
    UseFrame {
        riptide_facing: Default::default(),
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
        |identity, packet, guard, _| network.send_physics_packet(identity, packet, guard),
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
            |identity, packet, guard, _| {
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
    assert_eq!(
        runtime.take_riptide_launch().map(|launch| launch.level),
        Some(3)
    );
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
            |identity, packet, guard, _| {
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

/// Empty terrain supplies stable collision identity for release and correction tests.
struct RiptideWorld;

impl sim::CollisionWorld for RiptideWorld {
    fn collision_boxes(
        &self,
        _: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        Ok(sim::CollisionQuery::synthetic(Vec::new()))
    }
}

#[test]
fn discarded_riptide_release_cancels_pending_and_replayed_launch() {
    use client_interaction::movement::{LocalPhysicsController, MovementTicker};
    use gameplay::movement::{
        MovementSource, PhysicsCorrectionMode, reconcile_candidate_physics_correction,
    };
    for (predicted_ticks, corrected_tick, facing_written) in [
        (0, 110, false),
        (2, 110, false),
        (2, 112, false),
        (2, 110, true),
    ] {
        let (network, mut captured) = NetworkHandle::<PackApplication>::stub_capturing_packets();
        let origin = [0.0, 65.620_01, 0.0];
        let mut physics = LocalPhysicsController::default();
        physics.reanchor_network_position(origin, 100, false);
        let mut ticker = MovementTicker::with_epoch_publisher(network.physics_epoch_publisher());
        ticker.reset(7, 100, origin);
        ticker.set_source(MovementSource::Physics);
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
        let mut anchors = std::collections::BTreeMap::new();
        for tick in 101..=111 {
            let frame = physics.advance(
                simulation_clock::TICK_DURATION,
                sim::MovementInput::default(),
                &RiptideWorld,
            );
            let sample = frame.samples.into_iter().next().unwrap();
            anchors.insert(tick, sample.position);
            ticker.enqueue_completed_physics(sample).unwrap();
            if tick == 101 {
                runtime.observe_press(true);
                super::admit_with_action_aim(
                    &mut runtime,
                    &mut swings,
                    &mut ticker,
                    &mut view,
                    &trident(tick, true),
                    42,
                    6,
                    None,
                    &network,
                );
            }
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
        assert!(ticker.has_held_release());
        let launch = runtime.take_riptide_launch().unwrap();
        ticker.request_riptide(&mut physics, launch.level, launch.rotation);
        if facing_written {
            let mut written = Vec::new();
            gameplay::movement::flush_player_auth_inputs_guarded(
                &mut ticker,
                11,
                Some(evidence()),
                |identity, packet, guard, _| {
                    written.push(identity);
                    network.send_physics_packet(identity, packet, guard)
                },
            )
            .unwrap();
            assert_eq!(written.len(), 11);
            for identity in written {
                assert!(ticker.acknowledge_physics_send(identity));
            }
        }
        captured.drain();
        for _ in 0..predicted_ticks {
            let frame = physics.advance(
                simulation_clock::TICK_DURATION,
                sim::MovementInput::default(),
                &RiptideWorld,
            );
            let sample = frame.samples.into_iter().next().unwrap();
            anchors.insert(sample.tick, sample.position);
            ticker.enqueue_completed_physics(sample).unwrap();
        }
        // Corrections may carry a tick later than the discarded release's facing.
        let mut anchor = anchors[&corrected_tick];
        anchor[0] += 0.25;
        reconcile_candidate_physics_correction(
            &mut ticker,
            &mut physics,
            anchor,
            corrected_tick,
            false,
            PhysicsCorrectionMode::ReplayIfRetained,
            &RiptideWorld,
        )
        .unwrap();
        assert_eq!(ticker.has_held_release(), facing_written);
        if facing_written {
            ticker.send_held_release(|packets| network.send_inventory_packets(packets));
        } else {
            ticker.send_held_release(|_| panic!("a discarded release must never be sent"));
        }
        let next = physics.advance(
            simulation_clock::TICK_DURATION,
            sim::MovementInput::default(),
            &RiptideWorld,
        );
        ticker
            .enqueue_completed_physics(next.samples[0].clone())
            .unwrap();
        assert_eq!(
            physics.state().unwrap().spin_attack_ticks.is_some(),
            facing_written,
            "release lifetime differs after correction with {predicted_ticks} predicted ticks"
        );
        let state = physics.state().unwrap();
        assert_eq!(
            state.velocity.x != 0.0 || state.velocity.z != 0.0,
            facing_written
        );
        assert_eq!(
            ticker
                .pending_snapshots()
                .iter()
                .any(|sample| sample.flags.bits()
                    & protocol::PlayerInputFlags::START_SPIN_ATTACK.bits()
                    != 0),
            facing_written
        );
        assert_eq!(captured.drain().len(), usize::from(facing_written));
    }
}

#[test]
fn an_assisted_release_keeps_slowdown_until_its_facing_tick_is_simulated() {
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
    gameplay::movement::flush_player_auth_inputs_guarded(
        &mut ticker,
        8,
        Some(evidence()),
        |identity, packet, guard, _| network.send_physics_packet(identity, packet, guard),
    )
    .unwrap();
    captured.drain();
    let sample = ticker.between_ticks_sample().unwrap();
    let release = UseFrame {
        held: false,
        tick: sample.tick,
        position: sample.position,
        ..use_frame(sample.tick)
    };
    let mut view = client_presentation::local_player::LocalViewPose::default();
    super::admit_with_action_aim(
        &mut runtime,
        &mut swings,
        &mut ticker,
        &mut view,
        &release,
        42,
        6,
        Some(bevy::prelude::Quat::IDENTITY),
        &network,
    );
    assert!(
        runtime.is_using(),
        "the facing tick must still simulate item-use slowdown"
    );
    assert!(runtime.movement_modifier().is_some());
    assert!(!ticker.has_held_release());
    assert!(captured.drain().is_empty());
}

#[test]
fn assisted_release_uses_the_same_charge_clock_as_holding() {
    let air_use = classify("minecraft:crossbow", false, 0, None);
    let duration = match air_use.unwrap() {
        gameplay::item_use::AirUse::Hold { max_ticks, .. } => u64::from(max_ticks),
        _ => panic!("an unloaded crossbow must charge"),
    };
    for completed_charge in [false, true] {
        for (held, assisted) in [(true, false), (false, false), (false, true)] {
            let (network, mut captured) =
                NetworkHandle::<PackApplication>::stub_capturing_packets();
            let mut ticker = client_interaction::movement::MovementTicker::with_epoch_publisher(
                network.physics_epoch_publisher(),
            );
            ticker.reset(7, 100, completed(101).position);
            ticker.set_source(gameplay::movement::MovementSource::Physics);
            let mut runtime = ItemUseRuntime::default();
            let mut swings = SwingTracker::default();
            let start = UseFrame {
                air_use,
                charge_projectile: Some(assets::item_use::ItemProjectileRole::Arrow),
                ..use_frame(101)
            };
            runtime.observe_press(true);
            admit_on_tick(
                &mut runtime,
                &mut swings,
                &mut ticker,
                &start,
                42,
                6,
                |packets| network.send_inventory_packets(packets),
            );
            assert!(runtime.is_using());
            captured.drain();
            let end = start.tick + duration - if completed_charge { 1 } else { 2 };
            for tick in start.tick..=end {
                ticker.enqueue_completed_physics(completed(tick)).unwrap();
                if tick < end || !assisted {
                    gameplay::movement::flush_player_auth_inputs_guarded(
                        &mut ticker,
                        8,
                        Some(evidence()),
                        |identity, packet, guard, _| {
                            network.send_physics_packet(identity, packet, guard)
                        },
                    )
                    .unwrap();
                }
            }
            captured.drain();
            let frame = UseFrame {
                tick: if assisted { end } else { end + 1 },
                held,
                press_consumed: assisted,
                ..start.clone()
            };
            super::admit_with_action_aim(
                &mut runtime,
                &mut swings,
                &mut ticker,
                &mut client_presentation::local_player::LocalViewPose::default(),
                &frame,
                42,
                6,
                assisted.then_some(bevy::prelude::Quat::IDENTITY),
                &network,
            );
            let selection = start.selection.as_ref().unwrap();
            let predicted = runtime.predicted_projectile(
                selection.slot,
                start.inventory_revision.unwrap(),
                &held_stack(),
            );
            assert_eq!(
                predicted,
                completed_charge.then_some(Some(assets::item_use::ItemProjectileRole::Arrow)),
                "held and assisted release must share the completed charge boundary"
            );
            assert_eq!(runtime.is_using(), held && !completed_charge);
            assert_eq!(ticker.has_held_release(), assisted && !completed_charge);
        }
    }
}
