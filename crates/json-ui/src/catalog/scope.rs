//! Isolated definition namespaces for content embedded in an application-owned shell.

use super::{Catalog, RawControl};
use crate::ControlRef;
use std::{collections::BTreeMap, sync::Arc};

/// Handles a snapshot of definitions without replacing their original names.
#[derive(Clone, Debug, PartialEq)]
pub struct ControlScope {
    namespaces: Arc<BTreeMap<String, String>>,
}

impl ControlScope {
    /// The isolated name of an existing definition; other namespaces retain their names.
    pub fn reference(&self, reference: &ControlRef) -> ControlRef {
        ControlRef::new(
            self.namespaces
                .get(&reference.namespace)
                .unwrap_or(&reference.namespace),
            &reference.name,
        )
    }

    /// Uses this snapshot for a content subtree while keeping its public entry name.
    pub fn apply(&self, control: &mut RawControl) {
        control.definition_scope = Some(self.clone());
        for child in &mut control.children {
            self.apply(child);
        }
    }

    /// Whether a definition belongs to this snapshot.
    pub fn contains_namespace(&self, namespace: &str) -> bool {
        self.namespaces.values().any(|name| name == namespace)
    }
}

impl Catalog {
    /// Snapshots current, unscoped definitions into collision-free namespaces. References
    /// from those definitions stay in the snapshot, including references selected by variables.
    /// Variable values and ordinary text are unchanged; global variables remain catalog-owned.
    pub fn scope_definitions(&mut self) -> ControlScope {
        let namespaces = self
            .defs
            .keys()
            .filter(|name| !self.scopes.contains_key(*name))
            .cloned()
            .collect::<Vec<_>>();
        let mut serial = 0;
        let names = loop {
            let names = namespaces
                .iter()
                .enumerate()
                .map(|(index, original)| (original.clone(), format!("__scope_{serial}_{index}")))
                .collect::<BTreeMap<_, _>>();
            if names.values().all(|name| !self.defs.contains_key(name)) {
                break names;
            }
            serial += 1;
        };
        let names = Arc::new(names);
        let scope = ControlScope {
            namespaces: Arc::clone(&names),
        };
        let mut copies = Vec::new();
        for namespace in &namespaces {
            for control in self.defs[namespace].values() {
                let mut copy = control.clone();
                relocate(&mut copy, &names);
                scope.apply(&mut copy);
                copies.push(copy);
            }
        }
        for namespace in names.values() {
            self.scopes.insert(namespace.clone(), Arc::clone(&names));
        }
        for copy in copies {
            self.insert(copy);
        }
        scope
    }
}

/// Moves namespace ownership without rewriting text or unevaluated variable values.
fn relocate(control: &mut RawControl, namespaces: &BTreeMap<String, String>) {
    if let Some(namespace) = namespaces.get(&control.owner_ns) {
        control.owner_ns.clone_from(namespace);
    }
    for child in &mut control.children {
        relocate(child, namespaces);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Context, resolve};

    #[test]
    fn scoped_references_follow_variables_without_rewriting_text_values() {
        let mut catalog = Catalog::default();
        catalog.overlay_text(
            "ui/test.json",
            r#"{"namespace":"test",
            "base":{"type":"label","color":[1,0,0]},
            "root":{"type":"panel","$template":"test.base","controls":[
                {"caption@$template":{"text":"$template"}}]}}"#,
        );
        let scope = catalog.scope_definitions();
        catalog.overlay_text(
            "ui/test.json",
            r#"{"namespace":"test","base":{"ignored":true}}"#,
        );
        let reference = scope.reference(&ControlRef::new("test", "root"));
        let root = resolve(&catalog, &reference.to_string(), &Context::empty())
            .control
            .unwrap();
        assert_eq!(root.children[0].properties["text"], "test.base");
        assert_eq!(
            root.children[0].properties["color"],
            serde_json::json!([1, 0, 0])
        );
        assert!(
            resolve(&catalog, "test.base", &Context::empty())
                .control
                .is_none()
        );
    }
    #[test]
    fn scoped_animation_references_keep_their_identity_across_qualified_links() {
        let mut catalog = Catalog::default();
        catalog.overlay_text(
            "ui/test.json",
            r#"{"namespace":"test",
            "fade":{"anim_type":"alpha","duration":1,"from":0.25,"to":1,"next":"@test.fade"},
            "root":{"type":"label","text":"Animated","alpha":"@fade"}}"#,
        );
        let scope = catalog.scope_definitions();
        catalog.overlay_text(
            "ui/test.json",
            r#"{"namespace":"test","fade":{"from":0.9}}"#,
        );
        let reference = scope.reference(&ControlRef::new("test", "root"));
        let root = resolve(&catalog, &reference.to_string(), &Context::empty())
            .control
            .unwrap();
        let graph: crate::AnimGraph =
            serde_json::from_value(root.properties["anim_graph"].clone()).unwrap();
        assert_eq!(root.properties["alpha"], 0.25);
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.nodes[0].next, Some(0));
    }

    #[test]
    fn empty_scoped_grid_templates_keep_literal_children() {
        for template in ["", "$template"] {
            let mut catalog = Catalog::default();
            catalog.overlay_text(
                "ui/test.json",
                &serde_json::json!({"namespace":"test",
                    "root":{"type":"grid", "$template":"", "grid_item_template":template,
                        "grid_dimensions":[1,1],
                        "controls":[{"caption":{"type":"label", "text":"Literal caption"}}]}
                })
                .to_string(),
            );
            let scope = catalog.scope_definitions();
            let reference = scope.reference(&ControlRef::new("test", "root"));
            let context = Context::empty();
            let root = resolve(&catalog, &reference.to_string(), &context)
                .control
                .unwrap();
            let mut state = crate::BindState::new();
            let data = crate::DataSource::new();
            let bound = crate::bind_screen(&root, &catalog, &context, &data, &mut state);
            let caption = bound.child("caption").expect("literal grid child survives");
            assert_eq!(caption.properties["text"], "Literal caption");
        }
    }
}
