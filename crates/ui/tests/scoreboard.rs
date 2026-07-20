use std::sync::Arc;

use ui::{
    BossAction, BossBarEvent, BossBarStore, BossColor, BossOverlay, BossStyle, DisplaySlot,
    MAX_BOSS_BARS, MAX_OBJECTIVES, MAX_RETAINED_UI_TEXT_FIELD_BYTES,
    MAX_SCOREBOARD_RETAINED_TEXT_BYTES, MAX_SCORES, RetainedUiApply, RetainedUiSequenceError,
    ScoreAction, ScoreEntry, ScoreOwner, ScoreboardEvent, ScoreboardStore,
};

fn display(slot: &str, objective: &str, sort_order: i32) -> ScoreboardEvent {
    ScoreboardEvent::DisplayObjective {
        display_slot: Arc::from(slot),
        objective_name: Arc::from(objective),
        display_name: Arc::from(format!("{objective} title")),
        criteria_name: Arc::from("dummy"),
        sort_order,
    }
}

fn score(objective: &str, id: i64, value: i32, owner: ScoreOwner) -> ScoreEntry {
    ScoreEntry {
        scoreboard_id: id,
        objective_name: Arc::from(objective),
        score: value,
        owner,
    }
}

#[test]
fn objective_slots_scores_and_remove_lifecycle_are_atomic_and_deterministic() {
    let mut store = ScoreboardStore::default();
    store.apply(1, display("sidebar", "kills", 1)).unwrap();
    store.apply(2, display("list", "kills", 1)).unwrap();
    store
        .apply(
            3,
            ScoreboardEvent::Scores {
                action: ScoreAction::Change,
                entries: Arc::from([
                    score("kills", 20, 4, ScoreOwner::FakePlayer(Arc::from("twenty"))),
                    score("kills", 10, 4, ScoreOwner::FakePlayer(Arc::from("ten"))),
                    score("kills", 30, 9, ScoreOwner::Player(7)),
                ]),
            },
        )
        .unwrap();

    let sidebar = store.sidebar().expect("supported displayed objective");
    assert_eq!(sidebar.slot, DisplaySlot::Sidebar);
    assert_eq!(sidebar.objective_name.as_ref(), "kills");
    assert_eq!(
        sidebar
            .rows
            .iter()
            .map(|row| (row.score, row.identity.entry_id))
            .collect::<Vec<_>>(),
        [(9, 30), (4, 10), (4, 20)]
    );
    assert_eq!(store.list().unwrap().rows, sidebar.rows);
    assert!(store.below_name().is_none());

    store
        .apply(
            4,
            ScoreboardEvent::Scores {
                action: ScoreAction::Change,
                entries: Arc::from([score("kills", 10, 12, ScoreOwner::Entity(99))]),
            },
        )
        .unwrap();
    let updated = store.sidebar().unwrap();
    assert_eq!(updated.rows[0].identity.entry_id, 10);
    assert_eq!(updated.rows[0].owner, ScoreOwner::Entity(99));

    store
        .apply(
            5,
            ScoreboardEvent::Scores {
                action: ScoreAction::Remove,
                entries: Arc::from([score("kills", 20, -999, ScoreOwner::None)]),
            },
        )
        .unwrap();
    assert_eq!(
        store
            .sidebar()
            .unwrap()
            .rows
            .iter()
            .map(|row| row.identity.entry_id)
            .collect::<Vec<_>>(),
        [10, 30]
    );

    store
        .apply(
            6,
            ScoreboardEvent::RemoveObjective {
                objective_name: Arc::from("kills"),
            },
        )
        .unwrap();
    assert_eq!(store.objective_count(), 0);
    assert_eq!(store.score_count(), 0);
    assert!(store.sidebar().is_none());
    assert!(store.list().is_none());
    assert_eq!(store.retained_text_bytes(), 0);
}

#[test]
fn unsupported_orders_are_retained_without_an_invented_projection() {
    let mut store = ScoreboardStore::default();
    assert_eq!(
        store.apply(1, display("belowname", "health", 77)),
        Ok(RetainedUiApply::Applied)
    );

    assert_eq!(store.objective_count(), 1);
    assert!(store.below_name().is_none());
    assert_eq!(store.diagnostics().unsupported_sort_orders, 1);
}

#[test]
fn missing_score_identities_and_capacity_fail_without_mutation() {
    let mut store = ScoreboardStore::default();
    let before = store.clone();
    assert_eq!(
        store.apply(
            1,
            ScoreboardEvent::Scores {
                action: ScoreAction::Change,
                entries: Arc::from([score("absent", 1, 3, ScoreOwner::None)]),
            }
        ),
        Ok(RetainedUiApply::Ignored)
    );
    assert_eq!(store.objective_count(), before.objective_count());
    assert_eq!(store.score_count(), before.score_count());
    assert_eq!(store.diagnostics().missing_objectives, 1);

    for index in 0..MAX_OBJECTIVES {
        store
            .apply(
                index as u64 + 2,
                display("sidebar", &format!("o{index}"), 0),
            )
            .unwrap();
    }
    let sequence = MAX_OBJECTIVES as u64 + 2;
    let objective_count = store.objective_count();
    let retained_bytes = store.retained_text_bytes();
    assert_eq!(
        store.apply(sequence, display("sidebar", "overflow", 0)),
        Ok(RetainedUiApply::Ignored)
    );
    assert_eq!(store.objective_count(), objective_count);
    assert_eq!(store.retained_text_bytes(), retained_bytes);
    assert_eq!(store.diagnostics().objective_limit_rejections, 1);
}

#[test]
fn oversized_incoming_score_batch_is_rejected_before_duplicate_key_staging() {
    let mut store = ScoreboardStore::default();
    store.apply(1, display("sidebar", "bounded", 0)).unwrap();
    store
        .apply(
            2,
            ScoreboardEvent::Scores {
                action: ScoreAction::Change,
                entries: Arc::from([score("bounded", 1, 4, ScoreOwner::None)]),
            },
        )
        .unwrap();
    let before = store.sidebar().unwrap();
    let duplicate_entries = (0..=MAX_SCORES)
        .map(|_| score("bounded", 1, 99, ScoreOwner::None))
        .collect::<Vec<_>>()
        .into();

    assert_eq!(
        store.apply(
            3,
            ScoreboardEvent::Scores {
                action: ScoreAction::Change,
                entries: duplicate_entries,
            }
        ),
        Ok(RetainedUiApply::Ignored)
    );
    assert_eq!(store.sidebar().unwrap(), before);
    assert_eq!(store.diagnostics().score_event_limit_rejections, 1);
}

#[test]
fn malformed_or_missing_score_siblings_reject_the_whole_event() {
    let mut store = ScoreboardStore::default();
    store.apply(1, display("sidebar", "atomic", 0)).unwrap();
    store
        .apply(
            2,
            ScoreboardEvent::Scores {
                action: ScoreAction::Change,
                entries: Arc::from([score("atomic", 1, 5, ScoreOwner::None)]),
            },
        )
        .unwrap();
    let before = store.sidebar().unwrap();

    assert_eq!(
        store.apply(
            3,
            ScoreboardEvent::Scores {
                action: ScoreAction::Change,
                entries: Arc::from([
                    score("atomic", 2, 8, ScoreOwner::None),
                    score("absent", 3, 13, ScoreOwner::None),
                ]),
            }
        ),
        Ok(RetainedUiApply::Ignored)
    );
    assert_eq!(store.sidebar().unwrap(), before);

    assert_eq!(
        store.apply(
            4,
            ScoreboardEvent::Scores {
                action: ScoreAction::Remove,
                entries: Arc::from([
                    score("atomic", 1, 0, ScoreOwner::None),
                    score("atomic", 404, 0, ScoreOwner::None),
                ]),
            }
        ),
        Ok(RetainedUiApply::Ignored)
    );
    assert_eq!(store.sidebar().unwrap(), before);

    let oversized = Arc::from("x".repeat(MAX_RETAINED_UI_TEXT_FIELD_BYTES + 1));
    assert_eq!(
        store.apply(
            5,
            ScoreboardEvent::Scores {
                action: ScoreAction::Change,
                entries: Arc::from([
                    score("atomic", 2, 8, ScoreOwner::None),
                    score("atomic", 3, 13, ScoreOwner::FakePlayer(oversized)),
                ]),
            }
        ),
        Ok(RetainedUiApply::Ignored)
    );
    assert_eq!(store.sidebar().unwrap(), before);
    assert_eq!(store.diagnostics().missing_objectives, 1);
    assert_eq!(store.diagnostics().missing_scores, 1);
    assert_eq!(store.diagnostics().text_field_rejections, 1);
}

#[test]
fn overlong_remove_objective_name_is_rejected_at_the_store_boundary() {
    let mut store = ScoreboardStore::default();
    store.apply(1, display("sidebar", "kept", 0)).unwrap();
    let before = store.sidebar().unwrap();
    assert_eq!(
        store.apply(
            2,
            ScoreboardEvent::RemoveObjective {
                objective_name: Arc::from("x".repeat(MAX_RETAINED_UI_TEXT_FIELD_BYTES + 1),),
            }
        ),
        Ok(RetainedUiApply::Ignored)
    );
    assert_eq!(store.sidebar().unwrap(), before);
    assert_eq!(store.diagnostics().text_field_rejections, 1);
}

#[test]
fn score_item_limit_and_utf8_budget_updates_are_exact_and_atomic() {
    let mut store = ScoreboardStore::default();
    store.apply(1, display("sidebar", "scores", 0)).unwrap();
    let entries: Arc<[ScoreEntry]> = (0..MAX_SCORES)
        .map(|id| score("scores", id as i64, id as i32, ScoreOwner::None))
        .collect::<Vec<_>>()
        .into();
    store
        .apply(
            2,
            ScoreboardEvent::Scores {
                action: ScoreAction::Change,
                entries,
            },
        )
        .unwrap();
    assert_eq!(store.score_count(), MAX_SCORES);
    let before_bytes = store.retained_text_bytes();
    assert_eq!(
        store.apply(
            3,
            ScoreboardEvent::Scores {
                action: ScoreAction::Change,
                entries: Arc::from([score("scores", -1, -1, ScoreOwner::None)]),
            }
        ),
        Ok(RetainedUiApply::Ignored)
    );
    assert_eq!(store.score_count(), MAX_SCORES);
    assert_eq!(store.retained_text_bytes(), before_bytes);

    let exact_utf8 = "é".repeat(MAX_RETAINED_UI_TEXT_FIELD_BYTES / 2);
    assert_eq!(
        store.apply(
            4,
            ScoreboardEvent::DisplayObjective {
                display_slot: Arc::from("sidebar"),
                objective_name: Arc::from("scores"),
                display_name: Arc::from(exact_utf8),
                criteria_name: Arc::from("dummy"),
                sort_order: 0,
            }
        ),
        Ok(RetainedUiApply::Applied)
    );
    assert_eq!(
        store.sidebar().unwrap().display_name.len(),
        MAX_RETAINED_UI_TEXT_FIELD_BYTES
    );
    let old = store.sidebar().unwrap().display_name;
    let before_bytes = store.retained_text_bytes();
    let oversized_utf8 = "é".repeat(MAX_RETAINED_UI_TEXT_FIELD_BYTES / 2 + 1);
    assert_eq!(
        store.apply(
            5,
            ScoreboardEvent::DisplayObjective {
                display_slot: Arc::from("sidebar"),
                objective_name: Arc::from("scores"),
                display_name: Arc::from(oversized_utf8),
                criteria_name: Arc::from("dummy"),
                sort_order: 0,
            }
        ),
        Ok(RetainedUiApply::Ignored)
    );
    assert_eq!(store.sidebar().unwrap().display_name, old);
    assert_eq!(store.retained_text_bytes(), before_bytes);
}

#[test]
fn aggregate_scoreboard_text_budget_accepts_exact_limit_and_rejects_one_more_byte() {
    let mut store = ScoreboardStore::default();
    store
        .apply(
            1,
            ScoreboardEvent::DisplayObjective {
                display_slot: Arc::from("sidebar"),
                objective_name: Arc::from("x"),
                display_name: Arc::from(""),
                criteria_name: Arc::from(""),
                sort_order: 0,
            },
        )
        .unwrap();
    let full_fields = (MAX_SCOREBOARD_RETAINED_TEXT_BYTES - 1) / MAX_RETAINED_UI_TEXT_FIELD_BYTES;
    let remainder =
        MAX_SCOREBOARD_RETAINED_TEXT_BYTES - 1 - full_fields * MAX_RETAINED_UI_TEXT_FIELD_BYTES;
    let mut entries = (0..full_fields)
        .map(|id| {
            score(
                "x",
                id as i64,
                0,
                ScoreOwner::FakePlayer(Arc::from("a".repeat(MAX_RETAINED_UI_TEXT_FIELD_BYTES))),
            )
        })
        .collect::<Vec<_>>();
    entries.push(score(
        "x",
        full_fields as i64,
        0,
        ScoreOwner::FakePlayer(Arc::from("b".repeat(remainder))),
    ));
    store
        .apply(
            2,
            ScoreboardEvent::Scores {
                action: ScoreAction::Change,
                entries: entries.into(),
            },
        )
        .unwrap();
    assert_eq!(
        store.retained_text_bytes(),
        MAX_SCOREBOARD_RETAINED_TEXT_BYTES
    );

    let before = store.sidebar().unwrap();
    assert_eq!(
        store.apply(
            3,
            ScoreboardEvent::Scores {
                action: ScoreAction::Change,
                entries: Arc::from([score(
                    "x",
                    full_fields as i64,
                    0,
                    ScoreOwner::FakePlayer(Arc::from("b".repeat(remainder + 1))),
                )]),
            }
        ),
        Ok(RetainedUiApply::Ignored)
    );
    assert_eq!(store.sidebar().unwrap(), before);
    assert_eq!(
        store.retained_text_bytes(),
        MAX_SCOREBOARD_RETAINED_TEXT_BYTES
    );
}

#[test]
fn direct_store_fifo_rejection_and_clear_have_no_stale_mutation() {
    let mut store = ScoreboardStore::default();
    store.apply(9, display("sidebar", "live", 0)).unwrap();
    let before = store.sidebar();
    assert_eq!(
        store.apply(9, display("sidebar", "stale", 0)),
        Err(RetainedUiSequenceError {
            previous: 9,
            actual: 9,
        })
    );
    assert_eq!(store.sidebar(), before);
    store.clear();
    assert_eq!(store.objective_count(), 0);
    store.apply(1, display("sidebar", "new", 0)).unwrap();
}

fn boss(action: BossAction, id: i64, title: &str, health: f32) -> BossBarEvent {
    BossBarEvent {
        target_entity_id: id,
        player_id: 42,
        action,
        title: Arc::from(title),
        filtered_title: Arc::from(""),
        health,
        style: BossStyle {
            color: BossColor::Purple,
            overlay: BossOverlay::Notched10,
            darken_sky: Some(true),
            create_world_fog: Some(false),
        },
    }
}

#[test]
fn boss_lifecycle_style_health_membership_and_stacking_are_stable() {
    let mut store = BossBarStore::default();
    store
        .apply(1, boss(BossAction::Show, 20, "first", 0.5))
        .unwrap();
    store
        .apply(2, boss(BossAction::Show, 10, "second", 1.0))
        .unwrap();
    store
        .apply(3, boss(BossAction::RegisterPlayer, 20, "", 0.0))
        .unwrap();
    let mut progress = boss(BossAction::SetProgress, 20, "ignored", 1.25);
    progress.style.color = BossColor::Red;
    store.apply(4, progress).unwrap();
    let mut texture = boss(BossAction::Texture, 20, "ignored", 0.0);
    texture.style.color = BossColor::Blue;
    texture.style.overlay = BossOverlay::Notched20;
    store.apply(5, texture).unwrap();

    let bars = store.stacked();
    assert_eq!(
        bars.iter()
            .map(|bar| bar.target_entity_id)
            .collect::<Vec<_>>(),
        [20, 10]
    );
    assert_eq!(bars[0].health, 1.25);
    assert_eq!(bars[0].style.color, BossColor::Blue);
    assert_eq!(bars[0].style.overlay, BossOverlay::Notched20);
    assert_eq!(bars[0].registered_players.as_ref(), [42]);

    store
        .apply(6, boss(BossAction::Show, 20, "updated", 0.75))
        .unwrap();
    assert_eq!(store.stacked()[0].title.as_ref(), "updated");
    assert_eq!(store.stacked()[0].registered_players.as_ref(), [42]);
    store
        .apply(7, boss(BossAction::UnregisterPlayer, 20, "", 0.0))
        .unwrap();
    assert!(store.stacked()[0].registered_players.is_empty());
    store.apply(8, boss(BossAction::Hide, 20, "", 0.0)).unwrap();
    assert_eq!(
        store
            .stacked()
            .iter()
            .map(|bar| bar.target_entity_id)
            .collect::<Vec<_>>(),
        [10]
    );
}

#[test]
fn boss_bounds_nonfinite_health_and_missing_updates_do_not_mutate() {
    let mut store = BossBarStore::default();
    assert_eq!(
        store.apply(1, boss(BossAction::SetTitle, 99, "missing", 0.0)),
        Ok(RetainedUiApply::Ignored)
    );
    assert_eq!(store.diagnostics().missing_bars, 1);

    for index in 0..MAX_BOSS_BARS {
        store
            .apply(
                index as u64 + 2,
                boss(BossAction::Show, index as i64, "bar", 0.5),
            )
            .unwrap();
    }
    let sequence = MAX_BOSS_BARS as u64 + 2;
    let before = store.stacked();
    let before_bytes = store.retained_text_bytes();
    assert_eq!(
        store.apply(sequence, boss(BossAction::Show, -1, "overflow", 0.5)),
        Ok(RetainedUiApply::Ignored)
    );
    assert_eq!(store.stacked(), before);
    assert_eq!(store.retained_text_bytes(), before_bytes);

    assert_eq!(
        store.apply(sequence + 1, boss(BossAction::SetProgress, 0, "", f32::NAN)),
        Ok(RetainedUiApply::Ignored)
    );
    assert_eq!(store.stacked()[0].health, 0.5);
    assert_eq!(store.diagnostics().invalid_health_rejections, 1);
}

#[test]
fn below_name_and_list_owner_lookups_track_display_and_score_lifecycle() {
    let mut store = ScoreboardStore::default();
    let player = ScoreOwner::Player(31);
    let other = ScoreOwner::Entity(44);

    // Nothing resolves before the slots display an objective.
    assert_eq!(store.below_name_for_owner(&player), None);
    assert_eq!(store.list_score_for_owner(&player), None);

    store.apply(1, display("belowname", "health", 0)).unwrap();
    store.apply(2, display("list", "kills", 1)).unwrap();
    store
        .apply(
            3,
            ScoreboardEvent::Scores {
                action: ScoreAction::Change,
                entries: Arc::from(vec![
                    score("health", 1, 18, player.clone()),
                    score("health", 2, 7, other.clone()),
                    score("kills", 3, 5, player.clone()),
                ]),
            },
        )
        .unwrap();

    let (value, label) = store.below_name_for_owner(&player).unwrap();
    assert_eq!(value, 18);
    assert_eq!(label.as_ref(), "health title");
    assert_eq!(store.below_name_for_owner(&other).unwrap().0, 7);
    assert_eq!(store.list_score_for_owner(&player), Some(5));
    assert_eq!(store.list_score_for_owner(&other), None);

    // Score removal clears only the removed owner's lookup.
    store
        .apply(
            4,
            ScoreboardEvent::Scores {
                action: ScoreAction::Remove,
                entries: Arc::from(vec![score("health", 1, 0, ScoreOwner::None)]),
            },
        )
        .unwrap();
    assert_eq!(store.below_name_for_owner(&player), None);
    assert_eq!(store.below_name_for_owner(&other).unwrap().0, 7);

    // Moving the slot to a different objective replaces the lookup source.
    store.apply(5, display("belowname", "kills", 1)).unwrap();
    assert_eq!(store.below_name_for_owner(&other), None);
    assert_eq!(store.below_name_for_owner(&player).unwrap().0, 5);

    // Removing the displayed objective empties the slot lookups entirely.
    store
        .apply(
            6,
            ScoreboardEvent::RemoveObjective {
                objective_name: Arc::from("kills"),
            },
        )
        .unwrap();
    assert_eq!(store.below_name_for_owner(&player), None);
    assert_eq!(store.list_score_for_owner(&player), None);
}
