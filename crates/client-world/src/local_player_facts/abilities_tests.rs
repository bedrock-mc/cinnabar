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

/// Builds an original Instabuild definition in one chosen ability layer.
fn instabuild_update(layer_type: u16, value: bool) -> AbilitiesUpdate {
    let bit = crate::game_mode_capabilities::ability_bit::INSTANT_BUILD;
    AbilitiesUpdate {
        layers: AbilityLayersEvidence::Received(
            [AbilityLayerEvidence {
                layer_type,
                abilities: bit,
                values: if value { bit } else { 0 },
                fly_speed_bits: 0,
                vertical_fly_speed_bits: 0,
                walk_speed_bits: 0,
            }]
            .into(),
        ),
        ..update(17, 0)
    }
}

#[test]
fn instabuild_follows_mode_assignment_and_accepted_ability_replacement_order() {
    use protocol::PlayerGameMode::*;
    let mut facts = LocalPlayerFacts::new(7);
    facts.publish_bootstrap_game_modes(Creative, Creative, false);
    facts.bind_local_abilities(7, 4, 17, true);
    assert_eq!(facts.instabuild(), Some(true));
    assert!(facts.local_abilities().is_none());
    facts.apply_local_abilities(7, 4, 1, update(17, 0));
    assert_eq!(facts.instabuild(), Some(false));
    facts.publish_player_game_mode(Creative);
    assert_eq!(facts.instabuild(), Some(true));
    facts.apply_local_abilities(7, 4, 2, instabuild_update(1, false));
    assert_eq!(facts.instabuild(), Some(false));
    facts.publish_player_game_mode(Creative);
    assert_eq!(facts.instabuild(), Some(true));
    facts.apply_local_abilities(7, 4, 3, instabuild_update(3, false));
    facts.publish_player_game_mode(Creative);
    assert_eq!(facts.instabuild(), Some(false));
    facts.apply_local_abilities(7, 4, 4, update(17, 0));
    facts.publish_player_game_mode(Survival);
    assert_eq!(facts.instabuild(), Some(false));
    facts.begin_session(8);
    assert_eq!(facts.instabuild(), Some(false));
}

#[test]
fn mode_assignment_rebuilds_spectator_layer_but_preserves_higher_definitions() {
    use protocol::PlayerGameMode::*;
    let mut facts = LocalPlayerFacts::new(7);
    facts.bind_local_abilities(7, 4, 17, true);
    facts.apply_local_abilities(7, 4, 1, instabuild_update(2, false));
    facts.publish_player_game_mode(Creative);
    assert_eq!(facts.instabuild(), Some(true));
    facts.publish_player_game_mode(Spectator);
    assert_eq!(facts.instabuild(), Some(false));
    facts.apply_local_abilities(7, 4, 2, instabuild_update(3, true));
    facts.publish_player_game_mode(Spectator);
    assert_eq!(facts.instabuild(), Some(true));
    facts.publish_player_game_mode(Survival);
    assert_eq!(facts.instabuild(), Some(true));
}

#[test]
fn unavailable_and_rejected_ability_evidence_does_not_invent_instabuild() {
    let mut facts = LocalPlayerFacts::new(7);
    facts.bind_local_abilities(7, 4, 17, true);
    facts.apply_local_abilities(7, 4, 1, update(17, 33));
    assert_eq!(facts.instabuild(), None);
    facts.publish_player_game_mode(protocol::PlayerGameMode::Creative);
    assert_eq!(facts.instabuild(), None);
    facts.apply_local_abilities(7, 4, 2, instabuild_update(3, true));
    for (session, stream, sequence, owner) in
        [(6, 4, 3, 17), (7, 5, 3, 17), (7, 4, 3, 19), (7, 4, 2, 17)]
    {
        facts.apply_local_abilities(session, stream, sequence, update(owner, 0));
        assert_eq!(facts.instabuild(), Some(true));
    }
    facts.apply_local_abilities(7, 4, 3, update(17, 0));
    assert_eq!(facts.instabuild(), Some(false));
}

/// Admitted ability evidence keeps collision and flight permissions independent.
#[test]
fn ability_axes_local_facts_keep_permissions_independent() {
    use crate::game_mode_capabilities::{FlightAuthority, ability_bit as b};
    use protocol::PlayerGameMode::{Spectator, Survival};
    for (values, expected_flight, collision) in [
        (b::FLYING, FlightAuthority::Mutable(true), true),
        (b::NO_CLIP, FlightAuthority::Mutable(false), false),
        (
            b::FLYING | b::NO_CLIP,
            FlightAuthority::Mutable(true),
            false,
        ),
    ] {
        let mut runtime = LocalPlayerFacts::new(7);
        runtime.publish_bootstrap_game_modes(Survival, Survival, false);
        runtime.bind_local_abilities(7, 4, 17, true);
        runtime.apply_local_abilities(
            7,
            4,
            1,
            AbilitiesUpdate {
                layers: AbilityLayersEvidence::Received(
                    [AbilityLayerEvidence {
                        layer_type: 1,
                        abilities: b::FLYING | b::NO_CLIP,
                        values,
                        fly_speed_bits: 0,
                        vertical_fly_speed_bits: 0,
                        walk_speed_bits: 0,
                    }]
                    .into(),
                ),
                ..update(17, 0)
            },
        );
        let caps = runtime.game_mode_capabilities().unwrap();
        assert_eq!(caps.flight, expected_flight);
        assert_eq!(caps.has_collision, collision);
    }
    let mut spectator = LocalPlayerFacts::new(7);
    spectator.publish_bootstrap_game_modes(Spectator, Spectator, false);
    let caps = spectator.game_mode_capabilities().unwrap();
    assert_eq!(caps.flight, FlightAuthority::Fixed(true));
    assert!(!caps.has_collision);
}
