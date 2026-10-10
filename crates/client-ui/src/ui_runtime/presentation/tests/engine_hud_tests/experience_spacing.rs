//! The level's painted glyphs stay above the bar across GUI and DPI scales.

use super::*;

/// Measures the physical gap below the level's painted glyphs, including its outline.
fn level_gap(
    input: &render_model::UiRenderInput,
    presentation: &UiPresentationRuntime,
    level: u32,
    scale: f32,
) -> f32 {
    let text = level.to_string();
    let nodes = presentation.hud_draw_nodes();
    let color = nodes
        .iter()
        .find_map(|node| match &node.draw {
            Draw::Text {
                text: value, color, ..
            } if *value == text && color[..3] != [0, 0, 0] => Some(*color),
            _ => None,
        })
        .expect("visible level label");
    let bar = named(nodes, "empty_progress_bar")[0];
    let green: Vec<_> = input
        .vertices
        .chunks_exact(4)
        .filter(|quad| quad.iter().all(|vertex| vertex.color == color))
        .collect();
    assert!(!green.is_empty(), "the level must paint visible glyphs");
    let bottom = input
        .vertices
        .chunks_exact(4)
        .filter(|quad| {
            green.iter().any(|glyph| {
                quad.iter().zip(glyph.iter()).all(|(a, b)| {
                    a.uv == b.uv
                        && (a.position[0] - b.position[0]).abs() <= scale + 0.01
                        && (a.position[1] - b.position[1]).abs() <= scale + 0.01
                        && (a.color == [0, 0, 0, 255] || a.color == b.color)
                })
            })
        })
        .map(|quad| quad_bounds(quad)[3])
        .fold(f32::NEG_INFINITY, f32::max);
    bar.dest.y as f32 * scale - bottom
}

#[test]
fn experience_level_glyphs_leave_a_gap_above_the_bar() {
    let mut player = player_state::PlayerState::new(1);
    let snapshots = std::env::var_os("CINNABAR_FORM_SNAPSHOT_DIR").is_some();
    let font = if snapshots {
        super::super::super::forms::pack_harness::font()
    } else {
        crate::test_support::fixture_font()
    };
    let Some(carrier) = super::super::super::forms::pack_harness::carrier() else {
        eprintln!(
            "skipping experience_level_glyphs_leave_a_gap_above_the_bar: missing UI carrier; make assets"
        );
        return;
    };
    let hud = if snapshots {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/assets/compiled")
            .join(assets::carriers::HUD.output);
        let Ok(bytes) = std::fs::read(&path) else {
            eprintln!(
                "skipping experience_level_glyphs_leave_a_gap_above_the_bar: missing HUD carrier {}",
                path.display()
            );
            return;
        };
        Arc::new(assets::RuntimeHudCatalog::decode(&bytes).unwrap())
    } else {
        crate::test_support::fixture_hud()
    };
    let mut presentation = UiPresentationRuntime::with_hud(font, hud).unwrap();
    presentation.enable_json_ui(carrier).unwrap();
    let mut runtime = UiRuntime::new(1);
    player
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    full_stats(&mut player, &mut runtime, 1);
    if snapshots {
        presentation.set_gui_scale_preference(Some(3));
        presentation.set_server_ui_pack(&super::super::super::ServerUiPack {
            ui_layers: vec![vec![(
                "ui/hud_screen.json".into(),
                br#"{
                "namespace": "hud", "java_level_number": {"offset": [0, -35]}
            }"#
                .to_vec(),
            )]],
            ..Default::default()
        });
        runtime.hud.set_experience(45, 0.5);
        let input = build_at(&player, &mut presentation, &runtime, 0, [1920, 1080], 1.0);
        super::super::super::forms::snapshot::write(&input, "xp-level-before");
        assert!(
            level_gap(&input, &presentation, 45, 3.0) < 0.0,
            "the crowded label must demonstrate the overlap"
        );
        presentation.set_server_ui_pack(&Default::default());
    }
    for scale in [1, 2, 3, 4] {
        presentation.set_gui_scale_preference(Some(scale));
        for dpi in [1.0, 1.25, 1.5, 2.0] {
            for level in [2, 45, 123] {
                runtime.hud.set_experience(level, 0.5);
                let input = build_at(&player, &mut presentation, &runtime, 0, [1920, 1080], dpi);
                let gap = level_gap(&input, &presentation, level, f32::from(scale));
                assert!(
                    gap + 0.01 >= f32::from(scale),
                    "level {level}, GUI scale {scale}, DPI {dpi}: outlined glyph gap {gap}px"
                );
                if level == 45 && dpi == 1.0 {
                    super::super::super::forms::snapshot::write(
                        &input,
                        &format!("xp-level-scale-{scale}"),
                    );
                }
            }
            runtime.hud.set_experience(0, 0.5);
            build_at(&player, &mut presentation, &runtime, 0, [1920, 1080], dpi);
            assert!(
                !presentation
                    .hud_draw_nodes()
                    .iter()
                    .any(|node| { matches!(&node.draw, Draw::Text { text, .. } if text == "0") }),
                "level zero must stay hidden"
            );
        }
    }
}
