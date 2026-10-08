//! Cached outputs must agree with fresh baking after input changes.

use super::*;
use crate::bind::{EmptyLibrary, bind_shared};
use crate::{DataSource, ResolvedControl, Scalar};
use serde_json::json;

/// A minimal label whose properties can be shared by cloned templates.
fn label() -> ResolvedControl {
    ResolvedControl {
        name: "label".into(),
        control_type: Some("label".into()),
        base: None,
        unresolved_base: None,
        properties: [("text".into(), json!("#text"))].into(),
        children: Vec::new(),
        factory: None,
    }
}

/// A cache entry with independently editable bag, native and instance inputs.
fn node() -> Node {
    Node {
        src: crate::bind::source::Src::root(Arc::new(label())),
        key: 1,
        layout_key: String::new(),
        own: Bag::default(),
        native: crate::bind::native::Native::default(),
        memory: crate::bind::state::Retained::default(),
        bindings: Arc::default(),
        children: Vec::new(),
        deferred: None,
        retained: false,
        scope: Default::default(),
        track: Default::default(),
    }
}

#[test]
fn bag_native_and_instance_changes_invalidate_outputs() {
    let mut node = node();
    let properties = [("text".into(), json!("cached"))].into();
    put(&node, properties);
    assert!(get(&node).is_some());
    node.own.insert("#text".into(), Scalar::Text("next".into()));
    assert!(get(&node).is_none());
    node.own.clear();
    assert!(get(&node).is_some());
    node.native.props.insert("visible".into(), json!(false));
    assert!(get(&node).is_none());
    node.native.props.clear();
    node.scope.incarnation = Some(7);
    assert!(get(&node).is_none());
    node.scope.incarnation = None;
    assert!(get(&node).is_some());
    node.src = node.src.patched(|patch| {
        patch
            .properties
            .insert("grid_position".into(), json!([1, 2]));
    });
    assert!(get(&node).is_none());
}

#[test]
fn changing_dependencies_and_component_writes_match_cold_baking() {
    let mut root = label();
    root.children.push(label());
    root.properties.insert(
        "property_bag_for_children".into(),
        json!({"#text":"inherited"}),
    );
    root.properties.insert(
        "bindings".into(),
        json!([
            {"binding_name":"#text"},
            {"binding_name":"#shown", "binding_name_override":"#visible"}
        ]),
    );
    let mut root = Arc::new(root);
    let mut data = DataSource::new();
    for (text, shown, inherited) in [
        ("first", true, "one"),
        ("next", false, "two"),
        ("last", true, "three"),
    ] {
        data.set_global("#text", Scalar::Text(text.into()));
        data.set_global("#shown", Scalar::Bool(shown));
        Arc::make_mut(&mut root).properties.insert(
            "property_bag_for_children".into(),
            json!({"#text":inherited}),
        );
        let warm = bind_shared(&root, &data, &EmptyLibrary);
        assert_eq!(warm.properties["text"], json!(text));
        assert_eq!(warm.properties["visible"], json!(shown));
        if shown {
            assert_eq!(warm.children[0].properties["text"], json!(inherited));
        }
        CACHE.with(|cache| cache.borrow_mut().clear());
        assert_eq!(warm, bind_shared(&root, &data, &EmptyLibrary));
    }
    for text in ["component one", "component two"] {
        let mut components = crate::Components::default();
        components.write("/label", "text", json!(text));
        data.set_components(components);
        let warm = bind_shared(&root, &data, &EmptyLibrary);
        assert_eq!(warm.properties["text"], json!(text));
        CACHE.with(|cache| cache.borrow_mut().clear());
        assert_eq!(warm, bind_shared(&root, &data, &EmptyLibrary));
    }
    data.set_components(crate::Components::default());
    assert_eq!(
        bind_shared(&root, &data, &EmptyLibrary).properties["text"],
        json!("last")
    );
}

#[test]
fn many_live_instances_keep_the_cache_bounded() {
    CACHE.with(|cache| cache.borrow_mut().clear());
    let mut node = node();
    for key in 0..(MAX_OUTPUTS + 10) {
        node.key = key as u64;
        put(&node, Properties::default());
    }
    CACHE.with(|cache| assert!(cache.borrow().len() <= MAX_OUTPUTS));
}

/// A replaceable library models a refreshed factory template in the same screen.
struct Library(std::cell::RefCell<ResolvedControl>);

impl crate::ControlLibrary for Library {
    fn resolve(&self, _: &crate::ControlRef) -> Option<ResolvedControl> {
        Some(self.0.borrow().clone())
    }
}

#[test]
fn factory_template_replacement_cannot_reuse_stale_properties() {
    let library = Library(std::cell::RefCell::new(label()));
    let mut root = label();
    root.factory = Some(crate::Factory {
        control_ids: [("body".into(), crate::ControlRef::parse("test.body", ""))].into(),
        ..crate::Factory::default()
    });
    let root = Arc::new(root);
    let mut data = DataSource::new();
    data.set_factory_id("body");
    for text in ["first template", "replacement template"] {
        library
            .0
            .borrow_mut()
            .properties
            .insert("text".into(), json!(text));
        let warm = bind_shared(&root, &data, &library);
        assert_eq!(warm.children[0].properties["text"], json!(text));
        CACHE.with(|cache| cache.borrow_mut().clear());
        assert_eq!(warm, bind_shared(&root, &data, &library));
    }
}
