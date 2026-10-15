use super::*;

#[test]
fn presentation_generation_tracks_registry_and_compiled_source_replacements() {
    let mut store = ItemStateStore::diagnostic();
    let original = Arc::clone(store.presentation_generation());
    assert!(Arc::ptr_eq(&original, store.presentation_generation()));
    assert!(store.apply_registry(ItemRegistryEvent {
        entries: protocol::vanilla_item_registry()
    }));
    assert!(!Arc::ptr_eq(&original, store.presentation_generation()));
    let registry = Arc::clone(store.presentation_generation());
    let durations = Arc::new(BTreeMap::new());
    store.set_use_durations(Arc::clone(&durations));
    assert!(!Arc::ptr_eq(&registry, store.presentation_generation()));
    let compiled = Arc::clone(store.presentation_generation());
    store.set_use_durations(durations);
    assert!(Arc::ptr_eq(&compiled, store.presentation_generation()));
    store.set_use_overlays(Arc::new(BTreeMap::new()));
    assert!(!Arc::ptr_eq(&compiled, store.presentation_generation()));
    let other = ItemStateStore::diagnostic();
    assert!(!Arc::ptr_eq(
        other.presentation_generation(),
        store.presentation_generation()
    ));
}
