use std::sync::Arc;

use json_ui::{BindState, DataSource, EmptyLibrary, ResolvedControl, bind_stateful};
use serde_json::{Value, json};

#[global_allocator]
static ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;

/// An original control fixture with no pack or renderer dependency.
fn control(name: &str, properties: Value, children: Vec<ResolvedControl>) -> ResolvedControl {
    ResolvedControl {
        name: name.into(),
        control_type: Some("label".into()),
        base: None,
        unresolved_base: None,
        properties: properties
            .as_object()
            .unwrap()
            .clone()
            .into_iter()
            .collect(),
        children,
        factory: None,
    }
}

/// A sibling-scoped label observing the named control's bag.
fn reader(name: &str, source: &str) -> ResolvedControl {
    control(
        name,
        json!({"text":"#out", "bindings":[{
            "binding_type":"view", "source_control_name":source, "resolve_sibling_scope":true,
            "source_property_name":"#value", "target_property_name":"#out"
        }]}),
        vec![],
    )
}

/// A named source whose value identifies which duplicate won resolution.
fn source(name: &str, value: &str) -> ResolvedControl {
    control(name, json!({"property_bag":{"#value":value}}), vec![])
}

#[test]
fn sibling_sources_keep_parent_scope_and_breadth_first_order() {
    let root = Arc::new(control(
        "root",
        json!({}),
        vec![
            source("target", "outside"),
            control(
                "target",
                json!({"property_bag":{"#value":"parent"}}),
                vec![reader("parent_reader", "target"), source("target", "child")],
            ),
            control(
                "group",
                json!({}),
                vec![
                    control("nested", json!({}), vec![source("target", "deep")]),
                    reader("reader", "target"),
                    source("target", "first"),
                    source("target", "second"),
                ],
            ),
        ],
    ));
    let (bound, _) = bind_stateful(
        &root,
        &DataSource::new(),
        &EmptyLibrary,
        &mut BindState::new(),
    );
    assert_eq!(bound.children[1].children[0].properties["text"], "parent");
    assert_eq!(bound.children[2].children[1].properties["text"], "first");
}

#[test]
fn sibling_sources_resolve_after_hidden_subtree_expansion_and_refresh() {
    let root = Arc::new(control(
        "root",
        json!({}),
        vec![control(
            "group",
            json!({}),
            vec![
                control(
                    "hidden",
                    json!({"visible":false}),
                    vec![control(
                        "target",
                        json!({"bindings":[{
                            "binding_name":"#input", "binding_name_override":"#value"
                        }]}),
                        vec![],
                    )],
                ),
                reader("reader", "target"),
            ],
        )],
    ));
    let mut state = BindState::new();
    for value in ["first", "next"] {
        let mut data = DataSource::new();
        data.set_global("#input", json_ui::Scalar::Text(value.into()));
        let (bound, _) = bind_stateful(&root, &data, &EmptyLibrary, &mut state);
        assert_eq!(bound.children[0].children[1].properties["text"], value);
    }
}

/// Allocation requests for a wide sibling list, including the actual bind and output.
fn wide_bind_allocations(count: usize) -> usize {
    let mut children: Vec<_> = (0..count)
        .map(|i| reader(&format!("reader{i}"), &format!("source{i}")))
        .collect();
    children.extend((0..count).map(|i| source(&format!("source{i}"), "found")));
    let root = Arc::new(control("root", json!({}), children));
    let mut state = BindState::new();
    let data = DataSource::new();
    let ((bound, _), allocations) =
        test_alloc::count(|| bind_stateful(&root, &data, &EmptyLibrary, &mut state));
    for reader in &bound.children[..count] {
        assert_eq!(reader.properties["text"], "found");
    }
    allocations
}

#[test]
fn wide_sibling_resolution_has_linear_allocation_growth() {
    let small = wide_bind_allocations(128);
    let large = wide_bind_allocations(512);
    // Four times as many controls allows linear bookkeeping overhead, but not a
    // separate path-allocating tree traversal for every sibling binding.
    assert!(
        large <= small * 6,
        "128 readers: {small} allocations; 512 readers: {large}"
    );
}
