use super::super::retained_hud::{
    BOSS_PANEL_HEIGHT, BOSS_PANEL_WIDTH, BOSS_PROGRESS_HEIGHT, BOSS_PROGRESS_TOP,
    BOSS_STACK_VIEWPORT_FRACTION, MAX_PRESENTED_SCOREBOARD_ROWS, SCOREBOARD_HORIZONTAL_PADDING,
    SCOREBOARD_LIST_OFFSET, SCOREBOARD_MAIN_HORIZONTAL_EXPANSION, SCOREBOARD_NAME_WIDTH,
    SCOREBOARD_TEXT_HEIGHT, SCOREBOARD_TITLE_BACKGROUND_HEIGHT, SCOREBOARD_TITLE_WIDTH,
    project_boss_bars, project_scoreboard_sidebar,
};
use super::*;

#[test]
fn scoreboard_contract_matches_hash_pinned_1_26_3301_ui_definition() {
    assert_eq!(SCOREBOARD_MAIN_HORIZONTAL_EXPANSION, 4.0);
    assert_eq!(SCOREBOARD_TEXT_HEIGHT, 10.0);
    assert_eq!(SCOREBOARD_TITLE_BACKGROUND_HEIGHT, 9.0);
    assert_eq!(SCOREBOARD_TITLE_WIDTH, 170.0);
    assert_eq!(SCOREBOARD_NAME_WIDTH, 100.0);
    assert_eq!(SCOREBOARD_LIST_OFFSET, 10.0);
    assert_eq!(SCOREBOARD_HORIZONTAL_PADDING, 10.0);
    assert_eq!(MAX_PRESENTED_SCOREBOARD_ROWS, 15);
}

#[test]
fn scoreboard_projection_uses_authoritative_order_and_fake_player_names() {
    let mut runtime = UiRuntime::new(1);
    install_scoreboard(&mut runtime, "Wins", &[(8, "Beta", 4), (4, "Alpha", 9)]);

    let sidebar = project_scoreboard_sidebar(runtime.scoreboards()).unwrap();

    assert_eq!(sidebar.title.as_ref(), "Wins");
    assert_eq!(sidebar.rows.len(), 2);
    assert_eq!(sidebar.rows[0].label.as_ref(), "Alpha");
    assert_eq!(sidebar.rows[0].score, "9");
    assert_eq!(sidebar.rows[1].label.as_ref(), "Beta");
}

#[test]
fn scoreboard_fails_closed_without_native_alpha_authority_then_uses_exact_dynamic_geometry() {
    let font = fixture_font();
    let mut presentation = UiPresentationRuntime::with_hud(font, fixture_hud()).unwrap();
    let mut runtime = UiRuntime::new(1);
    install_scoreboard(&mut runtime, "W", &[(1, "A", 2)]);

    let hidden = presentation
        .build(&runtime, 0, [800, 600], DpiScale::new(1.0).unwrap())
        .unwrap();
    assert!(bounds_for_color(&hidden, [0, 0, 0, 77]).is_none());
    assert!(bounds_for_color(&hidden, [255, 0, 0, 255]).is_none());

    presentation.set_native_scoreboard_opacity(77, 88);
    let visible = presentation
        .build(&runtime, 0, [800, 600], DpiScale::new(1.0).unwrap())
        .unwrap();
    let body = bounds_for_color(&visible, [0, 0, 0, 77]).unwrap();
    let title = bounds_for_color(&visible, [0, 0, 0, 88]).unwrap();

    assert_eq!(body[2], 800.0);
    assert_eq!(body[3] - body[1], 20.0);
    assert!(body[2] - body[0] < SCOREBOARD_TITLE_WIDTH + 4.0);
    assert_eq!(title[0], body[0]);
    assert_eq!(title[2], body[2]);
    assert_eq!(title[3] - title[1], SCOREBOARD_TITLE_BACKGROUND_HEIGHT);
    assert!(bounds_for_color(&visible, [255, 0, 0, 255]).is_some());
}

#[test]
fn boss_contract_uses_exact_pack_geometry_and_ignores_non_bedrock_notch_overlay_art() {
    assert_eq!(BOSS_PANEL_WIDTH, 182.0);
    assert_eq!(BOSS_PANEL_HEIGHT, 20.0);
    assert_eq!(BOSS_PROGRESS_HEIGHT, 5.0);
    assert_eq!(BOSS_PROGRESS_TOP, 10.0);
    assert_eq!(BOSS_STACK_VIEWPORT_FRACTION, 0.30);

    let mut runtime = UiRuntime::new(1);
    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 1,
            local_millis: 0,
            server_tick: None,
            event: boss_event(
                ProtocolBossAction::Show,
                7,
                "Boss",
                0.5,
                ProtocolBossColor::Purple,
                ProtocolBossOverlay::Notched10,
            ),
        })
        .unwrap();

    let bars = project_boss_bars(runtime.boss_bars(), 800.0, 600.0);
    assert_eq!(bars.len(), 1);
    assert_eq!(bars[0].panel, [309.0, 2.0, 491.0, 22.0]);
    assert_eq!(bars[0].progress, [309.0, 12.0, 491.0, 17.0]);
    assert_eq!(bars[0].fill, [309.0, 12.0, 400.0, 17.0]);

    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    let input = presentation
        .build(&runtime, 0, [800, 600], DpiScale::new(1.0).unwrap())
        .unwrap();
    assert!(bounds_for_color(&input, [170, 0, 170, 255]).is_some());
    assert!(bounds_for_color(&input, [16, 16, 16, 255]).is_none());
}

fn install_scoreboard(runtime: &mut UiRuntime, title: &str, rows: &[(i64, &str, i32)]) {
    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 1,
            local_millis: 0,
            server_tick: None,
            event: UiEvent::Objective(ObjectiveEvent::Display {
                display_slot: Arc::from("sidebar"),
                objective_name: Arc::from("objective"),
                display_name: Arc::from(title),
                criteria_name: Arc::from("dummy"),
                sort_order: 1,
            }),
        })
        .unwrap();
    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 2,
            local_millis: 0,
            server_tick: None,
            event: UiEvent::Score(ScoreEvent {
                action: ProtocolScoreAction::Change,
                entries: rows
                    .iter()
                    .map(|(id, name, score)| ProtocolScoreEntry {
                        scoreboard_id: *id,
                        objective_name: Arc::from("objective"),
                        score: *score,
                        identity: ProtocolScoreIdentity::FakePlayer(Arc::from(*name)),
                    })
                    .collect(),
            }),
        })
        .unwrap();
}
