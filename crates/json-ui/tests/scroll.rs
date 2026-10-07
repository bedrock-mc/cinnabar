//! Scroll views as vanilla runs them: the named viewport,
//! track and panel, box sizing and axes, update requests, and the bag feedback.
//! Kept out of tests/it: the process latches the wheel sensitivity of the first view scrolled.

use std::collections::BTreeMap;

use json_ui::{
    DataSource, Draggable, EmptyLibrary, LaidOut, LayoutEnv, ResolvedControl, Scalar,
    ScrollMetrics, TextMeasure, TextureMeta, TextureSource, ViewState, bind, hit_regions,
    layout_with, scroll_target,
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

fn ctrl(name: &str, kind: &str, props: Value, children: Vec<ResolvedControl>) -> ResolvedControl {
    let properties: BTreeMap<String, Value> = match props {
        Value::Object(map) => map.into_iter().collect(),
        _ => BTreeMap::new(),
    };
    ResolvedControl {
        name: name.to_owned(),
        control_type: Some(kind.to_owned()),
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

fn at(props: Value) -> Value {
    let mut props = props;
    props["anchor_from"] = json!("top_left");
    props["anchor_to"] = json!("top_left");
    props
}

/// A scroll view naming all five references, with `extra` properties.
fn view(extra: Value, content: [f64; 2], viewport: [f64; 2], draggable: &str) -> ResolvedControl {
    let mut props = at(json!({
        "size": [100, 100], "scroll_content": "content", "scroll_view_port": "viewport",
        "scrollbar_track": "track", "scrollbar_box": "box", "scroll_box_and_track_panel": "bars"
    }));
    for (key, value) in extra.as_object().into_iter().flatten() {
        props[key] = value.clone();
    }
    ctrl(
        "view",
        "scroll_view",
        props,
        vec![
            ctrl(
                "viewport",
                "panel",
                at(json!({ "size": viewport })),
                vec![ctrl(
                    "content",
                    "panel",
                    at(json!({ "size": content })),
                    vec![],
                )],
            ),
            ctrl(
                "bars",
                "panel",
                at(json!({ "size": [8, 100], "offset": [92, 0] })),
                vec![
                    ctrl(
                        "track",
                        "scroll_track",
                        at(json!({ "size": [8, 40], "offset": [0, 50] })),
                        vec![],
                    ),
                    ctrl(
                        "box",
                        "scrollbar_box",
                        at(json!({ "size": [8, 10], "draggable": draggable })),
                        vec![ctrl("art", "image", json!({}), vec![])],
                    ),
                ],
            ),
        ],
    )
}

fn metrics_of(root: &ResolvedControl, state: &ViewState) -> ScrollMetrics {
    let (_, report) = layout_with(root, [200.0, 200.0], &env(), state);
    report.scrolls["/view"].clone()
}

fn scrolled(offset: f64) -> ViewState {
    ViewState {
        scroll: [("/view".to_owned(), offset)].into_iter().collect(),
        ..ViewState::default()
    }
}

// The named viewport, not the content's parent, bounds the scroll and takes the wheel.
#[test]
fn named_viewport_sets_the_extent_and_wheel_area() {
    let root = view(json!({}), [100.0, 200.0], [100.0, 40.0], "vertical");
    let (laid, report) = layout_with(&root, [200.0, 200.0], &env(), &scrolled(1000.0));
    let metrics = &report.scrolls["/view"];
    assert_eq!(metrics.max_offset(), 160.0);
    assert_eq!(find(&laid, "content").rect.y, -160.0);
    let regions = hit_regions(&laid);
    assert!(scroll_target(&regions, &report, [10.0, 20.0]).is_some());
    assert!(
        scroll_target(&regions, &report, [10.0, 80.0]).is_none(),
        "below the viewport, off the track"
    );
    assert!(
        scroll_target(&regions, &report, [95.0, 60.0]).is_some(),
        "on the track"
    );
}

// Without every reference resolved vanilla does not scroll at all.
#[test]
fn a_missing_reference_disables_scrolling() {
    let mut root = view(json!({}), [100.0, 200.0], [100.0, 40.0], "vertical");
    root.properties.remove("scrollbar_track");
    let (laid, report) = layout_with(&root, [200.0, 200.0], &env(), &scrolled(50.0));
    assert!(report.scrolls.is_empty());
    assert_eq!(find(&laid, "content").rect.y, 0.0);
}

// The box travels and sizes along the named track, not its own parent.
#[test]
fn box_uses_the_named_track() {
    let root = view(json!({}), [100.0, 200.0], [100.0, 50.0], "vertical");
    let half = metrics_of(&root, &scrolled(75.0));
    assert_eq!(half.track, Some([92.0, 50.0, 8.0, 40.0]));
    let thumb = half.thumb.expect("shown");
    assert_eq!(thumb[3], 10.0, "a quarter of the 40px track");
    assert_eq!(
        thumb[1], 15.0,
        "halfway along 30px of travel from its own place"
    );
}

// Fitting content hides the whole bar-and-track panel and reports it; always-visible keeps it.
#[test]
fn fitting_content_hides_the_panel_unless_always_visible() {
    let fits = view(json!({}), [100.0, 20.0], [100.0, 50.0], "vertical");
    let (laid, report) = layout_with(&fits, [200.0, 200.0], &env(), &ViewState::default());
    assert!(!find(&laid, "bars").visible);
    let metrics = &report.scrolls["/view"];
    assert_eq!(metrics.bar_visible, Some(false));
    assert!(metrics.hit_bottom, "fitting content has hit the bottom");
    let kept = view(
        json!({ "scrollbar_always_visible": true }),
        [100.0, 20.0],
        [100.0, 50.0],
        "vertical",
    );
    let (laid, report) = layout_with(&kept, [200.0, 200.0], &env(), &ViewState::default());
    assert!(find(&laid, "bars").visible);
    assert_eq!(report.scrolls["/view"].bar_visible, Some(true));
    assert_eq!(report.scrolls["/view"].thumb.expect("shown")[3], 40.0);
}

// A track press centres the viewport on its fraction; the configured ids are kept.
#[test]
fn track_press_centres_the_viewport_and_ids_are_read() {
    let root = view(
        json!({ "scrollbar_track_button": "button.custom_page", "scrollbar_touch_button": "button.custom_touch" }),
        [100.0, 400.0],
        [100.0, 100.0],
        "vertical",
    );
    let metrics = metrics_of(&root, &ViewState::default());
    assert_eq!(metrics.track_button.as_deref(), Some("button.custom_page"));
    assert_eq!(metrics.touch_button.as_deref(), Some("button.custom_touch"));
    assert_eq!(metrics.offset_for_track([95.0, 70.0]), 150.0);
    assert_eq!(metrics.offset_for_track([95.0, 50.0]), 0.0, "clamped");
}

// scroll_speed defaults to one pixel per notch.
#[test]
fn wheel_speed_defaults_to_one() {
    let root = view(json!({}), [100.0, 400.0], [100.0, 100.0], "vertical");
    let metrics = metrics_of(&root, &scrolled(50.0));
    assert_eq!(metrics.speed, 1.0);
    // A step up moves 120/127 of the sensitivity (the client's mouse byte scaling).
    assert!((metrics.offset_for_wheel(1.0) - (50.0 - 120.0 / 127.0)).abs() < 1e-9);
}

// always_handle_scrolling takes the wheel anywhere.
#[test]
fn always_handle_scrolling_takes_the_wheel_anywhere() {
    let root = view(
        json!({ "always_handle_scrolling": true }),
        [100.0, 400.0],
        [100.0, 40.0],
        "vertical",
    );
    let (laid, report) = layout_with(&root, [200.0, 200.0], &env(), &ViewState::default());
    assert!(scroll_target(&hit_regions(&laid), &report, [150.0, 150.0]).is_some());
}

// jump_to_bottom_on_update follows the end whenever the content grows.
#[test]
fn jump_to_bottom_follows_growth() {
    let small = view(
        json!({ "jump_to_bottom_on_update": true }),
        [100.0, 200.0],
        [100.0, 100.0],
        "vertical",
    );
    let mut state = ViewState::default();
    let (_, report) = layout_with(&small, [200.0, 200.0], &env(), &state);
    assert_eq!(report.scrolls["/view"].offset, 100.0, "opens at the end");
    assert!(state.settle(&report));
    state.scroll.insert("/view".to_owned(), 20.0);
    let (_, report) = layout_with(&small, [200.0, 200.0], &env(), &state);
    assert_eq!(
        report.scrolls["/view"].offset, 20.0,
        "the user's scroll holds"
    );
    let grown = view(
        json!({ "jump_to_bottom_on_update": true }),
        [100.0, 300.0],
        [100.0, 100.0],
        "vertical",
    );
    let (_, report) = layout_with(&grown, [200.0, 200.0], &env(), &state);
    assert_eq!(
        report.scrolls["/view"].offset, 200.0,
        "growth jumps to the new end"
    );
}

// draggable picks the axis: a horizontal box scrolls x; not_draggable and both keep the box size.
#[test]
fn draggable_sets_the_axis_and_sizing() {
    let horizontal = view(json!({}), [400.0, 100.0], [100.0, 100.0], "horizontal");
    let (laid, report) = layout_with(&horizontal, [200.0, 200.0], &env(), &scrolled(300.0));
    let metrics = &report.scrolls["/view"];
    assert!(metrics.horizontal);
    assert_eq!(metrics.box_drag, Draggable::Horizontal);
    assert_eq!(metrics.max_offset(), 300.0);
    assert_eq!(find(&laid, "content").rect.x, -300.0);
    assert_eq!(find(&laid, "content").rect.y, 0.0);
    for (draggable, expected) in [
        ("not_draggable", Draggable::NotDraggable),
        ("both", Draggable::Both),
    ] {
        let root = view(json!({}), [100.0, 400.0], [100.0, 100.0], draggable);
        let metrics = metrics_of(&root, &ViewState::default());
        assert_eq!(metrics.box_drag, expected);
        assert_eq!(
            metrics.thumb.expect("placed")[3],
            10.0,
            "authored size kept"
        );
        assert!(!metrics.horizontal);
    }
}

// A bound #force_scroll_to_end pins the view to its end.
#[test]
fn force_scroll_to_end_pins_the_end() {
    let root = view(
        json!({ "#force_scroll_to_end": true }),
        [100.0, 400.0],
        [100.0, 100.0],
        "vertical",
    );
    assert_eq!(metrics_of(&root, &ViewState::default()).offset, 300.0);
}

// Scrolling publishes #scrolled_to_end / #scrollbar_hit_bottom / #scroll_bar_visible.
#[test]
fn scrolling_publishes_bag_feedback() {
    let root = view(json!({}), [100.0, 400.0], [100.0, 100.0], "vertical");
    let top = metrics_of(&root, &ViewState::default());
    assert!(!top.scrolled_to_end && !top.hit_bottom);
    assert_eq!(top.bar_visible, Some(true));
    let end = metrics_of(&root, &scrolled(300.0));
    assert!(end.scrolled_to_end && end.hit_bottom);
    let feedback = end.feedback();
    assert_eq!(feedback["#scrolled_to_end"], Scalar::Bool(true));
    assert_eq!(feedback["#scroll_bar_visible"], Scalar::Bool(true));
    // Hit-bottom latches once the caller settles it.
    let mut state = scrolled(300.0);
    let (_, report) = layout_with(&root, [200.0, 200.0], &env(), &state);
    state.settle(&report);
    state.scroll.insert("/view".to_owned(), 0.0);
    assert!(metrics_of(&root, &state).hit_bottom);
}

// View bindings read a scroll view's bag: seeded on creation, then fed back per frame.
#[test]
fn view_bindings_read_the_scroll_view_bag() {
    let marker = |name: &str, control: &str, source: &str| {
        ctrl(
            name,
            "image",
            json!({ "bindings": [{
                "binding_type": "view", "source_control_name": control,
                "source_property_name": source, "target_property_name": "#visible"
            }] }),
            vec![],
        )
    };
    let root = view(json!({}), [100.0, 400.0], [100.0, 100.0], "vertical");
    let screen = |root: ResolvedControl| {
        ctrl(
            "screen",
            "panel",
            json!({}),
            vec![
                root,
                marker("end", "view", "#scrolled_to_end"),
                marker("bar", "box", "#is_scroll_bar_box"),
            ],
        )
    };
    let seeded = bind(&screen(root.clone()), &DataSource::new(), &EmptyLibrary);
    let visible = |tree: &ResolvedControl, name: &str| {
        tree.children
            .iter()
            .find(|child| child.name == name)
            .and_then(|child| child.properties.get("#visible").cloned())
    };
    assert_eq!(visible(&seeded, "end"), Some(json!(true)), "seeded true");
    assert_eq!(
        visible(&seeded, "bar"),
        Some(json!(true)),
        "a box marks itself"
    );
    let mut data = DataSource::new();
    data.set_control_values("view", metrics_of(&root, &ViewState::default()).feedback());
    let fed = bind(&screen(root), &data, &EmptyLibrary);
    assert_eq!(
        visible(&fed, "end"),
        Some(json!(false)),
        "fed back from layout"
    );
}

#[test]
fn gated_render_keeps_retained_scroll_state() {
    let root = view(
        json!({ "jump_to_bottom_on_update": true, "touch_mode": true }),
        [100.0, 300.0],
        [100.0, 100.0],
        "vertical",
    );
    for (previous_extent, expected_offset) in [(100.0, 200.0), (200.0, 20.0)] {
        let mut state = scrolled(20.0);
        state.scroll_state.insert(
            "/view".to_owned(),
            json_ui::ScrollRetained {
                extent: Some(previous_extent),
                hit_bottom: true,
                bar_fade: Some(0.0),
                ..Default::default()
            },
        );
        let render = json_ui::render_bound_gated(
            root.clone(),
            [200.0, 200.0],
            &env(),
            &state,
            &mut json_ui::MeasureCache::default(),
        );
        let metrics = &render.report.scrolls["/view"];
        assert_eq!(
            metrics.offset, expected_offset,
            "previous extent {previous_extent}"
        );
        assert!(
            metrics.hit_bottom,
            "reaching the bottom stays latched after scrolling up"
        );
        assert_eq!(
            metrics.bar_visible,
            Some(false),
            "the faded touch bar stays hidden"
        );
    }
}
