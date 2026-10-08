//! Tests the app chat input system against the shared UI renderer.

use json_ui::{Draw, DrawNode};

use super::*;

use client_ui::test_support::{engine_presentation, engine_presentation_with};

#[test]
fn link_confirmation_consumes_typing_and_escape_preserves_chat_draft() {
    use bevy::{
        input::keyboard::{Key, KeyboardInput},
        prelude::*,
        window::{CursorOptions, PrimaryWindow},
    };
    let Some(mut presentation) = engine_presentation() else {
        eprintln!("skipping link_confirmation input: missing installed UI carrier (make assets)");
        return;
    };
    let mut player = crate::player_runtime::PlayerRuntime::new(1);
    let mut runtime = UiRuntime::new(1);
    chat(
        &mut player,
        &mut runtime,
        1,
        "Visit https://example.com/info",
    );
    runtime.open_chat(&mut player);
    runtime.insert_chat_text("unsent draft").unwrap();
    build(&player, &mut presentation, &runtime, 0);
    let index = presentation
        .chat_hits()
        .iter()
        .find_map(|(hit, _)| match hit {
            client_ui::ui_runtime::presentation::ChatHit::Link(index) => Some(*index),
            _ => None,
        })
        .expect("visible chat URL has a hit target");
    presentation.request_chat_link(index);
    assert!(presentation.chat_link_confirmation_open());
    let mut app = App::new();
    app.add_message::<KeyboardInput>()
        .init_resource::<Time<Real>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<bevy::input::mouse::AccumulatedMouseMotion>()
        .insert_resource(player)
        .insert_resource(runtime)
        .insert_resource(presentation)
        .add_systems(Update, crate::ui_runtime::drive_chat_keyboard_input);
    let window = app
        .world_mut()
        .spawn((
            Window {
                focused: true,
                ..Default::default()
            },
            CursorOptions::default(),
            PrimaryWindow,
        ))
        .id();
    for key in [KeyCode::KeyX, KeyCode::Enter, KeyCode::Escape] {
        app.world_mut().write_message(KeyboardInput {
            key_code: key,
            logical_key: Key::Character("x".into()),
            state: bevy::input::ButtonState::Pressed,
            text: Some("x".into()),
            repeat: false,
            window,
        });
        app.update();
        let runtime = app.world().resource::<UiRuntime>();
        assert!(runtime.chat_focused());
        assert_eq!(runtime.chat_editor().as_str(), "unsent draft");
        assert!(runtime.pending_chat_sends().is_empty());
        assert_eq!(
            app.world()
                .resource::<UiPresentationRuntime>()
                .chat_link_confirmation_open(),
            key != KeyCode::Escape
        );
    }
}

/// Finds one rendered label by its content.
fn text_node<'a>(nodes: &'a [DrawNode], wanted: &str) -> Option<&'a DrawNode> {
    nodes
        .iter()
        .find(|node| matches!(&node.draw, Draw::Text { text, .. } if text == wanted))
}

/// Applies one authoritative chat message.
fn chat(
    player_runtime: &mut crate::player_runtime::PlayerRuntime,
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

/// Lays out one gameplay frame.
fn build(
    player_runtime: &crate::player_runtime::PlayerRuntime,
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

#[test]
fn wheel_input_system_scrolls_the_open_chat() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    use bevy::{
        input::mouse::AccumulatedMouseScroll, prelude::*, time::Real, window::PrimaryWindow,
    };
    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping wheel_input_system_scrolls_the_open_chat: fixture unavailable; requires installed local carriers (make assets)"
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
    assert!(text_node(presentation.chat_draw_nodes(), "line 1").is_none());
    let mut app = App::new();
    app.init_resource::<Time<Real>>()
        .init_resource::<crate::local_player::LocalPlayerFrameCarrier>()
        .init_resource::<crate::local_player::InteractionOriginSnapshot>()
        .init_resource::<crate::semantic_controls::SemanticInputSnapshot>()
        .init_resource::<crate::runtime::world::ClientWorld>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<Touches>()
        .insert_resource(AccumulatedMouseScroll {
            delta: Vec2::new(0.0, 1000.0),
            ..Default::default()
        })
        .insert_resource(runtime)
        .insert_resource(player_runtime)
        .insert_resource(presentation)
        .add_systems(Update, crate::ui_runtime::drive_chat_ui_actions);
    app.world_mut().spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        PrimaryWindow,
    ));
    app.update();
    let player_runtime = app
        .world_mut()
        .remove_resource::<crate::player_runtime::PlayerRuntime>()
        .unwrap();
    let runtime = app.world_mut().remove_resource::<UiRuntime>().unwrap();
    let mut presentation = app
        .world_mut()
        .remove_resource::<UiPresentationRuntime>()
        .unwrap();
    build(&player_runtime, &mut presentation, &runtime, 0);
    let oldest = text_node(presentation.chat_draw_nodes(), "line 1").unwrap();
    assert!(oldest.dest.y + oldest.dest.h > oldest.clip.y);
    assert!(oldest.dest.y < oldest.clip.y + oldest.clip.h);
}

/// Collects visible labels from the resolved chat screen.
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

#[test]
fn server_chat_screen_keeps_the_java_layout_and_input_policy() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(mut presentation) =
        engine_presentation_with(super::super::forms::pack_harness::font())
    else {
        eprintln!(
            "skipping server_chat_screen_keeps_the_java_layout_and_input_policy: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = UiRuntime::new(1);
    runtime.open_chat(&mut player_runtime);
    presentation.set_server_ui_pack(&client_ui::ui_runtime::presentation::forms::ServerUiPack {
        ui_layers: vec![vec![(
            "ui/chat_screen.json".into(),
            br#"{"namespace":"chat","chat_screen":{"render_game_behind":false},
                "chat_screen_content":{"controls":[{"server_marker":{"type":"label",
                    "text":"Server chat","offset":[0,40]}}]}}"#
                .to_vec(),
        )]],
        ..Default::default()
    });
    build(&player_runtime, &mut presentation, &runtime, 0);
    assert!(!texts(presentation.chat_draw_nodes()).contains(&"Server chat"));
    let menu = crate::menu::MenuRuntime::new(false, 2, "Tester".into());
    assert!(presentation.renders_game_behind(&player_runtime, &runtime, &menu));
    let input = presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    client_ui::ui_runtime::presentation::forms::snapshot::write(&input, "server-chat");
    presentation.set_server_ui_pack(&Default::default());
    build(&player_runtime, &mut presentation, &runtime, 0);
    assert!(!texts(presentation.chat_draw_nodes()).contains(&"Server chat"));
    assert!(presentation.renders_game_behind(&player_runtime, &runtime, &menu));
    assert!(presentation.absorbs_gameplay_input(&player_runtime, &runtime, &menu));
}
