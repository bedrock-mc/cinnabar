//! Screen positions and painted bounds of the Java-look HUD.

use super::*;

// Java Gui geometry: hotbar flush at the bottom centre, selection 1 px out,
// status rows and the level from H-39, with the XP bar at H-29.
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
    assert_eq!(level[4].dest.y, hearts[0].dest.y);
    // The frame lands in physical pixels, allowing float noise from DPI conversion.
    assert!(
        input
            .vertices
            .chunks_exact(4)
            .map(quad_bounds)
            .any(|bounds| {
                bounds
                    .into_iter()
                    .zip([364.0, 681.0, 436.0, 753.0])
                    .all(|(actual, expected)| (actual - expected).abs() < 1e-3)
            }),
        "selection frame at Java geometry"
    );
}
