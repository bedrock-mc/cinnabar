use super::*;
use protocol::{AbilitiesUpdate, AbilityLayerEvidence, AbilityLayersEvidence};

/// Builds empty or unavailable ability evidence without fabricating received layers.
fn update(owner: i64, count: u32) -> AbilitiesUpdate {
    AbilitiesUpdate {
        actor_unique_id: owner,
        player_permission: -1,
        command_permission: 255,
        layers: if count == 0 {
            AbilityLayersEvidence::Received([].into())
        } else {
            AbilityLayersEvidence::Unavailable {
                declared_layers: count,
            }
        },
    }
}

#[test]
fn unknown_received_empty_and_unavailable_preserve_their_distinct_origins() {
    let mut runtime = LocalPlayerFacts::new(7);
    assert!(runtime.local_abilities().is_none());
    runtime.bind_local_abilities(7, 4, 0, true);
    assert!(
        runtime.local_abilities().is_none(),
        "binding is not a packet receipt"
    );
    runtime.apply_local_abilities(7, 4, 1, update(0, 0));
    assert_eq!(runtime.local_abilities(), Some(&update(0, 0)));
    runtime.apply_local_abilities(7, 4, 2, update(0, 33));
    assert_eq!(runtime.local_abilities(), Some(&update(0, 33)));
}

#[test]
fn wrong_identity_session_stream_and_duplicate_fifo_cannot_retire_latest_evidence() {
    let mut runtime = LocalPlayerFacts::new(7);
    runtime.bind_local_abilities(7, 4, 17, true);
    runtime.apply_local_abilities(7, 4, 5, update(17, 0));
    for (session, stream, sequence, owner) in [
        (6, 4, 6, 17),
        (7, 3, 6, 17),
        (7, 4, 6, 0),
        (7, 4, 5, 17),
        (7, 4, 4, 17),
    ] {
        runtime.apply_local_abilities(session, stream, sequence, update(owner, 33));
        assert_eq!(runtime.local_abilities(), Some(&update(17, 0)));
    }
}

#[test]
fn retired_and_failed_repeat_bindings_cannot_be_resurrected_by_drain_or_clone() {
    let mut runtime = LocalPlayerFacts::new(7);
    runtime.bind_local_abilities(7, 4, 17, true);
    runtime.apply_local_abilities(7, 4, 1, update(17, 0));
    runtime.begin_session(7);
    assert!(
        runtime.local_abilities().is_some(),
        "same-session begin is a no-op"
    );
    runtime.clear_local_abilities();
    runtime.bind_local_abilities(7, 4, 17, false);
    runtime.synchronize_local_abilities(7, Some(4));
    runtime.apply_local_abilities(7, 4, 2, update(17, 0));
    assert!(runtime.local_abilities().is_none());
    let mut clone = runtime.clone();
    clone.apply_local_abilities(7, 4, 3, update(17, 0));
    assert!(clone.local_abilities().is_none());
    runtime.bind_local_abilities(7, 5, 17, true);
    runtime.apply_local_abilities(7, 5, 1, update(17, 0));
    runtime.synchronize_local_abilities(7, Some(6));
    assert!(runtime.local_abilities().is_none());
    runtime.bind_local_abilities(7, 6, 17, true);
    runtime.apply_local_abilities(7, 6, 1, update(17, 0));
    runtime.begin_session(8);
    assert!(runtime.local_abilities().is_none());
}

/// A confirmed mode switch changes mining immediately without rewriting wire evidence.
#[test]
fn creative_to_survival_keeps_abilities_but_stops_instant_destruction() {
    use crate::game_mode_capabilities::ability_bit::INSTANT_BUILD;
    use protocol::PlayerGameMode::{Creative, Survival};

    let mut runtime = LocalPlayerFacts::new(7);
    runtime.publish_bootstrap_game_modes(Creative, Creative, false);
    runtime.bind_local_abilities(7, 4, 17, true);
    let evidence = AbilitiesUpdate {
        layers: AbilityLayersEvidence::Received(
            [AbilityLayerEvidence {
                layer_type: 1,
                abilities: INSTANT_BUILD,
                values: INSTANT_BUILD,
                fly_speed_bits: 0,
                vertical_fly_speed_bits: 0,
                walk_speed_bits: 0,
            }]
            .into(),
        ),
        ..update(17, 0)
    };
    runtime.apply_local_abilities(7, 4, 1, evidence.clone());
    assert!(runtime.game_mode_capabilities().unwrap().instant_break);
    assert!(runtime.apply_game_mode_update(GameModeUpdate::Explicit(Survival)));
    assert!(!runtime.game_mode_capabilities().unwrap().instant_break);
    assert_eq!(runtime.local_abilities(), Some(&evidence));
    assert!(runtime.apply_game_mode_update(GameModeUpdate::Explicit(Creative)));
    assert!(runtime.game_mode_capabilities().unwrap().instant_break);
}
