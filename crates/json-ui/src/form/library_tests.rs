//! Factory prototype budgets apply before binding or caching expanded controls.

use super::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// An authored prototype much wider than the caller's retained presentation cap.
fn wide_catalog() -> Catalog {
    let mut catalog = Catalog::default();
    catalog.overlay_globals_text(r#"{"$title":"global"}"#);
    let children: Vec<_> = (0..1_000)
        .map(|index| json!({ format!("leaf{index}"): {"type":"panel"} }))
        .collect();
    catalog.overlay_text(
        "ui/limited.json",
        &json!({
            "namespace":"limited",
            "prototype":{"type":"panel", "text":"$title", "controls":children}
        })
        .to_string(),
    );
    catalog
}

/// Counts prototype nodes before any binder has had a chance to truncate them.
fn count(tree: &ResolvedControl) -> usize {
    1 + tree.children.iter().map(count).sum::<usize>()
}

#[test]
fn standalone_and_cached_factory_prototypes_resolve_with_the_requested_budget() {
    let catalog = wide_catalog();
    let context = Context::default().with_var("title", json!("context"));
    let reference = ControlRef::parse("limited.prototype", "limited");
    let library = || CatalogLibrary {
        catalog: &catalog,
        context: &context,
    };
    assert_eq!(count(&library().resolve(&reference).unwrap()), 1_001);
    assert_eq!(
        count(&library().with_node_limit(17).resolve(&reference).unwrap()),
        17
    );

    let mut cache = ResolveCache::default();
    let prior = CachedLibrary {
        library: library(),
        cache: &cache,
    }
    .resolve_shared(&reference, "", &BTreeMap::new)
    .unwrap();
    assert_eq!(count(&prior), 1_001);
    cache = cache.with_node_limit(17);
    for (key, value, expected) in [
        ("ordinary", json!("created"), "created"),
        ("null", Value::Null, "$title"),
    ] {
        let vars = || BTreeMap::from([("$title".into(), value.clone())]);
        let cached = CachedLibrary {
            library: library(),
            cache: &cache,
        };
        let first = cached.resolve_shared(&reference, key, &vars).unwrap();
        let second = cached
            .resolve_shared(&reference, key, &|| {
                panic!("cached resolution evaluated variables again")
            })
            .unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(count(&first), 17);
        assert_eq!(first.properties.get("text"), Some(&json!(expected)));
    }
    cache = cache.with_node_limit(3);
    let smaller = CachedLibrary {
        library: library(),
        cache: &cache,
    }
    .resolve_shared(&reference, "ordinary", &BTreeMap::new)
    .unwrap();
    assert_eq!(
        count(&smaller),
        3,
        "an old larger prototype escaped the new cap"
    );
}

#[test]
fn a_poisoned_factory_cache_keeps_the_resolution_cap() {
    let catalog = wide_catalog();
    let context = Context::default();
    let cache = ResolveCache::default().with_node_limit(5);
    let poisoned = std::panic::catch_unwind(|| {
        let _guard = cache.memo.lock().unwrap();
        panic!("poison the cache to exercise its uncached fallback");
    });
    assert!(poisoned.is_err());
    let library = CachedLibrary {
        library: CatalogLibrary {
            catalog: &catalog,
            context: &context,
        },
        cache: &cache,
    };
    let resolved = library
        .resolve(&ControlRef::parse("limited.prototype", "limited"))
        .unwrap();
    assert_eq!(count(&resolved), 5);
}
