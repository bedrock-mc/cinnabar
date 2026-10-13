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
        let mut melee = attack_painting(&ticker, &support);
        let mut mining = SurvivalMiningRuntime::default();
        let mut predicted = Vec::new();
        let held = mining.observe_attack_input(true, true, true, melee.actor_in_front());
        assert!(held);
        mining.break_on_press(
            &ticker,
            DestroyInput::Held(None),
            authority,
            |_| {},
            |_| panic!("the painting owns the initial press"),
        );

        assert!(!melee.observe_input(false, false));
        melee.cancel();
        let eligible = mining.observe_attack_input(true, false, false, melee.actor_in_front());
        let input = if eligible {
            DestroyInput::Held(Some(&support))
        } else {
            DestroyInput::Released
        };
        mining.break_on_press(
            &ticker,
            input,
            authority,
            |_| {},
            |cell| predicted.push(cell),
        );
        let next = ticker.completed_tick() + 1;
        ticker.enqueue_completed_physics(completed(next)).unwrap();
        mining.step_ticks(
            &mut ticker,
            input,
            authority,
            |_| {},
            |_, _| None,
            |cell| predicted.push(cell),
        );
        let mut packets = Vec::new();
        flush_player_auth_inputs(&mut ticker, 8, Some(evidence()), |_, packet| {
            packets.push(packet);
            Ok::<_, ()>(())
        })
        .unwrap();
        assert!(
            predicted.is_empty(),
            "a released actor press broke its support: {predicted:?}"
        );
        for packet in packets {
            let sample = protocol::player_auth_input_trace_sample(&packet).unwrap();
            assert!(!sample.flag_names.contains(&"PerformBlockActions"));
            assert!(!sample.flag_names.contains(&"PerformItemInteraction"));
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
    let mut mining = SurvivalMiningRuntime::default();
    assert!(mining.observe_attack_input(true, true, false, false));
    assert!(mining.observe_attack_input(true, false, false, false));
    let mut predicted = Vec::new();
    mining.break_on_press(
        &ticker,
        DestroyInput::Held(Some(&support)),
        Server,
        |_| {},
        |cell| predicted.push(cell),
    );
    assert_eq!(predicted, [support.position]);
}

#[test]
fn a_held_attack_can_mine_after_the_actor_moves_away() {
    let mut ticker = ticker_with_ticks(1);
    flush_player_auth_inputs(&mut ticker, 8, Some(evidence()), |_, _| Ok::<_, ()>(())).unwrap();
    let support = DestroyTarget {
        instant: true,
        ..target([3, 4, 5], "minecraft:stone", None)
    };
    let mut melee = attack_painting(&ticker, &support);
    let mut mining = SurvivalMiningRuntime::default();
    assert!(mining.observe_attack_input(true, true, true, melee.actor_in_front()));
    mining.break_on_press(
        &ticker,
        DestroyInput::Held(None),
        Server,
        |_| {},
        |_| panic!("the actor is still in front"),
    );
    melee.observe_crosshair(Crosshair::Block);
    assert!(mining.observe_attack_input(true, false, true, melee.actor_in_front()));
    let mut predicted = Vec::new();
    mining.break_on_press(
        &ticker,
        DestroyInput::Held(Some(&support)),
        Server,
        |_| {},
        |cell| predicted.push(cell),
    );
    assert_eq!(predicted, [support.position]);
}
