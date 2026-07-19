use super::super::retained_hud::{
    MAX_PRESENTED_BELOW_NAME_ROWS, MAX_PRESENTED_PLAYER_LIST_ROWS, MAX_PRESENTED_SCOREBOARD_ROWS,
    SCOREBOARD_HORIZONTAL_PADDING, SCOREBOARD_LIST_OFFSET, SCOREBOARD_MAIN_HORIZONTAL_EXPANSION,
    SCOREBOARD_NAME_WIDTH, SCOREBOARD_TEXT_HEIGHT, SCOREBOARD_TITLE_BACKGROUND_HEIGHT,
    SCOREBOARD_TITLE_WIDTH, ScoreboardPresentationScope, project_below_name_scores,
    project_scoreboard_for_scope, required_sidebar_owner_ids,
};
use super::*;
use ui::ScoreOwner;

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
    assert_eq!(
        MAX_PRESENTED_PLAYER_LIST_ROWS,
        protocol::MAX_PLAYER_LIST_RECORDS
    );
    assert_eq!(MAX_PRESENTED_BELOW_NAME_ROWS, ui::MAX_SCORES);
}

#[test]
fn scoreboard_projection_uses_authoritative_order_and_fake_player_names() {
    let mut runtime = UiRuntime::new(1);
    install_scoreboard(&mut runtime, "Wins", &[(8, "Beta", 4), (4, "Alpha", 9)]);

    let sidebar = project_scoreboard_for_scope(
        runtime.scoreboards(),
        ScoreboardPresentationScope::HudSidebar,
        |_| None,
    )
    .unwrap();

    assert_eq!(sidebar.title.as_ref(), "Wins");
    assert_eq!(sidebar.rows.len(), 2);
    assert_eq!(sidebar.rows[0].label.as_ref(), "Alpha");
    assert_eq!(sidebar.rows[0].score, "9");
    assert_eq!(sidebar.rows[1].label.as_ref(), "Beta");
}

#[test]
fn scoreboard_slots_remain_scoped_to_their_native_surfaces_and_resolve_protocol_owners() {
    let mut runtime = UiRuntime::new(1);
    install_mixed_scoreboard_slot(
        &mut runtime,
        "list",
        &[
            (3, ProtocolScoreIdentity::Player(17), 3),
            (4, ProtocolScoreIdentity::Entity(23), 2),
            (5, ProtocolScoreIdentity::FakePlayer(Arc::from("Server")), 1),
        ],
    );
    let projected = project_scoreboard_for_scope(
        runtime.scoreboards(),
        ScoreboardPresentationScope::PlayerList,
        |owner| match owner {
            ScoreOwner::Player(17) => Some(Arc::from("Alex")),
            ScoreOwner::Entity(23) => Some(Arc::from("Horse")),
            _ => None,
        },
    )
    .unwrap();

    assert_eq!(projected.scope, ScoreboardPresentationScope::PlayerList);
    assert_eq!(projected.rows.len(), 3);
    assert_eq!(projected.rows[0].label.as_ref(), "Alex");
    assert_eq!(projected.rows[1].label.as_ref(), "Horse");
    assert_eq!(projected.rows[2].label.as_ref(), "Server");
    assert!(
        project_scoreboard_for_scope(
            runtime.scoreboards(),
            ScoreboardPresentationScope::HudSidebar,
            |_| None,
        )
        .is_none()
    );
}

#[test]
fn below_name_projection_preserves_actor_identity_and_raw_objective_semantics() {
    let mut runtime = UiRuntime::new(1);
    install_mixed_scoreboard_slot(
        &mut runtime,
        "belowname",
        &[
            (3, ProtocolScoreIdentity::Player(17), 11),
            (4, ProtocolScoreIdentity::Entity(23), 7),
            (
                5,
                ProtocolScoreIdentity::FakePlayer(Arc::from("not an actor")),
                5,
            ),
        ],
    );

    let projected = project_below_name_scores(runtime.scoreboards()).unwrap();

    assert_eq!(projected.scope, ScoreboardPresentationScope::ActorNameplate);
    assert_eq!(projected.objective_display_name.as_ref(), "Objective");
    assert_eq!(projected.rows.len(), 2);
    assert_eq!(projected.rows[0].owner, ScoreOwner::Player(17));
    assert_eq!(projected.rows[0].score, 11);
    assert_eq!(projected.rows[1].owner, ScoreOwner::Entity(23));
    assert_eq!(projected.rows[1].score, 7);
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
fn production_sidebar_resolves_player_entity_and_fake_rows_from_owned_actor_authority() {
    let mut runtime = UiRuntime::new(1);
    install_mixed_scoreboard_slot(
        &mut runtime,
        "sidebar",
        &[
            (3, ProtocolScoreIdentity::Player(17), 3),
            (4, ProtocolScoreIdentity::Entity(23), 2),
            (5, ProtocolScoreIdentity::FakePlayer(Arc::from("Server")), 1),
        ],
    );
    assert_eq!(required_sidebar_owner_ids(runtime.scoreboards()), [17, 23]);
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    presentation
        .set_scoreboard_owner_names([(17, Arc::from("Alex")), (23, Arc::from("Beeatrice"))]);
    presentation.set_native_scoreboard_opacity(77, 88);

    let visible = presentation
        .build(&runtime, 0, [800, 600], DpiScale::new(1.0).unwrap())
        .unwrap();

    let red_score_vertices = visible
        .vertices
        .iter()
        .filter(|vertex| vertex.color == [255, 0, 0, 255])
        .count();
    assert_eq!(red_score_vertices, 3 * 4);
}

#[test]
fn boss_presentation_fails_closed_entirely_without_exact_title_shadow_authority() {
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

    assert_eq!(runtime.boss_bars().stacked().len(), 1);

    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    let input = presentation
        .build(&runtime, 0, [800, 600], DpiScale::new(1.0).unwrap())
        .unwrap();
    assert!(input.vertices.is_empty());
    assert!(input.indices.is_empty());
    assert!(input.batches.is_empty());
}

fn install_mixed_scoreboard_slot(
    runtime: &mut UiRuntime,
    slot: &str,
    rows: &[(i64, ProtocolScoreIdentity, i32)],
) {
    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 1,
            local_millis: 0,
            server_tick: None,
            event: UiEvent::Objective(ObjectiveEvent::Display {
                display_slot: Arc::from(slot),
                objective_name: Arc::from("objective"),
                display_name: Arc::from("Objective"),
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
                    .map(|(id, identity, score)| ProtocolScoreEntry {
                        scoreboard_id: *id,
                        objective_name: Arc::from("objective"),
                        score: *score,
                        identity: identity.clone(),
                    })
                    .collect(),
            }),
        })
        .unwrap();
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
