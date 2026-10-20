//! Incremental binds rebuild only the subtrees that read a changed key.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::{BindState, DataSource, EmptyLibrary, ResolvedControl, Scalar};

/// Counts the expanded tree, including controls materialized by nested factories.
fn budget_nodes(tree: &ResolvedControl) -> usize {
    1 + tree.children.iter().map(budget_nodes).sum::<usize>()
}

/// A stable catalog with factory references whose feeds can exceed a caller's budget.
fn budget_catalog() -> crate::Catalog {
    let mut catalog = crate::Catalog::default();
    catalog.overlay_text("ui/budget.json", r#"{"namespace":"budget",
        "leaf":{"type":"panel"},
        "row":{"type":"panel","factory":{"name":"leaves","control_ids":{"leaf":"budget.leaf"}}},
        "nested":{"type":"panel","factory":{"name":"rows","control_ids":{"row":"budget.row"}}},
        "reuse":{"type":"panel","controls":[
            {"expanding":{"type":"panel","factory":{"name":"rows","control_ids":{"leaf":"budget.leaf"}}}},
            {"stable":{"type":"panel","controls":[{"a":{}},{"b":{}},{"c":{}},{"d":{}},{"e":{}},{"f":{}},{"g":{}},{"h":{}}]}}
        ]}}"#);
    catalog
}

#[test]
fn a_caller_budget_limits_nested_factory_materialization_on_each_bind() {
    let catalog = budget_catalog();
    let context = crate::Context::default();
    let root = Arc::new(
        crate::resolve(&catalog, "budget.nested", &context)
            .control
            .unwrap(),
    );
    let library = crate::CatalogLibrary {
        catalog: &catalog,
        context: &context,
    };
    let mut state = BindState::new().with_node_limit(17);
    assert_eq!(BindState::default().node_limit(), crate::resolve::MAX_NODES);
    assert_eq!(BindState::new().with_node_limit(0).node_limit(), 1);
    assert_eq!(
        BindState::new().with_node_limit(usize::MAX).node_limit(),
        crate::resolve::MAX_NODES
    );
    for revision in 0..3 {
        let mut data = DataSource::new();
        data.set_factory(
            "rows",
            (0..100)
                .map(|_| crate::FactoryItem::new("row", 0.0))
                .collect(),
        );
        data.set_factory(
            "leaves",
            (0..100)
                .map(|_| crate::FactoryItem::new("leaf", 0.0))
                .collect(),
        );
        data.set_global("#unread", Scalar::Num(f64::from(revision)));
        let bound = crate::bind_incremental(&root, &Arc::new(data), &library, &mut state);
        assert_eq!(budget_nodes(&bound), 17);
        assert!(state.node_budget_exceeded);
        assert!(
            state.rebuilt() <= 17,
            "materialized more controls than the cap"
        );
    }
}

#[test]
fn reused_subtrees_share_the_creation_budget_and_recover_after_pressure() {
    let catalog = budget_catalog();
    let context = crate::Context::default();
    let root = Arc::new(
        crate::resolve(&catalog, "budget.reuse", &context)
            .control
            .unwrap(),
    );
    let library = crate::CatalogLibrary {
        catalog: &catalog,
        context: &context,
    };
    let mut state = BindState::new().with_node_limit(17);
    let mut bound = None;
    let mut measures = crate::MeasureCache::default();
    for (rows, expected) in [(0, 11), (10, 17), (0, 11), (10, 17)] {
        let mut data = DataSource::new();
        data.set_factory(
            "rows",
            (0..rows)
                .map(|_| crate::FactoryItem::new("leaf", 0.0))
                .collect(),
        );
        let data = Arc::new(data);
        if let Some(bound) = bound.as_mut() {
            crate::rebind(&root, &data, &library, &mut state, bound, &mut measures);
        } else {
            bound = Some(crate::bind_incremental(&root, &data, &library, &mut state));
        }
        assert_eq!(budget_nodes(bound.as_ref().unwrap()), expected);
        assert!(state.tree.as_ref().unwrap().track.count <= 17);
    }
}

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

/// Collects the rendered captions, excluding a grid's retained unbound template.
fn captions(control: &ResolvedControl) -> Vec<String> {
    if control
        .properties
        .get(crate::layout::GRID_TEMPLATE_KEY)
        .and_then(Value::as_bool)
        == Some(true)
    {
        return Vec::new();
    }
    let own = (control.control_type.as_deref() == Some("label"))
        .then(|| {
            control
                .properties
                .get("text")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .flatten();
    own.into_iter()
        .chain(control.children.iter().flat_map(captions))
        .collect()
}

#[test]
fn scoped_controller_answers_follow_collection_instances_and_refresh() {
    struct Labels(ResolvedControl);
    impl crate::ControlLibrary for Labels {
        /// Returns the original label template for each fixture row.
        fn resolve(&self, _: &crate::ControlRef) -> Option<ResolvedControl> {
            Some(self.0.clone())
        }
    }
    let library = Labels(reader("caption", "#caption"));
    let root = Arc::new(control(
        "grid",
        "grid",
        json!({
            "collection_name": "rows", "grid_dimensions": [1, 2], "grid_item_template": "fixture.caption"
        }),
        vec![],
    ));
    for incremental in [false, true] {
        let mut state = BindState::new();
        for labels in [["first", "second"], ["first", "changed"]] {
            let mut data = DataSource::new();
            data.set_global("#caption", Scalar::Text("fallback".into()));
            data.set_collection(
                "rows",
                labels
                    .into_iter()
                    .map(|label| {
                        crate::CollectionItem::default()
                            .with_controller_value("#caption", Scalar::Text(label.into()))
                    })
                    .collect(),
            );
            let data = Arc::new(data);
            let bound = if incremental {
                crate::bind_incremental(&root, &data, &library, &mut state)
            } else {
                crate::bind_stateful(&root, &data, &library, &mut state).0
            };
            assert_eq!(captions(&bound), labels, "incremental={incremental}");
        }
    }
}

#[test]
fn nested_collection_counts_read_the_enclosing_items_list() {
    struct Templates {
        group: ResolvedControl,
        label: ResolvedControl,
    }
    impl crate::ControlLibrary for Templates {
        /// Resolves the fixture's two nested collection templates.
        fn resolve(&self, reference: &crate::ControlRef) -> Option<ResolvedControl> {
            Some(if reference.name == "group" {
                self.group.clone()
            } else {
                self.label.clone()
            })
        }
    }
    let mut group = control(
        "entries",
        "stack_panel",
        json!({
            "collection_name": "entries", "bindings": [{
                "binding_type": "collection", "binding_collection_name": "entries",
                "binding_name": "#count", "binding_name_override": "#collection_length"
            }]
        }),
        vec![],
    );
    group.factory = Some(crate::Factory {
        name: Some("rows".into()),
        control_name: Some(crate::ControlRef::new("fixture", "label")),
        ..Default::default()
    });
    let templates = Templates {
        group,
        label: reader("caption", "#caption"),
    };
    let root = Arc::new(control(
        "groups",
        "grid",
        json!({
            "collection_name": "groups", "grid_dimensions": [1, 2], "grid_item_template": "fixture.group"
        }),
        vec![],
    ));
    for incremental in [false, true] {
        let mut state = BindState::new();
        for lengths in [[1, 2], [2, 1]] {
            let mut data = DataSource::new();
            data.set_collection("groups", vec![crate::CollectionItem::default(); 2]);
            for (group, count) in lengths.into_iter().enumerate() {
                data.set_scoped_collection(
                    "groups",
                    group,
                    "entries",
                    (0..count)
                        .map(|index| {
                            crate::CollectionItem::default()
                                .with("#count", Scalar::Int(count as i64))
                                .with_controller_value(
                                    "#caption",
                                    Scalar::Text(format!("{group}:{index}")),
                                )
                        })
                        .collect(),
                );
            }
            let data = Arc::new(data);
            let bound = if incremental {
                crate::bind_incremental(&root, &data, &templates, &mut state)
            } else {
                crate::bind_stateful(&root, &data, &templates, &mut state).0
            };
            let expected: Vec<_> = lengths
                .into_iter()
                .enumerate()
                .flat_map(|(group, count)| (0..count).map(move |index| format!("{group}:{index}")))
                .collect();
            assert_eq!(captions(&bound), expected, "incremental={incremental}");
        }
    }
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

#[test]
fn a_static_grid_enters_its_own_collection_without_an_authored_cell_index() {
    struct Geometry;
    impl crate::TextMeasure for Geometry {
        /// Gives the synthetic label a fixed nonzero extent.
        fn extent(&self, _: &str) -> [f64; 2] {
            [20.0, 10.0]
        }
    }
    impl crate::TextureSource for Geometry {
        /// This fixture contains no texture-backed controls.
        fn texture(&self, _: &str) -> Option<crate::TextureMeta> {
            None
        }
    }
    let env = crate::LayoutEnv {
        text: &Geometry,
        textures: &Geometry,
    };
    let root = Arc::new(control(
        "outer",
        "grid",
        json!({"collection_name":"outer", "grid_dimensions":[1,1]}),
        vec![control(
            "outer_cell",
            "panel",
            json!({}),
            vec![control(
                "inner",
                "grid",
                json!({"collection_name":"inner", "grid_dimensions":[1,1]}),
                vec![control(
                    "preview",
                    "button",
                    json!({"size":[20,20]}),
                    vec![reader("caption", "#caption")],
                )],
            )],
        )],
    ));
    let mut state = BindState::new();
    for text in ["nested", "updated"] {
        let mut data = DataSource::new();
        data.set_collection(
            "outer",
            vec![
                crate::CollectionItem::default()
                    .with_controller_value("#caption", Scalar::Text("wrong outer value".into())),
            ],
        );
        data.set_scoped_collection(
            "outer",
            0,
            "inner",
            vec![
                crate::CollectionItem::default()
                    .with_controller_value("#caption", Scalar::Text(text.into())),
            ],
        );
        let bound = crate::bind_incremental(&root, &Arc::new(data), &EmptyLibrary, &mut state);
        assert_eq!(captions(&bound), [text]);
        let placed = crate::layout(&bound, [100.0, 100.0], &env);
        let hits = crate::hit_regions(&placed);
        let preview = hits
            .iter()
            .find(|hit| hit.name == "preview")
            .expect("preview hit");
        assert_eq!(
            preview.collections,
            [("outer".to_owned(), 0), ("inner".to_owned(), 0)]
        );
    }
}

#[test]
fn a_direct_item_collection_binding_publishes_its_hit_identity() {
    struct Geometry;
    impl crate::TextMeasure for Geometry {
        /// Gives this fixture's controls a nonzero text extent.
        fn extent(&self, _: &str) -> [f64; 2] {
            [20.0, 10.0]
        }
    }
    impl crate::TextureSource for Geometry {
        /// No texture-backed controls are needed for this hit test.
        fn texture(&self, _: &str) -> Option<crate::TextureMeta> {
            None
        }
    }
    let name = crate::vanilla::Binding::ItemIdAux.as_str();
    let root = Arc::new(control(
        "preview",
        "custom",
        json!({
            "size":[20,20], "renderer":"inventory_item_renderer", "bindings":[{
                "binding_name":name, "binding_type":"collection", "binding_collection_name":"preview_items"
            }]
        }),
        vec![],
    ));
    let mut data = DataSource::new();
    data.set_collection(
        "preview_items",
        vec![crate::CollectionItem::default().with(name, Scalar::Int(6))],
    );
    let bound =
        crate::bind_incremental(&root, &Arc::new(data), &EmptyLibrary, &mut BindState::new());
    let env = crate::LayoutEnv {
        text: &Geometry,
        textures: &Geometry,
    };
    let placed = crate::layout(&bound, [100.0, 100.0], &env);
    let hits = crate::hit_regions(&placed);
    let preview = crate::hit_test(&hits, [50.0, 50.0]).expect("item preview hit");
    assert_eq!(preview.collections, [("preview_items".to_owned(), 0)]);
}
