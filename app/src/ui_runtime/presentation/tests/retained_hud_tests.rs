use super::super::retained_hud::{
    MAX_PRESENTED_BELOW_NAME_ROWS, MAX_PRESENTED_PLAYER_LIST_ROWS, MAX_PRESENTED_SCOREBOARD_HEARTS,
    MAX_PRESENTED_SCOREBOARD_ROWS, PresentedScoreValue, ScoreboardPresentationScope,
    project_below_name_scores, project_scoreboard_for_scope,
};
use super::*;
use ui::ScoreOwner;

#[test]
fn scoreboard_contract_matches_hash_pinned_1_26_3301_ui_definition() {
    assert_eq!(MAX_PRESENTED_SCOREBOARD_ROWS, 15);
    assert_eq!(
        MAX_PRESENTED_PLAYER_LIST_ROWS,
        protocol::MAX_PLAYER_LIST_RECORDS
    );
    assert_eq!(MAX_PRESENTED_BELOW_NAME_ROWS, ui::MAX_SCORES);
}

#[test]
fn scoreboard_projection_uses_authoritative_order_and_fake_player_names() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let mut runtime = UiRuntime::new(1);
    install_scoreboard(
        &mut player_runtime,
        &mut runtime,
        "Wins",
        &[(8, "Beta", 4), (4, "Alpha", 9)],
    );

    let sidebar = project_scoreboard_for_scope(
        runtime.scoreboards(),
        ScoreboardPresentationScope::HudSidebar,
        |_| None,
    )
    .unwrap();

    assert_eq!(sidebar.title.as_ref(), "Wins");
    assert_eq!(sidebar.rows.len(), 2);
    assert_eq!(sidebar.rows[0].label.as_ref(), "Alpha");
    assert_eq!(
        sidebar.rows[0].value,
        PresentedScoreValue::Text(Arc::from("9"))
    );
    assert_eq!(sidebar.rows[1].label.as_ref(), "Beta");
}

#[test]
fn scoreboard_slots_remain_scoped_to_their_native_surfaces_and_resolve_protocol_owners() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let mut runtime = UiRuntime::new(1);
    install_mixed_scoreboard_slot(
        &mut player_runtime,
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
}

#[test]
fn hearts_objectives_project_bounded_non_decimal_score_values() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let mut runtime = UiRuntime::new(1);
    install_mixed_scoreboard_slot_with_criteria(
        &mut player_runtime,
        &mut runtime,
        "sidebar",
        "health",
        &[(1, ProtocolScoreIdentity::FakePlayer(Arc::from("Alex")), 13)],
    );

    let sidebar = project_scoreboard_for_scope(
        runtime.scoreboards(),
        ScoreboardPresentationScope::HudSidebar,
        |_| None,
    )
    .unwrap();

    assert_eq!(
        sidebar.rows[0].value,
        PresentedScoreValue::Hearts {
            full_hearts: 6,
            half_heart: true,
        }
    );
    assert_ne!(
        sidebar.rows[0].value,
        PresentedScoreValue::Text(Arc::from("13"))
    );

    let mut capped_runtime = UiRuntime::new(1);
    install_mixed_scoreboard_slot_with_criteria(
        &mut player_runtime,
        &mut capped_runtime,
        "sidebar",
        "hearts",
        &[(
            1,
            ProtocolScoreIdentity::FakePlayer(Arc::from("Alex")),
            4_096,
        )],
    );
    let capped = project_scoreboard_for_scope(
        capped_runtime.scoreboards(),
        ScoreboardPresentationScope::HudSidebar,
        |_| None,
    )
    .unwrap();
    assert_eq!(
        capped.rows[0].value,
        PresentedScoreValue::Hearts {
            full_hearts: MAX_PRESENTED_SCOREBOARD_HEARTS,
            half_heart: false,
        }
    );
}

#[test]
fn unresolvable_owner_ids_fall_back_to_their_raw_retained_identity() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let mut runtime = UiRuntime::new(1);
    install_mixed_scoreboard_slot(
        &mut player_runtime,
        &mut runtime,
        "sidebar",
        &[
            (3, ProtocolScoreIdentity::Player(17), 3),
            (4, ProtocolScoreIdentity::Entity(23), 2),
            (5, ProtocolScoreIdentity::FakePlayer(Arc::from("Server")), 1),
        ],
    );

    let projected = project_scoreboard_for_scope(
        runtime.scoreboards(),
        ScoreboardPresentationScope::HudSidebar,
        |_| None,
    )
    .unwrap();

    assert_eq!(projected.rows.len(), 3);
    assert_eq!(projected.rows[0].label.as_ref(), "17");
    assert_eq!(projected.rows[1].label.as_ref(), "23");
    assert_eq!(projected.rows[2].label.as_ref(), "Server");
    assert_eq!(
        projected.rows[2].value,
        PresentedScoreValue::Text(Arc::from("1"))
    );
}

#[test]
fn xuid_keyed_owner_ids_resolve_through_the_roster_map_then_fall_back_cleanly() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    const XUID_KEYED_OWNER: i64 = 2535406042983449;
    let mut runtime = UiRuntime::new(1);
    install_mixed_scoreboard_slot(
        &mut player_runtime,
        &mut runtime,
        "sidebar",
        &[
            (3, ProtocolScoreIdentity::Player(XUID_KEYED_OWNER), 5),
            (4, ProtocolScoreIdentity::Entity(999), 2),
        ],
    );

    let resolved = project_scoreboard_for_scope(
        runtime.scoreboards(),
        ScoreboardPresentationScope::HudSidebar,
        |owner| {
            matches!(owner, ScoreOwner::Player(id) if *id == XUID_KEYED_OWNER)
                .then(|| Arc::from("KnownPlayer"))
        },
    )
    .unwrap();
    assert_eq!(resolved.rows.len(), 2);
    assert_eq!(resolved.rows[0].label.as_ref(), "KnownPlayer");
    assert_eq!(resolved.rows[1].label.as_ref(), "999");

    let fallback = project_scoreboard_for_scope(
        runtime.scoreboards(),
        ScoreboardPresentationScope::HudSidebar,
        |_| None,
    )
    .unwrap();
    assert_eq!(fallback.rows.len(), 2);
    assert_eq!(fallback.rows[0].label.as_ref(), "2535406042983449");
    assert_eq!(fallback.rows[1].label.as_ref(), "999");
}

#[test]
fn below_name_projection_preserves_actor_identity_and_raw_objective_semantics() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let mut runtime = UiRuntime::new(1);
    install_mixed_scoreboard_slot(
        &mut player_runtime,
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

pub(super) fn install_mixed_scoreboard_slot(
    player_runtime: &mut crate::player_runtime::PlayerRuntime,
    runtime: &mut UiRuntime,
    slot: &str,
    rows: &[(i64, ProtocolScoreIdentity, i32)],
) {
    install_mixed_scoreboard_slot_with_criteria(player_runtime, runtime, slot, "dummy", rows);
}

fn install_mixed_scoreboard_slot_with_criteria(
    player_runtime: &mut crate::player_runtime::PlayerRuntime,
    runtime: &mut UiRuntime,
    slot: &str,
    criteria_name: &str,
    rows: &[(i64, ProtocolScoreIdentity, i32)],
) {
    runtime
        .apply(
            player_runtime,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 1,
                local_millis: 0,
                server_tick: None,
                event: UiEvent::Objective(ObjectiveEvent::Display {
                    display_slot: Arc::from(slot),
                    objective_name: Arc::from("objective"),
                    display_name: Arc::from("Objective"),
                    criteria_name: Arc::from(criteria_name),
                    sort_order: 1,
                }),
            },
        )
        .unwrap();
    runtime
        .apply(
            player_runtime,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 2,
                local_millis: 0,
                server_tick: None,
                event: UiEvent::Score(ScoreEvent {
                    entries: rows
                        .iter()
                        .map(|(id, identity, score)| ProtocolScoreEntry {
                            action: ProtocolScoreAction::Change,
                            scoreboard_id: *id,
                            objective_name: Arc::from("objective"),
                            score: *score,
                            identity: identity.clone(),
                        })
                        .collect(),
                }),
            },
        )
        .unwrap();
}

fn install_scoreboard(
    player_runtime: &mut crate::player_runtime::PlayerRuntime,
    runtime: &mut UiRuntime,
    title: &str,
    rows: &[(i64, &str, i32)],
) {
    runtime
        .apply(
            player_runtime,
            SequencedUiEvent {
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
            },
        )
        .unwrap();
    runtime
        .apply(
            player_runtime,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 2,
                local_millis: 0,
                server_tick: None,
                event: UiEvent::Score(ScoreEvent {
                    entries: rows
                        .iter()
                        .map(|(id, name, score)| ProtocolScoreEntry {
                            action: ProtocolScoreAction::Change,
                            scoreboard_id: *id,
                            objective_name: Arc::from("objective"),
                            score: *score,
                            identity: ProtocolScoreIdentity::FakePlayer(Arc::from(*name)),
                        })
                        .collect(),
                }),
            },
        )
        .unwrap();
}
