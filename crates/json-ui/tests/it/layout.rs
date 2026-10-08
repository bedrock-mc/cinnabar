//! Layout, nine-slice, and emit against synthetic trees and real vanilla templates.
//! The `.local` pack is gitignored, so tests that need it skip (not fail) when it is
//! absent; the synthetic tests always run and pin the deterministic layout maths.

use crate::support;

use std::collections::BTreeMap;
use std::path::PathBuf;

use json_ui::{
    Context, Draw, LaidOut, LayoutEnv, Rect, ResolvedControl, TextMeasure, TextureMeta,
    TextureSource, emit, hit_regions, layout, nine_slice, parse_texture_meta, resolve,
};
use serde_json::{Value, json};

// --- test backends ----------------------------------------------------------

struct ZeroText;
impl TextMeasure for ZeroText {
    fn extent(&self, _text: &str) -> [f64; 2] {
        [0.0, 0.0]
    }
}

/// Fixed-width font stub so a label has a deterministic, non-zero natural size.
struct MonoText;
impl TextMeasure for MonoText {
    fn extent(&self, text: &str) -> [f64; 2] {
        [text.chars().count() as f64 * 6.0, 10.0]
    }
}

/// Counts natural-size requests separately for unwrapped and each wrapped width.
#[derive(Default)]
struct CountingText(std::cell::RefCell<BTreeMap<Option<u64>, usize>>);

impl TextMeasure for CountingText {
    /// Measure one unwrapped line and count the request.
    fn extent(&self, text: &str) -> [f64; 2] {
        *self.0.borrow_mut().entry(None).or_default() += 1;
        [text.chars().count() as f64 * 6.0, 10.0]
    }

    /// Measure the same line at a known width and count that exact width.
    fn wrapped(&self, text: &str, width: f64) -> [f64; 2] {
        *self
            .0
            .borrow_mut()
            .entry(Some(width.to_bits()))
            .or_default() += 1;
        [(text.chars().count() as f64 * 6.0).min(width), 10.0]
    }
}

#[test]
fn natural_measurements_run_once_per_width_and_reset_for_each_layout() {
    let mut label = ctrl(
        "label",
        Some("label"),
        json!({"text": "hello", "size": ["100%", "default"]}),
        vec![],
    );
    let text = CountingText::default();
    let env = LayoutEnv {
        text: &text,
        textures: &NoTextures,
    };
    {
        let laid = layout(&label, [120.0, 80.0], &env);
        assert_eq!(laid.rect.w, 120.0);
        assert_eq!(laid.rect.h, 10.0);
    }
    let first = text.0.borrow().clone();
    assert!(!first.is_empty());
    assert!(first.values().all(|count| *count == 1), "{first:?}");
    label
        .properties
        .insert("text".into(), json!("changed text"));
    {
        let laid = layout(&label, [180.0, 80.0], &env);
        assert_eq!(laid.rect.w, 180.0);
        assert_eq!(laid.rect.h, 10.0);
    }
    assert!(
        !text.0.borrow().contains_key(&None),
        "an explicit width needs no unconstrained measure"
    );
    assert_eq!(text.0.borrow().get(&Some(180.0_f64.to_bits())), Some(&1));
    let scaled = CountingText::default();
    let new_env = LayoutEnv {
        text: &scaled,
        textures: &NoTextures,
    };
    layout(&label, [180.0, 80.0], &new_env);
    assert!(scaled.0.borrow().values().all(|count| *count == 1));
}

struct NoTextures;
impl TextureSource for NoTextures {
    fn texture(&self, _path: &str) -> Option<TextureMeta> {
        None
    }
}

struct MapTextures(BTreeMap<String, TextureMeta>);
impl TextureSource for MapTextures {
    fn texture(&self, path: &str) -> Option<TextureMeta> {
        self.0.get(path).copied()
    }
}

/// Reads real `textures/ui/<name>.json` sidecars from the pack root.
struct DirTextures(PathBuf);
impl TextureSource for DirTextures {
    fn texture(&self, path: &str) -> Option<TextureMeta> {
        let file = self.0.join(format!("{path}.json"));
        let text = std::fs::read_to_string(file).ok()?;
        let value: Value = serde_json::from_str(&text).ok()?;
        parse_texture_meta(&value)
    }
}

// --- builders ---------------------------------------------------------------

fn ctrl(
    name: &str,
    control_type: Option<&str>,
    props: Value,
    children: Vec<ResolvedControl>,
) -> ResolvedControl {
    let properties: BTreeMap<String, Value> = match props {
        Value::Object(map) => map.into_iter().collect(),
        _ => BTreeMap::new(),
    };
    ResolvedControl {
        name: name.to_owned(),
        control_type: control_type.map(str::to_owned),
        base: None,
        unresolved_base: None,
        properties: properties.into(),
        children,
        factory: None,
    }
}

fn child_named<'a>(root: &'a LaidOut<'a>, name: &str) -> &'a LaidOut<'a> {
    root.children
        .iter()
        .find(|child| child.control.name == name)
        .unwrap_or_else(|| panic!("missing child {name}"))
}

fn zero_env() -> LayoutEnv<'static> {
    LayoutEnv {
        text: &ZeroText,
        textures: &NoTextures,
    }
}

// --- anchors ----------------------------------------------------------------

/// A 10x10 child anchored `from == to` at each of the nine points lands on the
/// matching corner/edge/centre of a known 100x100 parent.
#[test]
fn nine_anchor_points_place_a_child() {
    let parent_size = json!([100, 100]);
    let cases = [
        ("top_left", [0.0, 0.0]),
        ("top_middle", [45.0, 0.0]),
        ("top_right", [90.0, 0.0]),
        ("left_middle", [0.0, 45.0]),
        ("center", [45.0, 45.0]),
        ("right_middle", [90.0, 45.0]),
        ("bottom_left", [0.0, 90.0]),
        ("bottom_middle", [45.0, 90.0]),
        ("bottom_right", [90.0, 90.0]),
    ];
    let env = zero_env();
    for (anchor, expected) in cases {
        let child = ctrl(
            "child",
            Some("panel"),
            json!({ "size": [10, 10], "anchor_from": anchor, "anchor_to": anchor }),
            vec![],
        );
        let root = ctrl(
            "root",
            Some("panel"),
            json!({ "size": parent_size, "anchor_from": "top_left", "anchor_to": "top_left" }),
            vec![child],
        );
        let placed = layout(&root, [100.0, 100.0], &env);
        let rect = child_named(&placed, "child").rect;
        assert_eq!([rect.x, rect.y], expected, "anchor {anchor}");
        assert_eq!([rect.w, rect.h], [10.0, 10.0], "anchor {anchor} size");
    }
}

/// `anchor_from` names the parent point, `anchor_to` the child point, as the
/// vanilla stack-progress arrows rely on: a child's top-left pinned to the
/// parent's bottom-right lands at the far corner.
#[test]
fn asymmetric_anchor_splits_parent_and_child_points() {
    let child = ctrl(
        "child",
        Some("panel"),
        json!({ "size": [10, 10], "anchor_from": "bottom_right", "anchor_to": "top_left" }),
        vec![],
    );
    let root = ctrl(
        "root",
        Some("panel"),
        json!({ "size": [100, 100], "anchor_from": "top_left", "anchor_to": "top_left" }),
        vec![child],
    );
    let placed = layout(&root, [100.0, 100.0], &zero_env());
    let rect = child_named(&placed, "child").rect;
    assert_eq!([rect.x, rect.y], [100.0, 100.0]);
}

/// `offset` shifts a child after anchoring, in parent-relative pixels.
#[test]
fn offset_shifts_after_anchoring() {
    let child = ctrl(
        "child",
        Some("panel"),
        json!({ "size": [10, 10], "anchor_from": "top_left", "anchor_to": "top_left", "offset": [5, 7] }),
        vec![],
    );
    let root = ctrl(
        "root",
        Some("panel"),
        json!({ "size": [100, 100], "anchor_from": "top_left", "anchor_to": "top_left" }),
        vec![child],
    );
    let placed = layout(&root, [100.0, 100.0], &zero_env());
    let rect = child_named(&placed, "child").rect;
    assert_eq!([rect.x, rect.y], [5.0, 7.0]);
}

// --- stack panels -----------------------------------------------------------

fn stack_root(orientation: &str, size: Value, children: Vec<ResolvedControl>) -> ResolvedControl {
    ctrl(
        "stack",
        Some("stack_panel"),
        json!({ "size": size, "orientation": orientation, "anchor_from": "top_left", "anchor_to": "top_left" }),
        children,
    )
}

fn item(name: &str, size: Value) -> ResolvedControl {
    ctrl(name, Some("panel"), json!({ "size": size }), vec![])
}

#[test]
fn vertical_stack_packs_children_end_to_end() {
    let root = stack_root(
        "vertical",
        json!([100, 100]),
        vec![
            item("a", json!(["100%", 20])),
            item("b", json!(["100%", 30])),
            item("c", json!(["100%", 10])),
        ],
    );
    let placed = layout(&root, [100.0, 100.0], &zero_env());
    let y = |name| child_named(&placed, name).rect.y;
    let h = |name| child_named(&placed, name).rect.h;
    assert_eq!((y("a"), h("a")), (0.0, 20.0));
    assert_eq!((y("b"), h("b")), (20.0, 30.0));
    assert_eq!((y("c"), h("c")), (50.0, 10.0));
}

#[test]
fn horizontal_stack_packs_along_x() {
    let root = stack_root(
        "horizontal",
        json!([100, 50]),
        vec![
            item("a", json!([20, "100%"])),
            item("b", json!([30, "100%"])),
        ],
    );
    let placed = layout(&root, [100.0, 50.0], &zero_env());
    assert_eq!(child_named(&placed, "a").rect.x, 0.0);
    assert_eq!(child_named(&placed, "b").rect.x, 20.0);
    assert_eq!(child_named(&placed, "b").rect.w, 30.0);
}

// A width reading its own height (`["100%y", "100%"]`) resolves after the height,
// in a panel, a vertical stack, and a parent's `%c`; it never collapses to zero.
#[test]
fn width_from_own_height_resolves_after_the_height() {
    let icon = || item("icon", json!(["100%y", "100%"]));
    let panel = ctrl(
        "root",
        Some("panel"),
        json!({ "size": [80, 32], "anchor_from": "top_left", "anchor_to": "top_left" }),
        vec![icon()],
    );
    let placed = layout(&panel, [200.0, 100.0], &zero_env());
    let rect = child_named(&placed, "icon").rect;
    assert_eq!([rect.x, rect.w, rect.h], [24.0, 32.0, 32.0]);

    let stack = stack_root(
        "vertical",
        json!([80, "100%c"]),
        vec![item("icon", json!(["50%y", 20]))],
    );
    let placed = layout(&stack, [200.0, 100.0], &zero_env());
    let rect = child_named(&placed, "icon").rect;
    assert_eq!([rect.w, rect.h], [10.0, 20.0]);

    let wrapper = ctrl(
        "wrapper",
        Some("panel"),
        json!({ "size": ["100%c", 16], "anchor_from": "top_left", "anchor_to": "top_left" }),
        vec![item("logo", json!(["200%y", "100%"]))],
    );
    let placed = layout(&wrapper, [200.0, 100.0], &zero_env());
    assert_eq!(placed.rect.w, 32.0);

    // A header logo: a `%c`-wide wrapper in a horizontal stack sizes to it.
    let logo_wrapper = ctrl(
        "wrapper",
        Some("panel"),
        json!({ "size": ["100%c", "100% - 8px"] }),
        vec![item("logo", json!(["230%y", "120%"]))],
    );
    let header = stack_root("horizontal", json!([200, 26]), vec![logo_wrapper]);
    let placed = layout(&header, [200.0, 100.0], &zero_env());
    let wrapper = child_named(&placed, "wrapper");
    assert!(
        (wrapper.rect.w - 2.3 * 1.2 * 18.0).abs() < 1e-9,
        "{:?}",
        wrapper.rect
    );
    assert!((child_named(wrapper, "logo").rect.w - wrapper.rect.w).abs() < 1e-9);
}

/// The pinned `common.squaring_panel` nests opposite own-axis maximums with
/// omitted sizes. Both defaults must resolve before their bounds read them.
#[test]
fn omitted_panel_sizes_resolve_cross_axis_bounds_without_collapsing_descendants() {
    let image = ctrl(
        "image",
        Some("image"),
        json!({"texture": "preview"}),
        vec![],
    );
    let button = ctrl(
        "button",
        Some("button"),
        json!({"size": ["75%", "75%"]}),
        vec![image],
    );
    let inner = ctrl(
        "inner",
        Some("panel"),
        json!({"max_size": ["100%", "100%x"]}),
        vec![button],
    );
    let outer = ctrl(
        "outer",
        Some("panel"),
        json!({"max_size": ["100%y", "100%"]}),
        vec![inner],
    );
    let root = ctrl("root", Some("panel"), json!({}), vec![outer]);
    for viewport in [[640.0_f64, 360.0], [360.0, 640.0], [360.0, 360.0]] {
        let side = viewport[0].min(viewport[1]);
        let env = zero_env();
        let placed = layout(&root, viewport, &env);
        let outer = child_named(&placed, "outer");
        let inner = child_named(outer, "inner");
        let button = child_named(inner, "button");
        let image = child_named(button, "image");
        assert_eq!([outer.rect.w, outer.rect.h], [side, viewport[1]]);
        assert_eq!([inner.rect.w, inner.rect.h], [side, side]);
        assert_eq!(
            [inner.rect.x, inner.rect.y],
            [(viewport[0] - side) * 0.5, (viewport[1] - side) * 0.5]
        );
        assert_eq!([button.rect.w, button.rect.h], [side * 0.75; 2]);
        assert_eq!(image.rect, button.rect);

        let hits = hit_regions(&placed);
        let hit = hits.iter().find(|hit| hit.name == "button").unwrap();
        assert_eq!(hit.rect, button.rect.into());
        assert!(hit.contains([
            button.rect.x + button.rect.w * 0.5,
            button.rect.y + button.rect.h * 0.5
        ]));
        let draws = emit(&placed, &env);
        let sprite = draws
            .iter()
            .find(|node| node.name == "image" && matches!(node.draw, Draw::Sprite { .. }))
            .expect("the nonzero descendant image must draw");
        assert_eq!(sprite.dest, image.rect.into());
    }
}

#[test]
fn fill_child_absorbs_leftover_main_axis() {
    let root = stack_root(
        "vertical",
        json!([100, 100]),
        vec![
            item("a", json!(["100%", 20])),
            item("b", json!(["100%", "fill"])),
            item("c", json!(["100%", 10])),
        ],
    );
    let placed = layout(&root, [100.0, 100.0], &zero_env());
    assert_eq!(child_named(&placed, "b").rect.h, 70.0);
    assert_eq!(child_named(&placed, "c").rect.y, 90.0);
}

/// `100%c` sizes a container to the sum of its children's extents.
#[test]
fn percent_children_measures_content_extent() {
    let panel = ctrl(
        "panel",
        Some("panel"),
        json!({ "size": ["100%c", "100%c"], "anchor_from": "top_left", "anchor_to": "top_left" }),
        vec![item("a", json!([40, 15])), item("b", json!([25, 30]))],
    );
    let root = ctrl(
        "root",
        Some("panel"),
        json!({ "size": [200, 200], "anchor_from": "top_left", "anchor_to": "top_left" }),
        vec![panel],
    );
    let placed = layout(&root, [200.0, 200.0], &zero_env());
    let rect = child_named(&placed, "panel").rect;
    assert_eq!([rect.w, rect.h], [65.0, 45.0]);
}

/// A label's `default` width comes from its measured text extent.
#[test]
fn label_default_width_is_text_extent() {
    let env = LayoutEnv {
        text: &MonoText,
        textures: &NoTextures,
    };
    let label = ctrl(
        "label",
        Some("label"),
        json!({ "size": ["default", 10], "text": "hello", "anchor_from": "top_left", "anchor_to": "top_left" }),
        vec![],
    );
    let root = ctrl(
        "root",
        Some("panel"),
        json!({ "size": [200, 50], "anchor_from": "top_left", "anchor_to": "top_left" }),
        vec![label],
    );
    let placed = layout(&root, [200.0, 50.0], &env);
    assert_eq!(child_named(&placed, "label").rect.w, 30.0);
}

// A form-fitting button: the panel sizes to a label capped at `100%`/`100%c`,
// which must not collapse it to zero before the panel's width is known.
#[test]
fn a_child_sized_panel_fits_a_label_capped_by_percent_bounds() {
    let env = LayoutEnv {
        text: &MonoText,
        textures: &NoTextures,
    };
    for max in [json!(["100%", 10]), json!(["100%c", 10])] {
        let label = ctrl(
            "label",
            Some("label"),
            json!({ "size": ["default", 10], "max_size": max, "text": "Dressing" }),
            vec![],
        );
        let panel = ctrl(
            "panel",
            Some("panel"),
            json!({ "size": ["100%c + 6px", 16], "anchor_from": "top_left", "anchor_to": "top_left" }),
            vec![label],
        );
        let root = ctrl(
            "root",
            Some("panel"),
            json!({ "size": [200, 50] }),
            vec![panel],
        );
        let placed = layout(&root, [200.0, 50.0], &env);
        assert_eq!(child_named(&placed, "panel").rect.w, 54.0, "{max}");
    }
}

// A stack child honours `max_size`: the start screen's signing-in label wraps
// inside 120px instead of running across the screen.
#[test]
fn a_stack_child_is_capped_by_its_max_size() {
    let env = LayoutEnv {
        text: &MonoText,
        textures: &NoTextures,
    };
    let label = ctrl(
        "signingin",
        Some("label"),
        json!({ "size": ["default", "100%"], "max_size": [120, "100%"],
                "text": "Signing in with your Microsoft account..." }),
        vec![],
    );
    let stack = ctrl(
        "stack",
        Some("stack_panel"),
        json!({ "orientation": "horizontal", "size": ["100%", 32] }),
        vec![label],
    );
    let root = ctrl(
        "root",
        Some("panel"),
        json!({ "size": [400, 50] }),
        vec![stack],
    );
    let placed = layout(&root, [400.0, 50.0], &env);
    let stack = child_named(&placed, "stack");
    assert_eq!(child_named(stack, "signingin").rect.w, 120.0);
}

// --- nine-slice -------------------------------------------------------------

#[test]
fn nine_slice_splits_asymmetric_sidecar_into_nine_quads() {
    let meta =
        parse_texture_meta(&json!({ "nineslice_size": [8, 23, 8, 8], "base_size": [18, 33] }))
            .unwrap();
    let quads = nine_slice(Rect::new(0.0, 0.0, 225.0, 200.0), &meta);
    assert_eq!(quads.len(), 9);

    // Top-left corner: native size, source top-left of the texture.
    let top_left = quads[0];
    assert_eq!(
        (
            top_left.dest.x,
            top_left.dest.y,
            top_left.dest.w,
            top_left.dest.h
        ),
        (0.0, 0.0, 8.0, 23.0)
    );
    assert_eq!((top_left.uv.u0, top_left.uv.v0), (0.0, 0.0));
    assert!((top_left.uv.u1 - 8.0 / 18.0).abs() < 1e-6);
    assert!((top_left.uv.v1 - 23.0 / 33.0).abs() < 1e-6);

    // Centre: stretched on both axes, sampling the 2x2 middle of the source.
    let centre = quads[4];
    assert_eq!(
        (centre.dest.x, centre.dest.y, centre.dest.w, centre.dest.h),
        (8.0, 23.0, 225.0 - 16.0, 200.0 - 31.0)
    );

    // Bottom-right corner: native size again, pinned to the far corner.
    let bottom_right = quads[8];
    assert_eq!(
        (
            bottom_right.dest.x,
            bottom_right.dest.y,
            bottom_right.dest.w,
            bottom_right.dest.h
        ),
        (217.0, 192.0, 8.0, 8.0)
    );
}

#[test]
fn zero_top_and_bottom_leave_only_the_stretched_middle_row() {
    // [left, top, right, bottom] = [1, 0, 7, 0]: only the middle row survives.
    let meta =
        parse_texture_meta(&json!({ "nineslice_size": [1, 0, 7, 0], "base_size": [10, 10] }))
            .unwrap();
    let quads = nine_slice(Rect::new(0.0, 0.0, 100.0, 100.0), &meta);
    assert_eq!(quads.len(), 3);
    assert!(quads.iter().all(|q| q.dest.y == 0.0 && q.dest.h == 100.0));
    let widths: Vec<f64> = quads.iter().map(|q| q.dest.w).collect();
    assert_eq!(widths, vec![1.0, 92.0, 7.0]);
}

#[test]
fn nine_slice_image_emits_nine_sprites() {
    let mut map = BTreeMap::new();
    map.insert(
        "textures/ui/panel".to_owned(),
        TextureMeta {
            base_size: [16.0, 16.0],
            pixels: [16.0, 16.0],
            nineslice: Some(json_ui::NineSlice {
                left: 4.0,
                top: 4.0,
                right: 4.0,
                bottom: 4.0,
            }),
        },
    );
    let env = LayoutEnv {
        text: &ZeroText,
        textures: &MapTextures(map),
    };
    let image = ctrl(
        "bg",
        Some("image"),
        json!({ "texture": "textures/ui/panel", "anchor_from": "top_left", "anchor_to": "top_left" }),
        vec![],
    );
    let root = ctrl(
        "root",
        Some("panel"),
        json!({ "size": [80, 60], "anchor_from": "top_left", "anchor_to": "top_left" }),
        vec![image],
    );
    let placed = layout(&root, [80.0, 60.0], &env);
    let sprites = emit(&placed, &env)
        .iter()
        .filter(|node| matches!(node.draw, Draw::Sprite { .. }))
        .count();
    assert_eq!(sprites, 9);
}

// A grid listing its cells splits its rect into equal cells, each child sized
// and offset within its own cell (the brewing stand's bottle row).
#[test]
fn listed_grid_cells_share_the_grid_rect() {
    let panel = |name: &str, position: [u64; 2], offset: [f64; 2]| {
        ctrl(
            name,
            Some("panel"),
            json!({ "grid_position": position }),
            vec![ctrl(
                "item",
                Some("panel"),
                json!({ "size": [18, 18], "offset": offset }),
                vec![],
            )],
        )
    };
    let grid = ctrl(
        "grid",
        Some("grid"),
        json!({ "size": [54, 18], "grid_dimensions": [3, 1] }),
        vec![
            panel("right", [2, 0], [5.0, -7.0]),
            panel("left", [0, 0], [-5.0, -7.0]),
        ],
    );
    let env = LayoutEnv {
        text: &ZeroText,
        textures: &NoTextures,
    };
    let laid = layout(&grid, [54.0, 18.0], &env);
    let rects: Vec<(f64, f64, f64)> = laid
        .children
        .iter()
        .map(|cell| (cell.rect.x, cell.rect.w, cell.children[0].rect.x))
        .collect();
    assert_eq!(rects, [(36.0, 18.0, 41.0), (0.0, 18.0, -5.0)]);
}

// A stack child that inherits the tallest sibling's height spans the row, so a
// toolbar anchored to its top sits above the panel (the furnace's toolbar).
#[test]
fn stack_children_inherit_the_largest_sibling_cross_size() {
    let stack = ctrl(
        "stack",
        Some("stack_panel"),
        json!({ "orientation": "horizontal", "size": ["100%c", "100%cm"] }),
        vec![
            ctrl(
                "panel",
                Some("panel"),
                json!({ "size": [176, 166] }),
                vec![],
            ),
            ctrl(
                "anchor",
                Some("panel"),
                json!({ "size": [0, 0], "inherit_max_sibling_height": true }),
                vec![],
            ),
        ],
    );
    let env = LayoutEnv {
        text: &ZeroText,
        textures: &NoTextures,
    };
    let laid = layout(&stack, [400.0, 300.0], &env);
    let anchor = &laid.children[1];
    assert_eq!((anchor.rect.y, anchor.rect.h), (laid.rect.y, 166.0));
}

// A button laid out once emits both its default and hover looks, gated: the
// hover state picks one by filtering, with no second layout.
#[test]
fn gated_emission_filters_state_children_by_interaction() {
    let look = |name: &str, color: &str| {
        ctrl(
            name,
            Some("image"),
            json!({ "size": [10, 10], "color": color, "texture": "textures/ui/x" }),
            vec![],
        )
    };
    let button = ctrl(
        "button",
        Some("button"),
        json!({
            "size": [10, 10],
            "default_control": "default",
            "hover_control": "hover",
            "pressed_control": "hover",
        }),
        vec![look("default", "red"), look("hover", "blue")],
    );
    let env = LayoutEnv {
        text: &ZeroText,
        textures: &NoTextures,
    };
    let laid = layout(&button, [10.0, 10.0], &env);
    let nodes = json_ui::emit_gated(&laid, &env);
    let shown = |state: &json_ui::ViewState| -> Vec<String> {
        nodes
            .iter()
            .filter(|node| node.shown(state))
            .map(|node| node.name.clone())
            .collect()
    };
    assert_eq!(shown(&json_ui::ViewState::default()), ["default"]);
    let hovered = json_ui::ViewState {
        hovered: Some(laid.key.clone()),
        ..json_ui::ViewState::default()
    };
    assert_eq!(shown(&hovered), ["hover"]);
}

// --- end to end -------------------------------------------------------------

fn pack_root() -> Option<PathBuf> {
    let dir = support::vanilla_pack();
    dir.join("ui").is_dir().then_some(dir)
}

#[test]
fn main_panel_no_buttons_lays_out_with_nine_slice_background() {
    let Some(root) = pack_root() else {
        return;
    };
    let catalog = json_ui::Catalog::load_dir(&root.join("ui")).expect("index files load");
    let control = resolve(
        &catalog,
        "common_dialogs.main_panel_no_buttons",
        &Context::desktop(),
    )
    .control
    .expect("main_panel_no_buttons resolves");

    let env = LayoutEnv {
        text: &ZeroText,
        textures: &DirTextures(root),
    };
    let placed = layout(&control, [225.0, 200.0], &env);

    // Root fills the virtual dialog bounds.
    assert_eq!(
        (placed.rect.x, placed.rect.y, placed.rect.w, placed.rect.h),
        (0.0, 0.0, 225.0, 200.0)
    );

    // panel_indent inset from the arithmetic size and offset.
    let indent = child_named(&placed, "panel_indent");
    assert_eq!(
        (indent.rect.x, indent.rect.y, indent.rect.w, indent.rect.h),
        (8.0, 23.0, 209.0, 169.0)
    );

    // title_label is centred near the top and within the dialog.
    let title = child_named(&placed, "title_label");
    assert_eq!(title.rect.y, 9.0);
    assert!(title.rect.x >= 0.0 && title.rect.x <= 225.0);
    assert!(title.rect.w >= 0.0);

    // The background is nine-sliced: hollow_3 emits nine sprite quads, first drawn.
    let draws = emit(&placed, &env);
    let bg_sprites = draws
        .iter()
        .filter(|node| {
            matches!(&node.draw, Draw::Sprite { texture, .. }
                if texture == "textures/ui/dialog_background_hollow_3")
        })
        .count();
    assert_eq!(
        bg_sprites, 9,
        "hollow_3 background nine-slices into nine quads"
    );

    // Deterministic: a second run produces an identical draw list.
    let again = emit(&layout(&control, [225.0, 200.0], &env), &env);
    assert_eq!(draws, again);
}

// A flip-book `uv` starts on its first frame and keeps the animation for paint time.
#[test]
fn a_flip_book_uv_resolves_to_its_first_frame() {
    let screen = br#"{
        "namespace": "s",
        "bell": { "anim_type": "flip_book", "initial_uv": [0, 0], "frame_count": 28,
            "frame_step": 8, "fps": 10 },
        "icon": { "type": "image", "texture": "textures/ui/bell_ringing",
            "uv_size": [16, 16], "uv": "@s.bell", "size": [16, 16] }
    }"#;
    let catalog = json_ui::Catalog::from_files([
        ("ui/_global_variables.json", b"{}".as_slice()),
        (
            "ui/_ui_defs.json",
            br#"{"ui_defs":["ui/s.json"]}"#.as_slice(),
        ),
        ("ui/s.json", screen.as_slice()),
    ])
    .unwrap();
    let icon = resolve(&catalog, "s.icon", &Context::desktop())
        .control
        .unwrap();
    assert_eq!(icon.properties.get("uv"), Some(&json!([0, 0])));
    let graph = icon
        .properties
        .get("anim_graph")
        .expect("the flip-book rides along");
    assert_eq!(graph["nodes"][0]["frame_count"], json!(28));
}

// `{ "$layout": {} }` with `$layout: "@s.panel"` instances that panel by name.
#[test]
fn a_variable_child_key_instances_the_control_it_names() {
    let screen = br#"{
        "namespace": "s",
        "panel": { "type": "panel", "size": [10, 10] },
        "root": { "type": "panel", "$layout": "@s.panel", "controls": [ { "$layout": {} } ] }
    }"#;
    let catalog = json_ui::Catalog::from_files([
        ("ui/_global_variables.json", b"{}".as_slice()),
        (
            "ui/_ui_defs.json",
            br#"{"ui_defs":["ui/s.json"]}"#.as_slice(),
        ),
        ("ui/s.json", screen.as_slice()),
    ])
    .unwrap();
    let root = resolve(&catalog, "s.root", &Context::desktop())
        .control
        .unwrap();
    assert_eq!(root.children.len(), 1);
    assert_eq!(root.children[0].name, "panel");
    assert_eq!(
        root.children[0].properties.get("size"),
        Some(&json!([10, 10]))
    );
}

// An `offset` anim chain sweeps a clipped child at paint time: it starts at
// `from`, measures `%x` against its own width, waits, loops, and its clip stays put.
#[test]
fn an_offset_animation_moves_the_draw_inside_a_still_clip() {
    let screen = br#"{
        "namespace": "s",
        "sweep": { "anim_type": "offset", "easing": "linear", "from": ["-50%", "-25%x"],
            "to": ["50%", "25%x"], "duration": 2.0, "next": "@s.hold" },
        "hold": { "anim_type": "wait", "duration": 1.0, "next": "@s.sweep" },
        "card": { "type": "panel", "size": [40, 40], "clips_children": true,
            "anchor_from": "top_left", "anchor_to": "top_left", "controls": [
            { "shine": { "type": "image", "texture": "textures/ui/shine",
                "size": ["200%", "200%"], "anims": ["@s.sweep"] } } ] }
    }"#;
    let catalog = json_ui::Catalog::from_files([
        ("ui/_global_variables.json", b"{}".as_slice()),
        (
            "ui/_ui_defs.json",
            br#"{"ui_defs":["ui/s.json"]}"#.as_slice(),
        ),
        ("ui/s.json", screen.as_slice()),
    ])
    .unwrap();
    let card = resolve(&catalog, "s.card", &Context::desktop())
        .control
        .unwrap();
    let env = zero_env();
    let draws = emit(&layout(&card, [100.0, 100.0], &env), &env);
    let shine = draws.iter().find(|node| node.name == "shine").unwrap();
    // Laid out centred on the 40px card: 80px wide at -20.
    assert_eq!(
        (shine.dest.x, shine.dest.y, shine.dest.w),
        (-20.0, -20.0, 80.0)
    );
    let mut animator = json_ui::Animator::new();
    let mut at = |now: f64| {
        let drawn = shine.animate(&mut animator, now, None, None);
        (
            [drawn.dest.x, drawn.dest.y],
            [drawn.clip.x, drawn.clip.y, drawn.clip.w, drawn.clip.h],
        )
    };
    assert_eq!(
        at(0.0).0,
        [-40.0, -40.0],
        "starts at from: -50% of 40, -25% of 80"
    );
    let (dest, clip) = at(1.0);
    assert_eq!(dest, [-20.0, -20.0]);
    assert_eq!(clip, [0.0, 0.0, 40.0, 40.0], "the clip never moves");
    assert_eq!(at(2.5).0, [0.0, 0.0], "holds `to` through the wait");
    assert_eq!(at(3.0).0, [-40.0, -40.0], "and loops");
}
