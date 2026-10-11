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
    use crate::ui_runtime::presentation::LoadingStage;
    use launcher::menu::{MenuScreen, MenuView};

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
        let fitted = debug_overlay::visible::fitted_metrics(metrics, height);
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

#[test]
fn unchanged_overlay_paint_allocates_nothing_and_reuses_glyph_runs() {
    let mut presentation = mini_engine_presentation();
    presentation.set_debug_lines(Some(DebugLines {
        left: vec!["FPS 360".into(), "XYZ 1 2 3".into()],
        right: vec!["GPU".into()],
    }));
    let metrics = TextMetrics::for_viewport([800, 600], DpiScale::new(1.0).unwrap(), None);
    let mut nodes = Vec::with_capacity(64);
    presentation
        .append_debug_overlay(&mut nodes, &mut 1, metrics, [800.0, 600.0])
        .unwrap();
    let expected = nodes.clone();
    let passes = presentation.debug_overlay.passes;
    nodes.clear();
    let (result, allocations) = crate::allocation_count::count(|| {
        presentation.append_debug_overlay(&mut nodes, &mut 1, metrics, [800.0, 600.0])
    });
    result.unwrap();
    assert_eq!(
        allocations, 0,
        "unchanged diagnostics retain their painted nodes"
    );
    assert_eq!(presentation.debug_overlay.passes, passes);
    assert_eq!(nodes, expected);
    for (current, kept) in nodes.iter().zip(&expected) {
        if let (
            UiVisual::Text {
                layout: current, ..
            },
            UiVisual::Text { layout: kept, .. },
        ) = (current.visual(), kept.visual())
        {
            assert!(std::sync::Arc::ptr_eq(current, kept));
        }
    }
}

#[test]
fn changing_one_overlay_line_only_measures_its_text() {
    use std::cell::RefCell;

    struct Measured(RefCell<Vec<String>>);
    impl TextMeasure for Measured {
        fn extent(&self, text: &str) -> [f64; 2] {
            self.0.borrow_mut().push(text.to_owned());
            DiagnosticFont.extent(text)
        }
    }
    let measured = Measured(RefCell::default());
    let env = LayoutEnv {
        text: &measured,
        textures: &NoTextures,
    };
    let mut lines = DebugLines {
        left: vec!["FPS 360".into(), "XYZ 1 2 3".into()],
        right: vec!["GPU".into()],
    };
    let mut cache = debug_overlay::OverlayCache::default();
    debug_overlay::render(&mut cache, &lines, ([640.0, 360.0], 1.0), &env);
    measured.0.borrow_mut().clear();
    lines.left[0] = "FPS 359".into();
    debug_overlay::render(&mut cache, &lines, ([640.0, 360.0], 1.0), &env);
    let measured = measured.0.borrow();
    assert!(measured.iter().any(|line| line == "FPS 359"));
    assert!(
        measured.iter().all(|line| line == "FPS 359"),
        "measured: {measured:?}"
    );
}

#[test]
fn unchanged_overlay_frame_reuses_geometry_and_publication() {
    let player = player_state::PlayerState::new(1);
    let runtime = UiRuntime::new(1);
    let mut presentation = mini_engine_presentation();
    presentation.set_debug_lines(Some(DebugLines {
        left: vec!["FPS 360".into(), "XYZ 1 2 3".into()],
        right: vec!["GPU".into()],
    }));
    let first = presentation
        .build(
            &player,
            &runtime,
            0,
            [800, 600],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let tree_builds = presentation.tree_builds;
    let second = presentation
        .build(
            &player,
            &runtime,
            1,
            [800, 600],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    assert_eq!(presentation.tree_builds, tree_builds);
    assert_eq!(first.revision, second.revision);
    assert!(std::sync::Arc::ptr_eq(&first.vertices, &second.vertices));
    assert!(std::sync::Arc::ptr_eq(&first.indices, &second.indices));
    assert!(std::sync::Arc::ptr_eq(&first.textures, &second.textures));
}
#[test]
fn overlay_paint_refreshes_when_its_presentation_inputs_change() {
    for change in [
        "font", "viewport", "scale", "grid", "dpi", "solid", "safe", "engine",
    ] {
        let mut presentation = mini_engine_presentation();
        presentation.set_debug_lines(Some(DebugLines {
            left: vec!["FPS 360".into()],
            right: vec!["GPU".into()],
        }));
        let mut viewport = [800, 600];
        let mut content = [800.0, 600.0];
        let mut dpi = 1.0;
        let mut preference = None;
        let mut nodes = Vec::with_capacity(64);
        presentation
            .append_debug_overlay(
                &mut nodes,
                &mut 1,
                TextMetrics::for_viewport(viewport, DpiScale::new(dpi).unwrap(), preference),
                content,
            )
            .unwrap();
        match change {
            "font" => presentation.font = crate::test_support::fixture_font(),
            "viewport" => {
                viewport[0] += 100;
                content[0] += 100.0;
            }
            "scale" => preference = Some(1),
            "grid" => {}
            "dpi" => dpi = 2.0,
            "solid" => presentation.solid_texture_page += 1,
            "safe" => presentation.set_safe_area(ui::SafeArea::new(4.0, 2.0, 0.0, 0.0).unwrap()),
            "engine" => presentation
                .enable_json_ui(crate::test_support::mini_carrier())
                .unwrap(),
            _ => unreachable!(),
        }
        presentation.debug_overlay.retain_font(&presentation.font);
        let paints = presentation.debug_overlay.paints;
        let mut metrics =
            TextMetrics::for_viewport(viewport, DpiScale::new(dpi).unwrap(), preference);
        if change == "grid" {
            metrics.gui_scale += 0.5;
        }
        let previous_nodes = nodes.clone();
        nodes.clear();
        presentation
            .append_debug_overlay(&mut nodes, &mut 1, metrics, content)
            .unwrap();
        assert_eq!(presentation.debug_overlay.paints, paints + 1, "{change}");
        if change == "grid" {
            assert_ne!(nodes, previous_nodes);
        }
        nodes.clear();
        presentation
            .append_debug_overlay(&mut nodes, &mut 1, metrics, content)
            .unwrap();
        assert_eq!(
            presentation.debug_overlay.paints,
            paints + 1,
            "unchanged {change}"
        );
    }
}

#[test]
fn publishing_equal_overlay_lines_keeps_both_buffer_sets() {
    let mut presentation = mini_engine_presentation();
    let mut staged = Some(DebugLines {
        left: vec!["FPS 360".into()],
        right: Vec::new(),
    });
    let storage = staged.as_ref().unwrap().left[0].as_ptr();
    assert!(presentation.swap_debug_lines(&mut staged));
    assert!(staged.is_none());
    assert_eq!(
        presentation.debug_lines.as_ref().unwrap().left[0].as_ptr(),
        storage
    );
    staged = Some(DebugLines {
        left: vec!["FPS 360".into()],
        right: Vec::new(),
    });
    let staged_storage = staged.as_ref().unwrap().left[0].as_ptr();
    let (_, allocations) = crate::allocation_count::count(|| {
        assert!(!presentation.swap_debug_lines(&mut staged));
    });
    assert_eq!(allocations, 0);
    assert_eq!(staged.as_ref().unwrap().left[0].as_ptr(), staged_storage);
    assert_eq!(
        presentation.debug_lines.as_ref().unwrap().left[0].as_ptr(),
        storage
    );
    staged.as_mut().unwrap().left[0].clear();
    staged.as_mut().unwrap().left[0].push_str("FPS 359");
    assert!(presentation.swap_debug_lines(&mut staged));
    assert_eq!(staged.as_ref().unwrap().left[0].as_ptr(), storage);
    assert_eq!(
        presentation.debug_lines.as_ref().unwrap().left[0],
        "FPS 359"
    );
}
#[test]
fn changes_outside_displayed_rows_keep_paint_and_glyphs() {
    let mut presentation = mini_engine_presentation();
    let mut lines = DebugLines {
        left: (0..50).map(|index| format!("row {index}")).collect(),
        right: Vec::new(),
    };
    presentation.set_debug_lines(Some(lines.clone()));
    let metrics = TextMetrics::for_viewport([800, 600], DpiScale::new(1.0).unwrap(), None);
    let mut nodes = Vec::with_capacity(200);
    presentation
        .append_debug_overlay(&mut nodes, &mut 1, metrics, [800.0, 600.0])
        .unwrap();
    let paints = presentation.debug_overlay.paints;
    let passes = presentation.debug_overlay.passes;
    lines.left[49] = "changed hidden row".into();
    presentation.set_debug_lines(Some(lines));
    nodes.clear();
    let (_, allocations) = crate::allocation_count::count(|| {
        presentation
            .append_debug_overlay(&mut nodes, &mut 1, metrics, [800.0, 600.0])
            .unwrap();
    });
    assert_eq!(allocations, 0);
    assert_eq!(presentation.debug_overlay.paints, paints);
    assert_eq!(presentation.debug_overlay.passes, passes);
}

#[test]
fn diagnostic_eligibility_matches_the_rendered_stack_without_allocations() {
    use crate::ui_runtime::presentation::LoadingStage;
    use crate::ui_runtime::scene_stack::Scene;
    use launcher::menu::{MenuScreen, MenuView};
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime
        .publish_local_runtime_id(&mut player, 1, 42)
        .unwrap();
    runtime.publish_inventory_authority(&mut player, protocol::InventoryAuthority::Server);
    let mut presentation = mini_engine_presentation();
    for cover in [
        "game",
        "pause",
        "loading",
        "chat",
        "inventory",
        "disconnected",
    ] {
        match cover {
            "pause" => {
                let mut menu = MenuView::new(true, "Player".into());
                menu.screen = MenuScreen::Pause;
                menu.over_world = true;
                presentation.set_menu_view(Some(menu));
            }
            "loading" => presentation.set_loading_stage(Some(LoadingStage::Connecting)),
            "chat" => {
                runtime.open_chat(&mut player);
            }
            "inventory" => {
                runtime.toggle_inventory(&mut player);
            }
            "disconnected" => runtime = UiRuntime::new(0),
            _ => (),
        }
        let stack = runtime.scenes_in(
            &player,
            presentation.scene_host(),
            &presentation.screen_settings(),
        );
        let expected = stack.visible(false).contains(&Scene::Gameplay)
            && stack
                .scenes()
                .iter()
                .all(|entry| matches!(entry.key, Scene::Gameplay | Scene::Crosshair | Scene::Hud));
        let (allowed, allocations) = crate::allocation_count::count(|| {
            presentation.debug_overlay_allowed(&player, &runtime)
        });
        assert_eq!(allowed, expected, "{cover}");
        assert_eq!(allowed, cover == "game", "{cover}");
        assert_eq!(allocations, 0, "{cover}");
        presentation.set_menu_view(None);
        presentation.set_loading_stage(None);
        runtime.close_chat();
        runtime.close_inventory(&mut player);
    }
}

#[test]
fn retained_overlay_reparents_when_other_hud_nodes_change() {
    let mut presentation = mini_engine_presentation();
    presentation.set_debug_lines(Some(DebugLines {
        left: vec!["FPS 360".into()],
        right: Vec::new(),
    }));
    let metrics = TextMetrics::for_viewport([800, 600], DpiScale::new(1.0).unwrap(), None);
    let mut nodes = Vec::with_capacity(64);
    let mut next = 7;
    presentation
        .append_debug_overlay(&mut nodes, &mut next, metrics, [800.0, 600.0])
        .unwrap();
    let identities: Vec<_> = nodes
        .iter()
        .map(|node| (node.id().get(), node.parent().map(|parent| parent.get())))
        .collect();
    let end = next;
    let paints = presentation.debug_overlay.paints;
    nodes.clear();
    next = 17;
    presentation
        .append_debug_overlay(&mut nodes, &mut next, metrics, [800.0, 600.0])
        .unwrap();
    assert_eq!(next, end + 10);
    assert_eq!(presentation.debug_overlay.paints, paints);
    for (node, (id, parent)) in nodes.iter().zip(identities) {
        assert_eq!(node.id().get(), id + 10);
        assert_eq!(
            node.parent().map(|parent| parent.get()),
            parent.map(|parent| parent + 10)
        );
    }
}

#[test]
fn pack_hud_visibility_controls_diagnostic_eligibility_without_allocations() {
    use crate::ui_runtime::presentation::ServerUiPack;
    use crate::ui_runtime::scene_stack::Scene;

    let player = player_state::PlayerState::new(1);
    let runtime = UiRuntime::new(1);
    let mut presentation = mini_engine_presentation();
    let base = presentation.pack_catalog_base().unwrap();
    let (namespace, name) = json_ui::HUD_SCREEN.split_once('.').unwrap();
    for render_game_behind in [false, true] {
        let definition = serde_json::to_vec(&serde_json::json!({
            "namespace": namespace,
            (name): {
                "type": "screen",
                "render_game_behind": render_game_behind,
                "render_only_when_topmost": false
            }
        }))
        .unwrap();
        let pack = ServerUiPack {
            ui_layers: vec![vec![
                (
                    "ui/_ui_defs.json".into(),
                    br#"{"ui_defs":["ui/f3_policy.json"]}"#.to_vec(),
                ),
                ("ui/f3_policy.json".into(), definition),
            ]],
            ..Default::default()
        }
        .prepare_catalog(&base);
        presentation.set_server_ui_pack(&pack);
        let stack = runtime.scenes_in(
            &player,
            presentation.scene_host(),
            &presentation.screen_settings(),
        );
        assert!(stack.contains(Scene::Hud));
        assert_eq!(
            stack.visible(false).contains(&Scene::Gameplay),
            render_game_behind
        );
        let (allowed, allocations) = crate::allocation_count::count(|| {
            presentation.debug_overlay_allowed(&player, &runtime)
        });
        assert_eq!(allowed, render_game_behind);
        assert_eq!(allocations, 0);
    }
}

#[test]
fn changing_one_line_retains_other_painted_glyph_runs() {
    let mut presentation = mini_engine_presentation();
    let mut lines = DebugLines {
        left: vec!["FPS 360".into(), "XYZ 1 2 3".into()],
        right: vec!["GPU".into()],
    };
    presentation.set_debug_lines(Some(lines.clone()));
    let metrics = TextMetrics::for_viewport([800, 600], DpiScale::new(1.0).unwrap(), None);
    let mut nodes = Vec::with_capacity(64);
    presentation
        .append_debug_overlay(&mut nodes, &mut 1, metrics, [800.0, 600.0])
        .unwrap();
    let before: Vec<_> = nodes
        .iter()
        .filter_map(|node| match node.visual() {
            UiVisual::Text { layout, .. } => Some(std::sync::Arc::clone(layout)),
            _ => None,
        })
        .collect();
    lines.left[0] = "FPS 359".into();
    presentation.set_debug_lines(Some(lines));
    nodes.clear();
    presentation
        .append_debug_overlay(&mut nodes, &mut 1, metrics, [800.0, 600.0])
        .unwrap();
    let after: Vec<_> = nodes
        .iter()
        .filter_map(|node| match node.visual() {
            UiVisual::Text { layout, .. } => Some(layout),
            _ => None,
        })
        .collect();
    assert_eq!(before.len(), 3);
    assert_eq!(after.len(), 3);
    assert!(!std::sync::Arc::ptr_eq(&before[0], after[0]));
    assert!(std::sync::Arc::ptr_eq(&before[1], after[1]));
    assert!(std::sync::Arc::ptr_eq(&before[2], after[2]));
}

#[test]
fn unchanged_overlay_frames_reuse_scene_assembly_storage() {
    let player = player_state::PlayerState::new(1);
    let runtime = UiRuntime::new(1);
    let mut presentation = mini_engine_presentation();
    let mut lines = DebugLines {
        left: vec!["FPS 360".into(), "XYZ 1 2 3".into()],
        right: vec!["GPU".into()],
    };
    presentation.set_debug_lines(Some(lines.clone()));
    let first = presentation
        .build(
            &player,
            &runtime,
            0,
            [800, 600],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let capacity = presentation.assembly_nodes.capacity();
    let storage = presentation.assembly_nodes.as_ptr();
    let retained_storage = presentation.last_frame.as_ref().unwrap().nodes.as_ptr();
    let tree_builds = presentation.tree_builds;
    assert!(!presentation.assembly_nodes.is_empty());
    let second = presentation
        .build(
            &player,
            &runtime,
            1,
            [800, 600],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    assert_eq!(presentation.assembly_nodes.capacity(), capacity);
    assert_eq!(presentation.assembly_nodes.as_ptr(), storage);
    assert_eq!(
        presentation.last_frame.as_ref().unwrap().nodes.as_ptr(),
        retained_storage
    );
    assert_eq!(presentation.tree_builds, tree_builds);
    assert_eq!(first.revision, second.revision);
    assert!(std::sync::Arc::ptr_eq(&first.vertices, &second.vertices));
    assert!(std::sync::Arc::ptr_eq(&first.indices, &second.indices));
    lines.left[0] = "FPS 359".into();
    presentation.set_debug_lines(Some(lines));
    presentation
        .build(
            &player,
            &runtime,
            2,
            [800, 600],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    assert_eq!(presentation.assembly_nodes.capacity(), capacity);
    assert_eq!(presentation.assembly_nodes.as_ptr(), storage);
    assert_eq!(
        presentation.last_frame.as_ref().unwrap().nodes.as_ptr(),
        retained_storage
    );
}

#[test]
fn obfuscated_overlay_text_keeps_its_style_and_resamples_frame_geometry() {
    let player = player_state::PlayerState::new(1);
    let runtime = UiRuntime::new(1);
    let mut presentation = mini_engine_presentation();
    presentation.set_debug_lines(Some(DebugLines {
        left: vec!["Name: §kSecret".into()],
        right: Vec::new(),
    }));
    presentation
        .build(
            &player,
            &runtime,
            0,
            [800, 600],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let tree_builds = presentation.tree_builds;
    let paints = presentation.debug_overlay.paints;
    assert!(presentation.last_frame.is_none());
    assert!(presentation.assembly_nodes.iter().any(|node| {
        matches!(node.visual(), UiVisual::Text { layout, .. }
            if layout.glyphs().iter().any(|glyph| glyph.style.obfuscated))
    }));
    presentation
        .build(
            &player,
            &runtime,
            1,
            [800, 600],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    assert_eq!(presentation.tree_builds, tree_builds + 1);
    assert_eq!(presentation.debug_overlay.paints, paints);
    assert!(presentation.last_frame.is_none());
    assert!(presentation.assembly_nodes.iter().any(|node| {
        matches!(node.visual(), UiVisual::Text { layout, .. }
            if layout.glyphs().iter().any(|glyph| glyph.style.obfuscated))
    }));
}
