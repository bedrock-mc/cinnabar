use super::*;
use crate::ui_runtime::SequencedUiEvent;
use protocol::{BossAction, BossColor, BossEvent, BossOverlay, BossStyle, UiEvent};

fn event(action: BossAction, target_entity_id: i64) -> BossEvent {
    BossEvent {
        target_entity_id,
        action,
        title: "Dragon".into(),
        filtered_title: "Filtered dragon".into(),
        progress: 0.75,
        style: BossStyle {
            color: BossColor::Purple,
            overlay: BossOverlay::Progress,
            darken_sky: None,
            create_world_fog: None,
        },
    }
}

fn apply(
    runtime: &mut UiRuntime,
    player: &mut player_state::PlayerState,
    sequence: u64,
    event: BossEvent,
) {
    runtime
        .apply(
            player,
            SequencedUiEvent {
                session_id: runtime.session_id(),
                fifo_sequence: sequence,
                local_millis: sequence,
                server_tick: None,
                event: UiEvent::Boss(event),
            },
        )
        .unwrap();
}

#[test]
fn boss_subscription_lifecycle_replies_retry_in_order_without_duplicates() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    let show = event(BossAction::Show, -17);
    let hide = event(BossAction::Hide, -17);
    apply(&mut runtime, &mut player, 1, show.clone());
    apply(&mut runtime, &mut player, 2, show.clone());
    apply(
        &mut runtime,
        &mut player,
        3,
        event(BossAction::SetProgress, -17),
    );
    apply(&mut runtime, &mut player, 4, hide.clone());
    apply(&mut runtime, &mut player, 5, hide.clone());
    assert!(runtime.boss_bars().stacked().is_empty());
    assert_eq!(
        runtime.flush_boss_responses(|_| Err(FormTransportError::Full)),
        Err(FormTransportError::Full)
    );
    let mut packets = Vec::new();
    assert_eq!(
        runtime.flush_boss_responses(|packet| {
            packets.push(packet);
            Ok(())
        }),
        Ok(2)
    );
    assert_eq!(
        packets,
        [
            protocol::boss_registration_response(&show).unwrap(),
            protocol::boss_registration_response(&hide).unwrap(),
        ]
    );
    assert_eq!(
        runtime.flush_boss_responses(|_| panic!("duplicate reply")),
        Ok(0)
    );
}

#[test]
fn boss_replies_retire_with_the_session_without_retaining_dead_bars() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    apply(&mut runtime, &mut player, 1, event(BossAction::Show, 17));
    assert_eq!(runtime.boss_responses.pending.len(), 1);
    player.begin_session(2);
    runtime.begin_session(2);
    assert!(runtime.boss_bars().stacked().is_empty());
    assert_eq!(
        runtime.flush_boss_responses(|_| panic!("old session reply")),
        Ok(0)
    );
    apply(&mut runtime, &mut player, 1, event(BossAction::Show, 18));
    assert_eq!(runtime.flush_boss_responses(|_| Ok(())), Ok(1));
}

#[test]
fn repeated_boss_lifetimes_under_backpressure_keep_a_bounded_outbox() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    let mut sequence = 0;
    for id in 0..(MAX_PENDING_RESPONSES as i64 * 2) {
        sequence += 1;
        apply(
            &mut runtime,
            &mut player,
            sequence,
            event(BossAction::Show, id),
        );
        sequence += 1;
        apply(
            &mut runtime,
            &mut player,
            sequence,
            event(BossAction::Hide, id),
        );
        assert!(runtime.boss_responses.pending.len() <= MAX_PENDING_RESPONSES);
    }
    assert!(runtime.boss_responses.coalesced_pairs > 0);
    assert_eq!(runtime.boss_responses.dropped, 0);
    let mut membership = std::collections::BTreeSet::new();
    for event in &runtime.boss_responses.pending {
        match event.action {
            BossAction::Show => {
                assert!(membership.insert(event.target_entity_id));
            }
            BossAction::Hide => {
                assert!(membership.remove(&event.target_entity_id));
            }
            _ => panic!("not a subscription response"),
        }
    }
    assert!(membership.is_empty());
}

#[test]
fn closed_transport_retires_boss_responses_and_stale_input_cannot_requeue_them() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    let show = event(BossAction::Show, 17);
    apply(&mut runtime, &mut player, 1, show.clone());
    assert_eq!(
        runtime.flush_boss_responses(|_| Err(FormTransportError::Closed)),
        Err(FormTransportError::Closed)
    );
    assert_eq!(
        runtime.flush_boss_responses(|_| panic!("closed response retry")),
        Ok(0)
    );
    assert!(
        runtime
            .apply(
                &mut player,
                SequencedUiEvent {
                    session_id: 1,
                    fifo_sequence: 1,
                    local_millis: 1,
                    server_tick: None,
                    event: UiEvent::Boss(show),
                }
            )
            .is_err()
    );
    assert_eq!(
        runtime.flush_boss_responses(|_| panic!("stale response")),
        Ok(0)
    );
}
