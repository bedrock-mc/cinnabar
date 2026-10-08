use super::*;

fn attributes(sequence: u64, local_millis: u64, server_tick: u64) -> SequencedLocalAttributes {
    SequencedLocalAttributes {
        session_id: 1,
        fifo_sequence: sequence,
        local_millis,
        server_tick,
        attributes: Arc::from([]),
    }
}

fn remaining(runtime: &UiRuntime, effect_id: i32, millis: u64) -> Option<Option<u64>> {
    let tick = runtime.estimated_server_tick(millis);
    runtime
        .gameplay_hud()
        .effects()
        .iter()
        .find(|effect| effect.effect_id == effect_id && effect.visible_at_tick(tick))
        .map(|effect| effect.remaining_ticks(tick))
}

#[test]
fn zero_and_lagging_attributes_preserve_countdown_and_fractional_elapsed_time() {
    for advancing_wire_ticks in [false, true] {
        let mut runtime = UiRuntime::new(1);
        let mut player = player_state::PlayerState::new(1);
        runtime
            .apply_local_effect(1, 1, effect(ActorEffectAction::Add, 1, 80, 0), 1_000)
            .unwrap();
        // Packets arrive more often than one presentation tick. Lagging wire ticks
        // advance once per second; neither case may discard the 10 ms fractions.
        for sample in 1..=400 {
            let millis = 1_000 + sample * 10;
            let wire_tick = if advancing_wire_ticks {
                sample / 100
            } else {
                0
            };
            runtime
                .apply_local_attributes(&mut player, attributes(sample + 1, millis, wire_tick))
                .unwrap();
            let elapsed_ticks = sample / 5;
            assert_eq!(runtime.estimated_server_tick(millis), Some(elapsed_ticks));
            assert_eq!(
                remaining(&runtime, 1, millis),
                (elapsed_ticks < 80).then_some(Some(80 - elapsed_ticks)),
                "sample {sample}, advancing wire ticks {advancing_wire_ticks}"
            );
        }
        runtime.expire_gameplay_effects(5_000);
        assert!(runtime.gameplay_hud().effects().is_empty());
    }
}

#[test]
fn zero_tick_add_update_remove_and_infinite_effects_keep_independent_durations() {
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply_local_effect(1, 1, effect(ActorEffectAction::Add, 1, 100, 0), 1_000)
        .unwrap();
    runtime
        .apply_local_effect(1, 2, effect(ActorEffectAction::Add, 10, 160, 0), 6_000)
        .unwrap();
    assert_eq!(remaining(&runtime, 1, 6_000), None);
    assert_eq!(remaining(&runtime, 10, 6_000), Some(Some(160)));
    runtime.expire_gameplay_effects(6_000);
    assert_eq!(runtime.gameplay_hud().effects().len(), 1);

    runtime
        .apply_local_effect(1, 3, effect(ActorEffectAction::Add, 12, -1, 0), 6_500)
        .unwrap();
    assert_eq!(remaining(&runtime, 12, 6_500), Some(None));
    assert_eq!(remaining(&runtime, 10, 6_500), Some(Some(150)));
    runtime
        .apply_local_effect(1, 4, effect(ActorEffectAction::Remove, 12, 0, 0), 7_000)
        .unwrap();
    assert_eq!(remaining(&runtime, 12, 7_000), None);
    assert_eq!(remaining(&runtime, 10, 7_000), Some(Some(140)));

    runtime
        .apply_local_effect(1, 5, effect(ActorEffectAction::Update, 10, 40, 0), 7_500)
        .unwrap();
    assert_eq!(remaining(&runtime, 10, 7_500), Some(Some(40)));
    assert_eq!(remaining(&runtime, 10, 8_500), Some(Some(20)));
    runtime
        .apply_local_effect(1, 6, effect(ActorEffectAction::Remove, 10, 0, 0), 8_500)
        .unwrap();
    assert_eq!(remaining(&runtime, 10, 8_500), None);
}

#[test]
fn presentation_estimate_does_not_replace_incoming_tick_ordering_or_survive_session_reset() {
    let mut runtime = UiRuntime::new(1);
    let mut player = player_state::PlayerState::new(1);
    runtime
        .apply_local_effect(1, 1, effect(ActorEffectAction::Add, 1, 100, 40), 1_000)
        .unwrap();
    runtime
        .apply_local_attributes(&mut player, attributes(2, 3_000, 40))
        .unwrap();
    runtime
        .apply_local_attributes(&mut player, attributes(3, 3_020, 41))
        .unwrap();
    assert_eq!(runtime.estimated_server_tick(3_020), Some(80));
    assert_eq!(remaining(&runtime, 1, 3_020), Some(Some(60)));
    assert!(matches!(
        runtime.apply_local_attributes(&mut player, attributes(4, 3_030, 40)),
        Err(UiRuntimeError::NonMonotonicServerTick {
            previous: 41,
            actual: 40
        })
    ));
    assert_eq!(runtime.estimated_server_tick(3_030), Some(80));
    assert!(matches!(
        runtime.apply_local_effect(1, 3, effect(ActorEffectAction::Remove, 1, 0, 0), 3_040),
        Err(UiRuntimeError::StaleFifoSequence { .. })
    ));
    assert_eq!(remaining(&runtime, 1, 3_040), Some(Some(60)));

    runtime.begin_session(2);
    assert_eq!(runtime.estimated_server_tick(20_000), None);
    assert!(runtime.gameplay_hud().effects().is_empty());
    assert!(matches!(
        runtime.apply_local_effect(1, 1, effect(ActorEffectAction::Add, 1, 20, 0), 20_000),
        Err(UiRuntimeError::WrongSession { .. })
    ));
    assert_eq!(runtime.estimated_server_tick(20_000), None);
    runtime
        .apply_local_effect(2, 1, effect(ActorEffectAction::Add, 1, 20, 0), 20_000)
        .unwrap();
    assert_eq!(remaining(&runtime, 1, 20_000), Some(Some(20)));
    assert_eq!(remaining(&runtime, 1, 21_000), None);
}
