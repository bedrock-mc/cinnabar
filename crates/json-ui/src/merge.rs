//! `@base` inheritance. As in the vanilla client, a derived definition selects
//! each property whole: a property it names (objects and `controls` included)
//! replaces the base's, and only the properties it omits come from the base.

use std::collections::HashSet;

use crate::catalog::{Catalog, RawControl};
use crate::tree::ControlRef;

/// Longest `@base` chain flattened; a longer server-supplied chain rejects the control.
const MAX_CHAIN: usize = 256;

/// Flatten `(ns, name)` along its literal `@base` chain into one control, with the
/// immediate base recorded for provenance. Returns `None` if the control is absent
/// or its chain exceeds [`MAX_CHAIN`]. A `$var` base is left for the caller.
pub fn flatten_def(
    catalog: &Catalog,
    namespace: &str,
    name: &str,
    diagnostics: &mut Vec<String>,
) -> Option<(RawControl, Option<ControlRef>)> {
    flatten(catalog, namespace, name, diagnostics, true)
}

/// Applies the same inheritance rules without copying descendant controls.
pub(crate) fn flatten_properties(
    catalog: &Catalog,
    namespace: &str,
    name: &str,
    diagnostics: &mut Vec<String>,
) -> Option<RawControl> {
    flatten(catalog, namespace, name, diagnostics, false).map(|(control, _)| control)
}

fn flatten(
    catalog: &Catalog,
    namespace: &str,
    name: &str,
    diagnostics: &mut Vec<String>,
    children: bool,
) -> Option<(RawControl, Option<ControlRef>)> {
    let top = catalog.lookup(namespace, name)?;
    let provenance = literal_base(top);
    let mut seen = HashSet::from([(namespace.to_owned(), name.to_owned())]);
    let mut chain = vec![top];
    while let Some(current) = chain.last().copied()
        && let Some(base_ref) = literal_base(current)
    {
        let label = format!("{}.{}", current.owner_ns, current.name);
        if !seen.insert((base_ref.namespace.clone(), base_ref.name.clone())) {
            diagnostics.push(format!("{label}: inheritance cycle through {base_ref}"));
            break;
        }
        let Some(base) = catalog.lookup(&base_ref.namespace, &base_ref.name) else {
            diagnostics.push(format!("{label}: base {base_ref} not found"));
            break;
        };
        if chain.len() >= MAX_CHAIN {
            diagnostics.push(format!(
                "{namespace}.{name}: inheritance chain longer than {MAX_CHAIN}; dropped"
            ));
            return None;
        }
        chain.push(base);
    }
    let mut chain = chain.into_iter().rev();
    let first = chain.next()?;
    let mut flattened = clear_base(if children {
        first.clone()
    } else {
        RawControl {
            owner_ns: first.owner_ns.clone(),
            name: first.name.clone(),
            base: first.base.clone(),
            props: first.props.clone(),
            children: Vec::new(),
            has_controls: first.has_controls,
        }
    });
    for child in chain {
        flattened = clear_base(inherit_with_children(
            &flattened,
            child,
            Layering::Document,
            children,
        ));
    }
    Some((flattened, provenance))
}

fn literal_base(control: &RawControl) -> Option<ControlRef> {
    let base = control
        .base
        .as_deref()
        .filter(|base| !base.starts_with('$'))?;
    Some(ControlRef::parse(base, &control.owner_ns))
}

/// How a derived definition's explicit `null` treats the base's property.
#[derive(Clone, Copy)]
pub enum Layering {
    /// A named `@base` document: any member the derived one names, even `null`, wins.
    Document,
    /// An inline `name@base` entry: a `null` member reads through to the base.
    Inline,
}

/// `child` over `base` by whole-property selection, keeping `child`'s identity.
pub fn inherit(base: &RawControl, child: &RawControl, layering: Layering) -> RawControl {
    inherit_with_children(base, child, layering, true)
}

fn inherit_with_children(
    base: &RawControl,
    child: &RawControl,
    layering: Layering,
    children: bool,
) -> RawControl {
    let mut props = base.props.clone();
    if child.has_controls {
        props.remove("controls");
    }
    for (key, value) in &child.props {
        if value.is_null() && matches!(layering, Layering::Inline) {
            continue;
        }
        props.insert(key.clone(), value.clone());
    }
    RawControl {
        owner_ns: child.owner_ns.clone(),
        name: child.name.clone(),
        base: child.base.clone().or_else(|| base.base.clone()),
        props,
        children: if !children {
            Vec::new()
        } else if child.has_controls {
            child.children.clone()
        } else {
            base.children.clone()
        },
        has_controls: child.has_controls || base.has_controls,
    }
}

fn clear_base(mut control: RawControl) -> RawControl {
    control.base = None;
    control
}

#[cfg(test)]
mod tests {
    use super::{Layering, flatten_def, inherit};
    use crate::catalog::{Catalog, RawControl};
    use serde_json::{Value, json};

    /// `c0@ns.c1`, `c1@ns.c2`, ...; the last one points back at `c0` when `cyclic`.
    fn chain(len: usize, cyclic: bool) -> Catalog {
        let mut text = String::from(r#"{"namespace":"ns""#);
        for index in 0..len {
            let base = match (index + 1 < len, cyclic) {
                (true, _) => format!("@ns.c{}", index + 1),
                (false, true) => "@ns.c0".to_owned(),
                (false, false) => String::new(),
            };
            text.push_str(&format!(r#","c{index}{base}":{{"p{index}":{index}}}"#));
        }
        text.push('}');
        let mut catalog = Catalog::default();
        catalog.load_text("chain.json", &text);
        catalog
    }

    // A long server-supplied chain must be rejected, not overflow the stack.
    #[test]
    fn long_acyclic_inheritance_chain_is_rejected_with_a_diagnostic() {
        let mut diagnostics = Vec::new();
        let flattened = flatten_def(&chain(10_000, false), "ns", "c0", &mut diagnostics);
        assert!(flattened.is_none());
        assert!(
            diagnostics
                .iter()
                .any(|message| message.contains("inheritance chain"))
        );
    }

    #[test]
    fn bounded_chains_flatten_and_cycles_stop_at_the_repeat() {
        let mut diagnostics = Vec::new();
        let (control, base) =
            flatten_def(&chain(100, false), "ns", "c0", &mut diagnostics).expect("bounded chain");
        assert_eq!(control.props.len(), 100);
        assert_eq!(control.props.get("p99"), Some(&json!(99)));
        assert_eq!(base.map(|base| base.name), Some("c1".to_owned()));
        assert!(control.base.is_none() && diagnostics.is_empty());

        let (control, _) = flatten_def(&chain(3, true), "ns", "c0", &mut diagnostics)
            .expect("cyclic chain keeps the control");
        assert_eq!(control.props.len(), 3);
        assert!(
            diagnostics
                .iter()
                .any(|message| message.contains("inheritance cycle"))
        );
    }

    fn control(
        name: &str,
        base: Option<&str>,
        props: Value,
        children: Vec<RawControl>,
    ) -> RawControl {
        let Value::Object(props) = props else {
            unreachable!()
        };
        RawControl {
            owner_ns: "ns".to_owned(),
            name: name.to_owned(),
            base: base.map(str::to_owned),
            props,
            has_controls: !children.is_empty(),
            children,
        }
    }

    fn leaf(name: &str, base: Option<&str>) -> RawControl {
        control(name, base, json!({}), Vec::new())
    }

    #[test]
    fn child_overrides_scalars_and_keeps_base_only_properties() {
        let base = control(
            "btn",
            None,
            json!({ "size": [1, 1], "color": "base" }),
            Vec::new(),
        );
        let child = control(
            "btn",
            Some("ns.base"),
            json!({ "size": [2, 2] }),
            Vec::new(),
        );
        let merged = inherit(&base, &child, Layering::Document);
        assert_eq!(merged.props.get("size"), Some(&json!([2, 2])));
        assert_eq!(merged.props.get("color"), Some(&json!("base")));
    }

    // A derived `controls` array replaces the base's children outright.
    #[test]
    fn derived_controls_replace_inherited_children() {
        let base = control(
            "btn",
            None,
            json!({}),
            vec![leaf("old", None), leaf("kept", None)],
        );
        let child = control("btn", None, json!({}), vec![leaf("new", None)]);
        let merged = inherit(&base, &child, Layering::Document);
        let names: Vec<&str> = merged.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["new"]);
        let mut empty = control("btn", None, json!({}), Vec::new());
        empty.has_controls = true;
        assert!(
            inherit(&base, &empty, Layering::Document)
                .children
                .is_empty()
        );
        let silent = control("btn", None, json!({}), Vec::new());
        assert_eq!(
            inherit(&base, &silent, Layering::Document).children.len(),
            2
        );
    }

    // A derived object property is selected whole, not merged key by key.
    #[test]
    fn derived_objects_replace_the_base_object() {
        let base = control("p", None, json!({ "map": { "x": 1, "y": 2 } }), Vec::new());
        let child = control("p", None, json!({ "map": { "x": 3 } }), Vec::new());
        let merged = inherit(&base, &child, Layering::Document);
        assert_eq!(merged.props.get("map"), Some(&json!({ "x": 3 })));
    }

    // An inline entry's `null` reads the base; a document's `null` shadows it.
    #[test]
    fn null_members_follow_the_layering() {
        let base = control("p", None, json!({ "alpha": 0.5 }), Vec::new());
        let child = control("p", None, json!({ "alpha": null }), Vec::new());
        assert_eq!(
            inherit(&base, &child, Layering::Inline).props["alpha"],
            json!(0.5)
        );
        assert_eq!(
            inherit(&base, &child, Layering::Document).props["alpha"],
            json!(null)
        );
    }
}
