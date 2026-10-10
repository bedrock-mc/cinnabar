//! The open chat through built-in and native `chat.chat_screen`: history, edit box,
//! suggestions, send/back hits and scrolling. Needs the gitignored UI carrier;
//! each test skips when it is absent.

use json_ui::{Draw, DrawNode};

use super::engine_hud_tests::{engine_presentation, engine_presentation_with};
use super::*;
use crate::ui_runtime::presentation::ChatHit;

/// Selects the native catalog directly for native control checks.
fn native_chat_presentation() -> Option<UiPresentationRuntime> {
    let mut presentation = engine_presentation_with(super::super::forms::pack_harness::font())?;
    presentation.set_native_chat_fixture(true)?;
    Some(presentation)
}

#[test]
fn selected_chat_text_draws_the_native_inversion_over_only_the_selected_glyphs() {
    for native in [false, true] {
        let Some(mut presentation) = (if native {
            native_chat_presentation()
        } else {
            engine_presentation_with(super::super::forms::pack_harness::font())
        }) else {
            eprintln!("chat selection fixture unavailable; requires installed local carriers");
            return;
        };
        let mut player = player_state::PlayerState::new(1);
        let mut runtime = gameplay_runtime(&mut player);
        runtime.open_chat(&mut player);
        runtime.insert_chat_text("A世界🦀B").unwrap();
        // Discover the focused edit box from the real carrier's first frame.
        build(&player, &mut presentation, &runtime, 0);
        runtime.move_chat_cursor_left();
        runtime.mutate_chat_editor(ui::ChatEditor::select_left);
        runtime.mutate_chat_editor(ui::ChatEditor::select_left);
        let nodes = chat_visual_nodes(&mut presentation, &runtime, 0);
        let highlighted = nodes
            .iter()
            .find(|node| matches!(node.visual(), ui::UiVisual::InvertedSprite { .. }))
            .expect("chat's selected glyphs must emit native inversion geometry");
        let text = nodes.iter().find(|node| {
            matches!(node.visual(), ui::UiVisual::Text { layout, .. }
                if layout.glyphs().iter().map(|glyph| glyph.codepoint).collect::<String>() == "A世界🦀B")
        }).expect("selected draft stays visible with no inserted caret");
        assert!(highlighted.bounds().width() > 0.0);
        assert!(highlighted.bounds().width() < text.bounds().width());
        assert!(highlighted.bounds().min().x() > text.bounds().min().x());
        assert!(highlighted.bounds().max().x() < text.bounds().max().x());
        assert_eq!(highlighted.parent(), text.parent());
        let input = presentation
            .build(
                &player,
                &runtime,
                0,
                [1280, 720],
                DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        assert!(
            input
                .batches
                .iter()
                .any(|batch| batch.blend_mode == render_model::UI_BLEND_INVERT)
        );
        super::super::forms::snapshot::write(
            &input,
            if native {
                "chat_selection_native"
            } else {
                "chat_selection_java"
            },
        );

        runtime.mutate_chat_editor(|editor| {
            editor.move_home(false);
            editor.move_end(true);
        });
        let all = chat_visual_nodes(&mut presentation, &runtime, 0);
        let all = all
            .iter()
            .find(|node| matches!(node.visual(), ui::UiVisual::InvertedSprite { .. }))
            .unwrap();
        assert!(all.bounds().width() > highlighted.bounds().width());
        runtime.move_chat_cursor_right();
        let plain = chat_visual_nodes(&mut presentation, &runtime, 0);
        assert!(
            !plain
                .iter()
                .any(|node| matches!(node.visual(), ui::UiVisual::InvertedSprite { .. }))
        );
        assert!(texts(presentation.chat_draw_nodes()).contains(&"A世界🦀B|"));
    }
}

fn chat_visual_nodes(
    presentation: &mut UiPresentationRuntime,
    runtime: &UiRuntime,
    now: u64,
) -> Vec<ui::UiNode> {
    let (mut nodes, mut next) = (Vec::new(), 1);
    presentation
        .append_chat_screen(
            runtime,
            &mut nodes,
            &mut next,
            TextMetrics::for_viewport([1280, 720], DpiScale::new(1.0).unwrap(), None),
            [1280.0, 720.0],
            now,
        )
        .unwrap();
    nodes
}

/// Visible text in a rendered screen.
fn texts(nodes: &[DrawNode]) -> Vec<&str> {
    nodes
        .iter()
        .filter(|node| node.alpha > 0.0)
        .filter_map(|node| match &node.draw {
            Draw::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

/// Find a rendered label by its content.
fn text_node<'a>(nodes: &'a [DrawNode], wanted: &str) -> Option<&'a DrawNode> {
    nodes
        .iter()
        .find(|node| matches!(&node.draw, Draw::Text { text, .. } if text == wanted))
}

/// Add an authoritative chat message.
fn chat(
    player_runtime: &mut player_state::PlayerState,
    runtime: &mut UiRuntime,
    sequence: u64,
    message: &str,
) {
    runtime
        .apply(
            player_runtime,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence: sequence,
                local_millis: 0,
                server_tick: None,
                event: chat_event(message),
            },
        )
        .unwrap();
}

/// Supply autocomplete results for a command.
fn suggestions(
    player_runtime: &mut player_state::PlayerState,
    runtime: &mut UiRuntime,
    count: usize,
) {
    runtime.insert_chat_text("/").unwrap();
    let request = runtime.take_chat_autocomplete_request().unwrap();
    runtime
        .apply(
            player_runtime,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 100,
                local_millis: 0,
                server_tick: None,
                event: UiEvent::ChatAutocomplete(protocol::ChatAutocompleteEvent {
                    enum_name: Arc::from("commands"),
                    action: protocol::ChatAutocompleteAction::Replace,
                    suggestions: Arc::from(
                        (0..count)
                            .map(|index| Arc::from(format!("/give{index}")))
                            .collect::<Vec<_>>(),
                    ),
                }),
            },
        )
        .unwrap();
    assert!(runtime.complete_chat_autocomplete(player_runtime, request));
}

/// Lay out one fixed-size gameplay frame.
fn build(
    player_runtime: &player_state::PlayerState,
    presentation: &mut UiPresentationRuntime,
    runtime: &UiRuntime,
    now: u64,
) {
    presentation
        .build(
            player_runtime,
            runtime,
            now,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
}

/// Pointer position at a hit rectangle centre.
fn centre(bounds: ui::UiRect) -> UiPoint {
    UiPoint::new(
        (bounds.min().x() + bounds.max().x()) * 0.5,
        (bounds.min().y() + bounds.max().y()) * 0.5,
    )
    .unwrap()
}

/// A survival session with an authoritative selected hotbar slot.
fn gameplay_runtime(player_runtime: &mut player_state::PlayerState) -> UiRuntime {
    let mut runtime = UiRuntime::new(1);
    player_runtime
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    runtime.retain_local_selected_equipment(
        player_runtime,
        1,
        protocol::EquipmentEvent {
            actor_runtime_id: 42,
            stack: protocol::NetworkItemStack::empty(),
            inventory_slot: 0,
            selected_slot: 0,
            window_id: 0,
            handedness: None,
        },
    );
    runtime
}

#[test]
fn open_chat_draws_the_java_line_with_history_and_the_hud() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping open_chat_draws_the_java_line_with_history_and_the_hud: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = gameplay_runtime(&mut player_runtime);
    chat(
        &mut player_runtime,
        &mut runtime,
        1,
        "hello from the server",
    );
    build(&player_runtime, &mut presentation, &runtime, 0);
    let closed_history = text_node(presentation.hud_draw_nodes(), "hello from the server")
        .expect("HUD chat before opening")
        .dest;
    runtime.open_chat(&mut player_runtime);
    runtime.insert_chat_text("typed").unwrap();
    build(&player_runtime, &mut presentation, &runtime, 0);
    let nodes = presentation.chat_draw_nodes();
    let shown = texts(nodes);
    assert!(shown.contains(&"hello from the server"), "{shown:?}");
    assert!(shown.contains(&"typed|"), "caret at the end: {shown:?}");
    let edit = text_node(nodes, "typed|").unwrap();
    let newest = text_node(nodes, "hello from the server").unwrap();
    assert!((newest.dest.x - closed_history.x).abs() < 1.0);
    assert!(
        (newest.dest.y - closed_history.y).abs() < 1.0,
        "history stays put: {:?} -> {:?}",
        closed_history,
        newest.dest
    );
    assert!(edit.dest.y > 200.0 && edit.dest.h <= newest.dest.h * 2.0);
    assert!(newest.dest.y + newest.dest.h < edit.dest.y);
    assert!(presentation.hud_draw_nodes().iter().any(|node| {
        matches!(&node.draw, Draw::Custom { renderer, .. } if renderer == "hotbar_renderer")
    }));
    // The HUD's own chat lines hide while the screen shows the history.
    assert!(
        text_node(presentation.hud_draw_nodes(), "hello from the server")
            .is_none_or(|node| node.alpha <= 0.0)
    );
    build(&player_runtime, &mut presentation, &runtime, 600);
    assert!(
        texts(presentation.chat_draw_nodes()).contains(&"typed"),
        "caret blinks off"
    );
}

#[test]
fn suggestions_and_usage_list_above_the_edit_box_and_hit_by_index() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping suggestions_and_usage_list_above_the_edit_box_and_hit_by_index: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    for count in [1, 4] {
        let mut runtime = UiRuntime::new(1);
        chat(
            &mut player_runtime,
            &mut runtime,
            1,
            "history with suggestions",
        );
        runtime.open_chat(&mut player_runtime);
        suggestions(&mut player_runtime, &mut runtime, count);
        build(&player_runtime, &mut presentation, &runtime, 0);
        let nodes = presentation.chat_draw_nodes();
        let edit = text_node(nodes, "/|").expect("edit box text");
        assert!(texts(nodes).contains(&"history with suggestions"));
        for index in 0..count {
            let row = text_node(nodes, &format!("/give{index}")).expect("suggestion row");
            assert!(
                row.dest.y + row.dest.h <= edit.dest.y,
                "rows sit above the edit box"
            );
            assert!(row.layer > text_node(nodes, "history with suggestions").unwrap().layer);
        }
        let last = text_node(nodes, &format!("/give{}", count - 1)).unwrap();
        let gap = edit.dest.y - (last.dest.y + last.dest.h);
        assert!(
            gap >= 0.0 && gap <= last.dest.h,
            "suggestions stay adjacent to the input: gap {gap}, row {:?}, input {:?}",
            last.dest,
            edit.dest
        );
        assert_suggestion_hits(&presentation, count);
    }
}

fn assert_suggestion_hits(presentation: &UiPresentationRuntime, count: usize) {
    let hits = presentation.chat_hits();
    for index in 0..count {
        let (_, bounds) = hits
            .iter()
            .find(|(hit, _)| *hit == ChatHit::Suggestion(index))
            .expect("suggestion hit");
        assert_eq!(
            presentation.hit_test_chat(centre(*bounds)),
            Some(ChatHit::Suggestion(index))
        );
    }
}

#[test]
fn server_suggestion_offset_keeps_the_builtin_anchor_and_hits() {
    let mut player_runtime = player_state::PlayerState::new(1);
    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping server_suggestion_offset_keeps_the_builtin_anchor_and_hits: missing installed UI carrier; make assets"
        );
        return;
    };
    let mut runtime = UiRuntime::new(1);
    runtime.open_chat(&mut player_runtime);
    suggestions(&mut player_runtime, &mut runtime, 4);
    build(&player_runtime, &mut presentation, &runtime, 0);
    let baseline = text_node(presentation.chat_draw_nodes(), "/give3")
        .expect("suggestion row")
        .dest;
    presentation.set_server_ui_pack(&super::super::forms::ServerUiPack {
        ui_layers: vec![vec![(
            "ui/chat_screen.json".into(),
            br#"{
                "namespace": "chat",
                "java_chat_content/suggestions": {"offset": [0, -70]}
            }"#
            .to_vec(),
        )]],
        ..Default::default()
    });
    build(&player_runtime, &mut presentation, &runtime, 0);
    let moved = text_node(presentation.chat_draw_nodes(), "/give3")
        .expect("server-positioned suggestion")
        .dest;
    assert_eq!(moved, baseline);
    assert_suggestion_hits(&presentation, 4);
}

#[test]
fn server_chat_screen_keeps_builtin_content_and_scene_policy() {
    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping server_chat_screen_keeps_builtin_content_and_scene_policy: missing installed UI carrier; make assets"
        );
        return;
    };
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime.open_chat(&mut player);
    runtime.insert_chat_text("draft").unwrap();
    build(&player, &mut presentation, &runtime, 0);
    let baseline = text_node(presentation.chat_draw_nodes(), "draft|")
        .unwrap()
        .dest;
    presentation.set_server_ui_pack(&super::super::forms::ServerUiPack {
        ui_layers: vec![vec![("ui/chat_screen.json".into(), br#"{
            "namespace":"chat",
            "chat_screen":{"render_game_behind":false,"$screen_content":"chat.server_content"},
            "server_content":{"type":"panel","controls":[{"marker":{"type":"label","text":"Server chat"}}]}
        }"#.to_vec())]],
        ..Default::default()
    });
    build(&player, &mut presentation, &runtime, 0);
    assert_eq!(
        text_node(presentation.chat_draw_nodes(), "draft|")
            .unwrap()
            .dest,
        baseline
    );
    assert!(!texts(presentation.chat_draw_nodes()).contains(&"Server chat"));
    assert!(
        presentation
            .chat_scene_settings()
            .unwrap()
            .render_game_behind
    );
}

#[test]
fn history_opens_on_the_newest_line_and_the_wheel_reveals_older_ones() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping history_opens_on_the_newest_line_and_the_wheel_reveals_older_ones: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = UiRuntime::new(1);
    for sequence in 1..=80 {
        chat(
            &mut player_runtime,
            &mut runtime,
            sequence,
            &format!("line {sequence}"),
        );
    }
    runtime.open_chat(&mut player_runtime);
    build(&player_runtime, &mut presentation, &runtime, 0);
    let visible = |presentation: &UiPresentationRuntime, wanted: &str| {
        text_node(presentation.chat_draw_nodes(), wanted)
            .is_some_and(|node| node.clip.h > 0.0 && node.dest.y + node.dest.h > node.clip.y)
            && text_node(presentation.chat_draw_nodes(), wanted)
                .is_some_and(|node| node.dest.y < node.clip.y + node.clip.h)
    };
    for wanted in ["line 1", "line 80"] {
        let node = text_node(presentation.chat_draw_nodes(), wanted);
        eprintln!("{wanted}: {:?}", node.map(|node| (&node.dest, &node.clip)));
    }
    assert!(visible(&presentation, "line 80"));
    assert!(!visible(&presentation, "line 1"));
    presentation.scroll_chat(1_000.0, false);
    build(&player_runtime, &mut presentation, &runtime, 0);
    assert!(visible(&presentation, "line 1"));
    assert!(!visible(&presentation, "line 80"));
    // A new message jumps back to the newest line.
    chat(&mut player_runtime, &mut runtime, 81, "line 81");
    build(&player_runtime, &mut presentation, &runtime, 0);
    assert!(visible(&presentation, "line 81"));
}

#[test]
fn closed_chat_draws_no_screen_and_hits_nothing() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping closed_chat_draws_no_screen_and_hits_nothing: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = UiRuntime::new(1);
    runtime.open_chat(&mut player_runtime);
    build(&player_runtime, &mut presentation, &runtime, 0);
    assert!(!presentation.chat_draw_nodes().is_empty());
    runtime.close_chat();
    build(&player_runtime, &mut presentation, &runtime, 0);
    assert!(presentation.chat_hits().is_empty());
    assert_eq!(
        presentation.hit_test_chat(UiPoint::new(640.0, 700.0).unwrap()),
        None
    );
}

/// Local-only: writes `chat_screen.png` when `CINNABAR_FORM_SNAPSHOT_DIR` is set.
#[test]
fn chat_screen_snapshot() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let Some(mut presentation) =
        engine_presentation_with(super::super::forms::pack_harness::font())
    else {
        eprintln!(
            "skipping chat_screen_snapshot: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = gameplay_runtime(&mut player_runtime);
    for sequence in 1..=12 {
        chat(
            &mut player_runtime,
            &mut runtime,
            sequence,
            &format!("<Steve> message number {sequence}"),
        );
    }
    presentation.set_native_chat_fixture(true).unwrap();
    runtime.open_chat(&mut player_runtime);
    runtime.insert_chat_text("hello world").unwrap();
    let input = presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    super::super::forms::snapshot::write(&input, "before-chat_history");
    presentation.set_native_chat_fixture(false).unwrap();
    presentation.set_server_ui_pack(&Default::default());
    let input = presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    super::super::forms::snapshot::write(&input, "chat_history");
    runtime.close_chat();
    runtime.open_chat(&mut player_runtime);
    suggestions(&mut player_runtime, &mut runtime, 3);
    let input = presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    super::super::forms::snapshot::write(&input, "chat_screen");
}

/// The real carrier supplies the gear, popup and persisted controls without a network session.
#[test]
fn chat_settings_popup_routes_native_controls_and_retains_the_draft() {
    let mut player_runtime = player_state::PlayerState::new(1);

    use launcher::menu::{
        MenuAction,
        settings_options::{SETTINGS_OPTIONS, SettingsOptions},
    };
    let Some(mut presentation) = native_chat_presentation() else {
        eprintln!(
            "skipping chat_settings_popup_routes_native_controls_and_retains_the_draft: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = UiRuntime::new(1);
    let lang = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("../.local/assets/compiled/vanilla-v1.mcbelang");
    if let Some(lang) = std::fs::read(lang)
        .ok()
        .and_then(|bytes| assets::RuntimeLangCatalog::decode(&bytes).ok())
    {
        runtime.set_lang_catalog(Arc::new(lang));
    }
    chat(&mut player_runtime, &mut runtime, 1, "Visible chat history");
    runtime.open_chat(&mut player_runtime);
    runtime.insert_chat_text("Unsent draft").unwrap();
    let mut options = SettingsOptions::default();
    presentation.set_chat_settings_snapshot((Arc::new(options.clone()), None));
    let input = presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    super::super::forms::snapshot::write(&input, "settings-chat-before");
    assert!(
        presentation
            .chat_hits()
            .iter()
            .any(|(hit, _)| *hit == ChatHit::SettingsOpen)
    );
    presentation.set_chat_settings_open(true);
    let input = presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    super::super::forms::snapshot::write(&input, "settings-chat-after");
    let hits = presentation.chat_hits();
    assert!(
        hits.iter().any(|(hit, _)| *hit == ChatHit::SettingsClose),
        "{hits:?}"
    );
    let mute = SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == "hide_chat")
        .unwrap();
    assert!(
        hits.iter()
            .any(|(hit, _)| *hit
                == ChatHit::SettingsAction(MenuAction::SettingsOption(mute as u16, 1))),
        "{hits:?}"
    );
    assert!(!hits.iter().any(|(hit, _)| *hit == ChatHit::Send));
    options.set(mute, 1);
    presentation.set_chat_settings_snapshot((Arc::new(options), None));
    presentation.set_chat_settings_open(false);
    build(&player_runtime, &mut presentation, &runtime, 0);
    assert!(!texts(presentation.chat_draw_nodes()).contains(&"Visible chat history"));
    assert_eq!(runtime.chat_editor().as_str(), "Unsent draft");
}

#[test]
fn creator_coordinates_bind_native_copy_dropdown_and_invalid_target() {
    let mut player_runtime = player_state::PlayerState::new(1);

    use launcher::menu::settings_options::{SETTINGS_OPTIONS, SettingsOptions};
    let Some(mut presentation) = native_chat_presentation() else {
        eprintln!(
            "skipping creator_coordinates_bind_native_copy_dropdown_and_invalid_target: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = UiRuntime::new(1);
    runtime.open_chat(&mut player_runtime);
    runtime.insert_chat_text("draft").unwrap();
    let mut options = SettingsOptions::default();
    let coordinate_option = SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == "copy_coordinate_ui")
        .unwrap();
    options.set(coordinate_option, 1);
    presentation.set_chat_settings_snapshot((Arc::new(options.clone()), None));
    presentation.set_chat_coordinates(Some([1.25, 64.0, -3.5]), Some([1, 63, -4]));
    build(&player_runtime, &mut presentation, &runtime, 0);
    let hits = presentation.chat_hits();
    for expected in [
        ChatHit::CopyCoordinates,
        ChatHit::Paste,
        ChatHit::CoordinateDropdown,
    ] {
        assert!(
            hits.iter().any(|(hit, _)| *hit == expected),
            "{expected:?}: {hits:?}"
        );
    }
    assert!(texts(presentation.chat_draw_nodes()).contains(&"1.25 64.00 -3.50"));
    assert_eq!(
        presentation.chat_coordinate_text().as_deref(),
        Some("1.25 64.00 -3.50")
    );
    presentation.select_chat_coordinates(None);
    build(&player_runtime, &mut presentation, &runtime, 0);
    for expected in [
        ChatHit::CoordinateSource(false),
        ChatHit::CoordinateSource(true),
    ] {
        assert!(
            presentation
                .chat_hits()
                .iter()
                .any(|(hit, _)| *hit == expected),
            "{expected:?}: {:?}",
            presentation.chat_hits()
        );
    }
    presentation.select_chat_coordinates(Some(true));
    build(&player_runtime, &mut presentation, &runtime, 0);
    assert_eq!(
        presentation.chat_coordinate_text().as_deref(),
        Some("1 63 -4")
    );
    let input = presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    super::super::forms::snapshot::write(&input, "settings-creator-coordinates");
    presentation.chat_coordinates_copied(10);
    build(&player_runtime, &mut presentation, &runtime, 10);
    assert!(texts(presentation.chat_draw_nodes()).contains(&"chat.coordinateCopiedToast"));
    build(&player_runtime, &mut presentation, &runtime, 1000);
    assert!(!texts(presentation.chat_draw_nodes()).contains(&"chat.coordinateCopiedToast"));
    presentation.set_chat_coordinates(Some([1.25, 64.0, -3.5]), None);
    build(&player_runtime, &mut presentation, &runtime, 1000);
    assert_eq!(presentation.chat_coordinate_text(), None);
    assert!(
        !presentation
            .chat_hits()
            .iter()
            .any(|(hit, _)| *hit == ChatHit::CopyCoordinates)
    );
    options.set(coordinate_option, 0);
    presentation.set_chat_settings_snapshot((Arc::new(options), None));
    build(&player_runtime, &mut presentation, &runtime, 1000);
    assert!(!presentation.chat_hits().iter().any(|(hit, _)| matches!(
        hit,
        ChatHit::CopyCoordinates | ChatHit::Paste | ChatHit::CoordinateDropdown
    )));
    assert_eq!(runtime.chat_editor().as_str(), "draft");
}

#[test]
fn chat_links_use_rendered_history_and_native_confirmation_in_both_pack_styles() {
    for native in [false, true] {
        let Some(mut presentation) = (if native {
            native_chat_presentation()
        } else {
            engine_presentation()
        }) else {
            eprintln!(
                "skipping chat_links_use_rendered_history_and_native_confirmation_in_both_pack_styles: missing installed local UI carrier (make assets)"
            );
            return;
        };
        let mut player = player_state::PlayerState::new(1);
        let mut runtime = gameplay_runtime(&mut player);
        chat(
            &mut player,
            &mut runtime,
            1,
            "prefix https://example.com/a then https://example.net/b suffix",
        );
        runtime.open_chat(&mut player);
        runtime.insert_chat_text("keep my draft").unwrap();
        build(&player, &mut presentation, &runtime, 0);
        let links = presentation
            .chat_hits()
            .into_iter()
            .filter_map(|(hit, rect)| {
                if let ChatHit::Link(index) = hit {
                    Some((index, rect))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(
            links.len(),
            2,
            "style native={native}: {:?}",
            presentation.chat_draw_nodes()
        );
        assert_eq!(
            presentation.chat_link(links[0].0),
            Some("https://example.com/a")
        );
        assert_eq!(
            presentation.chat_link(links[1].0),
            Some("https://example.net/b")
        );
        assert_eq!(
            presentation.hit_test_chat(centre(links[0].1)),
            Some(ChatHit::Link(links[0].0))
        );
        assert_eq!(
            presentation.hit_test_chat(centre(links[1].1)),
            Some(ChatHit::Link(links[1].0))
        );
        // The first layout discovers the native edit box; the next frame applies focus.
        build(&player, &mut presentation, &runtime, 0);
        let old_target_pointer = presentation.chat_link(0).unwrap().as_ptr();
        build(&player, &mut presentation, &runtime, 1);
        assert_eq!(
            presentation.chat_link(0).unwrap().as_ptr(),
            old_target_pointer,
            "unchanged frame retains parsed targets"
        );
        presentation.set_chat_settings_open(true);
        build(&player, &mut presentation, &runtime, 2);
        assert!(
            !presentation
                .chat_hits()
                .iter()
                .any(|(hit, _)| matches!(hit, ChatHit::Link(_)))
        );
        presentation.set_chat_settings_open(false);
        build(&player, &mut presentation, &runtime, 3);
        presentation.request_chat_link(links[1].0);
        build(&player, &mut presentation, &runtime, 4);
        let popup = presentation.chat_hits();
        assert!(
            popup.iter().any(|(hit, _)| *hit == ChatHit::LinkOpen),
            "{popup:?}"
        );
        assert!(
            popup.iter().any(|(hit, _)| *hit == ChatHit::LinkCancel),
            "{popup:?}"
        );
        assert!(
            popup
                .iter()
                .all(|(hit, _)| matches!(hit, ChatHit::LinkOpen | ChatHit::LinkCancel))
        );
        presentation.cancel_chat_link();
        assert_eq!(runtime.chat_editor().as_str(), "keep my draft");
        assert_eq!(runtime.chat().messages().len(), 1);
        assert!(!presentation.chat_link_confirmation_open());
    }
}
