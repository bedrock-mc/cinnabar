use super::*;

#[test]
fn boss_removal_rebuilds_the_hud_without_cached_dead_tracks() {
    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping boss_removal_rebuilds_the_hud_without_cached_dead_tracks: requires local carriers (make assets)"
        );
        return;
    };
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    for (sequence, action, id, title) in [
        (1, ProtocolBossAction::Show, 7, "Dragon"),
        (2, ProtocolBossAction::Show, 8, "Other boss"),
    ] {
        runtime
            .apply(
                &mut player,
                SequencedUiEvent {
                    session_id: 1,
                    fifo_sequence: sequence,
                    local_millis: 0,
                    server_tick: None,
                    event: boss_event(
                        action,
                        id,
                        title,
                        0.75,
                        ProtocolBossColor::Purple,
                        ProtocolBossOverlay::Progress,
                    ),
                },
            )
            .unwrap();
    }
    build(&player, &mut presentation, &runtime, 0);
    assert!(text(presentation.hud_draw_nodes(), "Dragon").is_some());
    runtime
        .apply(
            &mut player,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 3,
                local_millis: 0,
                server_tick: None,
                event: boss_event(
                    ProtocolBossAction::Hide,
                    7,
                    "",
                    0.0,
                    ProtocolBossColor::Purple,
                    ProtocolBossOverlay::Progress,
                ),
            },
        )
        .unwrap();
    build(&player, &mut presentation, &runtime, 0);
    let nodes = presentation.hud_draw_nodes();
    assert!(text(nodes, "Dragon").is_none());
    assert!(text(nodes, "Other boss").is_some());
    assert_eq!(customs(nodes, "java_boss_notches").len(), 1);
}
