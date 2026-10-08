//! Engine-owned widget behaviour and input over synthetic trees: button/toggle
//! state children, scroll offsets and the scrollbar box, slider box travel,
//! grid packing, hit-testing through clips and modal panels, and mappings.

use std::collections::BTreeMap;

use json_ui::{
    HitKind, LaidOut, LayoutEnv, ResolvedControl, TextMeasure, TextureMeta, TextureSource,
    ViewState, global_mapping, hit_regions, hit_test, layout_with, scroll_target,
};
use serde_json::{Value, json};

struct ZeroText;
impl TextMeasure for ZeroText {
    fn extent(&self, _text: &str) -> [f64; 2] {
        [0.0, 0.0]
    }
}

struct NoTextures;
impl TextureSource for NoTextures {
    fn texture(&self, _path: &str) -> Option<TextureMeta> {
        None
    }
}

fn env() -> LayoutEnv<'static> {
    LayoutEnv {
        text: &ZeroText,
        textures: &NoTextures,
    }
}

fn ctrl(
    name: &str,
    control_type: &str,
    props: Value,
    children: Vec<ResolvedControl>,
) -> ResolvedControl {
    let properties: BTreeMap<String, Value> = match props {
        Value::Object(map) => map.into_iter().collect(),
        _ => BTreeMap::new(),
    };
    ResolvedControl {
        name: name.to_owned(),
        control_type: Some(control_type.to_owned()),
        base: None,
        unresolved_base: None,
        properties: properties.into(),
        children,
        factory: None,
    }
}

fn find<'a>(node: &'a LaidOut<'a>, name: &str) -> &'a LaidOut<'a> {
    fn walk<'a>(node: &'a LaidOut<'a>, name: &str) -> Option<&'a LaidOut<'a>> {
        if node.control.name == name {
            return Some(node);
        }
        node.children.iter().find_map(|child| walk(child, name))
    }
    walk(node, name).unwrap_or_else(|| panic!("missing {name}"))
}

fn top_left(size: Value) -> Value {
    json!({ "size": size, "anchor_from": "top_left", "anchor_to": "top_left" })
}

fn button() -> ResolvedControl {
    ctrl(
        "button",
        "button",
        json!({
            "size": [40, 20], "anchor_from": "top_left", "anchor_to": "top_left",
            "default_control": "default", "hover_control": "hover", "pressed_control": "pressed",
            "button_mappings": [
                { "from_button_id": "button.menu_select", "to_button_id": "button.go", "mapping_type": "pressed" }
            ]
        }),
        vec![
            ctrl("default", "panel", json!({}), vec![]),
            ctrl("hover", "panel", json!({}), vec![]),
            ctrl("pressed", "panel", json!({}), vec![]),
        ],
    )
}

fn screen(children: Vec<ResolvedControl>) -> ResolvedControl {
    ctrl("root", "panel", top_left(json!([200, 100])), children)
}

#[test]
fn decorative_modal_parent_keeps_its_child_input_scope() {
    let root = screen(vec![ctrl(
        "decoration",
        "panel",
        json!({"size": [100, 50], "modal": true}),
        vec![button()],
    )]);
    let (laid, _) = layout_with(&root, [200.0, 100.0], &env(), &ViewState::default());
    let hits = hit_regions(&laid);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].modal_root.as_deref(), Some("/root/decoration"));
    assert_eq!(hits[0].pressed.as_deref(), Some("button.go"));
}

#[test]
fn button_shows_exactly_the_state_child_for_its_interaction() {
    let root = screen(vec![button()]);
    let shown = |state: &ViewState| {
        let (laid, _) = layout_with(&root, [200.0, 100.0], &env(), state);
        ["default", "hover", "pressed"].map(|name| find(&laid, name).visible)
    };
    assert_eq!(shown(&ViewState::default()), [true, false, false]);
    let key = "/root/button".to_owned();
    let hovered = ViewState {
        hovered: Some(key.clone()),
        ..ViewState::default()
    };
    assert_eq!(shown(&hovered), [false, true, false]);
    let pressed = ViewState {
        hovered: Some(key.clone()),
        pressed: Some(key),
        ..ViewState::default()
    };
    assert_eq!(shown(&pressed), [false, false, true]);
}

#[test]
fn toggle_state_child_follows_the_bound_state() {
    let toggle = |checked: bool| {
        ctrl(
            "toggle",
            "toggle",
            json!({
                "size": [20, 20], "#toggle_state": checked,
                "checked_control": "checked", "unchecked_control": "unchecked",
                "checked_hover_control": "checked_hover", "unchecked_hover_control": "unchecked_hover",
                "toggle_name": "sound"
            }),
            vec![
                ctrl("unchecked", "panel", json!({}), vec![]),
                ctrl("checked", "panel", json!({}), vec![]),
                ctrl("unchecked_hover", "panel", json!({}), vec![]),
                ctrl("checked_hover", "panel", json!({}), vec![]),
            ],
        )
    };
    for checked in [false, true] {
        let root = screen(vec![toggle(checked)]);
        let (laid, _) = layout_with(&root, [200.0, 100.0], &env(), &ViewState::default());
        assert_eq!(find(&laid, "checked").visible, checked);
        assert_eq!(find(&laid, "unchecked").visible, !checked);
        assert!(!find(&laid, "checked_hover").visible);
        let regions = hit_regions(&laid);
        assert_eq!(regions[0].kind, HitKind::Toggle);
        assert_eq!(regions[0].checked, Some(checked));
        assert_eq!(regions[0].control_name.as_deref(), Some("sound"));
    }
}

fn scroll_view(content_height: f64) -> ResolvedControl {
    let content = ctrl(
        "scrolling_content",
        "panel",
        top_left(json!(["100%", content_height])),
        vec![],
    );
    let viewport = ctrl(
        "scrolling_view_port",
        "panel",
        json!({ "size": [90, 50], "anchor_from": "top_left", "anchor_to": "top_left", "clips_children": true }),
        vec![content],
    );
    let bar = ctrl(
        "bar_and_track",
        "panel",
        json!({ "size": [5, 50], "anchor_from": "top_right", "anchor_to": "top_right" }),
        vec![
            ctrl(
                "track",
                "scroll_track",
                json!({ "size": [5, "100%"] }),
                vec![],
            ),
            ctrl(
                "box",
                "scrollbar_box",
                json!({
                    "size": [5, "100%"], "anchor_from": "top_left", "anchor_to": "top_left",
                    "draggable": "vertical", "contained": true
                }),
                vec![],
            ),
        ],
    );
    ctrl(
        "scroll",
        "scroll_view",
        json!({
            "size": [100, 50], "anchor_from": "top_left", "anchor_to": "top_left",
            "scroll_content": "scrolling_content", "scroll_view_port": "scrolling_view_port",
            "scrollbar_track": "track", "scrollbar_box": "box",
            "scroll_box_and_track_panel": "bar_and_track", "scroll_speed": 15
        }),
        vec![viewport, bar],
    )
}

#[test]
fn scroll_view_offsets_content_and_sizes_its_box() {
    let root = screen(vec![scroll_view(200.0)]);
    let state = ViewState {
        scroll: [("/root/scroll".to_owned(), 1000.0)].into_iter().collect(),
        ..ViewState::default()
    };
    let (laid, report) = layout_with(&root, [200.0, 100.0], &env(), &state);
    let metrics = &report.scrolls["/root/scroll"];
    assert_eq!(metrics.offset, 150.0, "clamped to content - viewport");
    assert_eq!(find(&laid, "scrolling_content").rect.y, -150.0);
    let thumb = metrics.thumb.expect("overflow shows the box");
    assert_eq!(
        thumb[3], 13.0,
        "the visible fraction of the track, rounded up"
    );
    assert_eq!(thumb[1], 37.0, "fully scrolled puts the box at the bottom");
    assert_eq!(metrics.thumb_drag_target(-50.0), 0.0);
    // A control above the viewport scrolls back up to it.
    assert_eq!(metrics.offset_revealing(-40.0, -20.0), 110.0);

    let fits = screen(vec![scroll_view(20.0)]);
    let (laid, report) = layout_with(&fits, [200.0, 100.0], &env(), &ViewState::default());
    assert_eq!(report.scrolls["/root/scroll"].bar_visible, Some(false));
    assert!(
        !find(&laid, "bar_and_track").visible,
        "content that fits hides the bar and track"
    );
    let regions = hit_regions(&laid);
    assert!(scroll_target(&regions, &report, [10.0, 10.0]).is_some());
}

// A gated render lays out only the scroll content its viewport shows.
#[test]
fn gated_render_skips_scroll_content_outside_the_viewport() {
    let rows = (0..1000)
        .map(|index| {
            ctrl(
                "row",
                "button",
                json!({ "size": [90, 10], "collection_index": index }),
                vec![],
            )
        })
        .collect();
    let mut root = screen(vec![scroll_view(0.0)]);
    let content = &mut root.children[0].children[0].children[0];
    content.control_type = Some("stack_panel".to_owned());
    content
        .properties
        .insert("size".to_owned(), json!(["100%", "100%c"]));
    content.children = rows;
    let state = ViewState {
        scroll: [("/root/scroll".to_owned(), 500.0)].into_iter().collect(),
        ..ViewState::default()
    };
    let mut measures = json_ui::MeasureCache::default();
    let render = json_ui::render_bound_gated(root, [200.0, 100.0], &env(), &state, &mut measures);
    assert_eq!(render.report.scrolls["/root/scroll"].content, 10_000.0);
    let rows: Vec<usize> = render
        .hits
        .iter()
        .filter(|region| region.name == "row")
        .filter_map(|region| region.collection_index)
        .collect();
    assert_eq!(rows, (50..55).collect::<Vec<_>>());
    // Reused measurements lay a scrolled tree out as fresh ones do.
    let state = ViewState {
        scroll: [("/root/scroll".to_owned(), 205.0)].into_iter().collect(),
        ..ViewState::default()
    };
    let reused =
        json_ui::render_bound_gated(render.bound, [200.0, 100.0], &env(), &state, &mut measures);
    let fresh = json_ui::render_bound_gated(
        reused.bound.clone(),
        [200.0, 100.0],
        &env(),
        &state,
        &mut json_ui::MeasureCache::default(),
    );
    assert_eq!(reused.hits, fresh.hits);
    assert_eq!(
        reused
            .hits
            .iter()
            .filter(|region| region.name == "row")
            .count(),
        6
    );
}

#[test]
fn gated_render_keeps_pointer_and_drag_placement() {
    let root = screen(vec![
        ctrl(
            "cursor",
            "image",
            json!({ "size": [20, 10], "follows_cursor": true, "color": [1, 1, 1] }),
            vec![],
        ),
        ctrl(
            "dragged",
            "image",
            json!({ "size": [20, 10], "anchor_from": "top_left", "anchor_to": "top_left",
                    "draggable": "horizontal", "color": [1, 1, 1] }),
            vec![],
        ),
    ]);
    let mut state = ViewState {
        pointer: Some([90.0, 40.0]),
        ..ViewState::default()
    };
    state.drags.insert("/root/dragged".into(), [30.0, 30.0]);
    let render = json_ui::render_bound_gated(
        root,
        [200.0, 100.0],
        &env(),
        &state,
        &mut json_ui::MeasureCache::default(),
    );
    for (name, expected) in [("cursor", [80.0, 35.0]), ("dragged", [30.0, 0.0])] {
        let node = render.nodes.iter().find(|node| node.name == name).unwrap();
        assert_eq!([node.dest.x, node.dest.y], expected, "{name} draw");
    }
    let hit = render
        .hits
        .iter()
        .find(|hit| hit.name == "dragged")
        .unwrap();
    assert!(hit.contains([35.0, 5.0]), "input follows the dragged image");
    assert!(!hit.contains([5.0, 5.0]), "input leaves its old position");
    assert!(render.report.tracks_pointer);
}

#[test]
fn slider_box_travels_with_the_value_and_progress_clips() {
    let slider = ctrl(
        "slider",
        "slider",
        json!({
            "size": [100, 10], "anchor_from": "top_left", "anchor_to": "top_left",
            "#slider_value": 0.25, "slider_box_control": "slider_box",
            "progress_control": "progress", "default_control": "bar", "slider_name": "volume"
        }),
        vec![
            ctrl(
                "bar",
                "panel",
                json!({}),
                vec![ctrl("progress", "image", json!({}), vec![])],
            ),
            ctrl(
                "slider_box",
                "slider_box",
                json!({ "size": [10, 16] }),
                vec![],
            ),
        ],
    );
    let root = screen(vec![slider]);
    let (laid, _) = layout_with(&root, [200.0, 100.0], &env(), &ViewState::default());
    assert_eq!(
        find(&laid, "slider_box").rect.x,
        20.0,
        "centre at a quarter"
    );
    assert_eq!(find(&laid, "progress").clip_ratio, Some(0.75));
}

#[test]
fn fixed_grid_packs_cells_row_major() {
    let mut cells: Vec<_> = (0..4)
        .map(|index| {
            ctrl(
                "cell",
                "panel",
                json!({ "size": [18, 18], "collection_index": index }),
                vec![],
            )
        })
        .collect();
    cells.push(ctrl(
        "template",
        "panel",
        json!({ "size": [18, 18], "grid_template_node": true }),
        vec![],
    ));
    let grid = ctrl(
        "grid",
        "grid",
        json!({ "size": [36, 36], "anchor_from": "top_left", "anchor_to": "top_left",
                "grid_dimensions": [2, 2], "grid_item_template": "t.cell" }),
        cells,
    );
    let root = screen(vec![grid]);
    let (laid, _) = layout_with(&root, [200.0, 100.0], &env(), &ViewState::default());
    let grid = find(&laid, "grid");
    let corners: Vec<[f64; 2]> = grid
        .children
        .iter()
        .map(|cell| [cell.rect.x, cell.rect.y])
        .collect();
    assert_eq!(
        corners,
        [[0.0, 0.0], [18.0, 0.0], [0.0, 18.0], [18.0, 18.0]]
    );
    assert_eq!(grid.children[3].key, "/root/grid/cell[3]");
}

#[test]
fn modal_panel_blocks_what_is_beneath_but_not_its_own_controls() {
    let modal = ctrl(
        "popup",
        "input_panel",
        json!({ "size": ["100%", "100%"], "modal": true, "layer": 10 }),
        vec![ctrl(
            "ok",
            "button",
            json!({
                "size": [20, 20], "anchor_from": "top_left", "anchor_to": "top_left", "offset": [100, 0],
                "button_mappings": [
                    { "from_button_id": "button.menu_select", "to_button_id": "button.ok", "mapping_type": "pressed" }
                ]
            }),
            vec![],
        )],
    );
    let root = screen(vec![button(), modal]);
    let (laid, _) = layout_with(&root, [200.0, 100.0], &env(), &ViewState::default());
    let regions = hit_regions(&laid);
    assert!(
        hit_test(&regions, [5.0, 5.0]).is_none(),
        "the modal swallows the button below"
    );
    let ok = hit_test(&regions, [105.0, 5.0]).expect("modal child is hit");
    assert_eq!(ok.pressed.as_deref(), Some("button.ok"));
}

#[test]
fn clipped_controls_are_not_hit_outside_their_clip() {
    let clipper = ctrl(
        "clipper",
        "panel",
        json!({ "size": [30, 10], "anchor_from": "top_left", "anchor_to": "top_left", "clips_children": true }),
        vec![button()],
    );
    let root = screen(vec![clipper]);
    let (laid, _) = layout_with(&root, [200.0, 100.0], &env(), &ViewState::default());
    let regions = hit_regions(&laid);
    assert!(hit_test(&regions, [5.0, 5.0]).is_some());
    assert!(
        hit_test(&regions, [5.0, 15.0]).is_none(),
        "below the clip edge"
    );
}

#[test]
fn innermost_global_mapping_wins() {
    let mut inner = button();
    inner.properties.insert(
        "button_mappings".to_owned(),
        json!([{ "from_button_id": "button.menu_cancel", "to_button_id": "popup.escape", "mapping_type": "global" }]),
    );
    let mut root = screen(vec![inner]);
    root.properties.insert(
        "button_mappings".to_owned(),
        json!([{ "from_button_id": "button.menu_cancel", "to_button_id": "button.menu_exit", "mapping_type": "global" }]),
    );
    let (laid, _) = layout_with(&root, [200.0, 100.0], &env(), &ViewState::default());
    assert_eq!(
        global_mapping(&laid, "button.menu_cancel").as_deref(),
        Some("popup.escape")
    );
    root.properties.remove("button_mappings");
    let (laid, _) = layout_with(&root, [200.0, 100.0], &env(), &ViewState::default());
    assert_eq!(
        global_mapping(&laid, "button.menu_cancel").as_deref(),
        Some("popup.escape")
    );
}

struct CellLibrary;
impl json_ui::ControlLibrary for CellLibrary {
    fn resolve(&self, _reference: &json_ui::ControlRef) -> Option<ResolvedControl> {
        Some(ctrl(
            "cell",
            "input_panel",
            json!({ "size": [18, 18] }),
            vec![ctrl(
                "button",
                "button",
                json!({
                    "button_mappings": [
                        { "from_button_id": "button.menu_select", "to_button_id": "button.container_take_all_place_all", "mapping_type": "pressed" }
                    ]
                }),
                vec![],
            )],
        ))
    }
}

#[test]
fn bound_grid_cells_report_their_collection_and_index() {
    let grid = ctrl(
        "grid",
        "grid",
        json!({
            "size": [54, 18], "anchor_from": "top_left", "anchor_to": "top_left",
            "grid_dimensions": [3, 1], "grid_item_template": "common.cell",
            "collection_name": "container_items"
        }),
        vec![],
    );
    let bound = json_ui::bind(
        &screen(vec![grid]),
        &json_ui::DataSource::new(),
        &CellLibrary,
    );
    let (laid, _) = layout_with(&bound, [200.0, 100.0], &env(), &ViewState::default());
    let regions = hit_regions(&laid);
    let cell = hit_test(&regions, [40.0, 5.0]).expect("third cell");
    assert_eq!(cell.collection.as_deref(), Some("container_items"));
    assert_eq!(cell.collection_index, Some(2));
    assert_eq!(
        cell.pressed.as_deref(),
        Some("button.container_take_all_place_all")
    );
}

#[test]
fn review_hit_test_does_not_return_disabled_controls_or_click_through_them() {
    let underneath = button();
    let mut disabled = button();
    disabled.name = "disabled".into();
    disabled.properties.insert("enabled".into(), json!(false));
    let root = ctrl(
        "root",
        "panel",
        top_left(json!([100, 100])),
        vec![underneath, disabled],
    );
    let (laid, _) = layout_with(&root, [100.0, 100.0], &env(), &ViewState::default());
    assert!(hit_test(&hit_regions(&laid), [10.0, 10.0]).is_none());
}

#[test]
fn review_scroll_culling_preserves_unclipped_controls_and_escaping_descendants() {
    for nested in [false, true] {
        let mut root = screen(vec![scroll_view(200.0)]);
        let escaped = ctrl(
            "escaped",
            "button",
            json!({
                "size": [10, 10], "offset": [0, 70], "anchor_from": "top_left",
                "anchor_to": "top_left", "allow_clipping": false
            }),
            vec![],
        );
        let content = &mut root.children[0].children[0].children[0];
        content.children = if nested {
            vec![ctrl(
                "offscreen",
                "panel",
                json!({
                    "size": [10, 10], "offset": [0, 70], "anchor_from": "top_left",
                    "anchor_to": "top_left"
                }),
                vec![ctrl(
                    "escaped",
                    "button",
                    json!({
                        "size": [10, 10], "anchor_from": "top_left", "anchor_to": "top_left", "allow_clipping": false
                    }),
                    vec![],
                )],
            )]
        } else {
            vec![escaped]
        };
        let state = ViewState::default();
        let normal = json_ui::render_bound(root.clone(), [200.0, 100.0], &env(), &state);
        assert!(normal.hits.iter().any(|hit| hit.name == "escaped"));
        let culled = json_ui::render_bound_gated(
            root,
            [200.0, 100.0],
            &env(),
            &state,
            &mut json_ui::MeasureCache::default(),
        );
        assert!(
            culled.hits.iter().any(|hit| hit.name == "escaped"),
            "nested={nested}"
        );
    }
}
