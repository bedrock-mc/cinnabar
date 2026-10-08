//! Bound `#targets` reaching the components they drive, through the client's
//! typed readers, one case per post-binding target.

use std::collections::BTreeMap;

use json_ui::{DataSource, EmptyLibrary, ResolvedControl, bind};
use serde_json::{Value, json};

/// A `kind` control whose `target` is bound from a bag value by view.
fn bound(kind: &str, extra: Value, target: &str, value: Value) -> ResolvedControl {
    let mut props = json!({
        "property_bag": { "#value": value },
        "bindings": [{
            "binding_type": "view",
            "source_property_name": "#value",
            "target_property_name": target
        }]
    });
    if let (Value::Object(props), Value::Object(extra)) = (&mut props, extra) {
        props.extend(extra);
    }
    let properties: BTreeMap<String, Value> = match props {
        Value::Object(map) => map.into_iter().collect(),
        _ => BTreeMap::new(),
    };
    let control = ResolvedControl {
        name: "control".to_owned(),
        control_type: Some(kind.to_owned()),
        base: None,
        unresolved_base: None,
        properties: properties.into(),
        children: Vec::new(),
        factory: None,
    };
    bind(&control, &DataSource::new(), &EmptyLibrary)
}

fn prop(kind: &str, target: &str, value: Value, key: &str) -> Option<Value> {
    bound(kind, json!({}), target, value)
        .properties
        .get(key)
        .cloned()
}

// N01-N05, N09-N11: typed readers fall back instead of parsing text.
#[test]
fn readers_fall_back_on_mistyped_values() {
    assert_eq!(
        prop("panel", "#visible", json!("false"), "visible"),
        Some(json!(true))
    );
    assert_eq!(
        prop("panel", "#visible", json!(false), "visible"),
        Some(json!(false))
    );
    assert_eq!(
        prop("button", "#enabled", json!("false"), "#enabled"),
        Some(json!(true))
    );
    assert_eq!(
        prop(
            "panel",
            "#propagateAlpha",
            json!("false"),
            "propagate_alpha"
        ),
        Some(json!(true))
    );
    assert_eq!(
        prop("toggle", "#toggle_state", json!("true"), "#toggle_state"),
        Some(json!(false))
    );
    assert_eq!(
        prop("image", "#alpha", json!("0.5"), "alpha"),
        Some(json!(1.0))
    );
    assert_eq!(
        prop("image", "#alpha", json!(0.5), "alpha"),
        Some(json!(0.5))
    );
    assert_eq!(
        prop("image", "#clip_ratio", json!("0.5"), "clip_ratio"),
        Some(json!(0.0))
    );
    assert_eq!(
        prop("slider", "#slider_value", json!("0.5"), "#slider_value"),
        Some(json!(0.0))
    );
    assert_eq!(
        prop("slider", "#slider_steps", json!("3"), "#slider_steps"),
        Some(json!(1))
    );
    assert_eq!(
        prop("slider", "#slider_steps", json!(2.5), "#slider_steps"),
        Some(json!(1))
    );
}

// N06-N08: empty texture, file system category, array colour.
#[test]
fn sprite_targets() {
    let image = bound(
        "image",
        json!({ "texture": "textures/ui/default" }),
        "#texture",
        json!(""),
    );
    assert_eq!(image.properties.get("texture"), Some(&json!("")));
    let fs = bound(
        "image",
        json!({ "texture": "textures/ui/example" }),
        "#texture_file_system",
        json!("RawPath"),
    );
    assert_eq!(
        fs.properties.get("texture"),
        Some(&json!("textures/ui/example"))
    );
    assert_eq!(
        fs.properties.get("texture_file_system"),
        Some(&json!("RawPath"))
    );
    let tint = bound(
        "image",
        json!({ "color": [1, 1, 1, 1] }),
        "#color",
        json!([1, 0, 0, 1]),
    );
    assert_eq!(
        tint.properties.get("color"),
        Some(&json!([1.0, 0.0, 0.0, 1.0]))
    );
    assert_eq!(
        prop(
            "image",
            "#nineslice_size",
            json!([2, 2, 2, 2]),
            "nineslice_size"
        ),
        Some(json!([2.0, 2.0, 2.0, 2.0]))
    );
    assert_eq!(
        prop("image", "#bilinear", json!(true), "bilinear"),
        Some(json!(true))
    );
    assert_eq!(
        prop("image", "#grayscale", json!(true), "grayscale"),
        Some(json!(true))
    );
    assert_eq!(
        prop("image", "#zip_folder", json!("ui.zip"), "zip_folder"),
        Some(json!("ui.zip"))
    );
}

// N13-N21: grid cap, offset, sizes, anchored offsets, priority.
#[test]
fn layout_targets() {
    assert_eq!(
        bound(
            "grid",
            json!({ "collection_name": "items" }),
            "#maximum_grid_items",
            json!(1)
        )
        .properties
        .get("maximum_grid_items"),
        Some(&json!(1))
    );
    // An integral real reads as an int, as a controller count is.
    assert_eq!(
        bound(
            "grid",
            json!({ "collection_name": "items" }),
            "#maximum_grid_items",
            json!(120.0)
        )
        .properties
        .get("maximum_grid_items"),
        Some(&json!(120))
    );
    assert_eq!(
        prop("panel", "#offset", json!([20, 30]), "offset"),
        Some(json!([20.0, 30.0]))
    );
    let sized = |target| bound("panel", json!({ "size": [10, 10] }), target, json!(40));
    assert_eq!(
        sized("#size_binding_x_absolute").properties.get("size"),
        Some(&json!([40.0, 10]))
    );
    assert_eq!(
        sized("#size_binding_y_absolute").properties.get("size"),
        Some(&json!([10, 40.0]))
    );
    let relative = bound(
        "panel",
        json!({ "size": [10, 10] }),
        "#size_binding_x",
        json!(0.5),
    );
    assert_eq!(relative.properties.get("size"), Some(&json!(["50%", 10])));
    let anchored = bound(
        "panel",
        json!({ "use_anchored_offset": true }),
        "#anchored_offset_value_x",
        json!(20),
    );
    assert_eq!(
        anchored.properties.get("anchored_offset_value_x"),
        Some(&json!(20.0))
    );
    assert_eq!(
        prop("panel", "#priority", json!(5), "priority"),
        Some(json!(5))
    );
}

// N22-N24: text scale, font and alignment.
#[test]
fn text_targets() {
    let label = bound(
        "label",
        json!({ "text": "abc", "font_scale_factor": 1 }),
        "#font_scale_factor",
        json!(2),
    );
    assert_eq!(label.properties.get("font_scale_factor"), Some(&json!(2.0)));
    assert_eq!(
        prop("label", "#font_type", json!("smooth"), "font_type"),
        Some(json!("smooth"))
    );
    assert_eq!(
        prop("label", "#text_alignment", json!("right"), "text_alignment"),
        Some(json!("right"))
    );
}

// N29-N47: focus, input and widget component targets.
#[test]
fn focus_and_input_targets() {
    assert_eq!(
        prop("button", "#focus_enabled", json!(false), "focus_enabled"),
        Some(json!(false))
    );
    assert_eq!(
        prop(
            "button",
            "#focus_wrap_enabled",
            json!(false),
            "focus_wrap_enabled"
        ),
        Some(json!(false))
    );
    assert_eq!(
        prop(
            "button",
            "#default_focus_precedence",
            json!(10),
            "default_focus_precedence"
        ),
        Some(json!(10))
    );
    for target in [
        "#focus_identifier",
        "#focus_change_up",
        "#focus_change_down",
        "#focus_change_left",
        "#focus_change_right",
    ] {
        assert_eq!(
            prop("button", target, json!("next"), &target[1..]),
            Some(json!("next")),
            "{target}"
        );
    }
    for target in [
        "#focus_navigation_mode_up",
        "#focus_navigation_mode_down",
        "#focus_navigation_mode_left",
        "#focus_navigation_mode_right",
    ] {
        assert_eq!(
            prop("panel", target, json!("stop"), &target[1..]),
            Some(json!("stop")),
            "{target}"
        );
    }
    assert_eq!(
        prop("input_panel", "#modal", json!(true), "modal"),
        Some(json!(true))
    );
    assert_eq!(
        prop(
            "input_panel",
            "#always_handle_controller_direction",
            json!(true),
            "always_handle_controller_direction"
        ),
        Some(json!(true))
    );
    assert_eq!(
        prop(
            "scroll_view",
            "#gesture_control_enabled",
            json!(true),
            "gesture_control_enabled"
        ),
        Some(json!(true))
    );
    assert_eq!(
        prop(
            "toggle",
            "#toggle_group_forced_index",
            json!(3),
            "toggle_group_forced_index"
        ),
        Some(json!(3))
    );
    assert_eq!(
        prop(
            "edit_box",
            "#can_be_deselected",
            json!(false),
            "can_be_deselected"
        ),
        Some(json!(false))
    );
    assert_eq!(
        prop("slider", "#slider_timeout", json!(0.75), "slider_timeout"),
        Some(json!(0.75))
    );
    assert_eq!(
        prop(
            "selection_wheel",
            "#init_selection_wheel_input_mode",
            json!(2),
            "init_selection_wheel_input_mode"
        ),
        Some(json!(2))
    );
}

// A bag literal with no binding never reaches its component.
#[test]
fn unbound_bag_literal_does_not_reach_components() {
    let control = ResolvedControl {
        name: "panel".to_owned(),
        control_type: Some("panel".to_owned()),
        base: None,
        unresolved_base: None,
        properties: BTreeMap::from([("property_bag".to_owned(), json!({ "#visible": false }))])
            .into(),
        children: Vec::new(),
        factory: None,
    };
    let bound = bind(&control, &DataSource::new(), &EmptyLibrary);
    assert_eq!(bound.properties.get("visible"), None);
    assert_eq!(bound.properties.get("#visible"), Some(&json!(false)));
}
