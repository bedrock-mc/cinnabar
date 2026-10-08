use std::sync::Arc;

use json_ui::{
    BindState, Catalog, CatalogLibrary, CollectionItem, Context, DataSource, ResolvedControl,
    Scalar, bind, bind_incremental, resolve,
};
use serde_json::json;

fn catalog() -> Catalog {
    let mut catalog = Catalog::default();
    catalog.overlay_text(
        "ui/conditional.json",
        &json!({
            "namespace": "conditional",
            "screen": {
                "type": "panel",
                "controls": [{"screen_factory": {
                    "type": "factory",
                    "control_ids": {"long_form": "@conditional.layouts"}
                }}]
            },
            "layouts": {
                "type": "panel",
                "controls": [
                    {"tiles@conditional.selected": {
                        "$flag": "&tiles", "$roles": ["tiles"]
                    }},
                    {"rows@conditional.selected": {
                        "$flag": "&rows", "$roles": ["long_form"]
                    }}
                ]
            },
            "selected": {
                "type": "collection_panel",
                "property_bag": {"#if_true": "$roles", "#if_false": []},
                "factory": {
                    "name": "layout_factory",
                    "control_ids": {
                        "tiles": "tile_layout@conditional.tile_list",
                        "long_form": "row_layout@conditional.row_list"
                    }
                },
                "bindings": [
                    {"binding_name": "#title_text"},
                    {
                        "binding_type": "view",
                        "source_property_name": "(not ((('%.30s' * #title_text) - $flag) = ('%.30s' * #title_text)))",
                        "target_property_name": "#result"
                    },
                    {
                        "binding_type": "view",
                        "source_property_name": "('#if_' + #result)",
                        "target_property_name": "#collection_length"
                    }
                ]
            },
            "tile_list": {
                "type": "stack_panel", "collection_name": "items",
                "factory": {"name": "items", "control_name": "conditional.tile"}
            },
            "tile": {
                "type": "label", "text": "#text",
                "bindings": [{
                    "binding_type": "collection", "binding_collection_name": "items",
                    "binding_name": "#text"
                }]
            },
            "row_list": {"type": "label", "text": "rows"}
        })
        .to_string(),
    );
    catalog
}

fn data(title: &str) -> DataSource {
    let mut data = DataSource::new();
    data.set_factory_id("long_form");
    data.set_global("#title_text", Scalar::Text(title.to_owned()));
    data.set_collection(
        "items",
        ["Alpha", "Beta"]
            .into_iter()
            .map(|text| CollectionItem::new("tile").with("#text", Scalar::Text(text.to_owned())))
            .collect(),
    );
    data
}

fn labels(control: &ResolvedControl) -> Vec<&str> {
    let mut result = Vec::new();
    if control.control_type.as_deref() == Some("label") {
        result.extend(
            control
                .properties
                .get("text")
                .and_then(|text| text.as_str()),
        );
    }
    for child in &control.children {
        result.extend(labels(child));
    }
    result
}

#[test]
fn view_selected_roles_do_not_take_the_screen_factory_id() {
    let catalog = catalog();
    let context = Context::desktop();
    let library = CatalogLibrary {
        catalog: &catalog,
        context: &context,
    };
    let root = resolve(&catalog, "conditional.screen", &context)
        .control
        .unwrap();

    for (title, expected) in [
        ("&tiles Choose a game", vec!["Alpha", "Beta"]),
        ("&rows Choose an action", vec!["rows"]),
        ("Unmatched", vec![]),
    ] {
        let bound = bind(&root, &data(title), &library);
        assert_eq!(labels(&bound), expected, "{title}");
        if title.starts_with("&tiles") {
            assert!(
                bound
                    .find(&|control| control.name == "tile_layout")
                    .is_some()
            );
            assert!(
                bound
                    .find(&|control| control.name == "row_layout")
                    .is_none()
            );
        }
    }
}

#[test]
fn changing_the_selector_refreshes_roles_and_an_unchanged_bind_reuses_them() {
    let catalog = catalog();
    let context = Context::desktop();
    let library = CatalogLibrary {
        catalog: &catalog,
        context: &context,
    };
    let root = Arc::new(
        resolve(&catalog, "conditional.screen", &context)
            .control
            .unwrap(),
    );
    let mut state = BindState::new();
    for (title, expected) in [
        ("&tiles Choose a game", vec!["Alpha", "Beta"]),
        ("&rows Choose an action", vec!["rows"]),
        ("Unmatched", vec![]),
        ("&tiles Choose a game", vec!["Alpha", "Beta"]),
    ] {
        let data = Arc::new(data(title));
        let bound = bind_incremental(&root, &data, &library, &mut state);
        assert_eq!(labels(&bound), expected, "{title}");
        let repeated = bind_incremental(&root, &data, &library, &mut state);
        assert_eq!(labels(&repeated), expected, "repeat {title}");
        let settled = bind_incremental(&root, &data, &library, &mut state);
        assert_eq!(labels(&settled), expected, "settled {title}");
        assert!(
            state.rebuilt_paths().is_empty(),
            "repeat {title} rebuilt controls"
        );
    }
}

#[test]
fn a_new_nested_factory_waits_for_its_own_role_view() {
    let mut catalog = catalog();
    catalog.overlay_text(
        "ui/nested.json",
        &json!({
            "namespace": "conditional",
            "outer": {
                "type": "collection_panel",
                "factory": {
                    "name": "outer_factory",
                    "factory_variables": ["$flag", "$roles"],
                    "control_ids": {"selected": "@conditional.selected"}
                },
                "$flag": "&tiles", "$roles": ["tiles"],
                "property_bag": {"#roles": ["selected"]},
                "bindings": [{
                    "binding_type": "view", "source_property_name": "#roles",
                    "target_property_name": "#collection_length"
                }]
            }
        })
        .to_string(),
    );
    let context = Context::desktop();
    let library = CatalogLibrary {
        catalog: &catalog,
        context: &context,
    };
    let root = resolve(&catalog, "conditional.outer", &context)
        .control
        .unwrap();
    let bound = bind(&root, &data("&tiles Choose a game"), &library);
    assert_eq!(labels(&bound), ["Alpha", "Beta"]);
}
