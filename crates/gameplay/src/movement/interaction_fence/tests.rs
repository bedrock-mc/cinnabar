use super::*;
use crate::test_support::survival_mining::{completed, evidence, ticker_with_ticks};

#[test]
fn interaction_fence_preserves_the_original_tick_and_allows_older_inputs() {
    let mut ticker = ticker_with_ticks(3);
    let fence = ticker.reserve_interaction_before(102);
    let mut ticks = Vec::new();
    let mut flush = |ticker: &mut MovementTicker| {
        flush_player_auth_inputs_guarded(
            ticker,
            OUTBOX_CAPACITY,
            Some(evidence()),
            |identity, _, _, _| {
                ticks.push(identity.tick.get());
                Ok::<_, ()>(())
            },
        )
        .unwrap()
    };
    assert_eq!(flush(&mut ticker), 1);
    assert!(ticker.interaction_tick_available(102));
    assert_eq!(flush(&mut ticker), 0);
    drop(fence);
    assert_eq!(flush(&mut ticker), 2);
    assert_eq!(ticks, [101, 102, 103]);
}

#[test]
fn interaction_fence_cannot_survive_its_position_authority() {
    let mut ticker = ticker_with_ticks(1);
    let _old = ticker.reserve_interaction_before(101);
    ticker.reset(8, 200, [0.0; 3]);
    ticker.set_source(MovementSource::Physics);
    ticker.enqueue_completed_physics(completed(201)).unwrap();
    let mut sent = Vec::new();
    flush_player_auth_inputs_guarded(
        &mut ticker,
        OUTBOX_CAPACITY,
        Some(evidence()),
        |identity, _, _, _| {
            sent.push(identity.tick.get());
            Ok::<_, ()>(())
        },
    )
    .unwrap();
    assert_eq!(sent, [201]);
}
