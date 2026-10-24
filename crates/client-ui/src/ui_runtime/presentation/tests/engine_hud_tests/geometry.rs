//! Screen positions and painted bounds of the Java-look HUD.

use super::*;

// Java Gui geometry: hotbar flush at the bottom centre, selection 1 px out,
// status rows from H-39, the outlined level from H-40, and the XP bar at H-29.
#[test]
fn java_pack_geometry_on_a_real_viewport() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping java_pack_geometry_on_a_real_viewport: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = UiRuntime::new(1);
    player_runtime
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    select_slot(&mut player_runtime, &mut runtime);
    full_stats(&mut player_runtime, &mut runtime, 1);
    runtime.hud.set_experience(7, 0.4);
    let input = build_at(
        &player_runtime,
        &mut presentation,
        &runtime,
        0,
        [1280, 750],
        1.5,
    );
    let nodes = presentation.hud_draw_nodes();
    // 1280x750 at scale 3: a 426.67x250 GUI-px screen.
    let centre = 1280.0 / 3.0 / 2.0;
    let slots = customs(nodes, "hotbar_renderer");
    assert!(
        (slots[0].dest.x - (centre - 90.0)).abs() < 1e-3,
        "{:?} vs centre {centre}",
        slots[0].dest
    );
    assert_eq!(slots[0].dest.y, 250.0 - 22.0);
    let selected = named(nodes, "hotbar_slot_selected_image");
    assert!((selected[0].dest.x - (centre - 92.0)).abs() < 1e-3);
    assert_eq!(selected[0].dest.y, 250.0 - 23.0);
    let hearts = customs(nodes, "heart_renderer");
    assert!((hearts[0].dest.x - (centre - 91.0)).abs() < 1e-3);
    assert_eq!(hearts[0].dest.y, 250.0 - 39.0);
    let hunger = customs(nodes, "hunger_renderer");
    assert!((hunger[0].dest.x - (centre + 90.0)).abs() < 1e-3);
    let bar = named(nodes, "empty_progress_bar")
        .into_iter()
        .map(|node| node.dest.y)
        .fold(f64::INFINITY, f64::min);
    assert_eq!(bar, 250.0 - 29.0);
    let level: Vec<_> = nodes
        .iter()
        .filter(|node| matches!(&node.draw, Draw::Text { text, .. } if text == "7"))
        .collect();
    assert_eq!(level.len(), 5, "the level and its four outline copies");
    assert_eq!(level[4].dest.y + 1.0, hearts[0].dest.y);
    // The frame lands in physical pixels, allowing float noise from DPI conversion.
    assert!(
        quad_bounds(&input).any(|bounds| {
            bounds
                .into_iter()
                .zip([364.0, 681.0, 436.0, 753.0])
                .all(|(actual, expected)| (actual - expected).abs() < 1e-3)
        }),
        "selection frame at Java geometry"
    );
}

// The Video setting lifts the painted hotbar, its status rows, and the chat by one distance,
// in the HUD and in the focused chat history, and switching it back restores Java's flush hotbar.
#[test]
fn bedrock_hotbar_setting_raises_the_painted_hud_and_focused_chat() {
    use launcher::menu::settings_options::{BEDROCK_HOTBAR_OPTION, SETTINGS_OPTIONS};

    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping bedrock_hotbar_setting_raises_the_painted_hud_and_focused_chat: missing local UI carrier (make assets)"
        );
        return;
    };
    let mut player_runtime = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    player_runtime
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    select_slot(&mut player_runtime, &mut runtime);
    full_stats(&mut player_runtime, &mut runtime, 1);
    runtime
        .apply(
            &mut player_runtime,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 2,
                local_millis: 10,
                server_tick: None,
                event: chat_event("hotbar proof"),
            },
        )
        .unwrap();
    let option = SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == BEDROCK_HOTBAR_OPTION.name)
        .unwrap();
    let gui_height = 720.0 / ui::gui_scale([1280, 720], presentation.gui_scale_preference) as f64;
    let mut settings = launcher::menu::settings_options::SettingsOptions::default();
    let mut placed = |raised: bool| {
        settings.set(option, i32::from(raised));
        presentation.set_chat_settings_snapshot((std::sync::Arc::new(settings.clone()), None));
        runtime.close_chat();
        build(&player_runtime, &mut presentation, &runtime, 10);
        let nodes = presentation.hud_draw_nodes();
        let hud = [
            customs(nodes, "hotbar_renderer")[0].dest.y,
            customs(nodes, "heart_renderer")[0].dest.y,
            text(nodes, "hotbar proof").expect("HUD chat").dest.y,
        ];
        runtime.open_chat(&mut player_runtime);
        build(&player_runtime, &mut presentation, &runtime, 10);
        let history = text(presentation.chat_draw_nodes(), "hotbar proof")
            .expect("focused history")
            .dest
            .y;
        [hud[0], hud[1], hud[2], history]
    };
    let flush = placed(false);
    let raised = placed(true);
    assert_eq!(flush[0], gui_height - 22.0, "Java's hotbar is flush");
    let lift = flush[0] - raised[0];
    assert!(lift > 0.0, "the hotbar rises: {flush:?} -> {raised:?}");
    for (flush, raised) in flush.into_iter().zip(raised) {
        assert_eq!(raised, flush - lift, "{flush:?} -> {raised:?}");
    }
    assert_eq!(placed(false), flush);
}
