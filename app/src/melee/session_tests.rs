//! The session FIFO must admit attack packets before their associated movement tick.

use crate::runtime::network::resource_packs::PackApplication;
use client_runtime::network::NetworkHandle;
use gameplay::melee::{
    ActorHit, Crosshair, MeleeRuntime, PressContext, SwingTracker, resolve_and_send,
};
use protocol::PlayerInputMode;

/// Constructs a verified selected stack for one mouse attack.
fn press(input_mode: PlayerInputMode) -> PressContext {
    let stack = protocol::NetworkItemStack::empty();
    PressContext {
        refused_destroy: None,
        tick: 101,
        player_position: [0.5, 2.620_01, 0.5],
        input_mode,
        local_runtime_id: 42,
        selection: Some(gameplay::mining::FrozenMiningSelection {
            slot: 3,
            item: protocol::VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest)
                .unwrap(),
        }),
        swing_duration: 6,
        item_attack: None,
        now_millis: 1_000,
    }
}

/// Reads packet identifiers in the order accepted by the session queue.
fn kinds(packets: &[protocol::Packet]) -> Vec<String> {
    packets
        .iter()
        .map(|packet| format!("{:?}", packet.kind()))
        .collect()
}

const ZOMBIE: Crosshair = Crosshair::Actor(ActorHit {
    runtime_id: 9,
    distance: 2.0,
    point: [0.0, 1.5, -2.0],
});

#[test]
fn standalone_attack_packets_precede_their_tick_player_auth_input() {
    let (network, mut captured) = NetworkHandle::<PackApplication>::stub_capturing_packets();
    let mut ticker = gameplay::test_support::survival_mining::ticker_with_ticks(1);
    for (crosshair, expected) in [
        (
            ZOMBIE,
            vec![
                "InventoryTransactionPacket",
                "AnimatePacket",
                "PlayerAuthInputPacket",
            ],
        ),
        (
            Crosshair::Miss,
            vec!["AnimatePacket", "PlayerAuthInputPacket"],
        ),
    ] {
        let mut runtime = MeleeRuntime::default();
        runtime.observe_input(true, false);
        let tick = ticker.newest_unsent_sample().unwrap().tick;
        let outcome = runtime.resolve(
            crosshair,
            &PressContext {
                tick,
                ..press(PlayerInputMode::Mouse)
            },
            &mut SwingTracker::default(),
        );
        for packet in outcome.packets {
            network.send_inventory_packet(packet).unwrap();
        }
        if outcome.missed_swing {
            ticker.mark_missed_swing(tick);
        }
        gameplay::movement::flush_player_auth_inputs_guarded(
            &mut ticker,
            8,
            Some(gameplay::test_support::survival_mining::evidence()),
            |identity, packet, guard, _| network.send_physics_packet(identity, packet, guard),
        )
        .unwrap();
        let packets = captured.drain();
        assert_eq!(kinds(&packets), expected);
        let flags = protocol::player_auth_input_trace_sample(packets.last().unwrap())
            .unwrap()
            .flag_names;
        assert_eq!(flags.contains(&"MissedSwing"), crosshair == Crosshair::Miss);
        ticker
            .enqueue_completed_physics(gameplay::test_support::survival_mining::completed(tick + 1))
            .unwrap();
    }
}

#[test]
fn a_full_queue_rolls_back_the_press_and_the_swing_together() {
    use client_session::BatchSendError;
    let (network, _open) = NetworkHandle::<PackApplication>::with_command_capacity(1);
    let mut runtime = MeleeRuntime::default();
    let mut swings = SwingTracker::default();
    runtime.observe_input(true, true);
    let press = press(PlayerInputMode::Mouse);
    let missed = resolve_and_send(&mut runtime, &mut swings, ZOMBIE, &press, 1, |packets| {
        assert_eq!(packets.len(), 2);
        network.send_inventory_packets(packets)
    });
    assert!(!missed);
    assert_eq!(
        swings.take_started(),
        None,
        "no swing without its transaction"
    );
    assert!(!runtime.blocks_use_at(press.now_millis));

    let mut sent = Vec::new();
    resolve_and_send(&mut runtime, &mut swings, ZOMBIE, &press, 2, |packets| {
        sent = packets;
        Ok::<(), BatchSendError>(())
    });
    assert_eq!(
        kinds(&sent),
        ["InventoryTransactionPacket", "AnimatePacket"]
    );
}

/// Catch-up movement follows the attack's preceding pose, with a miss carried only by the first input.
#[test]
fn catchup_attacks_and_misses_precede_the_first_input_with_its_previous_pose() {
    for crosshair in [ZOMBIE, Crosshair::Miss] {
        let (network, mut captured) = NetworkHandle::<PackApplication>::stub_capturing_packets();
        let mut ticker = gameplay::test_support::survival_mining::ticker_with_ticks(0);
        let anchor = ticker.build_action_state().unwrap().position;
        for tick in 101..=103 {
            let mut sample = gameplay::test_support::survival_mining::completed(tick);
            sample.position[0] += (tick - 100) as f32 * 0.25;
            ticker.enqueue_completed_physics(sample).unwrap();
        }
        let mut runtime = MeleeRuntime::default();
        runtime.observe_input(true, false);
        let sample = runtime.press_sample(crosshair, &ticker, 3, 1).unwrap();
        let press = PressContext {
            tick: sample.tick,
            player_position: sample.position,
            ..press(PlayerInputMode::Mouse)
        };
        let missed = resolve_and_send(
            &mut runtime,
            &mut SwingTracker::default(),
            crosshair,
            &press,
            1,
            |packets| network.send_inventory_packets(packets),
        );
        if missed {
            ticker.mark_missed_swing(sample.tick);
        }
        gameplay::movement::flush_player_auth_inputs_guarded(
            &mut ticker,
            8,
            Some(gameplay::test_support::survival_mining::evidence()),
            |identity, packet, guard, _| network.send_physics_packet(identity, packet, guard),
        )
        .unwrap();
        let packets = captured.drain();
        let first_input = if crosshair == ZOMBIE {
            let expected = protocol::use_actor_packet(protocol::ActorUseRequest {
                actor_runtime_id: 9,
                action: protocol::ActorUseAction::Attack,
                selected_slot: press.selection.as_ref().unwrap().slot,
                selected_item: press.selection.as_ref().unwrap().item.clone(),
                player_position: anchor,
                hit_position: [0.0, 1.5, -2.0],
            })
            .unwrap();
            assert_eq!(
                protocol::encode(&packets[0]).unwrap(),
                protocol::encode(&expected).unwrap()
            );
            assert_eq!(
                kinds(&packets[..2]),
                ["InventoryTransactionPacket", "AnimatePacket"]
            );
            2
        } else {
            assert_eq!(kinds(&packets[..1]), ["AnimatePacket"]);
            1
        };
        assert_eq!(packets.len(), first_input + 3);
        for (offset, packet) in packets[first_input..].iter().enumerate() {
            let input = protocol::player_auth_input_trace_sample(packet).unwrap();
            assert_eq!(input.tick, 101 + offset as u64);
            assert_eq!(input.position[0], anchor[0] + (offset + 1) as f32 * 0.25);
            assert_eq!(
                input.flag_names.contains(&"MissedSwing"),
                missed && offset == 0
            );
        }
    }
}

#[test]
fn frame_attacks_follow_completed_inputs_and_precede_the_next_tick_and_swing() {
    for crosshair in [ZOMBIE, Crosshair::Miss] {
        let (network, mut captured) = NetworkHandle::<PackApplication>::stub_capturing_packets();
        let mut ticker = gameplay::test_support::survival_mining::ticker_with_ticks(0);
        let mut runtime = MeleeRuntime::default();
        runtime.observe_input(true, false);
        let mut preceding = [0.0; 3];
        for tick in 101..=102 {
            let mut sample = gameplay::test_support::survival_mining::completed(tick);
            sample.position[1] += (tick - 100) as f32 * 0.25;
            preceding = sample.position;
            ticker.enqueue_completed_physics(sample).unwrap();
        }
        assert!(runtime.between_ticks_press(&ticker).is_none());
        gameplay::movement::flush_player_auth_inputs_guarded(
            &mut ticker,
            8,
            Some(gameplay::test_support::survival_mining::evidence()),
            |identity, packet, guard, _| network.send_physics_packet(identity, packet, guard),
        )
        .unwrap();
        let sample = runtime.between_ticks_press(&ticker).unwrap();
        assert_eq!((sample.tick, sample.position), (103, preceding));
        let press = PressContext {
            tick: sample.tick,
            player_position: sample.position,
            ..press(PlayerInputMode::Mouse)
        };
        let missed = resolve_and_send(
            &mut runtime,
            &mut SwingTracker::default(),
            crosshair,
            &press,
            1,
            |packets| network.send_inventory_packets(packets),
        );
        if missed {
            assert!(ticker.mark_missed_swing(sample.tick));
        }
        ticker
            .enqueue_completed_physics(gameplay::test_support::survival_mining::completed(
                sample.tick,
            ))
            .unwrap();
        gameplay::movement::flush_player_auth_inputs_guarded(
            &mut ticker,
            8,
            Some(gameplay::test_support::survival_mining::evidence()),
            |identity, packet, guard, _| network.send_physics_packet(identity, packet, guard),
        )
        .unwrap();
        let packets = captured.drain();
        let expected = if crosshair == ZOMBIE {
            vec![
                "PlayerAuthInputPacket",
                "PlayerAuthInputPacket",
                "InventoryTransactionPacket",
                "AnimatePacket",
                "PlayerAuthInputPacket",
            ]
        } else {
            vec![
                "PlayerAuthInputPacket",
                "PlayerAuthInputPacket",
                "AnimatePacket",
                "PlayerAuthInputPacket",
            ]
        };
        assert_eq!(kinds(&packets), expected);
        for (index, tick) in [(0, 101), (1, 102), (packets.len() - 1, 103)] {
            let input = protocol::player_auth_input_trace_sample(&packets[index]).unwrap();
            assert_eq!(input.tick, tick);
            assert_eq!(
                input.flag_names.contains(&"MissedSwing"),
                missed && tick == 103
            );
        }
        if crosshair == ZOMBIE {
            let expected = protocol::use_actor_packet(protocol::ActorUseRequest {
                actor_runtime_id: 9,
                action: protocol::ActorUseAction::Attack,
                selected_slot: press.selection.as_ref().unwrap().slot,
                selected_item: press.selection.as_ref().unwrap().item.clone(),
                player_position: preceding,
                hit_position: [0.0, 1.5, -2.0],
            })
            .unwrap();
            assert_eq!(
                protocol::encode(&packets[2]).unwrap(),
                protocol::encode(&expected).unwrap()
            );
        }
    }
}
