use super::*;
use crate::melee::{ActorHit, Crosshair, MeleeRuntime, PressContext, SwingTracker};

/// Resolves an actor attack before the block-mining owner observes the same press.
fn attack_painting(ticker: &MovementTicker, support: &DestroyTarget) -> MeleeRuntime {
    let mut melee = MeleeRuntime::default();
    assert!(melee.observe_input(true, true));
    let sample = ticker.between_ticks_sample().unwrap();
    let outcome = melee.resolve(
        Crosshair::Actor(ActorHit {
            runtime_id: 9,
            distance: 2.0,
            point: [0.0, 1.5, -2.0],
        }),
        &PressContext {
            refused_destroy: None,
            tick: sample.tick,
            player_position: sample.position,
            input_mode: protocol::PlayerInputMode::Mouse,
            local_runtime_id: 8,
            selection: Some(support.selection.clone()),
            swing_duration: 6,
            item_attack: None,
            now_millis: 0,
        },
        &mut SwingTracker::default(),
    );
    assert_eq!(outcome.packets.len(), 2, "the actor attack was emitted");
    assert!(melee.actor_in_front());
    melee
}

#[test]
fn released_painting_attack_does_not_replay_a_support_block_destroy() {
    for authority in [Server, Client] {
        let mut ticker = ticker_with_ticks(1);
        flush_player_auth_inputs(&mut ticker, 8, Some(evidence()), |_, _| Ok::<_, ()>(())).unwrap();
        let support = DestroyTarget {
            instant: true,
            ..target([3, 4, 5], "minecraft:stone", None)
        };
        let mut harness = MiningAdmission::new(ticker);
        harness.melee = attack_painting(&harness.ticker, &support);
        harness.frame(DestroyInput::Held(None), authority);
        assert!(harness.predictions.is_empty());
        assert!(!harness.melee.observe_input(false, false));
        harness.melee.cancel();
        harness.frame(DestroyInput::Released, authority);
        let next = harness.ticker.completed_tick() + 1;
        harness
            .ticker
            .enqueue_completed_physics(completed(next))
            .unwrap();
        harness.frame(DestroyInput::Released, authority);
        harness.flush();
        assert!(
            harness.predictions.is_empty(),
            "a released actor press broke its support"
        );
        for packet in harness
            .packets
            .iter()
            .filter_map(protocol::player_auth_input_trace_sample)
        {
            assert!(!packet.flag_names.contains(&"PerformBlockActions"));
            assert!(!packet.flag_names.contains(&"PerformItemInteraction"));
        }
    }
}

#[test]
fn an_unstepped_block_attack_survives_release_until_its_destroy_is_carried() {
    let mut ticker = ticker_with_ticks(1);
    flush_player_auth_inputs(&mut ticker, 8, Some(evidence()), |_, _| Ok::<_, ()>(())).unwrap();
    let support = DestroyTarget {
        instant: true,
        ..target([3, 4, 5], "minecraft:stone", None)
    };
    let mut harness = MiningAdmission::new(ticker);
    assert!(harness.melee.observe_input(true, false));
    assert!(harness.melee.observe_input(false, false));
    harness.frame(DestroyInput::Held(Some(&support)), Server);
    assert_eq!(harness.predictions, [support.position]);
}

#[test]
fn a_held_attack_can_mine_after_the_actor_moves_away() {
    let mut ticker = ticker_with_ticks(1);
    flush_player_auth_inputs(&mut ticker, 8, Some(evidence()), |_, _| Ok::<_, ()>(())).unwrap();
    let support = DestroyTarget {
        instant: true,
        ..target([3, 4, 5], "minecraft:stone", None)
    };
    let mut harness = MiningAdmission::new(ticker);
    harness.melee = attack_painting(&harness.ticker, &support);
    harness.frame(DestroyInput::Held(None), Server);
    harness.melee.observe_crosshair(Crosshair::Block);
    harness.frame(DestroyInput::Held(Some(&support)), Server);
    assert!(
        harness.predictions.is_empty(),
        "held mining waits for its next tick"
    );
    let next = harness.ticker.completed_tick() + 1;
    harness
        .ticker
        .enqueue_completed_physics(completed(next))
        .unwrap();
    harness.frame(DestroyInput::Held(Some(&support)), Server);
    assert_eq!(harness.predictions, [support.position]);
}
