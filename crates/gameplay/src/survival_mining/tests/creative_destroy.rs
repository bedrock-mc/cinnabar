//! Creative actions share their input tick's retry and cancellation lifetime.

use super::*;

#[test]
fn an_instant_destroy_carries_the_creative_player_action() {
    let target = DestroyTarget {
        instant: true,
        ..target([3, 4, 5], "minecraft:stone", None)
    };
    for authority in [Server, Client] {
        let payload = held(&mut DestroyMachine::default(), &target, authority);
        assert_eq!(
            payload.creative_destroy,
            Some(CreativeDestroyBlock {
                local_runtime_id: target.local_runtime_id,
                position: target.position,
                face: target.face,
            })
        );
    }
}

#[test]
fn creative_destroy_retries_with_its_input_and_later_ticks_cannot_overtake_it() {
    let target = DestroyTarget {
        instant: true,
        ..target([3, 4, 5], "minecraft:stone", None)
    };
    let mut ticker = ticker_with_ticks(2);
    SurvivalMiningRuntime::default().step_ticks(
        &mut ticker,
        DestroyInput::Held(Some(&target)),
        Server,
        |_| {},
        |_, _| None,
        |_| {},
    );
    let mut attempted = Vec::new();
    let result = crate::movement::flush_player_auth_inputs_guarded(
        &mut ticker,
        usize::MAX,
        Some(evidence()),
        |identity, _, _, _| {
            attempted.push(identity.tick.get());
            Err("full")
        },
    );
    assert_eq!(
        result,
        Err(crate::movement::MovementSendError::Transport("full"))
    );
    assert_eq!(attempted, [101]);
    let mut delivered = Vec::new();
    crate::movement::flush_player_auth_inputs_guarded(
        &mut ticker,
        usize::MAX,
        Some(evidence()),
        |_, packet, guard, _| {
            if let Some(guard) = guard {
                let (preceding, packet) = guard.sanitize(packet);
                delivered.extend(preceding);
                delivered.push(packet);
            } else {
                delivered.push(packet);
            }
            Ok::<_, ()>(())
        },
    )
    .unwrap();
    assert_eq!(delivered.len(), 3);
    assert_eq!(
        protocol::encode(&delivered[0]).unwrap(),
        protocol::encode(&protocol::creative_destroy_block_packet(
            target.local_runtime_id,
            target.position,
            target.face
        ),)
        .unwrap()
    );
    assert_eq!(
        protocol::test_support::packets::predicted_block_breaks(&delivered[1]),
        [target.position]
    );
    let ticks: Vec<_> = delivered
        .iter()
        .filter_map(protocol::player_auth_input_trace_sample)
        .map(|sample| sample.tick)
        .collect();
    assert_eq!(ticks, [101, 102]);
}
