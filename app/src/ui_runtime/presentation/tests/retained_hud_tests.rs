use super::super::retained_hud::project_scoreboard_sidebar;
use super::*;

#[test]
fn scoreboard_sidebar_projection_uses_authoritative_order_and_omits_unresolved_names() {
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 1,
            local_millis: 10,
            server_tick: None,
            event: UiEvent::Objective(ObjectiveEvent::Display {
                display_slot: Arc::from("sidebar"),
                objective_name: Arc::from("wins"),
                display_name: Arc::from("Wins"),
                criteria_name: Arc::from("dummy"),
                sort_order: 1,
            }),
        })
        .unwrap();
    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 2,
            local_millis: 20,
            server_tick: None,
            event: UiEvent::Score(ScoreEvent {
                action: ProtocolScoreAction::Change,
                entries: Arc::from([
                    ProtocolScoreEntry {
                        scoreboard_id: 8,
                        objective_name: Arc::from("wins"),
                        score: 7,
                        identity: ProtocolScoreIdentity::FakePlayer(Arc::from("lower")),
                    },
                    ProtocolScoreEntry {
                        scoreboard_id: 4,
                        objective_name: Arc::from("wins"),
                        score: 12,
                        identity: ProtocolScoreIdentity::FakePlayer(Arc::from("leader")),
                    },
                    ProtocolScoreEntry {
                        scoreboard_id: 2,
                        objective_name: Arc::from("wins"),
                        score: 99,
                        identity: ProtocolScoreIdentity::Player(44),
                    },
                ]),
            }),
        })
        .unwrap();

    let sidebar = project_scoreboard_sidebar(runtime.scoreboards()).unwrap();
    assert_eq!(sidebar.title.as_ref(), "Wins");
    assert_eq!(
        sidebar
            .rows
            .iter()
            .map(|row| (row.label.as_ref(), row.score.as_str()))
            .collect::<Vec<_>>(),
        [("leader", "12"), ("lower", "7")]
    );
}

#[test]
fn maximum_scoreboard_projection_materializes_only_the_visible_rows_and_reuses_its_revision() {
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 1,
            local_millis: 10,
            server_tick: None,
            event: UiEvent::Objective(ObjectiveEvent::Display {
                display_slot: Arc::from("sidebar"),
                objective_name: Arc::from("maximum"),
                display_name: Arc::from("Maximum"),
                criteria_name: Arc::from("dummy"),
                sort_order: 0,
            }),
        })
        .unwrap();
    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 2,
            local_millis: 20,
            server_tick: None,
            event: UiEvent::Score(ScoreEvent {
                action: ProtocolScoreAction::Change,
                entries: (0..ui::MAX_SCORES)
                    .map(|index| ProtocolScoreEntry {
                        scoreboard_id: index as i64,
                        objective_name: Arc::from("maximum"),
                        score: index as i32,
                        identity: if index % 257 == 0 {
                            ProtocolScoreIdentity::Player(index as i64)
                        } else {
                            ProtocolScoreIdentity::FakePlayer(Arc::from(format!("row-{index}")))
                        },
                    })
                    .collect::<Vec<_>>()
                    .into(),
            }),
        })
        .unwrap();

    let mut cache = PresentedScoreboardCache::default();
    let first = cache.refresh(runtime.scoreboards()).unwrap();
    assert_eq!(
        first.rows.len(),
        retained_hud::MAX_PRESENTED_SCOREBOARD_ROWS
    );
    assert_eq!(first.rows[0].label.as_ref(), "row-1");
    assert_eq!(first.rows[14].label.as_ref(), "row-15");
    let first_rows = first.rows.as_ptr();
    assert_eq!(
        cache.refresh(runtime.scoreboards()).unwrap().rows.as_ptr(),
        first_rows,
        "an unchanged retained revision must not rematerialize the maximum store",
    );

    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 3,
            local_millis: 30,
            server_tick: None,
            event: UiEvent::Score(ScoreEvent {
                action: ProtocolScoreAction::Change,
                entries: Arc::from([ProtocolScoreEntry {
                    scoreboard_id: 1,
                    objective_name: Arc::from("maximum"),
                    score: -10_000,
                    identity: ProtocolScoreIdentity::FakePlayer(Arc::from("promoted")),
                }]),
            }),
        })
        .unwrap();
    let refreshed = cache.refresh(runtime.scoreboards()).unwrap();
    assert_eq!(
        refreshed.rows.len(),
        retained_hud::MAX_PRESENTED_SCOREBOARD_ROWS
    );
    assert_eq!(refreshed.rows[0].label.as_ref(), "promoted");
    assert_eq!(refreshed.rows[0].score, "-10000");
}

#[test]
fn scoreboard_rows_are_capped_and_scissored_to_the_provisional_row_height() {
    let font = fixture_font();
    let font_page = 0;
    let mut presentation = UiPresentationRuntime::new(font).unwrap();
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 1,
            local_millis: 10,
            server_tick: None,
            event: UiEvent::Objective(ObjectiveEvent::Display {
                display_slot: Arc::from("sidebar"),
                objective_name: Arc::from("wins"),
                display_name: Arc::from("Very long scoreboard title ".repeat(30)),
                criteria_name: Arc::from("dummy"),
                sort_order: 1,
            }),
        })
        .unwrap();
    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 2,
            local_millis: 20,
            server_tick: None,
            event: UiEvent::Score(ScoreEvent {
                action: ProtocolScoreAction::Change,
                entries: (0..16)
                    .map(|index| ProtocolScoreEntry {
                        scoreboard_id: index,
                        objective_name: Arc::from("wins"),
                        score: 100 - index as i32,
                        identity: ProtocolScoreIdentity::FakePlayer(Arc::from(
                            "long player label ".repeat(30),
                        )),
                    })
                    .collect::<Vec<_>>()
                    .into(),
            }),
        })
        .unwrap();

    assert_eq!(
        project_scoreboard_sidebar(runtime.scoreboards())
            .unwrap()
            .rows
            .len(),
        retained_hud::MAX_PRESENTED_SCOREBOARD_ROWS
    );
    let input = presentation
        .build(&runtime, 20, [800, 600], DpiScale::new(1.0).unwrap())
        .unwrap();
    let text_batches = input
        .batches
        .iter()
        .filter(|batch| batch.texture_page == font_page && batch.scissor.x >= 626)
        .collect::<Vec<_>>();
    assert!(
        !text_batches.is_empty(),
        "render batches: {:?}",
        input.batches
    );
    assert!(
        text_batches
            .iter()
            .all(|batch| batch.scissor.height <= SCOREBOARD_TEXT_HEIGHT as u32),
        "scoreboard text escaped its bounded provisional label row: {text_batches:?}"
    );
}

#[test]
fn boss_layout_is_bounded_and_preserves_normalized_color_and_notches() {
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 1,
            local_millis: 10,
            server_tick: None,
            event: boss_event(
                ProtocolBossAction::Show,
                44,
                "Dragon",
                0.5,
                ProtocolBossColor::Purple,
                ProtocolBossOverlay::Notched10,
            ),
        })
        .unwrap();

    let bars = project_boss_bars(runtime.boss_bars(), 800.0, 600.0);
    assert_eq!(bars.len(), 1);
    let bar = &bars[0];
    assert_eq!(bar.panel, [309.0, 2.0, 491.0, 22.0]);
    assert_eq!(bar.progress, [309.0, 12.0, 491.0, 17.0]);
    assert_eq!(bar.fill, [309.0, 12.0, 400.0, 17.0]);
    assert_eq!(bar.color, [170, 0, 170, 255]);
    assert_eq!(bar.notch_x.len(), 9);
    assert!((bar.notch_x[0] - 327.2).abs() < 0.001);
    assert!((bar.notch_x[8] - 472.8).abs() < 0.001);

    assert!(project_boss_bars(runtime.boss_bars(), 800.0, 60.0).is_empty());
}

#[test]
fn boss_projection_stacks_in_first_show_order_and_caps_to_viewport_capacity() {
    let mut runtime = UiRuntime::new(1);
    for (sequence, entity, title, color) in [
        (1, 40, "first", ProtocolBossColor::Purple),
        (2, 10, "second", ProtocolBossColor::Blue),
        (3, 30, "third", ProtocolBossColor::Green),
        (4, 20, "hidden-by-cap", ProtocolBossColor::Red),
    ] {
        runtime
            .apply(SequencedUiEvent {
                session_id: 1,
                fifo_sequence: sequence,
                local_millis: sequence * 10,
                server_tick: None,
                event: boss_event(
                    ProtocolBossAction::Show,
                    entity,
                    title,
                    0.5,
                    color,
                    ProtocolBossOverlay::Progress,
                ),
            })
            .unwrap();
    }

    let bars = project_boss_bars(runtime.boss_bars(), 800.0, 220.0);
    assert_eq!(bars.len(), 3);
    assert_eq!(
        bars.iter()
            .map(|bar| bar.title.as_ref())
            .collect::<Vec<_>>(),
        ["first", "second", "third"]
    );
    assert_eq!(bars[0].panel, [309.0, 2.0, 491.0, 22.0]);
    assert_eq!(bars[1].panel, [309.0, 22.0, 491.0, 42.0]);
    assert_eq!(bars[2].panel, [309.0, 42.0, 491.0, 62.0]);

    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 5,
            local_millis: 50,
            server_tick: None,
            event: boss_event(
                ProtocolBossAction::Show,
                40,
                "first-replaced",
                0.25,
                ProtocolBossColor::Red,
                ProtocolBossOverlay::Notched6,
            ),
        })
        .unwrap();
    let replaced = project_boss_bars(runtime.boss_bars(), 800.0, 220.0);
    assert_eq!(replaced[0].title.as_ref(), "first-replaced");
    assert_eq!(replaced[0].panel, [309.0, 2.0, 491.0, 22.0]);
    assert_eq!(replaced[0].fill, [309.0, 12.0, 354.5, 17.0]);
    assert_eq!(replaced[0].notch_x.len(), 5);
    assert_eq!(replaced[1].title.as_ref(), "second");
}

#[test]
fn scoreboard_and_boss_renderer_preserve_semantic_geometry_scissors_and_draw_order() {
    let font = fixture_font();
    let mut presentation = UiPresentationRuntime::new(font).unwrap();
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 1,
            local_millis: 10,
            server_tick: None,
            event: title_event(TitleAction::SetTitle, "Title"),
        })
        .unwrap();
    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 2,
            local_millis: 20,
            server_tick: None,
            event: title_event(TitleAction::ActionBar, "Action"),
        })
        .unwrap();
    let title_only = presentation
        .build(&runtime, 20, [800, 600], DpiScale::new(1.0).unwrap())
        .unwrap();
    assert_title_and_actionbar_geometry(&title_only);

    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 3,
            local_millis: 30,
            server_tick: None,
            event: UiEvent::Objective(ObjectiveEvent::Display {
                display_slot: Arc::from("sidebar"),
                objective_name: Arc::from("wins"),
                display_name: Arc::from("Wins"),
                criteria_name: Arc::from("dummy"),
                sort_order: 1,
            }),
        })
        .unwrap();
    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 4,
            local_millis: 40,
            server_tick: None,
            event: UiEvent::Score(ScoreEvent {
                action: ProtocolScoreAction::Change,
                entries: Arc::from([ProtocolScoreEntry {
                    scoreboard_id: 4,
                    objective_name: Arc::from("wins"),
                    score: 12,
                    identity: ProtocolScoreIdentity::FakePlayer(Arc::from("leader")),
                }]),
            }),
        })
        .unwrap();
    let with_scoreboard = presentation
        .build(&runtime, 40, [800, 600], DpiScale::new(1.0).unwrap())
        .unwrap();
    assert_eq!(
        bounds_for_color(&with_scoreboard, [0, 0, 0, 128]),
        Some([626.0, 290.0, 800.0, 310.0])
    );
    assert_eq!(
        bounds_for_color(&with_scoreboard, [0, 0, 0, 160]),
        Some([628.0, 290.0, 798.0, 300.0])
    );
    assert!(with_scoreboard.batches.iter().any(|batch| {
        batch.texture_page == 0 && batch.scissor == render::UiScissor::new(738, 300, 60, 10)
    }));
    assert_eq!(
        horizontal_bounds_for_color(&with_scoreboard, [255, 0, 0, 255]),
        Some([774.0, 798.0])
    );
    assert_title_and_actionbar_geometry(&with_scoreboard);

    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 5,
            local_millis: 50,
            server_tick: None,
            event: UiEvent::Score(ScoreEvent {
                action: ProtocolScoreAction::Change,
                entries: Arc::from([ProtocolScoreEntry {
                    scoreboard_id: 4,
                    objective_name: Arc::from("wins"),
                    score: 7,
                    identity: ProtocolScoreIdentity::FakePlayer(Arc::from("leader")),
                }]),
            }),
        })
        .unwrap();
    let updated_score = presentation
        .build(&runtime, 50, [800, 600], DpiScale::new(1.0).unwrap())
        .unwrap();
    assert_eq!(
        horizontal_bounds_for_color(&updated_score, [255, 0, 0, 255]),
        Some([782.0, 798.0]),
        "score replacement must update the right-aligned glyph geometry",
    );
    assert_title_and_actionbar_geometry(&updated_score);

    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 6,
            local_millis: 60,
            server_tick: None,
            event: boss_event(
                ProtocolBossAction::Show,
                44,
                "Dragon",
                0.5,
                ProtocolBossColor::Purple,
                ProtocolBossOverlay::Notched10,
            ),
        })
        .unwrap();
    let with_overlays = presentation
        .build(&runtime, 60, [800, 600], DpiScale::new(1.0).unwrap())
        .unwrap();
    assert_eq!(
        bounds_for_color(&with_overlays, [32, 32, 32, 255]),
        Some([309.0, 12.0, 491.0, 17.0])
    );
    assert_eq!(
        bounds_for_color(&with_overlays, [170, 0, 170, 255]),
        Some([309.0, 12.0, 400.0, 17.0])
    );
    assert_eq!(
        bounds_for_color(&with_overlays, [16, 16, 16, 255]),
        Some([326.7, 12.0, 473.3, 17.0])
    );
    let scoreboard_batch = with_overlays
        .batches
        .iter()
        .position(|batch| batch.scissor == render::UiScissor::new(738, 300, 60, 10))
        .unwrap();
    let boss_batch = with_overlays
        .batches
        .iter()
        .position(|batch| batch.scissor == render::UiScissor::new(309, 2, 182, 10))
        .unwrap();
    assert!(
        scoreboard_batch < boss_batch,
        "scoreboard must draw before the boss stack"
    );
    assert_title_and_actionbar_geometry(&with_overlays);

    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 7,
            local_millis: 70,
            server_tick: None,
            event: boss_event(
                ProtocolBossAction::SetProgress,
                44,
                "",
                0.25,
                ProtocolBossColor::Purple,
                ProtocolBossOverlay::Notched10,
            ),
        })
        .unwrap();
    let updated_boss = presentation
        .build(&runtime, 70, [800, 600], DpiScale::new(1.0).unwrap())
        .unwrap();
    assert_eq!(
        bounds_for_color(&updated_boss, [170, 0, 170, 255]),
        Some([309.0, 12.0, 354.5, 17.0]),
        "boss progress update must replace the fill geometry",
    );
    assert_title_and_actionbar_geometry(&updated_boss);

    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 8,
            local_millis: 80,
            server_tick: None,
            event: boss_event(
                ProtocolBossAction::Hide,
                44,
                "",
                0.0,
                ProtocolBossColor::Purple,
                ProtocolBossOverlay::Notched10,
            ),
        })
        .unwrap();
    let after_boss_removal = presentation
        .build(&runtime, 80, [800, 600], DpiScale::new(1.0).unwrap())
        .unwrap();
    assert_eq!(
        bounds_for_color(&after_boss_removal, [32, 32, 32, 255]),
        None
    );
    assert_eq!(
        bounds_for_color(&after_boss_removal, [170, 0, 170, 255]),
        None
    );
    assert_eq!(
        bounds_for_color(&after_boss_removal, [0, 0, 0, 128]),
        Some([626.0, 290.0, 800.0, 310.0])
    );
    assert_title_and_actionbar_geometry(&after_boss_removal);

    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence: 9,
            local_millis: 90,
            server_tick: None,
            event: UiEvent::Objective(ObjectiveEvent::Remove {
                objective_name: Arc::from("wins"),
            }),
        })
        .unwrap();
    let removed = presentation
        .build(&runtime, 90, [800, 600], DpiScale::new(1.0).unwrap())
        .unwrap();
    assert_eq!(bounds_for_color(&removed, [0, 0, 0, 128]), None);
    assert_eq!(bounds_for_color(&removed, [0, 0, 0, 160]), None);
    assert_eq!(
        horizontal_bounds_for_color(&removed, [255, 0, 0, 255]),
        None
    );
    assert_title_and_actionbar_geometry(&removed);
}
