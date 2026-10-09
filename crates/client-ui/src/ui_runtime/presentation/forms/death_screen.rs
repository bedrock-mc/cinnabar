//! Application-owned death-reason text policy layered over pack presentation.
use json_ui::{CatalogLibrary, ControlLibrary, ControlRef, ResolvedControl};
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) const REASON_BINDING: &str = "#death_reason_text";

/// Marks reason labels after variable substitution and inheritance resolution.
pub(super) fn literal_tree(tree: Option<ResolvedControl>) -> Option<ResolvedControl> {
    tree.map(|mut tree| {
        literal_label(&mut tree);
        tree
    })
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

/// Applies the same text policy to labels created by factories and grids.
pub(super) struct LiteralReasonLibrary<'a>(pub(super) CatalogLibrary<'a>);

impl ControlLibrary for LiteralReasonLibrary<'_> {
    fn resolve(&self, reference: &ControlRef) -> Option<ResolvedControl> {
        literal_tree(self.0.resolve(reference))
    }

    fn resolve_with(
        &self,
        reference: &ControlRef,
        key: &str,
        vars: &dyn Fn() -> BTreeMap<String, Value>,
    ) -> Option<ResolvedControl> {
        literal_tree(self.0.resolve_with(reference, key, vars))
    }
}
