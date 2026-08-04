//! Remote-data leniency and game-mode authority witnesses: semantically odd
//! but well-formed server values are skipped and counted without ending the
//! session, and mode changes never fabricate or discard authoritative stats.

use std::sync::Arc;

use protocol::{
    ActorAttribute, GameModeUpdate, HudEvent, PlayerGameMode, TitleAction, TitleEvent, UiEvent,
};
use ui::BoundedStat;

use super::*;

fn attribute(name: &str, current: f32, min: f32, max: f32) -> ActorAttribute {
    ActorAttribute {
        name: Arc::from(name),
        min,
        max,
        current,
        default: None,
        modifiers: Arc::from([]),
    }
}

fn local_attributes(
    session: u64,
    sequence: u64,
    attributes: Vec<ActorAttribute>,
) -> SequencedLocalAttributes {
    SequencedLocalAttributes {
        session_id: session,
        fifo_sequence: sequence,
        local_millis: sequence * 10,
        server_tick: sequence,
        attributes: attributes.into(),
    }
}

#[test]
fn semantically_odd_hud_values_are_skipped_counted_and_never_fatal() {
    let mut runtime = UiRuntime::new(1);
    // Establish authoritative baselines first.
    runtime
        .apply_local_attributes(local_attributes(
            1,
            1,
            vec![
                attribute("minecraft:health", 13.0, 0.0, 20.0),
                attribute("minecraft:player.hunger", 12.0, 0.0, 20.0),
            ],
        ))
        .unwrap();
    assert_eq!(
        runtime.hud().health(),
        BoundedStat::new_scaled(1_300, 2_000, 100)
    );

    // A well-formed packet whose health exceeds its own maximum is odd, not
    // fatal: the field is skipped, counted, and the baseline survives.
    runtime
        .apply_local_attributes(local_attributes(
            1,
            2,
            vec![attribute("minecraft:health", 25.0, 0.0, 20.0)],
        ))
        .unwrap();
    assert_eq!(
        runtime.hud().health(),
        BoundedStat::new_scaled(1_300, 2_000, 100)
    );

    // Non-finite hunger is equally odd and equally non-fatal.
    runtime
        .apply_local_attributes(local_attributes(
            1,
            3,
            vec![attribute("minecraft:player.hunger", f32::NAN, 0.0, 20.0)],
        ))
        .unwrap();
    assert_eq!(
        runtime.hud().hunger(),
        BoundedStat::new_scaled(1_200, 2_000, 100)
    );

    // Negative SetHealth is skipped, not a disconnect.
    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 4,
            local_millis: 40,
            server_tick: None,
            event: UiEvent::Hud(HudEvent::Health { health: -5 }),
        })
        .unwrap();
    assert_eq!(
        runtime.hud().health(),
        BoundedStat::new_scaled(1_300, 2_000, 100)
    );

    // Negative title durations keep the previous durations.
    let before = runtime.hud().durations();
    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 5,
            local_millis: 50,
            server_tick: None,
            event: UiEvent::Title(TitleEvent {
                action: TitleAction::SetDurations,
                text: Arc::from(""),
                document: None,
                fade_in_ticks: -1,
                stay_ticks: 40,
                fade_out_ticks: -3,
                xuid: Arc::from(""),
                platform_online_id: Arc::from(""),
                filtered_message: Arc::from(""),
            }),
        })
        .unwrap();
    assert_eq!(runtime.hud().durations(), before);

    assert_eq!(runtime.gameplay_hud().diagnostics().odd_attribute_values, 2);
    assert_eq!(runtime.gameplay_hud().diagnostics().odd_hud_packets, 2);
}

#[test]
fn oversized_chat_rows_are_skipped_and_counted_without_fatal() {
    let mut runtime = UiRuntime::new(1);
    let oversized = "x".repeat(2 * 1024 * 1024);
    runtime
        .apply(envelope(1, 1, text(&oversized)))
        .expect("an oversized server chat row is odd data, not a wire fault");
    assert!(runtime.chat().messages().is_empty());
    assert_eq!(runtime.gameplay_hud().diagnostics().oversized_chat_rows, 1);
    // The session continues normally afterwards.
    runtime.apply(envelope(1, 2, text("still alive"))).unwrap();
    assert_eq!(runtime.chat().messages().len(), 1);
}

#[test]
fn game_mode_changes_never_fabricate_or_discard_authoritative_stats() {
    let mut runtime = UiRuntime::new(1);
    runtime.publish_player_game_mode(PlayerGameMode::Survival);
    // No attributes have arrived: nothing is fabricated.
    assert_eq!(runtime.hud().health(), None);
    assert_eq!(runtime.hud().hunger(), None);

    runtime
        .apply_local_attributes(local_attributes(
            1,
            1,
            vec![
                attribute("minecraft:health", 13.0, 0.0, 20.0),
                attribute("minecraft:player.hunger", 9.0, 0.0, 20.0),
            ],
        ))
        .unwrap();

    // Creative hides the rows but the authority is retained, so returning to
    // survival presents the same server values.
    runtime.publish_player_game_mode(PlayerGameMode::Creative);
    assert_eq!(
        runtime.hud().health(),
        BoundedStat::new_scaled(1_300, 2_000, 100)
    );
    runtime.publish_player_game_mode(PlayerGameMode::Survival);
    assert_eq!(
        runtime.hud().health(),
        BoundedStat::new_scaled(1_300, 2_000, 100)
    );
    assert_eq!(
        runtime.hud().hunger(),
        BoundedStat::new_scaled(900, 2_000, 100)
    );
}

#[test]
fn fallback_and_default_game_type_resolve_against_the_world_default() {
    let mut runtime = UiRuntime::new(1);
    // StartGame: player mode is the level-default sentinel, world is creative.
    runtime.publish_bootstrap_game_modes(PlayerGameMode::Creative, PlayerGameMode::Creative, true);
    assert!(!runtime.survival_stats_visible());

    // An explicit runtime change detaches the player from the world default.
    runtime
        .apply(envelope(
            1,
            1,
            UiEvent::GameMode(protocol::GameModeEvent {
                update: GameModeUpdate::Explicit(PlayerGameMode::Survival),
            }),
        ))
        .unwrap();
    assert!(runtime.survival_stats_visible());

    // Changing the world default while explicit does not move the player.
    runtime
        .apply(envelope(
            1,
            2,
            UiEvent::DefaultGameMode(protocol::GameModeEvent {
                update: GameModeUpdate::Explicit(PlayerGameMode::Adventure),
            }),
        ))
        .unwrap();
    assert!(runtime.survival_stats_visible());

    // Returning to the level default re-binds to the updated default.
    runtime
        .apply(envelope(
            1,
            3,
            UiEvent::GameMode(protocol::GameModeEvent {
                update: GameModeUpdate::WorldDefault,
            }),
        ))
        .unwrap();
    assert!(runtime.survival_stats_visible(), "adventure shows stats");

    // While bound to the default, a default change moves the player with it.
    runtime
        .apply(envelope(
            1,
            4,
            UiEvent::DefaultGameMode(protocol::GameModeEvent {
                update: GameModeUpdate::Explicit(PlayerGameMode::Spectator),
            }),
        ))
        .unwrap();
    assert!(!runtime.survival_stats_visible());
    assert_eq!(runtime.selected_hotbar_slot(), None);

    // An unknown wire mode keeps the current authority and is counted.
    runtime
        .apply(envelope(
            1,
            5,
            UiEvent::GameMode(protocol::GameModeEvent {
                update: GameModeUpdate::Unknown(77),
            }),
        ))
        .unwrap();
    assert!(!runtime.survival_stats_visible());
    assert_eq!(runtime.gameplay_hud().diagnostics().odd_hud_packets, 1);
}
