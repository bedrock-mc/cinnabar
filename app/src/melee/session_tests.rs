//! The session FIFO must admit attack packets before their associated movement tick.

use gameplay::melee::{
    ActorHit, Crosshair, MeleeRuntime, PressContext, SwingTracker, resolve_and_send,
};
use protocol::PlayerInputMode;

/// Constructs a verified selected stack for one mouse attack.
fn press(input_mode: PlayerInputMode) -> PressContext {
    let stack = protocol::NetworkItemStack::empty();
    PressContext {
        tick: 101,
        player_position: [0.5, 2.620_01, 0.5],
        input_mode,
        local_runtime_id: 42,
        selection: Some(crate::mining::FrozenMiningSelection {
            slot: 3,
            item: protocol::VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest)
                .unwrap(),
        }),
        swing_duration: 6,
        now_millis: 1_000,
    }
}

/// Reads packet identifiers in the order accepted by the session queue.
fn kinds(packets: &[protocol::Packet]) -> Vec<String> {
    packets
        .iter()
        .map(|packet| format!("{:?}", packet.header.id))
        .collect()
}

const ZOMBIE: Crosshair = Crosshair::Actor(ActorHit {
    runtime_id: 9,
    distance: 2.0,
    point: [0.0, 1.5, -2.0],
});

#[test]
fn standalone_attack_packets_precede_their_tick_player_auth_input() {
    let (network, mut captured) = crate::runtime::network::NetworkHandle::stub_capturing_packets();
    let mut ticker = gameplay::test_support::survival_mining::ticker_with_ticks(1);
    for (crosshair, expected) in [
        (
            ZOMBIE,
            vec![
                "AnimatePacket",
                "InventoryTransactionPacket",
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
        crate::movement::flush_player_auth_inputs_guarded(
            &mut ticker,
            8,
            Some(gameplay::test_support::survival_mining::evidence()),
            |identity, packet, guard| network.send_physics_packet(identity, packet, guard),
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
    use crate::runtime::network::{BatchSendError, NetworkHandle};
    let (network, _open) = NetworkHandle::with_command_capacity(1);
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
        ["AnimatePacket", "InventoryTransactionPacket"]
    );
}
