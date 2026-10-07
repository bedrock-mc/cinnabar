//! Variable scopes, conditionals and factory declarations resolve as the vanilla
//! client resolves them; each fixture is one audited behaviour.

use json_ui::{Catalog, Context, ResolvedControl};
use serde_json::{Value, json};

fn catalog(body: &str) -> Catalog {
    let mut catalog = Catalog::default();
    catalog.overlay_text("ui/a.json", &format!(r#"{{"namespace":"a",{body}}}"#));
    catalog
}

fn resolve_in(body: &str, context: &Context) -> Option<ResolvedControl> {
    json_ui::resolve(&catalog(body), "a.root", context).control
}

fn root(body: &str) -> ResolvedControl {
    resolve_in(body, &Context::empty()).expect("root resolves")
}

fn text(control: &ResolvedControl) -> Value {
    control
        .properties
        .get("text")
        .cloned()
        .unwrap_or(Value::Null)
}

fn root_text(control: &str) -> Value {
    text(&root(&format!(r#""root":{control}"#)))
}

#[test]
fn empty_namespace_references_use_the_declaring_namespace() {
    let mut catalog = catalog(
        r#""root@.base":{"controls":[{"help@.header":{}},{"external@other.header":{}}]},
           "base":{"type":"panel","size":[120,30]},
           "header":{"type":"label","text":"Help"}"#,
    );
    catalog.overlay_text(
        "ui/other.json",
        r#"{"namespace":"other","header":{"type":"label","text":"External"}}"#,
    );
    let tree = json_ui::resolve(&catalog, "a.root", &Context::empty())
        .control
        .expect("relative base resolves");
    assert_eq!(tree.properties.get("size"), Some(&json!([120, 30])));
    assert_eq!(
        text(tree.child("help").expect("relative header")),
        json!("Help")
    );
    assert_eq!(
        text(tree.child("external").expect("qualified header")),
        json!("External")
    );
}

// A nearer `|default` beats an outer one; a block's default never beats a concrete value.
#[test]
fn defaults_resolve_after_every_concrete_scope() {
    let nearer = root(
        r#""root":{"type":"panel","$x|default":"outer","controls":[
            {"child":{"type":"label","$x|default":"inner","text":"$x"}}]}"#,
    );
    assert_eq!(text(&nearer.children[0]), json!("inner"));
    let concrete = root(
        r#""root":{"type":"panel","$x":"concrete","controls":[
            {"child":{"type":"label","variables":[{"requires":"true","$x|default":"fallback"}],
              "text":"$x"}}]}"#,
    );
    assert_eq!(text(&concrete.children[0]), json!("concrete"));
}

// Names are exact: dotted names and direct `|default` references resolve, and only
// the exact `|default` suffix has fallback meaning.
#[test]
fn variable_names_are_exact() {
    assert_eq!(
        root_text(r#"{"type":"label","$a.b":"OK","text":"$a.b"}"#),
        json!("OK")
    );
    assert_eq!(
        root_text(r#"{"type":"label","$x|default":"OK","text":"$x|default"}"#),
        json!("OK")
    );
    assert_eq!(
        root_text(r#"{"type":"label","$x|weird":"OK","text":"$x"}"#),
        json!("$x")
    );
}

// A structured variable keeps its nested strings for the consumer's own scope.
#[test]
fn structured_variables_resolve_in_their_consumers_scope() {
    let context = Context::empty().with_var("x", json!("outer"));
    let tree = resolve_in(
        r#""root":{"type":"panel","$children":[{"child":{"type":"label","$x":"inner","text":"$x"}}],
            "controls":"$children"}"#,
        &context,
    )
    .unwrap();
    assert_eq!(text(&tree.children[0]), json!("inner"));
}

// Only a whole-string `$name` is a reference; quoted operands stay literal.
#[test]
fn strings_are_not_interpolation_templates() {
    assert_eq!(
        root_text(r#"{"type":"label","$x":"red","text":"Color: $x"}"#),
        json!("Color: $x")
    );
    assert_eq!(
        root_text(r#"{"type":"label","$x":"red","text":"('$x')"}"#),
        json!("$x")
    );
    assert_eq!(
        root_text(r#"{"type":"label","$x":"Joe's","text":"('Hi ' + $x)"}"#),
        json!("Hi Joe's")
    );
}

// `__string` wrappers yield their `value`, raw text without evaluating it.
#[test]
fn wrapped_string_variables_unwrap() {
    assert_eq!(
        root_text(
            r#"{"type":"label","$s":{"__string":true,"__rawtext":true,"value":"(1 + 1)"},"text":"$s"}"#
        ),
        json!("(1 + 1)")
    );
    assert_eq!(
        root_text(r#"{"type":"label","$s":{"__string":true,"value":"(1 + 1)"},"text":"$s"}"#),
        json!(2)
    );
}

// A constant parenthesised property evaluates without any `$` reference; a
// token parses as an int from its leading digits, so
// `1.0` is 1 and only `.5` is a float.
#[test]
fn constant_property_expressions_fold() {
    let tree = root(r#""root":{"type":"panel","alpha":"(1.0 / 2.0)","layer":"(.5 * 3)"}"#);
    assert_eq!(tree.properties["alpha"], json!(0));
    assert_eq!(tree.properties["layer"], json!(1.5));
}

// `variables` may be one object or a `$var` holding the blocks.
#[test]
fn variables_accept_objects_and_variables() {
    assert_eq!(
        root_text(
            r#"{"type":"label","$x":"old","variables":{"requires":"true","$x":"new"},"text":"$x"}"#
        ),
        json!("new")
    );
    assert_eq!(
        root_text(
            r#"{"type":"label","$x":"old","$blocks":[{"requires":"true","$x":"new"}],"variables":"$blocks","text":"$x"}"#
        ),
        json!("new")
    );
}

// `requires` selects by type: nonzero numbers and nonempty strings; missing never.
#[test]
fn requires_uses_the_typed_dispatch() {
    let with = |condition: &str| {
        root_text(&format!(
            r#"{{"type":"label","$x":"old","variables":[{{"requires":{condition},"$x":"new"}}],"text":"$x"}}"#
        ))
    };
    for condition in ["false", "0", "null", "[]", r#""""#] {
        assert_eq!(with(condition), json!("old"), "{condition}");
    }
    for condition in [r#""false""#, r#""(1)""#, "true", "2"] {
        assert_eq!(with(condition), json!("new"), "{condition}");
    }
    assert_eq!(
        root_text(r#"{"type":"label","$x":"old","variables":[{"$x":"new"}],"text":"$x"}"#),
        json!("old")
    );
}

// `ignored` acts on integers and bools, not on literal strings, and reads the
// enclosing scope even at the root.
#[test]
fn ignored_uses_the_typed_dispatch_and_enclosing_scope() {
    assert!(resolve_in(r#""root":{"type":"panel","ignored":1}"#, &Context::empty()).is_none());
    assert!(
        resolve_in(
            r#""root":{"type":"panel","ignored":"(1)"}"#,
            &Context::empty()
        )
        .is_none()
    );
    assert!(
        resolve_in(
            r#""root":{"type":"panel","ignored":"true"}"#,
            &Context::empty()
        )
        .is_some()
    );
    let context = Context::empty().with_flag("omit", false);
    assert!(
        resolve_in(
            r#""root":{"type":"panel","$omit":true,"ignored":"$omit"}"#,
            &context
        )
        .is_some()
    );
}

// `factory` may be a `$var`, and its fields evaluate in the declaring scope.
#[test]
fn factory_declarations_evaluate() {
    let spec = root(
        r#""root":{"type":"panel","$spec":{"name":"f","control_ids":{"r":"a.r"}},"factory":"$spec"}"#,
    );
    let factory = spec.factory.expect("factory from a variable");
    assert_eq!(factory.name.as_deref(), Some("f"));
    assert_eq!(factory.control_ids["r"].name, "r");
    let named = root(
        r#""root":{"type":"panel","$n":"f","$m":1,"factory":{"name":"$n","max_children_size":"$m",
            "insert_location":"front","control_ids":{"r":"instance@a.r"},"factory_variables":["$n"]}}"#,
    );
    let factory = named.factory.unwrap();
    assert_eq!(factory.name.as_deref(), Some("f"));
    assert_eq!(factory.max_children_size, Some(1));
    assert!(factory.insert_front);
    assert_eq!(factory.instance_names["r"], "instance");
    assert_eq!(factory.variables["n"], json!("f"));
    let unlimited = root(
        r#""root":{"type":"panel","factory":{"name":"f","max_children_size":0,"control_ids":{"r":"a.r"}}}"#,
    );
    assert_eq!(unlimited.factory.unwrap().max_children_size, None);
}

// `control_name` wins over `control_ids`; a `type: "factory"` control's own fields
// win over a nested `factory` object.
#[test]
fn factory_declaration_precedence() {
    let template = root(
        r#""root":{"type":"panel","factory":{"name":"f","control_name":"a.r","control_ids":{"r":"a.s"}}}"#,
    );
    let factory = template.factory.unwrap();
    assert_eq!(factory.control_name.unwrap().name, "r");
    assert!(factory.control_ids.is_empty());
    let own = root(
        r#""root":{"type":"panel","controls":[{"f":{"type":"factory","control_ids":{"r":"a.r"},
            "factory":{"name":"g","control_ids":{"r":"a.s"}}}}]}"#,
    );
    let factory = own.children[0].factory.clone().unwrap();
    assert_eq!(factory.name.as_deref(), Some("f"));
    assert_eq!(factory.control_ids["r"].name, "r");
}

mod factories {
    use super::*;
    use json_ui::{CatalogLibrary, CollectionItem, DataSource, FactoryItem, Scalar};

    const TEMPLATES: &str = r#""r":{"type":"label","$x|default":"base","text":"$x"},
        "s":{"type":"label","text":"s"}"#;

    fn bound(host: &str, data: &DataSource) -> ResolvedControl {
        let catalog = catalog(&format!(
            r#"{TEMPLATES},"root":{{"type":"panel","controls":[{{"f":{host}}}]}}"#
        ));
        let context = Context::empty();
        let tree = json_ui::resolve(&catalog, "a.root", &context)
            .control
            .unwrap();
        json_ui::bind(
            &tree,
            data,
            &CatalogLibrary {
                catalog: &catalog,
                context: &context,
            },
        )
    }

    fn fed(items: Vec<FactoryItem>) -> DataSource {
        let mut data = DataSource::new();
        data.set_factory("f", items);
        data
    }

    fn names(control: &ResolvedControl) -> Vec<&str> {
        control
            .children
            .iter()
            .map(|child| child.name.as_str())
            .collect()
    }

    fn texts(control: &ResolvedControl) -> Vec<Value> {
        control.children.iter().map(text).collect()
    }

    // Captured `factory_variables` override a creation's own; nothing else of the host leaks.
    #[test]
    fn factory_variables_are_captured_by_whitelist() {
        let item = || vec![FactoryItem::new("r", 0.0).var("x", json!("event"))];
        let captured = bound(
            r#"{"type":"panel","$x":"host","factory":{"name":"f","control_ids":{"r":"a.r"},"factory_variables":["$x"]}}"#,
            &fed(item()),
        );
        assert_eq!(texts(&captured.children[0]), [json!("host")]);
        let plain = bound(
            r#"{"type":"panel","$x":"host","factory":{"name":"f","control_ids":{"r":"a.r"}}}"#,
            &fed(vec![FactoryItem::new("r", 0.0)]),
        );
        assert_eq!(texts(&plain.children[0]), [json!("base")]);
    }

    // A `control_name` template wins over the id and resolves in the declaring scope.
    #[test]
    fn templates_win_and_resolve_where_declared() {
        let host = bound(
            r#"{"type":"panel","$x":"host","factory":{"name":"f","control_name":"a.r","control_ids":{"r":"a.s"}}}"#,
            &fed(vec![FactoryItem::new("r", 0.0).var("x", json!("event"))]),
        );
        assert_eq!(texts(&host.children[0]), [json!("host")]);
    }

    // A named `control_ids` entry names its instance.
    #[test]
    fn named_references_keep_their_instance_name() {
        let host = bound(
            r#"{"type":"panel","factory":{"name":"f","control_ids":{"r":"instance@a.r"}}}"#,
            &fed(vec![FactoryItem::new("r", 0.0)]),
        );
        assert_eq!(names(&host.children[0]), ["instance"]);
    }

    // Zero is unlimited; the cap counts every child and front insertion evicts from the back.
    #[test]
    fn max_children_and_insert_location() {
        let two = || {
            vec![
                FactoryItem::new("r", 0.0).named("one"),
                FactoryItem::new("r", 1.0).named("two"),
            ]
        };
        let unlimited = bound(
            r#"{"type":"panel","factory":{"name":"f","max_children_size":0,"control_ids":{"r":"a.r"}}}"#,
            &fed(two()),
        );
        assert_eq!(names(&unlimited.children[0]), ["one", "two"]);
        let capped = bound(
            r#"{"type":"panel","$m":1,"factory":{"name":"f","max_children_size":"$m","control_ids":{"r":"a.r"}}}"#,
            &fed(two()),
        );
        assert_eq!(names(&capped.children[0]), ["two"]);
        let mut three = two();
        three.push(FactoryItem::new("r", 2.0).named("three"));
        let front = bound(
            r#"{"type":"panel","factory":{"name":"f","insert_location":"front","max_children_size":2,"control_ids":{"r":"a.r"}}}"#,
            &fed(three),
        );
        assert_eq!(names(&front.children[0]), ["three", "two"]);
    }

    // Literal children stay beside created ones; a template clears them.
    #[test]
    fn literal_children_follow_the_factory_mode() {
        let ids = bound(
            r#"{"type":"panel","controls":[{"literal":{"type":"panel"}}],"factory":{"name":"f","control_ids":{"r":"a.r"}}}"#,
            &fed(vec![FactoryItem::new("r", 0.0)]),
        );
        assert_eq!(names(&ids.children[0]), ["literal", "r"]);
        let template = bound(
            r#"{"type":"panel","controls":[{"literal":{"type":"panel"}}],"factory":{"name":"f","control_name":"a.r"}}"#,
            &DataSource::new(),
        );
        assert!(template.children[0].children.is_empty());
    }

    // A `type: "factory"` control creates into its parent.
    #[test]
    fn type_factories_create_siblings() {
        let host = bound(
            r#"{"type":"panel","controls":[{"g":{"type":"factory","control_ids":{"r":"a.r"}}}]}"#,
            &{
                let mut data = DataSource::new();
                data.set_factory("g", vec![FactoryItem::new("r", 0.0)]);
                data
            },
        );
        assert_eq!(names(&host.children[0]), ["g", "r"]);
        assert!(host.children[0].children[0].children.is_empty());
    }

    // An empty `collection_name` is inactive, so the named feed still creates.
    #[test]
    fn empty_collection_names_are_inactive() {
        let host = bound(
            r#"{"type":"panel","collection_name":"","factory":{"name":"f","control_ids":{"r":"a.r"}}}"#,
            &fed(vec![FactoryItem::new("r", 0.0)]),
        );
        assert_eq!(names(&host.children[0]), ["r"]);
    }

    // A collection item whose role the factory lacks creates nothing.
    #[test]
    fn unknown_collection_roles_create_nothing() {
        let mut data = DataSource::new();
        data.set_collection(
            "rows",
            vec![CollectionItem::new("unknown"), CollectionItem::new("r")],
        );
        let host = bound(
            r#"{"type":"panel","collection_name":"rows","factory":{"name":"f","control_ids":{"r":"a.r"}}}"#,
            &data,
        );
        assert_eq!(names(&host.children[0]), ["r"]);
    }

    fn row_text(host: &str) -> Vec<Value> {
        let mut data = DataSource::new();
        let rows = (0..3)
            .map(|index| CollectionItem::default().with("#t", Scalar::Text(format!("row{index}"))))
            .collect();
        data.set_collection("rows", rows);
        let tree = bound(host, &data);
        let mut found = Vec::new();
        let mut stack = vec![&tree];
        while let Some(control) = stack.pop() {
            if control.name == "leaf" {
                found.push(text(control));
            }
            stack.extend(&control.children);
        }
        found
    }

    const BIND: &str = r##""bindings":[{"binding_type":"collection","binding_collection_name":"rows","binding_name":"#t","binding_name_override":"#text"}],"text":"#text""##;

    // An item index counts only under the collection's own panel; an opted-out child is excluded.
    #[test]
    fn collection_items_are_direct_children_of_the_panel() {
        let nested = row_text(&format!(
            r#"{{"type":"panel","collection_name":"rows","controls":[{{"wrap":{{"type":"panel","collection_index":1,
                "controls":[{{"leaf":{{"type":"label","collection_index":2,{BIND}}}}}]}}}}]}}"#
        ));
        assert_eq!(nested, [json!("row1")]);
        let ignored = row_text(&format!(
            r#"{{"type":"panel","collection_name":"rows","controls":[{{"leaf":{{"type":"label","collection_index":2,
                "ignoreCollectionItem":true,{BIND}}}}}]}}"#
        ));
        assert_eq!(ignored, [json!("row0")]);
    }
}

// A screen's `ignored` reads the scope before its own `$` values, for settings as for resolution.
#[test]
fn screen_settings_read_ignored_before_own_variables() {
    let catalog = catalog(r#""root":{"type":"screen","$hide":true,"ignored":"$hide"}"#);
    let context = Context::empty();
    assert!(
        json_ui::resolve(&catalog, "a.root", &context)
            .control
            .is_some()
    );
    assert!(json_ui::screen_settings("a.root", &catalog, &context).is_some());
}
