//! Application-owned death-reason text policy layered over pack presentation.
use json_ui::{Catalog, RawControl};
use serde_json::Value;

pub(super) const REASON_BINDING: &str = "#death_reason_text";

/// Keeps already formatted death reasons literal in vanilla and server labels.
pub(super) fn prefer_literal_reason(catalog: &mut Catalog) {
    for control in catalog.controls_mut() {
        literal_label(control);
    }
}

/// Recurses through authored controls and marks resolved reason labels as literal.
fn literal_label(control: &mut RawControl) {
    let direct = control.props.get("text").and_then(Value::as_str) == Some(REASON_BINDING);
    let bound_text = control
        .props
        .get("bindings")
        .and_then(Value::as_array)
        .is_some_and(|bindings| {
            bindings.iter().any(|binding| {
                binding.get("binding_name").and_then(Value::as_str) == Some(REASON_BINDING)
                    && binding.get("binding_name_override").and_then(Value::as_str) == Some("#text")
            })
        });
    if direct || bound_text {
        control.props.insert("localize".into(), Value::Bool(false));
    }
    for child in &mut control.children {
        literal_label(child);
    }
}
