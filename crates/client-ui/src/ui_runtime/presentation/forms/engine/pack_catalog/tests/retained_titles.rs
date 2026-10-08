use std::sync::Arc;

use super::{Draw, vanilla};
use crate::ui_runtime::{
    UiRuntime,
    presentation::forms::{ServerUiPack, pack_harness, snapshot},
};

fn admitted_pack() -> Option<ServerUiPack> {
    let Some(directory) = std::env::var_os("CINNABAR_TEST_RETAINED_HUD_DIR") else {
        eprintln!(
            "skipping admitted_retained_title_panel_publishes_and_survives_other_titles: missing CINNABAR_TEST_RETAINED_HUD_DIR fixture"
        );
        return None;
    };
    let root = std::path::PathBuf::from(directory);
    if !root.join("ui/mineville/titles/objective.json").is_file() {
        eprintln!(
            "skipping admitted_retained_title_panel_publishes_and_survives_other_titles: missing objective HUD fixture in {}",
            root.display()
        );
        return None;
    }
    let ui = pack_harness::pack_files(&root.join("ui"))
        .into_iter()
        .map(|(path, bytes)| (format!("ui/{path}"), bytes))
        .collect();
    let textures = pack_harness::pack_files(&root.join("textures"))
        .into_iter()
        .map(|(path, bytes)| (format!("textures/{path}"), bytes))
        .collect();
    Some(ServerUiPack {
        ui_layers: vec![ui],
        textures,
        ..Default::default()
    })
}

#[test]
fn admitted_retained_title_panel_publishes_and_survives_other_titles() {
    let Some(pack) = admitted_pack() else { return };
    let Some(_vanilla) =
        vanilla("admitted_retained_title_panel_publishes_and_survives_other_titles")
    else {
        return;
    };
    let text = "Tutorial [1/10]\nWelcome to the world!\nFinish the tutorial to continue (9s)";
    let payload = format!("%obj%{text}");
    let mut presentation = crate::test_support::engine_presentation_with(pack_harness::font())
        .expect("installed UI carrier already loaded by this fixture");
    let player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime.set_server_ui(Some(Arc::new(pack)));
    for now in [0, 16, 32] {
        presentation
            .build(
                &player,
                &runtime,
                now,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
    }
    runtime.hud.set_title(Arc::from(payload), 1, 0);
    for now in [100, 116, 132] {
        presentation
            .build(
                &player,
                &runtime,
                now,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
    }
    presentation.finish_menu_artwork();
    let frame = presentation
        .build(
            &player,
            &runtime,
            148,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    snapshot::write(&frame, "retained-tutorial");
    assert_eq!(
        presentation
            .hud_draw_nodes()
            .iter()
            .filter(|node| matches!(&node.draw, Draw::Text { text: drawn, .. } if drawn == text))
            .count(),
        1,
        "authored objective panel did not bind: {:?}",
        presentation
            .hud_draw_nodes()
            .iter()
            .filter_map(|node| match &node.draw {
                Draw::Text { text, .. } => Some((text, &node.key)),
                _ => None,
            })
            .collect::<Vec<_>>()
    );
    assert!(!presentation.hud_draw_nodes().iter().any(|node| matches!(&node.draw,
        Draw::Text { text: drawn, .. } if drawn.contains("%obj%") || (drawn != text && drawn.contains("Welcome to the world!")))),
        "objective trigger leaked into another panel");
    for (now, title) in [
        (200, "An unrelated title"),
        (300, "Another unrelated title"),
    ] {
        runtime.hud.set_title(Arc::from(title), now, now);
        for now in [now, now + 16, now + 32] {
            presentation
                .build(
                    &player,
                    &runtime,
                    now,
                    [1280, 720],
                    ui::DpiScale::new(1.0).unwrap(),
                )
                .unwrap();
        }
        let nodes = presentation.hud_draw_nodes();
        let drawn = nodes
            .iter()
            .filter(|node| matches!(&node.draw, Draw::Text { text: drawn, .. } if drawn == text))
            .collect::<Vec<_>>();
        assert_eq!(
            drawn.len(),
            1,
            "retained objective disappeared after another title"
        );
        assert!(
            drawn[0].dest.y < 720.0 / 2.0,
            "retained objective lost top-center placement"
        );
        assert!(!nodes.iter().any(|node| matches!(&node.draw,
            Draw::Text { text: drawn, .. } if drawn.contains("%obj%") || (drawn != text && drawn.contains("Welcome to the world!")))),
            "retained objective trigger leaked into another panel");
    }
}
