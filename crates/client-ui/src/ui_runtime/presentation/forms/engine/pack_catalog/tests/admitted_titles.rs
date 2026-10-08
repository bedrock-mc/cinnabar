use std::sync::Arc;

use json_ui::HudTitle;

use super::{Draw, HudModel, layer_pack_catalog, render_model, vanilla};
use crate::ui_runtime::{
    UiRuntime,
    presentation::forms::{ServerUiPack, pack_harness, snapshot},
};

#[test]
fn admitted_dynamic_title_selects_only_its_authored_branch() {
    let Some(directory) = std::env::var_os("CINNABAR_TEST_DYNAMIC_HUD_DIR") else {
        eprintln!(
            "skipping admitted_dynamic_title_selects_only_its_authored_branch: missing CINNABAR_TEST_DYNAMIC_HUD_DIR fixture"
        );
        return;
    };
    let root = std::path::PathBuf::from(directory);
    if !root.join("ui/hud_screen.json").is_file() {
        eprintln!(
            "skipping admitted_dynamic_title_selects_only_its_authored_branch: missing HUD fixture in {}",
            root.display()
        );
        return;
    }
    let Some(vanilla) = vanilla("admitted_dynamic_title_selects_only_its_authored_branch") else {
        return;
    };
    let pack = pack_harness::pack_files(&root.join("ui"))
        .into_iter()
        .map(|(path, bytes)| (format!("ui/{path}"), bytes))
        .collect::<Vec<_>>();
    let catalog = layer_pack_catalog(&vanilla, &[pack.clone()]);
    let title = "§m§aRound finished";
    let subtitle = "Spectating";
    let frame = render_model(
        &catalog,
        &HudModel {
            title: Some(HudTitle {
                title: title.into(),
                subtitle: subtitle.into(),
                stay: 3.0,
                ..Default::default()
            }),
            ..Default::default()
        },
        [480.0, 270.0],
    );
    let labels = frame
        .nodes
        .iter()
        .filter_map(|node| match &node.draw {
            Draw::Text { text, scale, .. } if text == title || text == subtitle => {
                Some((text.as_str(), *scale, node.alpha, &node.key, &node.dest))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    eprintln!("admitted title labels: {labels:?}");
    assert_eq!(
        labels.iter().filter(|label| label.0 == title).count(),
        2,
        "authored title and shadow must replace the standard title"
    );
    assert_eq!(
        labels.iter().filter(|label| label.0 == subtitle).count(),
        1,
        "authored body must replace the standard subtitle"
    );
    assert!(
        labels.iter().all(|label| label.1 <= 2.0),
        "standard title drew alongside the authored modal"
    );

    let mut presentation = crate::test_support::engine_presentation_with(pack_harness::font())
        .expect("installed UI carrier already loaded by this fixture");
    let player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime.set_server_ui(Some(Arc::new(ServerUiPack {
        ui_layers: vec![pack],
        textures: pack_harness::pack_files(&root.join("textures"))
            .into_iter()
            .map(|(path, bytes)| (format!("textures/{path}"), bytes))
            .collect(),
        ..Default::default()
    })));
    runtime.hud.set_title(Arc::from("Ordinary title"), 1, 0);
    runtime
        .hud
        .set_subtitle(Arc::from("Ordinary subtitle"), 2, 0);
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
    runtime.hud.set_title(Arc::from(title), 3, 0);
    runtime.hud.set_subtitle(Arc::from(subtitle), 4, 0);
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
            1100,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    snapshot::write(&frame, "admitted-dynamic-title");
    let labels = presentation
        .hud_draw_nodes()
        .iter()
        .filter_map(|node| match &node.draw {
            Draw::Text { text, scale, .. } if text == title || text == subtitle => {
                Some((text.as_str(), *scale, node.alpha, &node.key, &node.dest))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    eprintln!("published admitted title labels: {labels:?}");
    assert_eq!(labels.iter().filter(|label| label.0 == title).count(), 2);
    assert_eq!(labels.iter().filter(|label| label.0 == subtitle).count(), 1);
    assert!(labels.iter().all(|label| label.1 <= 2.0));
    assert!(
        !presentation
            .hud_draw_nodes()
            .iter()
            .any(|node| matches!(&node.draw,
        Draw::Text { text, .. } if text == "Ordinary title" || text == "Ordinary subtitle"))
    );

    runtime.hud.set_title(Arc::from("Back in round"), 5, 1200);
    runtime.hud.set_subtitle(Arc::from("Playing"), 6, 1200);
    for now in [1200, 1216, 1232] {
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
    let frame = presentation
        .build(
            &player,
            &runtime,
            1800,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    snapshot::write(&frame, "admitted-dynamic-title-restored");
    let nodes = presentation.hud_draw_nodes();
    assert_eq!(
        nodes
            .iter()
            .filter(|node| matches!(&node.draw,
        Draw::Text { text, .. } if text == "Back in round"))
            .count(),
        1
    );
    assert!(!nodes.iter().any(|node| matches!(&node.draw,
        Draw::Text { text, .. } if text == title || text == subtitle)));

    runtime.hud.set_title(Arc::from(title), 7, 1800);
    runtime.hud.set_subtitle(Arc::from(subtitle), 8, 1800);
    presentation
        .build(
            &player,
            &runtime,
            2300,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let nodes = presentation.hud_draw_nodes();
    assert_eq!(
        nodes
            .iter()
            .filter(|node| matches!(&node.draw,
        Draw::Text { text, scale, .. } if text == title && *scale <= 2.0))
            .count(),
        2
    );
    assert!(!nodes.iter().any(|node| matches!(&node.draw,
        Draw::Text { text, .. } if text == "Back in round" || text == "Playing")));

    runtime.hud.clear_titles();
    presentation
        .build(
            &player,
            &runtime,
            2400,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    assert!(
        !presentation
            .hud_draw_nodes()
            .iter()
            .any(|node| matches!(&node.draw,
        Draw::Text { text, .. } if text == title || text == subtitle))
    );
}
