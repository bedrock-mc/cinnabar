//! Tests the app chat input system against the shared UI renderer.

use json_ui::{Draw, DrawNode};

use super::*;

use client_ui::test_support::{engine_presentation, engine_presentation_with};

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
fn server_chat_screen_withdraws_the_java_layout_and_restores_on_removal() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(mut presentation) =
        engine_presentation_with(super::super::forms::pack_harness::font())
    else {
        eprintln!(
            "skipping server_chat_screen_withdraws_the_java_layout_and_restores_on_removal: fixture unavailable; requires installed local carriers (make assets)"
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
    assert!(texts(presentation.chat_draw_nodes()).contains(&"Server chat"));
    let menu = crate::menu::MenuRuntime::new(false, 2, "Tester".into());
    assert!(!presentation.renders_game_behind(&player_runtime, &runtime, &menu));
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
