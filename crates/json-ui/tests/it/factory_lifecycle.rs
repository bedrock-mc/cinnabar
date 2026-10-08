use std::sync::Arc;

use json_ui::{
    BindState, CachedLibrary, Catalog, CatalogLibrary, Context, DataSource, FactoryItem,
    ResolveCache, ResolvedControl, Scalar, bind_incremental,
};
use serde_json::json;

#[test]
fn replacement_factory_birth_resets_only_its_own_once_bindings() {
    verify_lifecycle(false);
}

#[test]
fn factory_message_identity_replaces_a_control_at_the_same_birth_time() {
    verify_lifecycle(true);
}

fn verify_lifecycle(identified: bool) {
    let mut catalog = Catalog::default();
    catalog.overlay_text(
        "ui/lifecycle.json",
        r##"{"namespace":"lifecycle",
        "root":{"type":"panel","controls":[
            {"retained":{"type":"label","text":"#text","bindings":[
                {"binding_name":"#value","binding_name_override":"#text","binding_condition":"once"}
            ]}},
            {"factory":{"type":"panel","factory":{"name":"feed","control_ids":{"label":"lifecycle.instance"}}}}
        ]},
        "instance":{"type":"panel","controls":[
            {"label":{"type":"label","text":"#text","bindings":[
                {"binding_name":"#value","binding_name_override":"#text","binding_condition":"once"}
            ]}},
            {"gated":{"type":"panel","bindings":[
                {"binding_name":"#show","binding_name_override":"#visible"}
            ],"controls":[{"label":{"type":"label","text":"#text","bindings":[
                {"binding_name":"#value","binding_name_override":"#text","binding_condition":"once"}
            ]}}]}}
        ]}}
        "##,
    );
    let context = Context::empty();
    let root = Arc::new(
        json_ui::resolve(&catalog, "lifecycle.root", &context)
            .control
            .unwrap(),
    );
    let mut state = BindState::new();
    let mut cache = ResolveCache::default();
    let mut refresh = |value: &str, born: f64, shown: bool| {
        let mut data = DataSource::new();
        data.set_global("#value", Scalar::Text(value.into()));
        data.set_global("#show", Scalar::Bool(shown));
        let item = FactoryItem::new("label", if identified { 0.0 } else { born }).named("label");
        let item = if identified {
            item.identified(born as u64)
        } else {
            item
        };
        data.set_factory("feed", vec![item]);
        bind_incremental(
            &root,
            &Arc::new(data),
            &CachedLibrary {
                library: CatalogLibrary {
                    catalog: &catalog,
                    context: &context,
                },
                cache: &mut cache,
            },
            &mut state,
        )
    };
    assert_eq!(
        labels(&refresh("first", 0.0, true)),
        ["first", "first", "first"]
    );
    assert_eq!(
        labels(&refresh("ordinary update", 0.0, false)),
        ["first", "first"]
    );
    assert_eq!(
        labels(&refresh("replacement", 1.0, false)),
        ["first", "replacement"],
        "a replacement creates fresh binding state without resetting its sibling"
    );
    assert_eq!(
        labels(&refresh("replacement", 1.0, true)),
        ["first", "replacement", "replacement"],
        "a dormant descendant belongs to the replacement's incarnation"
    );
    assert_eq!(
        labels(&refresh("later update", 1.0, true)),
        ["first", "replacement", "replacement"]
    );
    refresh("later update", 1.0, true);
    assert_eq!(
        state.rebuilt(),
        0,
        "an unchanged instance rebuilds no controls"
    );
}

fn labels(control: &ResolvedControl) -> Vec<String> {
    let mut texts = Vec::new();
    if control.properties.get("visible") == Some(&json!(false)) {
        return texts;
    }
    if control.control_type.as_deref() == Some("label")
        && let Some(text) = control
            .properties
            .get("text")
            .and_then(serde_json::Value::as_str)
    {
        texts.push(text.to_owned());
    }
    for child in &control.children {
        texts.extend(labels(child));
    }
    texts
}

#[test]
fn dormant_nested_factory_keeps_its_own_incarnation_below_the_same_outer_instance() {
    let mut catalog = Catalog::default();
    catalog.overlay_text("ui/nested.json", r##"{"namespace":"nested",
        "root":{"type":"panel","factory":{"name":"outer","control_ids":{"role":"nested.outer"}}},
        "outer":{"type":"panel","controls":[{"gate":{"type":"panel","bindings":[
            {"binding_name":"#show","binding_name_override":"#visible"}
        ],"controls":[{"inner":{"type":"panel","factory":{"name":"inner","control_ids":{"role":"nested.inner"}}}}]}}]},
        "inner":{"type":"label","text":"#text","bindings":[
            {"binding_name":"#value","binding_name_override":"#text","binding_condition":"once"}
        ]}}
        "##);
    let context = Context::empty();
    let root = Arc::new(
        json_ui::resolve(&catalog, "nested.root", &context)
            .control
            .unwrap(),
    );
    let mut state = BindState::new();
    let mut cache = ResolveCache::default();
    let mut refresh = |value: &str, shown: bool, outer: u64| {
        let mut data = DataSource::new();
        data.set_global("#value", Scalar::Text(value.into()));
        data.set_global("#show", Scalar::Bool(shown));
        data.set_factory(
            "outer",
            vec![FactoryItem::new("role", 0.0).identified(outer)],
        );
        data.set_factory("inner", vec![FactoryItem::new("role", 0.0).identified(7)]);
        bind_incremental(
            &root,
            &Arc::new(data),
            &CachedLibrary {
                library: CatalogLibrary {
                    catalog: &catalog,
                    context: &context,
                },
                cache: &mut cache,
            },
            &mut state,
        )
    };
    assert_eq!(labels(&refresh("first", true, 1)), ["first"]);
    assert!(labels(&refresh("changed while hidden", false, 1)).is_empty());
    assert_eq!(labels(&refresh("changed while hidden", true, 1)), ["first"]);
    assert!(labels(&refresh("replacement", false, 2)).is_empty());
    assert_eq!(labels(&refresh("replacement", true, 2)), ["replacement"]);
}

#[test]
fn custom_control_lifetime_survives_hiding_but_not_factory_destruction() {
    let mut catalog = Catalog::default();
    catalog.overlay_text(
        "ui/custom_lifetime.json",
        &json!({
            "namespace":"lifetime",
            "root":{"type":"panel", "controls":[{"factory":{"type":"factory",
                "factory":{"name":"feed", "control_ids":{"item":"lifetime.item"}}}}]},
            "item":{"type":"panel", "bindings":[
                {"binding_name":"#shown", "binding_name_override":"#visible"}
            ], "controls":[{"renderer":{"type":"custom", "renderer":"test_renderer"}}]}
        })
        .to_string(),
    );
    let context = Context::empty();
    let root = Arc::new(
        json_ui::resolve(&catalog, "lifetime.root", &context)
            .control
            .unwrap(),
    );
    let mut state = BindState::new();
    let mut refresh = |present: bool, shown: bool| {
        let mut data = DataSource::new();
        data.set_global("#shown", Scalar::Bool(shown));
        data.set_factory(
            "feed",
            if present {
                vec![FactoryItem::new("item", 0.0).named("same_name")]
            } else {
                Vec::new()
            },
        );
        bind_incremental(
            &root,
            &Arc::new(data),
            &CatalogLibrary {
                catalog: &catalog,
                context: &context,
            },
            &mut state,
        )
    };
    let identity = |tree: &ResolvedControl| {
        fn find(tree: &ResolvedControl) -> Option<u64> {
            tree.properties
                .get(json_ui::CUSTOM_CONTROL_INSTANCE_KEY)
                .and_then(serde_json::Value::as_u64)
                .or_else(|| tree.children.iter().find_map(find))
        }
        find(tree).expect("custom control has a lifetime identity")
    };
    let first = identity(&refresh(true, true));
    refresh(true, false);
    assert_eq!(
        identity(&refresh(true, true)),
        first,
        "hidden ancestor keeps its renderer"
    );
    assert_eq!(
        identity(&refresh(true, true)),
        first,
        "unchanged control keeps its identity"
    );
    refresh(false, true);
    assert_ne!(
        identity(&refresh(true, true)),
        first,
        "recreation starts a new lifetime"
    );
}

#[test]
fn a_live_custom_factory_control_refreshes_literal_bags_without_replacing_its_lifetime() {
    let mut catalog = Catalog::default();
    catalog.overlay_text(
        "ui/custom_bag.json",
        &json!({
            "namespace":"bag",
            "root":{"type":"panel", "controls":[{"factory":{"type":"factory",
                "factory":{"name":"feed", "control_ids":{"item":"bag.item"}}}}]},
            "item":{"type":"custom", "renderer":"test_renderer", "property_bag":{"#x":"$x"}}
        })
        .to_string(),
    );
    let context = Context::empty();
    let root = Arc::new(
        json_ui::resolve(&catalog, "bag.root", &context)
            .control
            .unwrap(),
    );
    let mut state = BindState::new();
    let mut first_identity = None;
    for value in [1, 2] {
        let mut data = DataSource::new();
        data.set_factory(
            "feed",
            vec![
                FactoryItem::new("item", 0.0)
                    .named("same_name")
                    .identified(1)
                    .var("x", json!(value)),
            ],
        );
        let tree = bind_incremental(
            &root,
            &Arc::new(data),
            &CatalogLibrary {
                catalog: &catalog,
                context: &context,
            },
            &mut state,
        );
        let custom = tree
            .find(&|control| control.control_type.as_deref() == Some("custom"))
            .unwrap();
        assert_eq!(
            custom.properties.get("#x"),
            Some(&json!(value)),
            "factory variables reach the live renderer"
        );
        let identity = custom
            .properties
            .get(json_ui::CUSTOM_CONTROL_INSTANCE_KEY)
            .unwrap()
            .clone();
        assert_eq!(
            first_identity.get_or_insert(identity.clone()),
            &identity,
            "updating parameters keeps the control lifetime"
        );
    }
}
