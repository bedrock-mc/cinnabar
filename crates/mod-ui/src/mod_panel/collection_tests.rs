//! The bounded collection follows real native binding, layout and hit-region creation.

use json_ui::{Context, Draw, LayoutEnv, TextMeasure, TextureMeta, TextureSource, ViewState};
use serde_json::json;
use std::{collections::BTreeSet, sync::Arc};
use ui::mod_panel::{Panel, PanelPatch};
use ui_engine::cached_screen::CachedScreen;

struct Metrics;

/// Native hover values reach an unchanged collection and remain eased between pointer events.
#[test]
fn collection_hover_tracks_live_native_feedback_without_republishing_items() {
    let mut value = serde_json::to_value(catalogue()).unwrap();
    value["motion"] = json!(true);
    let mut document: serde_json::Value = serde_json::from_str(
        value["surface"]["collections"]["modules"]["definition"]["template"]["document"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    document["root"]["controls"][0]["label"]["bindings"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "binding_name":"#item_open_hover", "binding_name_override":"#alpha"
        }));
    value["surface"]["collections"]["modules"]["definition"]["template"]["document"] =
        json!(document.to_string());
    let panel: Panel = serde_json::from_value(value).unwrap();
    let surface = panel.surface.as_ref().unwrap();
    let catalog = Arc::new(super::surface::catalog(surface).unwrap());
    let mut screen = CachedScreen::default();
    let mut data = Arc::new(super::data::control_data(&panel));
    let mut motion = super::motion::PanelMotion::default();
    let mut view = ViewState::default();
    let env = LayoutEnv {
        text: &Metrics,
        textures: &Metrics,
    };
    for (now, hovered, expected) in [
        (0.0, None, 0.0),
        (0.01, Some(0), 0.0),
        (0.06, Some(0), 0.875),
        (0.12, Some(0), 1.0),
        (0.13, None, 1.0),
        (0.23, None, 0.0),
    ] {
        motion.advance(&panel, &mut view, hovered, now, true);
        motion.write(&panel, Arc::make_mut(&mut data));
        let render = screen
            .render_shared_with(
                &surface.screen,
                &catalog,
                &Context::default(),
                Arc::clone(&data),
                ([308., 120.], 1., [0; 3]),
                &env,
                &view,
            )
            .unwrap();
        let alpha = render
            .nodes
            .iter()
            .find_map(|node| match &node.draw {
                Draw::Text { text, .. } if text == "Module 0" => Some(node.alpha),
                _ => None,
            })
            .unwrap_or(0.0);
        assert!((alpha - expected).abs() < 0.00001, "at {now}: {alpha}");
    }
}

impl TextMeasure for Metrics {
    /// Measures predictable plain labels without requiring a local font carrier.
    fn extent(&self, text: &str) -> [f64; 2] {
        [text.len() as f64 * 4.0, 8.0]
    }
}
impl TextureSource for Metrics {
    /// The fixture has no texture dependency.
    fn texture(&self, _: &str) -> Option<TextureMeta> {
        None
    }
}

/// Makes an independently actionable 99-item catalogue behind one small tile template.
fn catalogue() -> Panel {
    let template = json!({"screen":"tile.root", "bindings":{}, "document":json!({
        "namespace":"tile","root":{"type":"button","size":[100,40],
            "button_mappings":[{"from_button_id":"button.menu_select","mapping_type":"pressed","to_button_id":"mod.item:open"}],
            "controls":[{"label":{"type":"label","text":"#item_name","size":[95,20],
                "bindings":[{"binding_name":"#item_name"}]}}]}}).to_string()});
    serde_json::from_value(json!({"title":"Collection","toggle_key":"ShiftRight","dark":true,
        "controls":(0..99).map(|i|json!({"kind":"button","id":format!("module_{i}"),"label":format!("Module {i}")})).collect::<Vec<_>>(),
        "surface":{"screen":"list.root","document":json!({"namespace":"list","root":{
            "type":"panel","size":[308,120],"controls":[{"tiles":{"type":"panel","cinnabar_collection":"modules"}}]}}).to_string(),
        "collections":{"modules":{"definition":{"template":template,"columns":3,"viewport":[308,120],
            "items":(0..99).map(|i|json!({"values":{"#item_name":format!("Module {i}")},"controls":{"open":i}})).collect::<Vec<_>>()},
            "order":(0..99).collect::<Vec<_>>()}}}})).unwrap()
}

/// Makes four columns of independently bound artwork behind the native viewport.
fn four_column_catalogue() -> Panel {
    let mut panel = catalogue();
    let surface = panel.surface.as_mut().unwrap();
    surface.document = json!({"namespace":"list","root":{"type":"panel","size":[452,289],
        "controls":[{"tiles":{"type":"panel","cinnabar_collection":"modules"}}]}})
    .to_string();
    let collection = surface.collections.get_mut("modules").unwrap();
    collection.order = (0..66).collect();
    let definition = Arc::make_mut(&mut collection.definition);
    definition.columns = 4;
    definition.viewport = [452., 289.];
    definition.items.truncate(66);
    definition.template.document = json!({"namespace":"tile","root":{
        "type":"button","size":[111,97],
        "button_mappings":[{"from_button_id":"button.menu_select","mapping_type":"pressed","to_button_id":"mod.item:open"}],
        "controls":[{"label":{"type":"label","text":"#item_name","size":[105,20],
            "bindings":[{"binding_name":"#item_name"}]}},
            {"art":{"type":"custom","renderer":json_ui::vector_path::RENDERER,"size":[105,60],
                "paths":[[[0,0],[1,1]]],"view_box":[0,0,1,1],"stroke_width":1,"closed":true,
                "bindings":[
                    {"binding_name":"(#item_kind = 'armor')","binding_name_override":"#visible"},
                    {"binding_name":"#item_art_paths","binding_name_override":"#paths"},
                    {"binding_name":"#item_art_view_box","binding_name_override":"#view_box"},
                    {"binding_name":"#item_art_stroke_width","binding_name_override":"#stroke_width"},
                    {"binding_name":"#item_art_closed","binding_name_override":"#closed"}]}}]}})
        .to_string();
    for (index, item) in definition.items.iter_mut().enumerate() {
        item.values.insert(
            "#item_kind".into(),
            ui::mod_panel::SurfaceValue::Text(
                if index % 2 == 0 { "armor" } else { "other" }.into(),
            ),
        );
        item.art = Some(ui::mod_panel::CollectionArt {
            view_box: [0., 0., 105., 60.],
            paths: vec![vec![[index as f32, 8.], [72., 28.], [52., 48.]]],
            stroke_width: 2.,
            closed: false,
        });
    }
    panel
}

/// Scrolling and filtering preserve per-item visibility, artwork and cached steady frames.
#[test]
fn collection_four_columns_render_item_art_and_reuse_steady_frames() {
    let panel = four_column_catalogue();
    panel.validate().unwrap();
    let surface = panel.surface.as_ref().unwrap();
    let catalog = Arc::new(super::surface::catalog(surface).unwrap());
    let context = Context::default();
    let mut screen = CachedScreen::default();
    screen.virtualize_clipped_grid("modules", ui::mod_panel::SurfaceCollection::INITIAL_WINDOW);
    let env = LayoutEnv {
        text: &Metrics,
        textures: &Metrics,
    };
    let mut view = ViewState::default();
    let mut data = Arc::new(super::data::control_data(&panel));
    let mut seen = BTreeSet::new();
    for row in 0..17 {
        let render = screen
            .render_shared_with(
                &surface.screen,
                &catalog,
                &context,
                Arc::clone(&data),
                ([452., 289.], 1., [0; 3]),
                &env,
                &view,
            )
            .unwrap();
        let grid = render
            .bound
            .find(&|c| c.control_type.as_deref() == Some("grid"))
            .unwrap();
        assert!(
            grid.children
                .iter()
                .filter(|c| !c.children.is_empty()
                    && c.properties.get("grid_template_node") != Some(&json!(true)))
                .count()
                <= ui::mod_panel::SurfaceCollection::INITIAL_WINDOW
        );
        let labels: BTreeSet<_> = render
            .nodes
            .iter()
            .filter_map(|node| match &node.draw {
                Draw::Text { text, .. } => text.strip_prefix("Module ")?.parse::<usize>().ok(),
                _ => None,
            })
            .collect();
        seen.extend(labels.iter().copied());
        let artwork: BTreeSet<_> = render
            .nodes
            .iter()
            .filter_map(|node| match &node.draw {
                Draw::Custom { renderer, data } if renderer == json_ui::vector_path::RENDERER => {
                    assert!(json_ui::vector_path::parse(data).is_some());
                    assert_eq!(data["view_box"], json!([0., 0., 105., 60.]));
                    assert_eq!(data["stroke_width"], json!(2.));
                    assert_eq!(data["closed"], false);
                    Some(data["paths"][0][0][0].as_f64().unwrap() as usize)
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            artwork,
            labels.iter().copied().filter(|i| i % 2 == 0).collect()
        );
        let mut hits = render.hits.to_vec();
        super::surface::resolve_collection_hits(surface, &mut hits);
        for index in labels {
            assert!(
                hits.iter()
                    .any(|hit| hit.pressed.as_deref() == Some(&format!("mod.control:{index}")))
            );
        }
        let scroll = render.report.scrolls.keys().next().unwrap().clone();
        let nodes = Arc::clone(&render.nodes);
        let passes = screen.passes;
        let (_, allocations) = test_alloc::measure(|| {
            let render = screen
                .render_shared_with(
                    &surface.screen,
                    &catalog,
                    &context,
                    Arc::clone(&data),
                    ([452., 289.], 1., [0; 3]),
                    &env,
                    &view,
                )
                .unwrap();
            assert!(Arc::ptr_eq(&nodes, &render.nodes));
        });
        assert_eq!(screen.passes, passes);
        assert_eq!(allocations.calls, 0);
        view.scroll.insert(scroll, (row + 1) as f64 * 97.);
    }
    assert_eq!(seen, (0..66).collect());
    let filtered = PanelPatch::parse(r#"{"collection_order":{"modules":[64,4,9]}}"#)
        .unwrap()
        .apply(&panel, &Default::default())
        .unwrap();
    data = Arc::new(super::data::control_data(&filtered));
    view.scroll.clear();
    let render = screen
        .render_shared_with(
            &surface.screen,
            &catalog,
            &context,
            data,
            ([452., 289.], 1., [0; 3]),
            &env,
            &view,
        )
        .unwrap();
    let artwork: Vec<_> = render
        .nodes
        .iter()
        .filter_map(|node| match &node.draw {
            Draw::Custom { renderer, data } if renderer == json_ui::vector_path::RENDERER => {
                Some(data["paths"][0][0][0].as_f64().unwrap() as usize)
            }
            _ => None,
        })
        .collect();
    assert_eq!(artwork, [64, 4]);
}

/// Scrolling reaches every tile while keeping only the viewport's descendants alive.
#[test]
fn collection_scrolls_all_items_with_bounded_nodes_and_correct_actions() {
    let panel = catalogue();
    panel.validate().unwrap();
    let surface = panel.surface.as_ref().unwrap();
    let catalog = Arc::new(super::surface::catalog(surface).unwrap());
    let root = Arc::new(
        json_ui::resolve(&catalog, &surface.screen, &Context::default())
            .control
            .unwrap(),
    );
    let mut screen = CachedScreen::from_resolved(
        &surface.screen,
        Arc::clone(&catalog),
        Context::default(),
        root,
    );
    screen.virtualize_clipped_grid("modules", ui::mod_panel::SurfaceCollection::INITIAL_WINDOW);
    let data = Arc::new(super::data::control_data(&panel));
    let env = LayoutEnv {
        text: &Metrics,
        textures: &Metrics,
    };
    let mut view = ViewState::default();
    let mut seen = BTreeSet::new();
    let mut first_hits = Vec::new();
    let mut scroll: Option<String> = None;
    for row in 0..33 {
        if let Some(key) = &scroll {
            view.scroll.insert(key.clone(), row as f64 * 40.0);
        }
        let render = screen
            .render_shared_with(
                &surface.screen,
                &catalog,
                &Context::default(),
                Arc::clone(&data),
                ([308., 120.], 1., [0; 3]),
                &env,
                &view,
            )
            .unwrap();
        scroll = render.report.scrolls.keys().next().cloned();
        let mut hits = render.hits.to_vec();
        super::surface::resolve_collection_hits(surface, &mut hits);
        if row == 0 {
            first_hits = hits.clone();
        }
        let grid = render
            .bound
            .find(&|c| c.control_type.as_deref() == Some("grid"))
            .unwrap();
        let built = grid
            .children
            .iter()
            .filter(|c| {
                !c.children.is_empty()
                    && c.properties.get("grid_template_node") != Some(&json!(true))
            })
            .count();
        assert!(
            built <= ui::mod_panel::SurfaceCollection::INITIAL_WINDOW,
            "live tiles: {built}"
        );
        let mut labels = 0;
        for node in render.nodes.iter() {
            if let Draw::Text { text, .. } = &node.draw
                && let Some(i) = text
                    .strip_prefix("Module ")
                    .and_then(|s| s.parse::<usize>().ok())
            {
                labels += 1;
                seen.insert(i);
                assert!(
                    hits.iter()
                        .any(|hit| hit.pressed.as_deref() == Some(&format!("mod.control:{i}")))
                );
            }
        }
        assert!(
            labels > 0 && labels <= ui::mod_panel::SurfaceCollection::INITIAL_WINDOW,
            "row {row}, visible labels: {labels}"
        );
    }
    assert_eq!(seen, (0..99).collect());
    for order in [vec![98, 4], Vec::new(), vec![0, 1, 2]] {
        let filtered =
            PanelPatch::parse(&json!({"collection_order":{"modules":order}}).to_string())
                .unwrap()
                .apply(&panel, &Default::default())
                .unwrap();
        let filtered_surface = filtered.surface.as_ref().unwrap();
        let data = Arc::new(super::data::control_data(&filtered));
        view.scroll.clear();
        let render = screen
            .render_shared_with(
                &surface.screen,
                &catalog,
                &Context::default(),
                data,
                ([308., 120.], 1., [0; 3]),
                &env,
                &view,
            )
            .unwrap();
        let labels: Vec<_> = render
            .nodes
            .iter()
            .filter_map(|node| match &node.draw {
                Draw::Text { text, .. } => text
                    .strip_prefix("Module ")
                    .and_then(|s| s.parse::<usize>().ok()),
                _ => None,
            })
            .collect();
        assert_eq!(labels, order, "partial rows must not show padding tiles");
        let mut hits = render.hits.to_vec();
        super::surface::resolve_collection_hits(filtered_surface, &mut hits);
        let actions: BTreeSet<_> = hits
            .iter()
            .filter_map(|h| h.pressed.as_deref())
            .filter(|a| a.starts_with("mod.control:"))
            .map(str::to_owned)
            .collect();
        assert_eq!(
            actions,
            order.iter().map(|i| format!("mod.control:{i}")).collect()
        );
        assert!(
            first_hits
                .iter()
                .any(|h| h.pressed.as_deref() == Some("mod.control:0")),
            "a later filter cannot redirect the already painted hit"
        );
    }
}

/// A filter cannot redirect already painted input, and its admitted definitions stay shared.
#[test]
fn collection_filter_is_transactional_and_keeps_painted_action_identity() {
    let panel = catalogue();
    let surface = panel.surface.as_ref().unwrap();
    let original = &surface.collections["modules"];
    assert_eq!(original.control(0, "open"), Some(0));
    let filtered = PanelPatch::parse(r#"{"collection_order":{"modules":[98,4]}}"#)
        .unwrap()
        .apply(&panel, &Default::default())
        .unwrap();
    let changed = &filtered.surface.as_ref().unwrap().collections["modules"];
    assert!(Arc::ptr_eq(&original.definition, &changed.definition));
    assert_eq!(changed.control(0, "open"), Some(98));
    assert_eq!(original.control(0, "open"), Some(0));
    for order in [json!([99]), json!([1, 1]), json!([-1]), json!([1.5])] {
        assert!(
            PanelPatch::parse(&json!({"collection_order":{"modules":order}}).to_string())
                .and_then(|patch| patch.apply(&filtered, &Default::default()))
                .is_err()
        );
        assert_eq!(
            filtered.surface.as_ref().unwrap().collections["modules"].order,
            [98, 4]
        );
    }
}

/// Invalid collection geometry, references and vector payloads fail before any rendering.
#[test]
fn collection_admission_rejects_unsafe_metadata() {
    let original = serde_json::to_value(catalogue()).unwrap();
    for (field, value) in [
        ("columns", json!(0)),
        ("viewport", json!([308, 4096])),
        ("items", json!([{"controls":{"missing":0}}])),
    ] {
        let mut changed = original.clone();
        changed["surface"]["collections"]["modules"]["definition"][field] = value;
        assert!(
            serde_json::from_value::<Panel>(changed)
                .unwrap()
                .validate()
                .is_err()
        );
    }
    for art in [
        json!({"view_box":[0,0,24,24],"paths":[[[0,0],[24,24]]],"stroke_width":0,"closed":false}),
        json!({"view_box":[0,0,24,24],"paths":[[[0,0]]],"stroke_width":2,"closed":false}),
    ] {
        let mut changed = original.clone();
        changed["surface"]["collections"]["modules"]["definition"]["items"][0]["art"] = art;
        assert!(
            serde_json::from_value::<Panel>(changed)
                .unwrap()
                .validate()
                .is_err()
        );
    }
    let mut changed = original;
    changed["surface"]["collections"]["modules"]["definition"]["items"][0]["controls"]["open"] =
        json!(999);
    assert!(
        serde_json::from_value::<Panel>(changed)
            .unwrap()
            .validate()
            .is_err()
    );
}

/// Tiny positive tile dimensions are rejected before visible-cell arithmetic can overflow.
#[test]
fn collection_admission_rejects_tiny_geometry_without_overflow() {
    let mut panel = serde_json::to_value(catalogue()).unwrap();
    let template = &mut panel["surface"]["collections"]["modules"]["definition"]["template"];
    let mut document: serde_json::Value =
        serde_json::from_str(template["document"].as_str().unwrap()).unwrap();
    document["root"]["size"] = json!([100, 1e-300]);
    template["document"] = json!(document.to_string());
    assert!(
        serde_json::from_value::<Panel>(panel)
            .unwrap()
            .validate()
            .is_err()
    );
}
