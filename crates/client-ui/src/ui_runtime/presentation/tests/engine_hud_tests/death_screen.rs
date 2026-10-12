use super::*;

/// The gameplay HUD stays hidden behind the death screen and returns on respawn.
#[test]
fn the_death_screen_hides_the_gameplay_hud() {
    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping the_death_screen_hides_the_gameplay_hud: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut player_runtime = player_state::PlayerState::new(1);
    let runtime = UiRuntime::new(1);
    player_runtime
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    player_runtime.inventory.set_local_selected_slot(0);
    let shown = build(&player_runtime, &mut presentation, &runtime, 0);

    presentation.hud_frame_mut().death_screen = true;
    let hidden = build(&player_runtime, &mut presentation, &runtime, 0);
    assert!(hidden.vertices.len() < shown.vertices.len());

    presentation.hud_frame_mut().death_screen = false;
    let restored = build(&player_runtime, &mut presentation, &runtime, 0);
    assert_eq!(restored.vertices.len(), shown.vertices.len());
}
