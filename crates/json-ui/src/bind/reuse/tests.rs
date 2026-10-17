//! Incremental binds rebuild only the subtrees that read a changed key.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::{BindState, DataSource, EmptyLibrary, ResolvedControl, Scalar};

fn control(
    name: &str,
    kind: &str,
    properties: Value,
    children: Vec<ResolvedControl>,
) -> ResolvedControl {
    let Value::Object(properties) = properties else {
        unreachable!("control properties are an object");
    };
    ResolvedControl {
        name: name.into(),
        control_type: Some(kind.into()),
        base: None,
        unresolved_base: None,
        properties: properties
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>()
            .into(),
        children,
        factory: None,
    }
}

/// A label showing the screen global `name`.
fn reader(control_name: &str, name: &str) -> ResolvedControl {
    control(
        control_name,
        "label",
        json!({ "text": name, "bindings": [{ "binding_name": name }] }),
        Vec::new(),
    )
}

#[test]
fn creation_text_does_not_replace_a_view_bound_caption_on_refresh() {
    let root = Arc::new(control(
        "row",
        "panel",
        json!({}),
        vec![
            control(
                "source",
                "panel",
                json!({"bindings":[{
                    "binding_type":"view","source_property_name":"(#text - 'private:')",
                    "target_property_name":"#caption"
                }]}),
                vec![],
            ),
            control(
                "caption",
                "label",
                json!({"text":"#text","bindings":[{
                    "binding_type":"view","source_control_name":"source","resolve_sibling_scope":true,
                    "source_property_name":"#caption","target_property_name":"#text"
                }]}),
                vec![],
            ),
        ],
    ));
    for incremental in [false, true] {
        let mut state = BindState::new();
        for (input, caption) in [
            ("private:shown", "shown"),
            ("shown", "shown"),
            ("private:next", "next"),
            ("private:next", "next"),
        ] {
            let mut data = DataSource::new();
            data.set_creation_value("#text", Scalar::Text(input.into()));
            let data = Arc::new(data);
            let bound = if incremental {
                crate::bind_incremental(&root, &data, &EmptyLibrary, &mut state)
            } else {
                crate::bind_stateful(&root, &data, &EmptyLibrary, &mut state).0
            };
            assert_eq!(
                bound.children[1].properties["text"], caption,
                "creation text leaked during refresh (incremental={incremental})"
            );
        }
    }
}

#[test]
fn own_scope_views_receive_changed_creation_inputs() {
    for expression in ["#text", "(#text - 'encoded:')"] {
        let root = Arc::new(control(
            "caption",
            "label",
            json!({"text":"#text","bindings":[{
                "binding_type":"view","source_property_name":expression,"target_property_name":"#text"
            }]}),
            vec![],
        ));
        for incremental in [false, true] {
            let mut state = BindState::new();
            for input in ["first", "first", "encoded:first", "encoded:next", "next"] {
                let mut data = DataSource::new();
                data.set_creation_value("#text", Scalar::Text(input.into()));
                let data = Arc::new(data);
                let bound = if incremental {
                    crate::bind_incremental(&root, &data, &EmptyLibrary, &mut state)
                } else {
                    crate::bind_stateful(&root, &data, &EmptyLibrary, &mut state).0
                };
                let expected = if expression == "#text" {
                    input
                } else {
                    input.trim_start_matches("encoded:")
                };
                assert_eq!(
                    bound.properties["text"], expected,
                    "{expression}, incremental={incremental}"
                );
            }
        }
    }
}

#[test]
fn chained_views_receive_changed_creation_inputs() {
    for reverse in [false, true] {
        let mut bindings = vec![
            json!({"binding_type":"view","source_property_name":"#text","target_property_name":"#raw"}),
            json!({"binding_type":"view","source_property_name":"(#raw - 'encoded:')","target_property_name":"#text"}),
        ];
        if reverse {
            bindings.reverse();
        }
        let root = Arc::new(control(
            "caption",
            "label",
            json!({"text":"#text","bindings":bindings}),
            vec![],
        ));
        for incremental in [false, true] {
            let mut state = BindState::new();
            for input in ["first", "encoded:first", "encoded:next", "next"] {
                let mut data = DataSource::new();
                data.set_creation_value("#text", Scalar::Text(input.into()));
                let data = Arc::new(data);
                let bound = if incremental {
                    crate::bind_incremental(&root, &data, &EmptyLibrary, &mut state)
                } else {
                    crate::bind_stateful(&root, &data, &EmptyLibrary, &mut state).0
                };
                assert_eq!(
                    bound.properties["text"],
                    input.trim_start_matches("encoded:"),
                    "reverse={reverse}, incremental={incremental}"
                );
            }
        }
    }
}

#[test]
fn retained_factory_views_receive_changed_item_values() {
    let mut catalog = crate::Catalog::default();
    catalog.overlay_text("ui/fixture.json", &json!({"namespace":"fixture",
        "root":{"type":"panel","factory":{"name":"rows","control_ids":{"row":"fixture.caption"}}},
        "caption":{"type":"label","text":"#text","bindings":[{
            "binding_type":"view","source_property_name":"(#text - 'encoded:')",
            "target_property_name":"#text"}]}
    }).to_string());
    let context = crate::Context::empty();
    let root = crate::resolve(&catalog, "fixture.root", &context)
        .control
        .unwrap();
    let mut state = BindState::new();
    for input in ["first", "encoded:first", "encoded:next", "next"] {
        let mut data = DataSource::new();
        data.set_factory(
            "rows",
            vec![
                crate::FactoryItem::new("row", 0.0)
                    .identified(1)
                    .value("#text", Scalar::Text(input.into())),
            ],
        );
        let bound = crate::bind_screen(&root, &catalog, &context, &data, &mut state);
        assert_eq!(
            bound.children[0].properties["text"],
            input.trim_start_matches("encoded:")
        );
    }
}

#[test]
fn refreshed_creation_targets_preserve_the_last_view_writer() {
    let root = Arc::new(control(
        "caption",
        "label",
        json!({"text":"#text","bindings":[
            {"binding_type":"view","source_property_name":"#a","target_property_name":"#text"},
            {"binding_type":"view","source_property_name":"#b","target_property_name":"#text"}
        ]}),
        vec![],
    ));
    for incremental in [false, true] {
        let mut state = BindState::new();
        for input in ["seed", "B", "B", "next", "B"] {
            let mut data = DataSource::new();
            for (name, value) in [("#text", input), ("#a", "A"), ("#b", "B")] {
                data.set_creation_value(name, Scalar::Text(value.into()));
            }
            let data = Arc::new(data);
            let bound = if incremental {
                crate::bind_incremental(&root, &data, &EmptyLibrary, &mut state)
            } else {
                crate::bind_stateful(&root, &data, &EmptyLibrary, &mut state).0
            };
            assert_eq!(
                bound.properties["text"], "B",
                "incremental={incremental}, input={input}"
            );
        }
    }
}

#[test]
fn changed_creation_inputs_bound_cyclic_view_notifications() {
    for count in [10, 100] {
        let root = Arc::new(control(
            "cycle",
            "label",
            json!({"text":"#x", "bindings":
            (0..count).map(|_| json!({"binding_type":"view", "source_property_name":"(#x + 1)",
                "target_property_name":"#x"})).collect::<Vec<_>>() }),
            vec![],
        ));
        let mut state = BindState::new();
        for value in [0, 1] {
            let mut data = DataSource::new();
            data.set_creation_value("#x", Scalar::Int(value));
            crate::bind_stateful(&root, &data, &EmptyLibrary, &mut state);
            assert!(
                state.views_run() <= 2 * crate::bind::view::VIEW_PASSES * count,
                "cyclic input evaluated {} views for {count} bindings",
                state.views_run()
            );
        }
    }
}

#[test]
fn an_unread_change_leaves_a_subtree_reading_many_keys_standing() {
    // Enough distinct reads that a fixed-size summary of them would claim every key.
    const READERS: usize = 2_000;
    let wide = control(
        "wide",
        "panel",
        json!({}),
        (0..READERS)
            .map(|index| reader(&format!("reader{index}"), &format!("#value{index}")))
            .collect(),
    );
    let root = Arc::new(control(
        "root",
        "panel",
        json!({}),
        vec![wide, reader("narrow", "#other")],
    ));
    let data = |other: &str| {
        let mut data = DataSource::new();
        for index in 0..READERS {
            data.set_global(format!("#value{index}"), Scalar::Text("shown".into()));
        }
        data.set_global("#other", Scalar::Text(other.into()));
        Arc::new(data)
    };
    let mut state = BindState::new();
    // The second bind settles what the first created.
    for other in ["first", "first", "second"] {
        crate::bind_incremental(&root, &data(other), &EmptyLibrary, &mut state);
    }
    let rebuilt = state.rebuilt_paths();
    assert!(
        rebuilt.iter().any(|path| path == "/root/narrow"),
        "{rebuilt:?}"
    );
    assert!(
        !rebuilt.iter().any(|path| path.starts_with("/root/wide")),
        "{rebuilt:?}"
    );
}
