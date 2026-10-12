//! Application-owned death-reason text policy layered over pack presentation.
use json_ui::ResolvedControl;
use serde_json::Value;

use ui_engine::engine::screen::ScreenRegistry;

pub(super) const REASON_BINDING: &str = "#death_reason_text";

/// Keeps resolved reason labels literal in every engine screen and factory-created control.
pub fn register(registry: &mut ScreenRegistry) {
    registry.tree_hook(literal_label);
}

/// Recurses through authored controls and marks resolved reason labels as literal.
fn literal_label(control: &mut ResolvedControl) {
    let direct = control.properties.get("text").and_then(Value::as_str) == Some(REASON_BINDING);
    let bound_text = control
        .properties
        .get("bindings")
        .and_then(Value::as_array)
        .is_some_and(|bindings| {
            bindings.iter().any(|binding| {
                binding.get("binding_name").and_then(Value::as_str) == Some(REASON_BINDING)
                    && binding.get("binding_name_override").and_then(Value::as_str) == Some("#text")
            })
        });
    if direct || bound_text {
        control
            .properties
            .insert("localize".into(), Value::Bool(false));
    }
    for child in &mut control.children {
        literal_label(child);
    }
}
