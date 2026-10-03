//! Layout parity regressions against the client's layout rules, one per audited
//! row (`G`, `S`, `D` ids). Trees are written in pack syntax and laid out in a
//! 100x100 screen unless a test says otherwise.

use std::collections::BTreeMap;

use json_ui::{
    ControlLibrary, ControlRef, DataSource, EmptyLibrary, LaidOut, LayoutEnv, ResolvedControl,
    Scalar, TextMeasure, TextureMeta, TextureSource, bind, layout,
};
use serde_json::{Value, json};

struct MonoText;
impl TextMeasure for MonoText {
    fn extent(&self, text: &str) -> [f64; 2] {
        [text.chars().count() as f64 * 6.0, 10.0]
    }
}

/// Every texture is 40x20 texels; ratio sizing ignores the sidecar `base_size`.
struct Textures;
impl TextureSource for Textures {
    fn texture(&self, _path: &str) -> Option<TextureMeta> {
        Some(TextureMeta {
            base_size: [8.0, 8.0],
            nineslice: None,
            pixels: [40.0, 20.0],
        })
    }
}

fn env() -> LayoutEnv<'static> {
    LayoutEnv {
        text: &MonoText,
        textures: &Textures,
    }
}

/// A control from pack syntax: `{"type": ..., "controls": [{"name": {...}}]}`.
fn control(name: &str, body: Value) -> ResolvedControl {
    let Value::Object(mut map) = body else {
        panic!("{name}: not an object");
    };
    let control_type = map
        .remove("type")
        .and_then(|kind| kind.as_str().map(str::to_owned));
    let children = match map.remove("controls") {
        Some(Value::Array(items)) => items
            .into_iter()
            .map(|item| {
                let Value::Object(entry) = item else {
                    panic!("{name}: child not an object");
                };
                let (child, body) = entry.into_iter().next().expect("named child");
                control(&child, body)
            })
            .collect(),
        _ => Vec::new(),
    };
    ResolvedControl {
        name: name.to_owned(),
        control_type,
        base: None,
        unresolved_base: None,
        properties: map.into_iter().collect::<BTreeMap<_, _>>().into(),
        children,
        factory: None,
    }
}

/// `controls` under a 100x100 top-left root.
fn screen(controls: Value) -> ResolvedControl {
    control(
        "root",
        json!({
            "type": "panel",
            "size": [100, 100],
            "anchor_from": "top_left",
            "anchor_to": "top_left",
            "controls": controls,
        }),
    )
}

fn find<'a>(node: &'a LaidOut<'a>, name: &str) -> &'a LaidOut<'a> {
    if node.control.name == name {
        return node;
    }
    node.children
        .iter()
        .find_map(|child| {
            let found = find(child, name);
            (found.control.name == name).then_some(found)
        })
        .unwrap_or(node)
}

/// `[x, y, w, h]` of the control named `name`.
fn rect(root: &ResolvedControl, name: &str) -> [f64; 4] {
    let laid = layout(root, [100.0, 100.0], &env());
    let node = find(&laid, name);
    assert_eq!(node.control.name, name, "{name} not laid out");
    [node.rect.x, node.rect.y, node.rect.w, node.rect.h]
}

fn size(root: &ResolvedControl, name: &str) -> [f64; 2] {
    let [_, _, w, h] = rect(root, name);
    [w, h]
}

fn top_left(body: Value) -> Value {
    let mut body = body;
    body["anchor_from"] = json!("top_left");
    body["anchor_to"] = json!("top_left");
    body
}

// G03: upper-case units and a repeated sign parse as the client normalizes them.
#[test]
fn g03_normalized_expressions() {
    let root = screen(json!([
        { "a": { "type": "panel", "size": ["20PX", 10] } },
        { "b": { "type": "panel", "size": ["100% + -4px", 10] } },
    ]));
    assert_eq!(size(&root, "a"), [20.0, 10.0]);
    assert_eq!(size(&root, "b"), [96.0, 10.0]);
}

// G05: an ordinary panel's `%c` sums its children.
#[test]
fn g05_percent_children_sums() {
    let root = screen(json!([{ "p": {
        "type": "panel", "size": ["100%c", 10],
        "controls": [
            { "a": { "type": "panel", "size": [20, 10] } },
            { "b": { "type": "panel", "size": [30, 10] } },
        ],
    } }]));
    assert_eq!(size(&root, "p"), [50.0, 10.0]);
}

// G06: `%cm` is the largest child whatever its coefficient.
#[test]
fn g06_child_max_ignores_its_coefficient() {
    let root = screen(json!([{ "p": {
        "type": "panel", "size": ["50%cm", 10],
        "controls": [{ "c": { "type": "panel", "size": [40, 10] } }],
    } }]));
    assert_eq!(size(&root, "p"), [40.0, 10.0]);
}

// G07: `%sm` reads a sibling's resolved size, coefficient ignored.
#[test]
fn g07_sibling_max_reads_resolved_siblings() {
    let root = screen(json!([{ "p": {
        "type": "panel", "size": [100, 20],
        "controls": [
            { "a": { "type": "panel", "size": ["50%", 10] } },
            { "b": { "type": "panel", "size": ["50%sm", 10] } },
        ],
    } }]));
    assert_eq!(size(&root, "b"), [50.0, 10.0]);
}

// G08: a height from the width reads the width after its bounds.
#[test]
fn g08_own_width_is_read_after_clamping() {
    let root = screen(json!([{ "p": {
        "type": "panel", "size": [100, "100%x"], "max_size": [50, 200],
    } }]));
    assert_eq!(size(&root, "p"), [50.0, 50.0]);
}

// G09: a width from the height resolves after the height.
#[test]
fn g09_own_height_resolves_first() {
    let root = screen(json!([{ "p": { "type": "panel", "size": ["100%y", 40] } }]));
    assert_eq!(size(&root, "p"), [40.0, 40.0]);
}

// G11: a `default` offset is no offset.
#[test]
fn g11_default_offset_is_none() {
    let root = screen(json!([{ "p": top_left(json!({
        "type": "panel", "size": [20, 10], "offset": ["default", 0],
    })) }]));
    assert_eq!(rect(&root, "p")[0], 0.0);
}

// G12: `default`/`fill` bounds install no rule.
#[test]
fn g12_keyword_bounds_are_unbounded() {
    let root = screen(json!([
        { "a": { "type": "panel", "size": [20, 10], "min_size": ["fill", 0] } },
        { "b": { "type": "panel", "size": [20, 10], "min_size": ["default", 0] } },
        { "c": { "type": "panel", "size": [120, 10], "max_size": ["default", 100] } },
    ]));
    assert_eq!(size(&root, "a")[0], 20.0);
    assert_eq!(size(&root, "b")[0], 20.0);
    assert_eq!(size(&root, "c")[0], 120.0);
}

// G15: bounds read the control's own other axis, its siblings and its children.
#[test]
fn g15_bounds_read_their_full_context() {
    let root = screen(json!([
        { "a": { "type": "panel", "size": [80, 1], "min_size": [0, "50%x"] } },
        { "b": { "type": "panel", "size": [80, 20], "max_size": ["50%y", 100] } },
        { "tall": { "type": "panel", "size": [10, 80] } },
        { "c": { "type": "panel", "size": [10, 20], "min_size": [0, "100%sm"] } },
        { "d": {
            "type": "stack_panel", "orientation": "vertical", "size": [20, 10],
            "min_size": [0, "100%cm"],
            "controls": [
                { "x": { "type": "panel", "size": [20, 20] } },
                { "y": { "type": "panel", "size": [20, 30] } },
            ],
        } },
    ]));
    assert_eq!(size(&root, "a")[1], 40.0);
    assert_eq!(size(&root, "b")[0], 10.0);
    assert_eq!(size(&root, "c")[1], 80.0);
    assert_eq!(size(&root, "d")[1], 30.0);
}

// A childless control's `%c` bound builds no rule, so a label keeps its text.
#[test]
fn a_childless_percent_children_bound_is_no_rule() {
    let root = screen(json!([{ "l": {
        "type": "label", "text": "hello", "size": ["default", 10], "max_size": ["100%c", 10],
    } }]));
    assert_eq!(size(&root, "l"), [30.0, 10.0]);
}

// When the minimum exceeds the maximum, an over-large value takes the maximum.
#[test]
fn an_overflowing_value_takes_the_maximum_over_a_larger_minimum() {
    let root = screen(json!([
        { "a": { "type": "panel", "size": [90, 10], "min_size": [60, 0], "max_size": [40, 100] } },
        { "b": { "type": "panel", "size": [20, 10], "min_size": [60, 0], "max_size": [40, 100] } },
    ]));
    assert_eq!(size(&root, "a")[0], 40.0);
    assert_eq!(size(&root, "b")[0], 60.0);
}

// `fill` off a stack's main axis is an empty rule.
#[test]
fn fill_outside_a_stack_is_zero() {
    let root = screen(json!([{ "p": { "type": "panel", "size": ["fill", 10] } }]));
    assert_eq!(size(&root, "p"), [0.0, 10.0]);
}

// G16: offsets read own, children-max and sibling units.
#[test]
fn g16_offsets_read_their_full_context() {
    let root = screen(json!([
        { "a": top_left(json!({ "type": "panel", "size": [20, 10], "offset": ["50%x", 0] })) },
        { "b": top_left(json!({
            "type": "panel", "size": [50, 20], "offset": ["100%cm", 0],
            "controls": [{ "c": { "type": "panel", "size": [40, 10] } }],
        })) },
    ]));
    assert_eq!(rect(&root, "a")[0], 10.0);
    assert_eq!(rect(&root, "b")[0], 40.0);
}

// G17: an inheriting control takes its largest resolved sibling.
#[test]
fn g17_inherit_max_sibling_width_reads_resolved_siblings() {
    let root = screen(json!([{ "p": {
        "type": "panel", "size": [100, 20],
        "controls": [
            { "a": { "type": "panel", "size": ["50%", 10] } },
            { "b": { "type": "panel", "size": [20, 10], "inherit_max_sibling_width": true } },
        ],
    } }]));
    assert_eq!(size(&root, "b"), [50.0, 10.0]);
}

// G18: a ratio-scaled image derives its default axis from the other.
#[test]
fn g18_default_size_scales_to_ratio() {
    let root = screen(json!([
        { "a": { "type": "image", "texture": "t", "size": ["default", 10],
                 "default_size_scales_to_ratio": true } },
        { "b": { "type": "image", "texture": "t", "size": [80, "default"],
                 "default_size_scales_to_ratio": true } },
        { "c": { "type": "image", "texture": "t", "size": ["default", "default"],
                 "default_size_scales_to_ratio": true } },
        { "d": { "type": "image", "texture": "t", "size": ["default", 10] } },
    ]));
    assert_eq!(size(&root, "a"), [20.0, 10.0]);
    assert_eq!(size(&root, "b"), [80.0, 40.0]);
    assert_eq!(size(&root, "c"), [40.0, 20.0]);
    assert_eq!(size(&root, "d"), [100.0, 10.0]);
}

fn stack(orientation: &str, extra: Value, controls: Value) -> Value {
    let mut body = json!({
        "type": "stack_panel", "orientation": orientation, "size": [100, 80],
        "controls": controls,
    });
    for (key, value) in extra.as_object().unwrap() {
        body[key] = value.clone();
    }
    top_left(body)
}

// S02: orientation `none` chains both axes.
#[test]
fn s02_orientation_none_chains_both_axes() {
    let root = screen(json!([{ "s": stack("none", json!({}), json!([
        { "a": { "type": "panel", "size": [20, 10] } },
        { "b": { "type": "panel", "size": [30, 10] } },
    ])) }]));
    assert_eq!(rect(&root, "a"), [0.0, 0.0, 20.0, 10.0]);
    assert_eq!(rect(&root, "b"), [20.0, 10.0, 30.0, 10.0]);
}

// S03: stack children obey their bounds, `fill` included.
#[test]
fn s03_stack_children_are_bounded() {
    let root = screen(
        json!([{ "s": stack("horizontal", json!({ "size": [100, 20] }), json!([
        { "a": { "type": "panel", "size": [80, 10], "max_size": [20, 20] } },
        { "b": { "type": "panel", "size": [10, 10] } },
        { "f": { "type": "panel", "size": ["fill", 10], "max_size": [30, 10] } },
    ])) }]),
    );
    assert_eq!(rect(&root, "a")[2], 20.0);
    assert_eq!(rect(&root, "b")[0], 20.0);
    assert_eq!(rect(&root, "f")[2], 30.0);
}

// S04: child anchors apply only with `use_child_anchors`.
#[test]
fn s04_use_child_anchors() {
    let child = json!([{ "c": {
        "type": "panel", "size": [20, 10], "anchor_from": "bottom_right", "anchor_to": "top_left",
    } }]);
    let off = screen(json!([{ "s": stack("vertical", json!({}), child.clone()) }]));
    let on =
        screen(json!([{ "s": stack("vertical", json!({ "use_child_anchors": true }), child) }]));
    assert_eq!(rect(&off, "c")[..2], [0.0, 0.0]);
    assert_eq!(rect(&on, "c")[..2], [100.0, 10.0]);
}

// Stack children ignore `offset`.
#[test]
fn stack_children_ignore_offset() {
    let root = screen(json!([{ "s": stack("vertical", json!({}), json!([
        { "a": { "type": "panel", "size": [20, 10], "offset": [5, 5] } },
    ])) }]));
    assert_eq!(rect(&root, "a")[..2], [0.0, 0.0]);
}

// S05: `use_priority` hides the lowest priority that overflows.
#[test]
fn s05_priority_hides_the_overflow() {
    let root = screen(json!([{ "s": stack("horizontal",
        json!({ "size": [80, 20], "use_priority": true }), json!([
        { "a": { "type": "panel", "size": [60, 20], "priority": 1 } },
        { "b": { "type": "panel", "size": [60, 20], "priority": 2 } },
    ])) }]));
    let laid = layout(&root, [100.0, 100.0], &env());
    assert!(find(&laid, "a").visible);
    assert!(!find(&laid, "b").visible);
}

// S06: a hidden stack child adds no main-axis space.
#[test]
fn s06_hidden_children_collapse() {
    let root = screen(json!([{ "s": stack("vertical", json!({}), json!([
        { "a": { "type": "panel", "size": [20, 10], "visible": false } },
        { "b": { "type": "panel", "size": [20, 10] } },
    ])) }]));
    assert_eq!(rect(&root, "b")[1], 0.0);
}

fn template_grid(extra: Value, cells: usize, template: Value) -> Value {
    let mut controls: Vec<Value> = (0..cells)
        .map(|index| json!({ "cell": { "type": "panel", "size": template["size"], "collection_index": index } }))
        .collect();
    let mut node = template;
    node["grid_template_node"] = json!(true);
    controls.push(json!({ "template": node }));
    let mut body = json!({
        "type": "grid", "grid_item_template": "t.cell", "controls": controls,
    });
    for (key, value) in extra.as_object().unwrap() {
        body[key] = value.clone();
    }
    top_left(body)
}

fn cells(root: &ResolvedControl) -> Vec<[f64; 4]> {
    let laid = layout(root, [100.0, 100.0], &env());
    let grid = find(&laid, "g");
    grid.children
        .iter()
        .map(|cell| [cell.rect.x, cell.rect.y, cell.rect.w, cell.rect.h])
        .collect()
}

// D01: a templated grid places no cell past its capacity.
#[test]
fn d01_capacity_limits_cells() {
    let root = screen(json!([{ "g": template_grid(
        json!({ "size": [20, 20], "grid_dimensions": [1, 1] }), 2,
        json!({ "type": "panel", "size": [20, 10] })) }]));
    assert_eq!(cells(&root).len(), 1);
}

// D02: a templated grid measures dimensions × template.
#[test]
fn d02_template_geometry_sizes_the_grid() {
    let root = screen(json!([{ "g": template_grid(
        json!({ "size": ["100%c", "100%c"], "grid_dimensions": [2, 2] }), 0,
        json!({ "type": "panel", "size": [20, 10] })) }]));
    assert_eq!(size(&root, "g"), [40.0, 20.0]);
}

// D04: a horizontally rescaling grid fits whole columns, centring the leftover.
#[test]
fn d04_horizontal_rescaling() {
    let root = screen(json!([{ "g": template_grid(
        json!({ "size": [90, "default"], "grid_rescaling_type": "horizontal",
                "maximum_grid_items": 6 }), 6,
        json!({ "type": "panel", "size": [20, 10] })) }]));
    let placed = cells(&root);
    assert_eq!(size(&root, "g"), [90.0, 20.0]);
    assert_eq!(placed.len(), 6);
    // A bound capacity arrives as a whole float.
    let bound = screen(json!([{ "g": template_grid(
        json!({ "size": [90, "default"], "grid_rescaling_type": "horizontal",
                "#maximum_grid_items": 6.0 }), 6,
        json!({ "type": "panel", "size": [20, 10] })) }]));
    assert_eq!(size(&bound, "g"), [90.0, 20.0]);
    assert_eq!(placed[0], [5.0, 0.0, 20.0, 10.0]);
    assert_eq!(placed[4], [5.0, 10.0, 20.0, 10.0]);
}

// D05: a vertical fill direction makes one column of whole rows.
#[test]
fn d05_fill_direction() {
    let root = screen(json!([{ "g": template_grid(
        json!({ "size": [105, 85], "grid_fill_direction": "vertical" }), 8,
        json!({ "type": "panel", "size": ["100%", 15] })) }]));
    let placed = cells(&root);
    assert_eq!(placed.len(), 5);
    assert_eq!(placed[1][..2], [0.0, 17.0]);
}

// D08: listed cells sit at their `grid_position`, dividing the grid evenly.
#[test]
fn d08_listed_cells_use_grid_position() {
    let root = screen(json!([{ "g": top_left(json!({
        "type": "grid", "size": [40, 40], "grid_dimensions": [2, 2],
        "controls": [
            { "a": { "type": "panel", "grid_position": [1, 1] } },
            { "b": { "type": "panel", "size": [5, 5] } },
        ],
    })) }]));
    assert_eq!(rect(&root, "a"), [20.0, 20.0, 20.0, 20.0]);
    assert_eq!(rect(&root, "b"), [0.0, 0.0, 5.0, 5.0]);
}

/// A library holding `t.cell`, a 20x10 panel.
struct CellLibrary;
impl ControlLibrary for CellLibrary {
    fn resolve(&self, reference: &ControlRef) -> Option<ResolvedControl> {
        (reference.name == "cell")
            .then(|| control("cell", json!({ "type": "panel", "size": [20, 10] })))
    }
}

fn bound_grid(body: Value, data: &DataSource) -> ResolvedControl {
    bind(&control("g", body), data, &CellLibrary)
}

fn instances(grid: &ResolvedControl) -> usize {
    grid.children
        .iter()
        .filter(|child| !child.properties.contains_key("grid_template_node"))
        .count()
}

fn items(count: usize) -> DataSource {
    let mut data = DataSource::new();
    data.set_collection(
        "items",
        (0..count)
            .map(|_| json_ui::CollectionItem::new("item"))
            .collect(),
    );
    data
}

// D02: the template is created and kept without a collection.
#[test]
fn d02_template_is_kept_without_a_collection() {
    let grid = bound_grid(
        json!({ "type": "grid", "grid_dimensions": [2, 2], "grid_item_template": "t.cell" }),
        &DataSource::new(),
    );
    assert_eq!(instances(&grid), 4);
    assert!(
        grid.children
            .iter()
            .any(|child| child.properties.contains_key("grid_template_node"))
    );
}

// D03: an unanswered dimension binding keeps zero dimensions.
#[test]
fn d03_unresolved_dimension_binding_creates_nothing() {
    let grid = bound_grid(
        json!({ "type": "grid", "grid_dimension_binding": "#missing",
                "grid_item_template": "t.cell", "collection_name": "items" }),
        &items(3),
    );
    assert_eq!(instances(&grid), 0);
}

// D06: a fixed grid holds columns × rows; a rescaling one `maximum_grid_items`.
#[test]
fn d06_capacity_follows_the_grid_mode() {
    let fixed = bound_grid(
        json!({ "type": "grid", "grid_dimensions": [2, 2], "grid_rescaling_type": "none",
                "maximum_grid_items": 1, "grid_item_template": "t.cell", "collection_name": "items" }),
        &items(6),
    );
    let rescaling = bound_grid(
        json!({ "type": "grid", "grid_dimensions": [2, 2], "grid_rescaling_type": "horizontal",
                "maximum_grid_items": 6, "grid_item_template": "t.cell", "collection_name": "items" }),
        &items(6),
    );
    let unset = bound_grid(
        json!({ "type": "grid", "grid_rescaling_type": "horizontal",
                "grid_item_template": "t.cell", "collection_name": "items" }),
        &items(3),
    );
    assert_eq!(instances(&fixed), 4);
    assert_eq!(instances(&rescaling), 6);
    assert_eq!(instances(&unset), 0);
}

// D07: a bound `#maximum_grid_items` sets the rescaling capacity.
#[test]
fn d07_bound_maximum_grid_items() {
    let mut data = items(3);
    data.set_global("#limit", Scalar::Num(0.0));
    let grid = bound_grid(
        json!({ "type": "grid", "grid_rescaling_type": "horizontal", "maximum_grid_items": 3,
                "grid_item_template": "t.cell", "collection_name": "items",
                "bindings": [{ "binding_type": "global", "binding_name": "#limit",
                               "binding_name_override": "#maximum_grid_items" }] }),
        &data,
    );
    assert_eq!(instances(&grid), 0);
}

// D07: vanilla's container grid binds its collection's size as the capacity.
#[test]
fn d07_collection_total_items_sets_the_capacity() {
    let grid = bound_grid(
        json!({ "type": "grid", "grid_rescaling_type": "horizontal",
                "grid_item_template": "t.cell", "collection_name": "items",
                "bindings": [{ "binding_type": "collection", "binding_collection_name": "items",
                               "binding_name": "#collection_total_items",
                               "binding_name_override": "#maximum_grid_items" }] }),
        &items(5),
    );
    assert_eq!(instances(&grid), 5);
}

// D10: the grid reports its capacity as `#grid_number_size`.
#[test]
fn d10_grid_number_size() {
    let grid = bound_grid(
        json!({ "type": "grid", "grid_dimensions": [2, 2], "grid_item_template": "t.cell",
                "collection_name": "items", "text": "#grid_number_size" }),
        &items(1),
    );
    assert_eq!(
        grid.properties.get("text").and_then(Value::as_f64),
        Some(4.0)
    );
}

/// A vanilla-shaped scroll view: a `fill` viewport beside an 8-wide bar panel
/// holding the track and a vertical box. `content` is the content's body.
fn scroll_view(extra: Value, content: Value, bar_first: bool) -> ResolvedControl {
    let viewport = json!({ "area": {
        "type": "panel", "size": ["fill", "100%"],
        "controls": [{ "port": top_left(json!({
            "type": "panel", "size": ["100%", "100%"], "clips_children": true,
            "controls": [{ "content": top_left(content) }],
        })) }],
    } });
    let bar = json!({ "bar": {
        "type": "panel", "size": [8, "100%"],
        "controls": [{ "track": {
            "type": "scroll_track", "size": [4, "100%"],
            "controls": [{ "box": top_left(json!({
                "type": "scrollbar_box", "size": ["100%", "100%"], "draggable": "vertical",
            })) }],
        } }],
    } });
    let items = if bar_first {
        json!([bar, viewport])
    } else {
        json!([viewport, bar])
    };
    let mut body = top_left(json!({
        "type": "scroll_view", "size": [100, 100],
        "scroll_view_port": "port", "scroll_content": "content", "scrollbar_track": "track",
        "scrollbar_box": "box", "scroll_box_and_track_panel": "bar", "scroll_speed": 15,
        "controls": [{ "stack": top_left(json!({
            "type": "stack_panel", "orientation": "horizontal", "size": ["100%", "100%"],
            "controls": items,
        })) }],
    }));
    for (key, value) in extra.as_object().unwrap() {
        body[key] = value.clone();
    }
    screen(json!([{ "view": body }]))
}

fn scrolled(
    root: &ResolvedControl,
    state: &json_ui::ViewState,
) -> (json_ui::ScrollMetrics, Vec<(String, [f64; 4], bool)>) {
    let (laid, report) = json_ui::layout_with(root, [100.0, 100.0], &env(), state);
    let mut nodes = Vec::new();
    fn walk(node: &LaidOut, out: &mut Vec<(String, [f64; 4], bool)>) {
        out.push((
            node.control.name.clone(),
            [node.rect.x, node.rect.y, node.rect.w, node.rect.h],
            node.visible,
        ));
        for child in &node.children {
            walk(child, out);
        }
    }
    walk(&laid, &mut nodes);
    (report.scrolls["/root/view"].clone(), nodes)
}

fn node<'a>(nodes: &'a [(String, [f64; 4], bool)], name: &str) -> &'a (String, [f64; 4], bool) {
    nodes
        .iter()
        .find(|(named, _, _)| named == name)
        .unwrap_or_else(|| panic!("no {name}"))
}

fn at_offset(offset: f64) -> json_ui::ViewState {
    json_ui::ViewState {
        scroll: [("/root/view".to_owned(), offset)].into_iter().collect(),
        ..json_ui::ViewState::default()
    }
}

// V02/V21: the named viewport sets the range; the thumb sizes before or after the content.
#[test]
fn v02_v21_named_roles_in_any_order() {
    for bar_first in [false, true] {
        let root = scroll_view(
            json!({}),
            json!({ "type": "panel", "size": ["100%", 250] }),
            bar_first,
        );
        let (metrics, nodes) = scrolled(&root, &at_offset(1000.0));
        assert_eq!(metrics.max_offset(), 150.0);
        assert_eq!(node(&nodes, "content").1[1], -150.0);
        assert_eq!(metrics.thumb.map(|thumb| thumb[3]), Some(40.0));
    }
}

// V07: the thumb is at least a tenth of the track.
#[test]
fn v07_thumb_minimum_is_a_tenth_of_the_track() {
    let root = scroll_view(
        json!({}),
        json!({ "type": "panel", "size": ["100%", 10000] }),
        false,
    );
    let (metrics, _) = scrolled(&root, &at_offset(0.0));
    assert_eq!(metrics.thumb.map(|thumb| thumb[3]), Some(10.0));
}

// V11: a track click jumps to the clicked fraction less half a viewport.
#[test]
fn v11_track_click_targets_the_clicked_fraction() {
    let root = scroll_view(
        json!({}),
        json!({ "type": "panel", "size": ["100%", 1000] }),
        false,
    );
    let (metrics, _) = scrolled(&root, &at_offset(0.0));
    assert_eq!(metrics.offset_for_track([94.0, 75.0]), 700.0);
}

// V06/V17: content that fits hides the bar panel and the `fill` viewport takes its space.
#[test]
fn v06_v17_fitting_content_hides_the_bar_panel() {
    let root = scroll_view(
        json!({}),
        json!({ "type": "panel", "size": ["100%", 50] }),
        false,
    );
    let (metrics, nodes) = scrolled(&root, &at_offset(0.0));
    assert!(!node(&nodes, "bar").2);
    assert_eq!(metrics.bar_visible, Some(false));
    assert_eq!(node(&nodes, "content").1[2], 100.0);
    let always = scroll_view(
        json!({ "scrollbar_always_visible": true }),
        json!({ "type": "panel", "size": ["100%", 50] }),
        false,
    );
    let (_, nodes) = scrolled(&always, &at_offset(0.0));
    assert!(node(&nodes, "bar").2);
    assert_eq!(node(&nodes, "content").1[2], 92.0);
}

// V08: a horizontally draggable box scrolls the content along x (`_updateScroll`
// reads the box's `draggable`).
#[test]
fn v08_horizontal_scrolling() {
    let mut root = scroll_view(
        json!({}),
        json!({ "type": "panel", "size": [200, 20] }),
        false,
    );
    let bar_box = &mut root.children[0].children[0].children[1].children[0].children[0];
    bar_box
        .properties
        .insert("draggable".into(), json!("horizontal"));
    let (metrics, nodes) = scrolled(&root, &at_offset(60.0));
    assert!(metrics.horizontal);
    assert_eq!(metrics.max_offset(), 108.0);
    assert_eq!(node(&nodes, "content").1[..2], [-60.0, 0.0]);
}

// V18: `jump_to_bottom_on_update` jumps whenever the maximum changes.
#[test]
fn v18_jump_to_bottom_when_the_maximum_changes() {
    let root = scroll_view(
        json!({ "jump_to_bottom_on_update": true }),
        json!({ "type": "panel", "size": ["100%", 300] }),
        false,
    );
    let mut state = at_offset(20.0);
    let extent = |max| json_ui::ScrollRetained {
        extent: Some(max),
        ..Default::default()
    };
    state
        .scroll_state
        .insert("/root/view".to_owned(), extent(100.0));
    assert_eq!(scrolled(&root, &state).0.offset, 200.0);
    state
        .scroll_state
        .insert("/root/view".to_owned(), extent(200.0));
    assert_eq!(scrolled(&root, &state).0.offset, 20.0);
}

// V19: a true `#force_scroll_to_end` holds the view at its end.
#[test]
fn v19_force_scroll_to_end() {
    let root = scroll_view(
        json!({ "#force_scroll_to_end": true }),
        json!({ "type": "panel", "size": ["100%", 300] }),
        false,
    );
    assert_eq!(scrolled(&root, &at_offset(0.0)).0.offset, 200.0);
}

// V20: the view reports its end, bottom and bar states.
#[test]
fn v20_scroll_feedback() {
    let root = scroll_view(
        json!({}),
        json!({ "type": "panel", "size": ["100%", 300] }),
        false,
    );
    let (middle, _) = scrolled(&root, &at_offset(100.0));
    assert!(!middle.scrolled_to_end && !middle.hit_bottom && middle.bar_visible == Some(true));
    let (end, _) = scrolled(&root, &at_offset(200.0));
    assert!(end.scrolled_to_end && end.hit_bottom);
}

// V09/V13: wheel steps move by the speed with the client's byte scaling.
#[test]
fn v09_wheel_steps_scale_by_speed() {
    let root = scroll_view(
        json!({}),
        json!({ "type": "panel", "size": ["100%", 1000] }),
        false,
    );
    let (metrics, _) = scrolled(&root, &at_offset(100.0));
    assert!((metrics.offset_for_wheel(-1.0) - (100.0 + 15.0 * 120.0 / 128.0)).abs() < 1e-9);
    assert!((metrics.offset_for_wheel(1.0) - (100.0 - 15.0 * 120.0 / 127.0)).abs() < 1e-9);
}

fn laid_node<'a>(laid: &'a LaidOut<'a>, name: &str) -> &'a LaidOut<'a> {
    let found = find(laid, name);
    assert_eq!(found.control.name, name, "{name} not laid out");
    found
}

// P02: `clip_offset` insets the clip a clipping control gives its children.
#[test]
fn p02_clip_offset_insets_the_child_clip() {
    let root = screen(json!([{ "p": top_left(json!({
        "type": "panel", "size": [100, 100], "clips_children": true, "clip_offset": [5, 5],
        "controls": [{ "c": { "type": "image", "size": [100, 100], "texture": "t" } }],
    })) }]));
    let laid = layout(&root, [100.0, 100.0], &env());
    let clip = laid_node(&laid, "c").clip;
    assert_eq!([clip.x, clip.y, clip.w, clip.h], [5.0, 5.0, 90.0, 90.0]);
}

// P03: `allow_clipping: false` draws outside the ancestor clip; children inherit it.
#[test]
fn p03_allow_clipping_opts_out_of_the_ancestor_clip() {
    let root = screen(json!([{ "p": top_left(json!({
        "type": "panel", "size": [20, 20], "clips_children": true,
        "controls": [{ "c": top_left(json!({
            "type": "image", "size": [40, 40], "texture": "t", "allow_clipping": false,
            "controls": [{ "g": { "type": "image", "size": [40, 40], "texture": "t" } }],
        })) }],
    })) }]));
    let laid = layout(&root, [100.0, 100.0], &env());
    let free = laid_node(&laid, "c").clip;
    assert_eq!([free.w, free.h], [100.0, 100.0]);
    let inherited = laid_node(&laid, "g").clip;
    assert_eq!([inherited.w, inherited.h], [100.0, 100.0]);
}

// P05: a control with `clip_state_change_event` reports when it is wholly clipped.
#[test]
fn p05_clip_state_is_reported() {
    let root = screen(json!([{ "p": top_left(json!({
        "type": "panel", "size": [20, 20], "clips_children": true,
        "controls": [
            { "inside": top_left(json!({ "type": "panel", "size": [5, 5],
                "clip_state_change_event": "inside.changed" })) },
            { "outside": top_left(json!({ "type": "panel", "size": [5, 5], "offset": [40, 0],
                "clip_state_change_event": "outside.changed" })) },
        ],
    })) }]));
    let (_, report) = json_ui::layout_with(&root, [100.0, 100.0], &env(), &Default::default());
    assert_eq!(
        report.clip_states["/root/p/inside"],
        ("inside.changed".to_owned(), false)
    );
    assert_eq!(
        report.clip_states["/root/p/outside"],
        ("outside.changed".to_owned(), true)
    );
}

// P12: a disabled ancestor locks its descendants.
#[test]
fn p12_disabled_ancestors_lock_descendants() {
    let root = screen(json!([{ "p": {
        "type": "panel", "enabled": false,
        "controls": [{ "b": {
            "type": "button", "size": [20, 20], "enabled": true,
            "default_control": "d", "locked_control": "l",
            "controls": [
                { "d": { "type": "panel" } },
                { "l": { "type": "panel" } },
            ],
        } }],
    } }]));
    let laid = layout(&root, [100.0, 100.0], &env());
    assert!(!laid_node(&laid, "b").enabled);
    assert!(!laid_node(&laid, "d").visible);
    assert!(laid_node(&laid, "l").visible);
    let regions = json_ui::hit_regions(&laid);
    assert!(regions.iter().all(|region| !region.enabled));
}

// V12: the track's press routes by the configured `scrollbar_track_button`.
#[test]
fn v12_configured_button_names_route_presses() {
    let root = scroll_view(
        json!({}),
        json!({ "type": "panel", "size": ["100%", 300] }),
        false,
    );
    let (metrics, _) = scrolled(&root, &at_offset(0.0));
    assert_eq!(metrics.track_button, None, "no track button configured");
    let mut body = root.clone();
    let view = &mut body.children[0];
    view.properties
        .insert("scrollbar_track_button".into(), json!("button.skip"));
    let track = &mut view.children[0].children[1].children[0];
    track.properties.insert(
        "button_mappings".into(),
        json!([{ "from_button_id": "button.menu_select", "to_button_id": "button.skip",
                 "mapping_type": "pressed" }]),
    );
    let (metrics, _) = scrolled(&body, &at_offset(0.0));
    assert_eq!(metrics.track_button.as_deref(), Some("button.skip"));
}

// V15: a view that always handles scrolling takes the wheel outside its viewport.
#[test]
fn v15_always_handle_scrolling_routes_the_wheel() {
    let root = scroll_view(
        json!({ "always_handle_scrolling": true, "size": [50, 50] }),
        json!({ "type": "panel", "size": ["100%", 300] }),
        false,
    );
    let (laid, report) = json_ui::layout_with(&root, [100.0, 100.0], &env(), &Default::default());
    let regions = json_ui::hit_regions(&laid);
    assert!(json_ui::scroll_target(&regions, &report, [90.0, 90.0]).is_some());
}

// A02: an anchored offset measures the fraction in from the anchored edge.
#[test]
fn a02_anchored_offset() {
    let root = screen(json!([{ "p": {
        "type": "panel", "size": [20, 10], "anchor_from": "bottom_right", "anchor_to": "top_left",
        "use_anchored_offset": true, "property_bag": { "#anchored_offset_value_x": 0.25,
                                                       "#anchored_offset_value_y": 0.1 },
    } }]));
    assert_eq!(rect(&root, "p")[..2], [55.0, 80.0]);
    // A view-bound value reaches placement through the component property.
    let bound = bind(
        &screen(json!([{ "p": {
            "type": "panel", "size": [20, 10], "anchor_from": "bottom_right",
            "anchor_to": "top_left", "use_anchored_offset": true,
            "property_bag": { "#x": 0.25 },
            "bindings": [{ "binding_type": "view", "source_property_name": "#x",
                           "target_property_name": "#anchored_offset_value_x" }],
        } }])),
        &DataSource::new(),
        &EmptyLibrary,
    );
    assert_eq!(rect(&bound, "p")[..2], [55.0, 90.0]);
}

// A03/A04: cursor-following controls centre on, or sit beside, the pointer.
#[test]
fn a03_a04_follow_the_cursor() {
    let root = screen(json!([
        { "a": { "type": "panel", "size": [20, 10], "follows_cursor": true } },
        { "b": { "type": "panel", "size": [20, 10], "follows_cursor_inside_parent": true } },
    ]));
    let state = json_ui::ViewState {
        pointer: Some([90.0, 40.0]),
        ..Default::default()
    };
    let (laid, report) = json_ui::layout_with(&root, [100.0, 100.0], &env(), &state);
    let a = laid_node(&laid, "a").rect;
    let b = laid_node(&laid, "b").rect;
    assert!(report.tracks_pointer);
    assert_eq!([a.x, a.y], [80.0, 35.0]);
    assert_eq!([b.x, b.y], [60.0, 50.0]);
}

// A05/A06: a drag moves only along `draggable`, `contained` keeps it inside.
#[test]
fn a05_a06_drag_and_containment() {
    let root = screen(json!([
        { "free": top_left(json!({ "type": "panel", "size": [20, 10], "draggable": "horizontal" })) },
        { "kept": top_left(json!({ "type": "panel", "size": [20, 10], "draggable": "both",
                                   "contained": true })) },
    ]));
    let mut state = json_ui::ViewState::default();
    state.drags.insert("/root/free".into(), [30.0, 30.0]);
    state.drags.insert("/root/kept".into(), [300.0, 300.0]);
    let (laid, _) = json_ui::layout_with(&root, [100.0, 100.0], &env(), &state);
    let free = laid_node(&laid, "free").rect;
    let kept = laid_node(&laid, "kept").rect;
    assert_eq!([free.x, free.y], [30.0, 0.0]);
    assert_eq!([kept.x, kept.y], [80.0, 90.0]);
    let regions = json_ui::hit_regions(&laid);
    assert!(
        regions
            .iter()
            .any(|region| region.kind == json_ui::HitKind::Draggable)
    );
}

// G05: a button's `%c` counts only the state child it shows at rest.
#[test]
fn g05_state_children_hidden_at_rest_add_nothing() {
    let state = |name: &str| json!({ name: { "type": "panel", "size": [85, 25] } });
    let root = screen(json!([{ "b": {
        "type": "button", "size": ["100%c + 2px", 25],
        "default_control": "default", "hover_control": "hover",
        "pressed_control": "pressed", "locked_control": "locked",
        "controls": [state("default"), state("hover"), state("pressed"), state("locked")],
    } }]));
    assert_eq!(size(&root, "b"), [87.0, 25.0]);
}

#[test]
fn review_contained_drag_clamps_the_destination_in_parent_coordinates() {
    let root = screen(
        json!([{ "kept": top_left(json!({"type":"panel", "size":[20,10], "offset":[30,40], "draggable":"both", "contained":true})) }]),
    );
    let mut state = json_ui::ViewState::default();
    for (delta, expected) in [
        ([-10.0, -10.0], [20.0, 30.0]),
        ([-300.0, -300.0], [0.0, 0.0]),
        ([300.0, 300.0], [80.0, 90.0]),
    ] {
        state.drags.insert("/root/kept".into(), delta);
        let (laid, _) = json_ui::layout_with(&root, [100.0, 100.0], &env(), &state);
        let kept = laid_node(&laid, "kept").rect;
        assert_eq!([kept.x, kept.y], expected);
    }
}

#[test]
fn review_hidden_stack_anchors_do_not_shift_the_next_visible_child() {
    let root = screen(
        json!([{ "stack": top_left(json!({"type":"stack_panel", "size":[100,100], "use_child_anchors":true, "controls":[
        {"first":top_left(json!({"type":"panel", "size":[10,10]}))},
        {"hidden":{"type":"panel", "visible":false, "size":[20,20], "anchor_from":"bottom_left", "anchor_to":"top_left"}},
        {"last":top_left(json!({"type":"panel", "size":[10,10]}))}
    ]})) }]),
    );
    assert_eq!(rect(&root, "last")[1], 10.0);
}

#[test]
fn review_nonfinite_string_values_do_not_create_nonfinite_slider_geometry() {
    for value in ["NaN", "inf", "-inf"] {
        let root = screen(
            json!([{ "slider":top_left(json!({"type":"slider", "size":[100,20], "#slider_value":value, "slider_box_control":"box", "controls":[{"box":{"type":"panel", "size":[10,10]}}]})) }]),
        );
        assert!(
            rect(&root, "box")
                .iter()
                .all(|coordinate| coordinate.is_finite())
        );
    }
}

#[test]
fn review_sibling_max_uses_independent_dimensions_of_deferred_siblings() {
    let root = screen(json!([
        { "a": { "type": "panel", "size": [20, "100%sm"] } },
        { "b": { "type": "panel", "size": ["100%sm", 30] } }
    ]));
    assert_eq!(size(&root, "a"), [20.0, 30.0]);
    assert_eq!(size(&root, "b"), [20.0, 30.0]);
}

#[test]
fn review_vertical_grid_positions_count_only_admitted_cells() {
    let root = screen(json!([{ "g": top_left(json!({
        "type": "grid", "size": [40, 20], "grid_rescaling_type": "vertical",
        "grid_item_template": "t.cell", "maximum_grid_items": 4,
        "controls": (0..6).map(|i| json!({format!("cell{i}"): {
            "type": "panel", "size": [10, 10]
        }})).collect::<Vec<_>>()
    })) }]));
    for name in ["cell0", "cell1", "cell2", "cell3"] {
        let at = rect(&root, name);
        assert!(at[0] < 40.0, "{name}: {at:?}");
    }
    assert_eq!(rect(&root, "cell2")[1], 10.0);
}
