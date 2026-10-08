//! Post-binding component updates: after a binding
//! writes a `#target`, the component that target drives takes the value
//! through its typed reader. Here the component state is the literal property
//! layout, emit and input read, so a bound target reaches them as if authored.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::predicate::Scalar;
use crate::tree::ResolvedControl;

/// What bindings set on a control's components.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Native {
    /// Literal properties that override the authored ones.
    pub(super) props: BTreeMap<String, Value>,
    /// A collection's bound length: a count or control ids.
    pub(super) collection_length: Option<Value>,
}

impl Native {
    /// The visibility the control's own flag holds.
    pub(super) fn visible(&self, control: &ResolvedControl) -> bool {
        match self.props.get("visible") {
            Some(value) => value != &Value::Bool(false),
            None => control.properties.get("visible") != Some(&Value::Bool(false)),
        }
    }
}

/// A bool target reads only a JSON bool, else `default`.
fn boolean(value: &Value, default: bool) -> bool {
    value.as_bool().unwrap_or(default)
}

/// A float target reads any number or bool, else `default`.
fn float(value: &Value, default: f32) -> f32 {
    match value {
        Value::Number(number) => number.as_f64().map_or(default, |number| number as f32),
        Value::Bool(flag) => f32::from(u8::from(*flag)),
        _ => default,
    }
}

/// An int target reads only an integral JSON number, else `default`.
fn int(value: &Value, default: i32) -> i32 {
    match value {
        Value::Number(number) => number
            .as_i64()
            .map(|int| int as i32)
            .or_else(|| number.as_u64().map(|uint| uint as i32))
            // An integral real in range also counts as an int.
            .or_else(|| {
                number
                    .as_f64()
                    .filter(|real| real.fract() == 0.0 && real.abs() <= f64::from(i32::MAX))
                    .map(|real| real as i32)
            })
            .unwrap_or(default),
        _ => default,
    }
}

/// Vanilla JSON int reads: integers truncate to 32 bits, reals cast, bools 0/1.
fn as_int(value: &Value) -> i32 {
    match value {
        Value::Number(number) => number
            .as_i64()
            .map(|int| int as i32)
            .or_else(|| number.as_u64().map(|uint| uint as i32))
            .unwrap_or_else(|| number.as_f64().unwrap_or(0.0) as i32),
        Value::Bool(flag) => i32::from(*flag),
        _ => 0,
    }
}

/// A string target reads only a JSON string, else `default`.
fn string(value: &Value, default: &str) -> String {
    value.as_str().unwrap_or(default).to_owned()
}

/// A typed property bag read: a present value of the right type.
fn bag_string(bag: &BTreeMap<String, Scalar>, name: &str) -> String {
    match bag.get(name) {
        Some(Scalar::Text(text)) => text.clone(),
        _ => String::new(),
    }
}

fn number(value: f32) -> Value {
    serde_json::Number::from_f64(f64::from(value)).map_or(Value::Null, Value::Number)
}

/// The authored `size`/`offset` pair, `[x, y]`.
fn axes(control: &ResolvedControl, key: &str, native: &Native) -> [Value; 2] {
    let pair = native
        .props
        .get(key)
        .or_else(|| control.properties.get(key))
        .and_then(Value::as_array);
    let axis = |index: usize| {
        pair.and_then(|pair| pair.get(index))
            .cloned()
            .unwrap_or_else(|| Value::from(0))
    };
    [axis(0), axis(1)]
}

/// Apply `target` = `value` (the bound value, or the binding expression's
/// result) to the control's components.
pub(super) fn apply(
    target: &str,
    value: &Value,
    control: &ResolvedControl,
    bag: &BTreeMap<String, Scalar>,
    native: &mut Native,
) {
    let kind = control.control_type.as_deref().unwrap_or("");
    let grid = kind == "grid";
    if grid
        && control
            .properties
            .get("grid_dimension_binding")
            .and_then(Value::as_str)
            == Some(target)
    {
        if let Some([columns, rows]) = value
            .as_array()
            .and_then(|pair| Some([as_int(pair.first()?), as_int(pair.get(1)?)]))
        {
            native
                .props
                .insert("grid_dimensions".to_owned(), json!([columns, rows]));
        }
        return;
    }
    let props = &mut native.props;
    let mut set = |key: &str, value: Value| {
        props.insert(key.to_owned(), value);
    };
    match target {
        "#maximum_grid_items" if grid => set("maximum_grid_items", Value::from(int(value, 0))),
        "#collection_length"
            if control.properties.contains_key("collection_name") || kind == "collection_panel" =>
        {
            let length = match value {
                Value::Array(_) => Some(value.clone()),
                other => {
                    Some(Value::from(int(other, -1))).filter(|length| length != &Value::from(-1))
                }
            };
            if length.is_some() {
                native.collection_length = length;
            }
        }
        "#visible" => set("visible", Value::Bool(boolean(value, true))),
        "#enabled" => {
            let enabled = boolean(value, true);
            set("enabled", Value::Bool(enabled));
            set("#enabled", Value::Bool(enabled));
        }
        "#alpha" => {
            let alpha = bag.get(target).map_or(Value::Null, Scalar::to_json);
            set("alpha", number(float(&alpha, 1.0)));
        }
        "#propagateAlpha" => set("propagate_alpha", Value::Bool(boolean(value, true))),
        "#clip_ratio" => set("clip_ratio", number(float(value, 0.0).clamp(0.0, 1.0))),
        "#texture" => set("texture", Value::String(string(value, ""))),
        "#texture_file_system" => set("texture_file_system", Value::String(string(value, ""))),
        "#zip_folder" => set("zip_folder", Value::String(string(value, ""))),
        "#grayscale" => set("grayscale", Value::Bool(boolean(value, false))),
        "#bilinear" => set("bilinear", Value::Bool(boolean(value, false))),
        "#nineslice_size" => {
            let size = match value {
                Value::Number(_) | Value::Bool(_) => {
                    Some(Value::Array(vec![number(float(value, 0.0)); 4]))
                }
                Value::Array(items) if items.len() == 4 => Some(Value::Array(
                    items.iter().map(|item| number(float(item, 0.0))).collect(),
                )),
                _ => None,
            };
            if let Some(size) = size {
                set("nineslice_size", size);
            }
        }
        "#color" => {
            let color = match value.as_array() {
                Some(items) if items.len() == 4 => {
                    Value::Array(items.iter().map(|item| number(float(item, 0.0))).collect())
                }
                _ => json!([1.0, 1.0, 1.0, 1.0]),
            };
            set("color", color.clone());
            set("#color", color);
        }
        "#toggle_state" => set("#toggle_state", Value::Bool(boolean(value, false))),
        "#toggle_group_forced_index" => {
            set("toggle_group_forced_index", Value::from(int(value, -1)));
        }
        "#slider_value" => set("#slider_value", number(float(value, 0.0))),
        "#slider_steps" => set("#slider_steps", Value::from(int(value, 1))),
        "#slider_timeout" => set("slider_timeout", number(float(value, 0.0))),
        "#font_scale_factor" => set("font_scale_factor", number(float(value, 1.0))),
        "#font_type" => set("font_type", Value::String(string(value, ""))),
        "#text_alignment" => set("text_alignment", Value::String(string(value, ""))),
        "#offset" => {
            if let Some([x, y]) = value
                .as_array()
                .and_then(|pair| Some([float(pair.first()?, 0.0), float(pair.get(1)?, 0.0)]))
            {
                set("offset", json!([number(x), number(y)]));
            }
        }
        "#size_binding_x"
        | "#size_binding_y"
        | "#size_binding_x_absolute"
        | "#size_binding_y_absolute" => {
            let [mut x, mut y] = axes(control, "size", native);
            let length = float(value, 0.0);
            let axis = match target {
                "#size_binding_x_absolute" | "#size_binding_y_absolute" => number(length),
                // A parent-relative size axis holds a fraction of the parent.
                _ => Value::String(format!("{}%", length * 100.0)),
            };
            if target.starts_with("#size_binding_x") {
                x = axis;
            } else {
                y = axis;
            }
            native
                .props
                .insert("size".to_owned(), Value::Array(vec![x, y]));
        }
        "#anchored_offset_value_x" | "#anchored_offset_value_y" => {
            let key = &target[1..];
            set(key, number(float(value, 0.0)));
        }
        "#priority" => {
            let priority = bag.get(target).map_or(Value::Null, Scalar::to_json);
            set("priority", Value::from(int(&priority, 0)));
        }
        "#focus_enabled" => set("focus_enabled", Value::Bool(boolean(value, true))),
        "#focus_wrap_enabled" => set("focus_wrap_enabled", Value::Bool(boolean(value, true))),
        "#default_focus_precedence" => {
            set("default_focus_precedence", Value::from(int(value, 0)));
        }
        "#focus_identifier"
        | "#focus_change_up"
        | "#focus_change_down"
        | "#focus_change_left"
        | "#focus_change_right"
        | "#focus_navigation_mode_up"
        | "#focus_navigation_mode_down"
        | "#focus_navigation_mode_left"
        | "#focus_navigation_mode_right" => {
            set(&target[1..], Value::String(bag_string(bag, target)));
        }
        "#modal" => set("modal", Value::Bool(boolean(value, false))),
        "#always_handle_controller_direction" => {
            set(
                "always_handle_controller_direction",
                Value::Bool(boolean(value, false)),
            );
        }
        "#gesture_control_enabled" => {
            let enabled = matches!(bag.get(target), Some(Scalar::Bool(true)));
            set("gesture_control_enabled", Value::Bool(enabled));
        }
        "#can_be_deselected" => set("can_be_deselected", Value::Bool(boolean(value, true))),
        "#init_selection_wheel_input_mode" => {
            set(
                "init_selection_wheel_input_mode",
                Value::from(int(value, 0).max(0)),
            );
        }
        _ => {}
    }
}
