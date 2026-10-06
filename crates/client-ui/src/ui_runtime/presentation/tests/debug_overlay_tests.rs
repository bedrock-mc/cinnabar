//! The F3 overlay adds strips and text only while lines are set.

use json_ui::{Draw, DrawNode, FormRender, LayoutEnv, TextMeasure, TextureMeta, TextureSource};
use ui::{DpiScale, UiVisual};

use super::super::{TextMetrics, debug_overlay};
use crate::test_support::mini_engine_presentation;
use crate::ui_runtime::UiRuntime;
use crate::ui_runtime::presentation::{DebugLines, UiPresentationRuntime};

fn vertex_count(
    player_runtime: &player_state::PlayerState,
    presentation: &mut UiPresentationRuntime,
    runtime: &UiRuntime,
) -> usize {
    presentation
        .build(
            player_runtime,
            runtime,
            0,
            [800, 600],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap()
        .vertices
        .len()
}

#[test]
fn debug_lines_add_geometry_and_clearing_them_restores_the_frame() {
    let player_runtime = player_state::PlayerState::new(1);

    let mut presentation = mini_engine_presentation();
    let runtime = UiRuntime::new(1);
    let bare = vertex_count(&player_runtime, &mut presentation, &runtime);

    presentation.set_debug_lines(Some(DebugLines {
        left: vec!["0 fps".to_owned(), "XYZ: 0 / 2 / 0".to_owned()],
        right: vec!["Targeted Block: 0, 0, 0".to_owned()],
    }));
    let shown = vertex_count(&player_runtime, &mut presentation, &runtime);
    assert!(shown > bare);

    presentation.set_debug_lines(None);
    assert_eq!(
        vertex_count(&player_runtime, &mut presentation, &runtime),
        bare
    );
}

#[test]
fn menus_and_other_screens_hide_debug_until_gameplay_resumes() {
    use crate::menu::{MenuScreen, MenuView};
    use crate::ui_runtime::presentation::LoadingStage;

    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime
        .publish_local_runtime_id(&mut player, 1, 42)
        .unwrap();
    runtime.publish_inventory_authority(&mut player, protocol::InventoryAuthority::Server);
    let mut presentation = mini_engine_presentation();
    let lines = DebugLines {
        left: vec!["FPS".into()],
        right: vec!["GPU".into()],
    };
    let bare = vertex_count(&player, &mut presentation, &runtime);
    presentation.set_debug_lines(Some(lines.clone()));
    let gameplay = vertex_count(&player, &mut presentation, &runtime);
    assert!(gameplay > bare);

    let assert_hidden = |presentation: &mut UiPresentationRuntime,
                         runtime: &UiRuntime,
                         player: &player_state::PlayerState| {
        presentation.set_debug_lines(None);
        let without_debug = vertex_count(player, presentation, runtime);
        presentation.set_debug_lines(Some(lines.clone()));
        assert_eq!(vertex_count(player, presentation, runtime), without_debug);
    };
    for screen in [MenuScreen::Pause, MenuScreen::Settings, MenuScreen::Home] {
        let mut view = MenuView::new(true, "Player".into());
        view.screen = screen;
        view.over_world = screen != MenuScreen::Home;
        presentation.set_menu_view(Some(view));
        assert_hidden(&mut presentation, &runtime, &player);
        presentation.set_menu_view(None);
        assert_eq!(vertex_count(&player, &mut presentation, &runtime), gameplay);
    }

    runtime.toggle_inventory(&mut player);
    assert!(runtime.inventory_open());
    assert_hidden(&mut presentation, &runtime, &player);
    runtime.close_inventory(&mut player);
    assert_eq!(vertex_count(&player, &mut presentation, &runtime), gameplay);

    runtime.open_chat(&mut player);
    assert_hidden(&mut presentation, &runtime, &player);
    runtime.close_chat();
    assert_eq!(vertex_count(&player, &mut presentation, &runtime), gameplay);

    presentation.set_loading_stage(Some(LoadingStage::Connecting));
    assert_hidden(&mut presentation, &runtime, &player);
    presentation.set_loading_stage(None);
    assert_eq!(vertex_count(&player, &mut presentation, &runtime), gameplay);

    assert_hidden(&mut presentation, &UiRuntime::new(0), &player);
}

#[test]
fn changing_values_row_counts_and_target_properties_keep_the_same_glyph_geometry() {
    let mut presentation = mini_engine_presentation();
    let mut baseline = None;
    for (rows, value, properties) in [
        (1, "1".to_owned(), 0),
        (25, "123456789".to_owned(), 0),
        (40, "W".repeat(500), 10),
        (12, "1".to_owned(), 2),
        (1, "1".to_owned(), 0),
    ] {
        let mut left = vec!["FPS".to_owned()];
        left.extend((1..rows).map(|row| format!("counter {row}: {value}")));
        let mut right = vec![format!("GPU: {value}")];
        right.extend((0..properties).map(|index| format!("property_{index}: {value}")));
        presentation.set_debug_lines(Some(DebugLines { left, right }));
        let mut nodes = Vec::new();
        presentation
            .append_debug_overlay(
                &mut nodes,
                &mut 1,
                TextMetrics::for_viewport([1280, 720], DpiScale::new(1.0).unwrap(), Some(3)),
                [1280.0, 720.0],
            )
            .unwrap();
        let marker = nodes
            .iter()
            .find_map(|node| match node.visual() {
                UiVisual::Text { layout, .. } => Some((node.bounds(), layout.glyphs().to_vec())),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            marker
                .1
                .iter()
                .map(|glyph| glyph.codepoint)
                .collect::<String>(),
            "FPS"
        );
        if let Some(expected) = &baseline {
            assert_eq!(
                &marker, expected,
                "content must not change the F3 font size"
            );
        } else {
            baseline = Some(marker);
        }
    }
}

struct DiagnosticFont;

const LINE_HEIGHT: f64 =
    ui::TEXT_LINE_HEIGHT_64 as f64 / 64.0 / ui::FONT_DESIGN_PIXEL_TEXELS as f64;

impl TextMeasure for DiagnosticFont {
    fn extent(&self, text: &str) -> [f64; 2] {
        [text.chars().count() as f64 * 6.0, LINE_HEIGHT]
    }
}

struct NoTextures;

impl TextureSource for NoTextures {
    fn texture(&self, _: &str) -> Option<TextureMeta> {
        None
    }
}

const ENV: LayoutEnv<'static> = LayoutEnv {
    text: &DiagnosticFont,
    textures: &NoTextures,
};

fn render(lines: DebugLines, viewport: [f64; 2]) -> FormRender {
    let mut cache = debug_overlay::OverlayCache::default();
    debug_overlay::render(&mut cache, &lines, (viewport, 1.0), &ENV);
    cache.into_render().expect("rendered once")
}

/// Lines changing frame to frame rebind only the changed rows, yet draw exactly what a
/// fresh bind would.
#[test]
fn cached_overlay_matches_a_fresh_render_as_lines_change() {
    let frames = [
        (["FPS 360", "XYZ 1 2 3"], ["GPU", "Display"]),
        (["FPS 359", "XYZ 1 2 3"], ["GPU", "Display"]),
        (["FPS 359", "XYZ 1 2 3"], ["GPU", "Display"]),
        (["FPS 160 while streaming", ""], ["GPU", "Display 2"]),
    ];
    let mut cache = debug_overlay::OverlayCache::default();
    for (index, (left, right)) in frames.into_iter().enumerate() {
        let lines = DebugLines {
            left: left.map(String::from).to_vec(),
            right: right.map(String::from).to_vec(),
        };
        let passes = cache.passes;
        let nodes = debug_overlay::render(&mut cache, &lines, ([640.0, 360.0], 1.0), &ENV)
            .nodes
            .clone();
        assert_eq!(nodes, render(lines, [640.0, 360.0]).nodes);
        let rebound = cache.passes != passes;
        assert_eq!(
            rebound,
            index != 2,
            "only changed lines rebind, frame {index}"
        );
    }
}

/// A font swap at the same size and lines re-measures instead of reusing the old glyph widths.
#[test]
fn swapping_the_font_relays_the_overlay() {
    let lines = DebugLines {
        left: vec!["FPS 360".to_owned()],
        right: vec!["GPU".to_owned()],
    };
    let mut cache = debug_overlay::OverlayCache::default();
    let font = crate::test_support::fixture_font();
    for (font, relaid) in [
        (&font, true),
        (&font, false),
        (&crate::test_support::fixture_font(), true),
    ] {
        cache.retain_font(font);
        let passes = cache.passes;
        debug_overlay::render(&mut cache, &lines, ([640.0, 360.0], 1.0), &ENV);
        assert_eq!(cache.passes != passes, relaid);
    }
}

fn text<'a>(nodes: &'a [DrawNode], wanted: &str) -> &'a DrawNode {
    nodes
        .iter()
        .find(|node| matches!(&node.draw, Draw::Text { text, .. } if text == wanted))
        .unwrap()
}

#[test]
fn json_ui_rows_measure_their_strips_and_keep_blank_spacers() {
    let frame = render(
        DebugLines {
            left: vec!["FPS".into(), String::new(), "XYZ".into()],
            right: vec!["GPU".into(), "Display".into()],
        },
        [640.0, 360.0],
    );
    assert!(frame.hits.is_empty(), "diagnostics do not capture input");
    let strips: Vec<_> = frame
        .nodes
        .iter()
        .filter(|node| matches!(node.draw, Draw::Solid { .. }))
        .collect();
    assert_eq!(strips.len(), 4, "blank rows have no background");
    assert_eq!(strips[0].dest.x, 2.0);
    assert_eq!(text(&frame.nodes, "FPS").dest.y, strips[0].dest.y + 1.0);
    assert_eq!(text(&frame.nodes, "GPU").dest.y, strips[2].dest.y + 1.0);
    assert_eq!(
        strips[0].dest.w, 20.0,
        "width follows the measured text plus padding"
    );
    assert!(strips.iter().all(|node| matches!(
        node.draw,
        Draw::Solid {
            color: [80, 80, 80, 144]
        }
    )));
    assert_eq!(
        text(&frame.nodes, "XYZ").dest.y - text(&frame.nodes, "FPS").dest.y,
        LINE_HEIGHT * 2.0
    );
    let right = text(&frame.nodes, "GPU");
    assert_eq!(right.dest.x + right.dest.w, 637.0);
    assert!(matches!(
        &right.draw,
        Draw::Text {
            align: json_ui::TextAlign::Right,
            color,
            shadow: false,
            localize: false,
            ..
        } if *color == [255; 4]
    ));
}

#[test]
fn crowded_columns_keep_their_gap_and_omit_rows_past_the_viewport() {
    let frame = render(
        DebugLines {
            left: (0..50)
                .map(|index| format!("left {index}: {}", "W".repeat(100)))
                .collect(),
            right: (0..50)
                .map(|index| format!("right {index}: {}", "W".repeat(100)))
                .collect(),
        },
        [320.0, 100.0],
    );
    let strips: Vec<_> = frame
        .nodes
        .iter()
        .filter(|node| matches!(node.draw, Draw::Solid { .. }))
        .collect();
    assert_eq!(strips.len(), 20, "ten fully visible rows per column");
    let left = strips[0].dest;
    let right = strips[10].dest;
    assert!(right.x - (left.x + left.w) >= 8.0 - 1e-3);
    assert!(strips.iter().all(|node| {
        node.dest.x >= 0.0
            && node.dest.x + node.dest.w <= 320.0 + 1e-3
            && node.dest.y >= 0.0
            && node.dest.y + node.dest.h <= 100.0
    }));
    assert!(frame.hits.is_empty());
}

#[test]
fn actual_engine_truncates_long_diagnostics_to_one_line() {
    let mut presentation = mini_engine_presentation();
    presentation.set_debug_lines(Some(DebugLines {
        left: vec!["W".repeat(200)],
        right: vec!["W".repeat(200)],
    }));
    let mut nodes = Vec::new();
    presentation
        .append_debug_overlay(
            &mut nodes,
            &mut 1,
            TextMetrics::for_viewport([800, 600], DpiScale::new(1.0).unwrap(), None),
            [320.0, 100.0],
        )
        .unwrap();
    let layouts: Vec<_> = nodes
        .iter()
        .filter_map(|node| match node.visual() {
            UiVisual::Text { layout, .. } => Some(layout),
            _ => None,
        })
        .collect();
    assert_eq!(layouts.len(), 2);
    assert!(
        layouts
            .iter()
            .all(|layout| layout.line_count() == 1 && layout.ellipsized())
    );
}

#[test]
fn overlay_scaling_fits_dense_columns_at_desktop_dpi_and_gui_scales() {
    let lines = DebugLines {
        left: vec!["FPS".into(); 40],
        right: vec!["GPU".into(); 40],
    };
    for (size, dpi, preference) in [
        ([800, 600], 1.0, None),
        ([1920, 1080], 2.0, Some(4)),
        ([1366, 768], 1.0, Some(3)),
    ] {
        let metrics = TextMetrics::for_viewport(size, DpiScale::new(dpi).unwrap(), preference);
        let height = size[1] as f32 / dpi;
        let fitted = debug_overlay::fitted_metrics(metrics, height);
        assert!(fitted.scale.get() <= metrics.scale.get());
        assert!(fitted.scale.get() >= metrics.scale.get() / 2.0);
        let px = fitted.scale.get() * ui::FONT_DESIGN_PIXEL_TEXELS as f32;
        let root = [size[0] as f64 / f64::from(dpi * px), f64::from(height / px)];
        let frame = render(lines.clone(), root);
        assert_eq!(
            frame.nodes.len(),
            160,
            "all forty rows fit at {size:?}, DPI {dpi}"
        );
        assert!(
            frame
                .nodes
                .iter()
                .all(|node| node.dest.y + node.dest.h <= root[1] + 1e-3)
        );
    }
}

#[test]
fn coordinates_and_facing_remain_complete_when_hardware_names_are_long() {
    let mut presentation = mini_engine_presentation();
    presentation.set_debug_lines(Some(DebugLines {
        left: vec![
            "XYZ: -12345.123 / 65.12345 / -12345.123".into(),
            "Facing: southwest (-X, +Z) (135.0 / -35.0)".into(),
        ],
        right: vec![format!("GPU: {}", "W".repeat(500))],
    }));
    let mut nodes = Vec::new();
    presentation
        .append_debug_overlay(
            &mut nodes,
            &mut 1,
            TextMetrics::for_viewport([1366, 768], DpiScale::new(1.0).unwrap(), Some(3)),
            [1366.0, 768.0],
        )
        .unwrap();
    let layouts: Vec<_> = nodes
        .iter()
        .filter_map(|node| match node.visual() {
            UiVisual::Text { layout, .. } => Some(layout),
            _ => None,
        })
        .collect();
    assert_eq!(layouts.len(), 3);
    assert!(
        !layouts[0].ellipsized(),
        "all position coordinates remain readable"
    );
    assert!(
        !layouts[1].ellipsized(),
        "heading and both angles remain readable"
    );
    assert!(
        layouts[2].ellipsized(),
        "extreme device names give way to position data"
    );
}
