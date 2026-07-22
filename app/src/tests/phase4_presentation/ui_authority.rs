use super::*;

#[test]
fn committed_local_mode_authority_updates_the_live_ui_runtime() {
    let mut ui_runtime = UiRuntime::new(7);
    ui_runtime.publish_player_game_mode(PlayerGameMode::Survival);

    apply_committed_ui_event(
        &mut ui_runtime,
        7,
        100,
        CommittedUiEvent::LocalGameMode {
            sequence: 3,
            game_mode: PlayerGameMode::Spectator,
        },
    )
    .expect("ordered local mode authority reaches the UI runtime");

    assert_eq!(
        ui_runtime.player_game_mode(),
        Some(PlayerGameMode::Spectator)
    );
    assert!(!ui_runtime.survival_stats_visible());
}
