use super::*;

/// Starts with one displayed objective and a named score.
fn populated() -> ScoreboardStore {
    let mut store = ScoreboardStore::default();
    store.apply(1, display("sidebar", "round", 0)).unwrap();
    store
        .apply(
            2,
            ScoreboardEvent::Scores {
                entries: [score(
                    "round",
                    7,
                    42,
                    ScoreOwner::FakePlayer("Amber".into()),
                )]
                .into(),
            },
        )
        .unwrap();
    store
}

#[test]
fn empty_display_clears_the_slot_and_discards_unreferenced_scores_before_redisplay() {
    let mut store = populated();
    assert_eq!(
        store.apply(3, display("sidebar", "", 999)).unwrap(),
        RetainedUiApply::Applied
    );
    assert!(store.sidebar().is_none());
    assert_eq!(store.objective_count(), 0);
    assert_eq!(store.score_count(), 0);
    assert_eq!(store.retained_text_bytes(), 0);
    assert_eq!(store.score_for_named_owner("round", "Amber"), None);

    assert_eq!(
        store
            .apply(
                4,
                ScoreboardEvent::Scores {
                    entries: [score(
                        "round",
                        7,
                        99,
                        ScoreOwner::FakePlayer("Amber".into())
                    )]
                    .into(),
                },
            )
            .unwrap(),
        RetainedUiApply::Ignored
    );
    store.apply(5, display("sidebar", "round", 0)).unwrap();
    assert!(store.sidebar().unwrap().rows.is_empty());
    store
        .apply(
            6,
            ScoreboardEvent::Scores {
                entries: [score(
                    "round",
                    7,
                    12,
                    ScoreOwner::FakePlayer("Birch".into()),
                )]
                .into(),
            },
        )
        .unwrap();
    let rows = store.sidebar().unwrap().rows;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].score, 12);
    assert_eq!(rows[0].owner, ScoreOwner::FakePlayer("Birch".into()));
}

#[test]
fn empty_display_preserves_an_objective_until_its_last_display_slot_is_cleared() {
    let mut store = populated();
    store.apply(3, display("list", "round", 0)).unwrap();
    store.apply(4, display("belowname", "round", 0)).unwrap();
    let retained_bytes = store.retained_text_bytes();

    store.apply(5, display("sidebar", "", 0)).unwrap();
    assert!(store.sidebar().is_none());
    assert_eq!(store.list().unwrap().rows.len(), 1);
    assert_eq!(store.below_name().unwrap().rows.len(), 1);
    assert_eq!(store.objective_count(), 1);
    assert_eq!(store.score_count(), 1);
    assert_eq!(store.retained_text_bytes(), retained_bytes);

    store.apply(6, display("sidebar", "round", 0)).unwrap();
    assert_eq!(store.sidebar().unwrap().rows[0].score, 42);
    store.apply(7, display("list", "", 0)).unwrap();
    store.apply(8, display("belowname", "", 0)).unwrap();
    assert_eq!(store.sidebar().unwrap().rows[0].score, 42);
    assert!(store.list().is_none());
    assert!(store.below_name().is_none());

    store.apply(9, display("sidebar", "", 0)).unwrap();
    assert!(store.sidebar().is_none());
    assert_eq!(store.objective_count(), 0);
    assert_eq!(store.score_count(), 0);
    assert_eq!(store.retained_text_bytes(), 0);
}

#[test]
fn empty_display_for_an_unbound_slot_does_not_create_an_objective() {
    let mut store = populated();
    let revision = store.revision();
    let retained_bytes = store.retained_text_bytes();
    assert_eq!(
        store.apply(3, display("list", "", 0)).unwrap(),
        RetainedUiApply::Ignored
    );
    assert!(store.list().is_none());
    assert_eq!(store.sidebar().unwrap().rows[0].score, 42);
    assert_eq!(store.objective_count(), 1);
    assert_eq!(store.score_count(), 1);
    assert_eq!(store.retained_text_bytes(), retained_bytes);
    assert_eq!(store.revision(), revision);
}

#[test]
fn empty_display_ignores_unused_display_name_and_criteria() {
    let mut store = populated();
    assert_eq!(
        store
            .apply(
                3,
                ScoreboardEvent::DisplayObjective {
                    display_slot: "sidebar".into(),
                    objective_name: "".into(),
                    display_name: "x".repeat(MAX_RETAINED_UI_TEXT_FIELD_BYTES + 1).into(),
                    criteria_name: "y".repeat(MAX_RETAINED_UI_TEXT_FIELD_BYTES + 1).into(),
                    sort_order: i32::MAX,
                },
            )
            .unwrap(),
        RetainedUiApply::Applied
    );
    assert!(store.sidebar().is_none());
    assert_eq!(store.objective_count(), 0);
    assert_eq!(store.score_count(), 0);
    assert_eq!(store.retained_text_bytes(), 0);
    assert_eq!(store.diagnostics().text_field_rejections, 0);
}
