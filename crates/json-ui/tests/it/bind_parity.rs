//! Binding lifecycle, scheduling, bags, views and collections against the
//! vanilla client, one case per audited behaviour.

use std::collections::BTreeMap;
use std::sync::Arc;

use json_ui::{
    BindState, CollectionItem, ControlLibrary, ControlRef, DataSource, EmptyLibrary, Factory,
    ResolvedControl, Scalar, bind, bind_reporting, bind_stateful,
};
use serde_json::{Value, json};

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

fn leaf(name: &str, kind: &str, props: Value) -> ResolvedControl {
    ctrl(name, kind, props, Vec::new())
}

fn find<'a>(control: &'a ResolvedControl, name: &str) -> &'a ResolvedControl {
    fn walk<'a>(control: &'a ResolvedControl, name: &str) -> Option<&'a ResolvedControl> {
        if control.name == name {
            return Some(control);
        }
        control.children.iter().find_map(|child| walk(child, name))
    }
    walk(control, name).unwrap_or_else(|| panic!("no control {name}"))
}

fn get<'a>(control: &'a ResolvedControl, key: &str) -> Option<&'a Value> {
    control.properties.get(key)
}

fn global(name: &str, value: Scalar) -> DataSource {
    let mut data = DataSource::new();
    data.set_global(name, value);
    data
}

fn text(value: &str) -> Scalar {
    Scalar::Text(value.to_owned())
}

/// Bind `root` twice over one state, the second time against `then`.
fn refresh(root: &ResolvedControl, first: &DataSource, then: &DataSource) -> ResolvedControl {
    let root = Arc::new(root.clone());
    let mut state = BindState::new();
    bind_stateful(&root, first, &EmptyLibrary, &mut state);
    bind_stateful(&root, then, &EmptyLibrary, &mut state).0
}

fn seeded(condition: &str, extra: Value) -> ResolvedControl {
    let mut props = json!({
        "text": "#out",
        "property_bag": { "#out": "seed" },
        "bindings": [{
            "binding_name": "#source",
            "binding_name_override": "#out",
            "binding_condition": condition
        }]
    });
    if let (Value::Object(props), Value::Object(extra)) = (&mut props, extra) {
        props.extend(extra);
    }
    leaf("label", "label", props)
}

// B14: a `once` binding keeps its first value.
#[test]
fn once_binding_retains_its_first_value() {
    let bound = refresh(
        &seeded("once", json!({})),
        &global("#source", text("A")),
        &global("#source", text("B")),
    );
    assert_eq!(get(&bound, "text"), Some(&json!("A")));
}

// B15/B12: `always` and `none` rerun on every refresh.
#[test]
fn always_and_none_bindings_follow_each_refresh() {
    for condition in ["always", "none"] {
        let bound = refresh(
            &seeded(condition, json!({})),
            &global("#source", text("A")),
            &global("#source", text("B")),
        );
        assert_eq!(get(&bound, "text"), Some(&json!("B")), "{condition}");
    }
}

// B16/B17: `visible` and `always_when_visible` skip a hidden control.
#[test]
fn visible_conditions_skip_hidden_controls() {
    for condition in ["visible", "always_when_visible"] {
        let control = seeded(condition, json!({ "visible": false }));
        let bound = bind(&control, &global("#source", text("remote")), &EmptyLibrary);
        assert_eq!(get(&bound, "#out"), Some(&json!("seed")), "{condition}");
    }
}

// B18: `visibility_changed` applies on a visibility transition only.
#[test]
fn visibility_changed_applies_on_transitions() {
    let bound = refresh(
        &seeded("visibility_changed", json!({})),
        &global("#source", text("A")),
        &global("#source", text("B")),
    );
    assert_eq!(get(&bound, "text"), Some(&json!("A")));
}

// B13: an unknown condition reads `none` and is reported.
#[test]
fn unknown_condition_falls_back_to_none() {
    let control = seeded("sometimes", json!({}));
    let (bound, notes) = bind_reporting(
        &Arc::new(control),
        &global("#source", text("A")),
        &EmptyLibrary,
    );
    assert_eq!(get(&bound, "text"), Some(&json!("A")));
    assert!(
        notes.iter().any(|note| note.contains("sometimes")),
        "{notes:?}"
    );
}

// B06: `binding_type: none` creates no binding.
#[test]
fn none_binding_type_binds_nothing() {
    let control = leaf(
        "label",
        "label",
        json!({
            "text": "#x",
            "property_bag": { "#x": "seed" },
            "bindings": [{ "binding_type": "none", "binding_name": "#x" }]
        }),
    );
    let bound = bind(&control, &global("#x", text("remote")), &EmptyLibrary);
    assert_eq!(get(&bound, "text"), Some(&json!("seed")));
}

// B07: an unknown binding type is reported and binds as global.
#[test]
fn unknown_binding_type_binds_as_global() {
    let control = leaf(
        "label",
        "label",
        json!({ "text": "#x", "bindings": [{ "binding_type": "future_type", "binding_name": "#x" }] }),
    );
    let (bound, notes) =
        bind_reporting(&Arc::new(control), &global("#x", text("v")), &EmptyLibrary);
    assert_eq!(get(&bound, "text"), Some(&json!("v")));
    assert!(notes.iter().any(|note| note.contains("future_type")));
}

// B09: an empty override writes under the source name.
#[test]
fn empty_override_uses_the_source_name() {
    let control = leaf(
        "label",
        "label",
        json!({ "text": "#x", "bindings": [{ "binding_name": "#x", "binding_name_override": "" }] }),
    );
    let bound = bind(&control, &global("#x", text("value")), &EmptyLibrary);
    assert_eq!(get(&bound, "text"), Some(&json!("value")));
}

// B08: an expression binding stores the raw answer and drives the component
// through its expression rewritten to read the target.
#[test]
fn expression_binding_writes_raw_value_and_applies_the_expression() {
    let control = leaf(
        "panel",
        "panel",
        json!({ "bindings": [{ "binding_name": "(not #hidden)", "binding_name_override": "#visible" }] }),
    );
    let bound = bind(
        &control,
        &global("#hidden", Scalar::Bool(true)),
        &EmptyLibrary,
    );
    assert_eq!(get(&bound, "#visible"), Some(&json!(true)));
    assert_eq!(get(&bound, "visible"), Some(&json!(false)));
}

// B08: a non-`#`, non-expression name binds nothing.
#[test]
fn plain_word_binding_name_binds_nothing() {
    let control = leaf(
        "label",
        "label",
        json!({ "property_bag": { "x": "seed" }, "bindings": [{ "binding_name": "x" }] }),
    );
    let bound = bind(&control, &global("x", text("remote")), &EmptyLibrary);
    assert_eq!(get(&bound, "#x"), None);
}

fn view(source: &str, property: &str, target: &str) -> Value {
    json!({
        "binding_type": "view",
        "source_control_name": source,
        "source_property_name": property,
        "target_property_name": target
    })
}

// B26: an unnamed view reads its own bag, never an ancestor's; a property the
// bag lacks comes from the screen controller (Zeqa picks its form layouts with
// unnamed views over `#title_text`, which only the controller answers).
#[test]
fn unnamed_view_reads_its_own_bag_then_the_controller() {
    let child = leaf(
        "child",
        "panel",
        json!({ "property_bag": { "#out": "seed" }, "bindings": [view("", "#x", "#out")] }),
    );
    let root = ctrl(
        "root",
        "panel",
        json!({ "property_bag": { "#x": "ancestor" } }),
        vec![child],
    );
    let bound = bind(&root, &DataSource::new(), &EmptyLibrary);
    assert_eq!(get(find(&bound, "child"), "#out"), Some(&json!("seed")));
    let bound = bind(&root, &global("#x", text("global")), &EmptyLibrary);
    assert_eq!(get(find(&bound, "child"), "#out"), Some(&json!("global")));
}

// B27: a named view has no globals fallback.
#[test]
fn named_view_has_no_global_fallback() {
    let source = leaf("source", "panel", json!({}));
    let reader = leaf(
        "reader",
        "panel",
        json!({ "property_bag": { "#out": "seed" }, "bindings": [view("source", "#x", "#out")] }),
    );
    let root = ctrl("root", "panel", json!({}), vec![source, reader]);
    let bound = bind(&root, &global("#x", text("global")), &EmptyLibrary);
    assert_eq!(get(find(&bound, "reader"), "#out"), Some(&json!("seed")));
}

// B20: a view finds its source inside a hidden subtree.
#[test]
fn view_source_resolves_through_hidden_controls() {
    let source = leaf(
        "source",
        "panel",
        json!({ "property_bag": { "#x": "hidden value" } }),
    );
    let hidden = ctrl("hidden", "panel", json!({ "visible": false }), vec![source]);
    let reader = leaf(
        "reader",
        "label",
        json!({ "text": "#out", "bindings": [view("source", "#x", "#out")] }),
    );
    let root = ctrl("root", "panel", json!({}), vec![hidden, reader]);
    let bound = bind(&root, &DataSource::new(), &EmptyLibrary);
    assert_eq!(
        get(find(&bound, "reader"), "text"),
        Some(&json!("hidden value"))
    );
}

// B28/B19: values settle along a five-edge view chain.
#[test]
fn five_edge_view_chain_settles() {
    let mut controls = vec![leaf("a", "panel", json!({ "property_bag": { "#x": 1 } }))];
    for (name, source) in [("b", "a"), ("c", "b"), ("d", "c"), ("e", "d"), ("f", "e")] {
        controls.push(leaf(
            name,
            "panel",
            json!({ "property_bag": { "#x": 0 }, "bindings": [view(source, "#x", "#x")] }),
        ));
    }
    // Readers first, so each pass moves the value one hop.
    controls.reverse();
    let root = ctrl("root", "panel", json!({}), controls);
    let bound = bind(&root, &DataSource::new(), &EmptyLibrary);
    assert_eq!(get(find(&bound, "f"), "#x"), Some(&json!(1)));
}

// B25: sibling scope wins over ancestor scope, with a diagnostic.
#[test]
fn sibling_scope_wins_over_ancestor_scope() {
    let mut binding = view("source", "#x", "#out");
    binding["resolve_sibling_scope"] = json!(true);
    binding["resolve_ancestor_scope"] = json!(true);
    let reader = leaf("reader", "panel", json!({ "bindings": [binding] }));
    let sibling = leaf(
        "source",
        "panel",
        json!({ "property_bag": { "#x": "sibling" } }),
    );
    let parent = ctrl("parent", "panel", json!({}), vec![reader, sibling]);
    let root = ctrl(
        "source",
        "panel",
        json!({ "property_bag": { "#x": "ancestor" } }),
        vec![parent],
    );
    let (bound, notes) = bind_reporting(&Arc::new(root), &DataSource::new(), &EmptyLibrary);
    assert_eq!(get(find(&bound, "reader"), "#out"), Some(&json!("sibling")));
    assert!(notes.iter().any(|note| note.contains("cannot both be set")));
}

// B21: a view's source expression evaluates against the source bag.
#[test]
fn view_source_expression_reads_the_source_bag() {
    let control = leaf(
        "panel",
        "panel",
        json!({ "property_bag": { "#a": 2, "#b": 3 }, "bindings": [view("", "(#a + #b)", "#result")] }),
    );
    let bound = bind(&control, &DataSource::new(), &EmptyLibrary);
    assert_eq!(get(&bound, "#result"), Some(&json!(5)));
}

// B19: a view rewrites only when its source changes, not every refresh.
#[test]
fn view_writes_only_on_source_change() {
    let control = leaf(
        "panel",
        "panel",
        json!({
            "property_bag": { "#x": "seen" },
            "bindings": [
                view("", "#x", "#out"),
                { "binding_name": "#late", "binding_name_override": "#out" }
            ]
        }),
    );
    let root = Arc::new(control);
    let mut state = BindState::new();
    let first = bind_stateful(&root, &DataSource::new(), &EmptyLibrary, &mut state).0;
    assert_eq!(get(&first, "#out"), Some(&json!("seen")));
    let second = bind_stateful(
        &root,
        &global("#late", text("global")),
        &EmptyLibrary,
        &mut state,
    )
    .0;
    assert_eq!(get(&second, "#out"), Some(&json!("global")));
}

// B29/B30/B31: bag values keep their JSON types, compound values included,
// and a view moves an array.
#[test]
fn bag_values_keep_json_types() {
    let control = leaf(
        "panel",
        "panel",
        json!({
            "property_bag": {
                "#int": 2, "#float": 2.5, "#flag": true, "#text": "t",
                "#position": [1, 2, 3], "#details": { "selected": true }, "#empty": null
            },
            "bindings": [view("", "#position", "#copy")]
        }),
    );
    let bound = bind(&control, &DataSource::new(), &EmptyLibrary);
    assert_eq!(get(&bound, "#int"), Some(&json!(2)));
    assert_eq!(get(&bound, "#float"), Some(&json!(2.5)));
    assert_eq!(get(&bound, "#position"), Some(&json!([1, 2, 3])));
    assert_eq!(get(&bound, "#details"), Some(&json!({ "selected": true })));
    assert_eq!(get(&bound, "#empty"), Some(&json!(null)));
    assert_eq!(get(&bound, "#copy"), Some(&json!([1, 2, 3])));
}

// B32: a constant expression in a bag literal is evaluated.
#[test]
fn bag_literal_expressions_evaluate() {
    let control = leaf(
        "panel",
        "panel",
        json!({ "property_bag": { "#n": "(1 + 2)" } }),
    );
    let bound = bind(&control, &DataSource::new(), &EmptyLibrary);
    assert_eq!(get(&bound, "#n"), Some(&json!(3)));
}

// B33: children inherit `property_bag_for_children` without overriding their own.
#[test]
fn children_bags_inherit_without_overwrite() {
    let inherits = leaf(
        "inherits",
        "label",
        json!({ "text": "#out", "bindings": [view("", "#message", "#out")] }),
    );
    let local = leaf(
        "local",
        "panel",
        json!({ "property_bag": { "#message": "local" } }),
    );
    let grandchild = leaf("grandchild", "panel", json!({}));
    let middle = ctrl("middle", "panel", json!({}), vec![grandchild]);
    let root = ctrl(
        "root",
        "panel",
        json!({ "property_bag_for_children": { "#message": "inherited" } }),
        vec![inherits, local, middle],
    );
    let bound = bind(&root, &DataSource::new(), &EmptyLibrary);
    assert_eq!(
        get(find(&bound, "inherits"), "text"),
        Some(&json!("inherited"))
    );
    assert_eq!(
        get(find(&bound, "local"), "#message"),
        Some(&json!("local"))
    );
    assert_eq!(
        get(find(&bound, "grandchild"), "#message"),
        Some(&json!("inherited"))
    );
    assert_eq!(get(&bound, "#message"), None);
}

fn items(names: &[&str]) -> Vec<CollectionItem> {
    names
        .iter()
        .map(|name| CollectionItem::default().with("#name", text(name)))
        .collect()
}

fn collection_child(index: Value, ignore: bool) -> ResolvedControl {
    let mut props = json!({
        "text": "#out",
        "property_bag": { "#out": "seed" },
        "collection_index": index,
        "bindings": [{
            "binding_type": "collection",
            "binding_collection_name": "items",
            "binding_name": "#name",
            "binding_name_override": "#out"
        }]
    });
    if ignore {
        props["ignoreCollectionItem"] = json!(true);
    }
    leaf("child", "label", props)
}

fn items_data() -> DataSource {
    let mut data = DataSource::new();
    data.set_collection("items", items(&["zero", "one", "two", "three"]));
    data
}

// B36/B03: a negative item index skips collection bindings.
#[test]
fn negative_collection_index_skips_the_binding() {
    let root = ctrl(
        "panel",
        "panel",
        json!({ "collection_name": "items" }),
        vec![collection_child(json!(-2), false)],
    );
    let bound = bind(&root, &items_data(), &EmptyLibrary);
    assert_eq!(get(find(&bound, "child"), "text"), Some(&json!("seed")));
}

// B37: a child opted out of the collection stays out of it.
#[test]
fn ignore_collection_item_reads_item_zero() {
    let root = ctrl(
        "panel",
        "panel",
        json!({ "collection_name": "items" }),
        vec![collection_child(json!(3), true)],
    );
    let bound = bind(&root, &items_data(), &EmptyLibrary);
    assert_eq!(get(find(&bound, "child"), "text"), Some(&json!("zero")));
    let indexed = ctrl(
        "panel",
        "panel",
        json!({ "collection_name": "items" }),
        vec![collection_child(json!(3), false)],
    );
    let bound = bind(&indexed, &items_data(), &EmptyLibrary);
    assert_eq!(get(find(&bound, "child"), "text"), Some(&json!("three")));
}

fn details(name: Option<&str>, prefix: &str) -> ResolvedControl {
    let mut binding =
        json!({ "binding_type": "collection_details", "binding_collection_prefix": prefix });
    if let Some(name) = name {
        binding["binding_collection_name"] = json!(name);
    }
    let detail = leaf("detail", "panel", json!({ "bindings": [binding] }));
    let item = ctrl(
        "item",
        "panel",
        json!({ "collection_index": 3 }),
        vec![detail],
    );
    ctrl(
        "panel",
        "panel",
        json!({ "collection_name": "items" }),
        vec![item],
    )
}

// B11/B38: details publish an int index and the name, under a prefix.
#[test]
fn collection_details_publish_prefixed_metadata() {
    let bound = bind(&details(Some("items"), "row"), &items_data(), &EmptyLibrary);
    let detail = find(&bound, "detail");
    assert_eq!(get(detail, "#row_collection_name"), Some(&json!("items")));
    assert_eq!(get(detail, "#row_collection_index"), Some(&json!(3)));
    let bound = bind(&details(Some("items"), ""), &items_data(), &EmptyLibrary);
    assert_eq!(
        get(find(&bound, "detail"), "#collection_index"),
        Some(&json!(3))
    );
}

// B39: unnamed details publish every enclosing collection's index.
#[test]
fn unnamed_collection_details_publish_all_collections() {
    let bound = bind(&details(None, "row"), &items_data(), &EmptyLibrary);
    assert_eq!(
        get(find(&bound, "detail"), "#row_collections"),
        Some(&json!({ "row_items": 3 }))
    );
    let bound = bind(&details(None, ""), &items_data(), &EmptyLibrary);
    assert_eq!(
        get(find(&bound, "detail"), "#collections"),
        Some(&json!({ "items": 3 }))
    );
}

// B14: a `once` collection binding waits for an item index.
#[test]
fn once_collection_binding_waits_for_an_index() {
    let mut child = collection_child(json!(-1), false);
    if let Some(Value::Array(bindings)) = child.properties.get_mut("bindings") {
        bindings[0]["binding_condition"] = json!("once");
    }
    let root = ctrl(
        "panel",
        "panel",
        json!({ "collection_name": "items" }),
        vec![child],
    );
    let bound = bind(&root, &items_data(), &EmptyLibrary);
    assert_eq!(get(find(&bound, "child"), "text"), Some(&json!("seed")));
}

struct Stub(BTreeMap<String, ResolvedControl>);

impl ControlLibrary for Stub {
    fn resolve(&self, reference: &ControlRef) -> Option<ResolvedControl> {
        self.0
            .get(&format!("{}.{}", reference.namespace, reference.name))
            .cloned()
    }
}

fn factory_panel(binding: Value) -> (ResolvedControl, Stub) {
    let mut panel = leaf(
        "rows",
        "stack_panel",
        json!({ "collection_name": "items", "bindings": [binding] }),
    );
    panel.factory = Some(Factory {
        name: Some("rows".to_owned()),
        control_ids: BTreeMap::new(),
        control_name: Some(ControlRef::parse("audit.row", "")),
        ..Factory::default()
    });
    let row = leaf("row", "panel", json!({}));
    (panel, Stub(BTreeMap::from([("audit.row".to_owned(), row)])))
}

// N12/B41: a bound `#collection_length` decides creation over supplied items
// and publishes `#collection_number_size`.
#[test]
fn bound_collection_length_drives_the_factory() {
    let (panel, lib) = factory_panel(
        json!({ "binding_name": "#count", "binding_name_override": "#collection_length" }),
    );
    let mut data = items_data();
    data.set_global("#count", Scalar::Int(1));
    let bound = bind(&panel, &data, &lib);
    assert_eq!(bound.children.len(), 1);
    assert_eq!(get(&bound, "#collection_number_size"), Some(&json!(1)));
    let mut many = DataSource::new();
    many.set_global("#count", Scalar::Int(65));
    assert_eq!(bind(&panel, &many, &lib).children.len(), 65);
    let mut ids = DataSource::new();
    ids.set_global("#count", Scalar::Json(json!(["a", "b"])));
    assert_eq!(bind(&panel, &ids, &lib).children.len(), 2);
}

// B40: a grid's dimension binding reads the bag array it binds.
#[test]
fn grid_dimension_binding_reads_the_bound_array() {
    let grid = leaf(
        "grid",
        "grid",
        json!({
            "collection_name": "items",
            "grid_dimension_binding": "#dims",
            "bindings": [{ "binding_name": "#dims" }]
        }),
    );
    let bound = bind(
        &grid,
        &global("#dims", Scalar::Json(json!([2, 3]))),
        &EmptyLibrary,
    );
    assert_eq!(get(&bound, "grid_dimensions"), Some(&json!([2, 3])));
}

// B46: a toggle publishes its default state into its bag when created.
#[test]
fn toggle_publishes_its_default_state() {
    let toggle = leaf("t", "toggle", json!({ "toggle_default_state": true }));
    let mirror = leaf(
        "mirror",
        "panel",
        json!({ "property_bag": { "#value": false }, "bindings": [view("t", "#toggle_state", "#value")] }),
    );
    let root = ctrl("root", "panel", json!({}), vec![toggle, mirror]);
    let bound = bind(&root, &DataSource::new(), &EmptyLibrary);
    assert_eq!(get(find(&bound, "mirror"), "#value"), Some(&json!(true)));
}

// B43/B44/B45: a widget's published bag value reaches views on the next refresh.
#[test]
fn published_widget_state_reaches_views() {
    let toggle = leaf("t", "toggle", json!({}));
    let mirror = leaf(
        "mirror",
        "panel",
        json!({ "property_bag": { "#value": false }, "bindings": [view("t", "#toggle_state", "#value")] }),
    );
    let root = Arc::new(ctrl("root", "panel", json!({}), vec![toggle, mirror]));
    let mut state = BindState::new();
    bind_stateful(&root, &DataSource::new(), &EmptyLibrary, &mut state);
    state.publish("/root/t", "#toggle_state", Scalar::Bool(true));
    let bound = bind_stateful(&root, &DataSource::new(), &EmptyLibrary, &mut state).0;
    assert_eq!(get(find(&bound, "mirror"), "#value"), Some(&json!(true)));
    assert_eq!(
        state.value("/root/t", "#toggle_state"),
        Some(&Scalar::Bool(true))
    );
}

// A controller value written into a named control's bag reaches its views.
#[test]
fn controller_control_values_feed_views() {
    let source = leaf("source", "panel", json!({}));
    let reader = leaf(
        "reader",
        "label",
        json!({ "text": "#out", "bindings": [view("source", "#title", "#out")] }),
    );
    let root = ctrl("root", "panel", json!({}), vec![source, reader]);
    let mut data = DataSource::new();
    data.set_control_value("source", "#title", text("Hello"));
    let bound = bind(&root, &data, &EmptyLibrary);
    assert_eq!(get(find(&bound, "reader"), "text"), Some(&json!("Hello")));
}

// L08: a leading `##` text is literal.
#[test]
fn double_hash_text_is_literal() {
    let label = leaf(
        "label",
        "label",
        json!({ "text": "##literal", "localize": false }),
    );
    let bound = bind(&label, &DataSource::new(), &EmptyLibrary);
    assert_eq!(get(&bound, "text"), Some(&json!("##literal")));
}

// B55/B60: grids publish their cell count; a scrollbar box marks itself.
#[test]
fn component_markers_publish_into_their_bags() {
    let grid = leaf("grid", "grid", json!({ "grid_dimensions": [2, 3] }));
    assert_eq!(
        get(
            &bind(&grid, &DataSource::new(), &EmptyLibrary),
            "#grid_number_size"
        ),
        Some(&json!(6))
    );
    let bar = leaf("box", "scrollbar_box", json!({}));
    assert_eq!(
        get(
            &bind(&bar, &DataSource::new(), &EmptyLibrary),
            "#is_scroll_bar_box"
        ),
        Some(&json!(true))
    );
}

// B61-B63: a layout's scroll state reaches the scroll view's bag.
#[test]
fn scroll_state_publishes_end_and_bar_visibility() {
    use json_ui::{LayoutReport, ScrollMetrics};
    let metrics = |offset: f64| ScrollMetrics {
        offset,
        content: 300.0,
        viewport: 100.0,
        thumb: Some([0.0, 0.0, 4.0, 10.0]),
        scrolled_to_end: offset >= 200.0,
        hit_bottom: true,
        bar_visible: Some(true),
        ..ScrollMetrics::default()
    };
    let report = |offset| LayoutReport {
        scrolls: BTreeMap::from([("/root/scroll".to_owned(), metrics(offset))]),
        ..LayoutReport::default()
    };
    let mut state = BindState::new();
    assert!(state.publish_scrolls(&report(200.0)));
    assert_eq!(
        state.value("/root/scroll", "#scrolled_to_end"),
        Some(&Scalar::Bool(true))
    );
    assert!(state.publish_scrolls(&report(50.0)));
    assert_eq!(
        state.value("/root/scroll", "#scrolled_to_end"),
        Some(&Scalar::Bool(false))
    );
    assert_eq!(
        state.value("/root/scroll", "#scrollbar_hit_bottom"),
        Some(&Scalar::Bool(true))
    );
    assert_eq!(
        state.value("/root/scroll", "#scroll_bar_visible"),
        Some(&Scalar::Bool(true))
    );
    assert!(!state.publish_scrolls(&report(50.0)));
}

// A view's source name and an `ignored` entry go through the def evaluator.
#[test]
fn binding_fields_evaluate_constant_expressions() {
    let source = leaf(
        "dropdown",
        "toggle",
        json!({ "property_bag": { "#toggle_state": false } }),
    );
    let content = leaf(
        "content",
        "panel",
        json!({ "visible": false, "bindings": [
            view("('dropdown')", "#toggle_state", "#visible"),
            { "binding_name": "#shown", "binding_name_override": "#visible", "ignored": "(not false)" }
        ] }),
    );
    let root = ctrl("root", "panel", json!({}), vec![source, content]);
    let bound = bind(&root, &global("#shown", Scalar::Bool(true)), &EmptyLibrary);
    assert_eq!(get(find(&bound, "content"), "visible"), Some(&json!(false)));
}

// An edit box's `once` content binding re-reads the host's live text: the host owns
// the text vanilla's edit box keeps, so typing shows as it happens. Vanilla's
// `common.text_edit_box` puts that binding on its child label (`ui_common.json`).
#[test]
fn edit_box_content_follows_the_host_text_despite_once() {
    let label = leaf(
        "text_edit_box_label",
        "label",
        json!({
            "text": "#item_name",
            "property_bag": { "#property_field": "#item_name" },
            "bindings": [{
                "binding_name": "#ip_text_box_content",
                "binding_name_override": "#item_name",
                "binding_condition": "once"
            }]
        }),
    );
    let edit = ctrl("edit_box", "edit_box", json!({}), vec![label]);
    let bound = refresh(
        &edit,
        &global("#ip_text_box_content", text("pl")),
        &global("#ip_text_box_content", text("play.example")),
    );
    assert_eq!(
        get(find(&bound, "text_edit_box_label"), "text"),
        Some(&json!("play.example"))
    );
}

#[test]
fn edit_label_content_refreshes_while_ordinary_once_labels_stay_seeded() {
    for editable in [false, true] {
        let label = leaf(
            "display_text",
            "label",
            json!({
                "text": "#item_name",
                "property_bag": if editable { json!({"#property_field": "#item_name"}) } else { json!({}) },
                "bindings": [{
                    "binding_name": "#content", "binding_name_override": "#item_name",
                    "binding_condition": "once"
                }]
            }),
        );
        let bound = refresh(
            &label,
            &global("#content", text("first")),
            &global("#content", text("typed")),
        );
        assert_eq!(
            get(&bound, "text"),
            Some(&json!(if editable { "typed" } else { "first" }))
        );
    }
}
