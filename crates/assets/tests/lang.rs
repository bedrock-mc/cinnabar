//! Localization carrier round-trip, ordering, and bounds coverage.

use std::sync::Arc;

use assets::{
    LangEntry, MAX_LANG_ENTRIES, MAX_LANG_VALUE_BYTES, RuntimeLangCatalog, encode_lang_catalog,
};

fn entry(key: &str, value: &str) -> LangEntry {
    LangEntry {
        key: key.into(),
        value: Arc::from(value),
    }
}

#[test]
fn catalog_round_trips_sorted_entries_and_resolves_lookups() {
    let entries = [
        entry("commands.op.success", "Opped: %s"),
        entry("item.apple.name", "Apple"),
        entry("tile.stone.name", "Stone"),
    ];
    let bytes = encode_lang_catalog([5; 32], &entries).unwrap();
    let catalog = RuntimeLangCatalog::decode(&bytes).unwrap();

    assert_eq!(catalog.source_manifest_sha256(), [5; 32]);
    assert_eq!(catalog.len(), 3);
    assert_eq!(catalog.lookup("item.apple.name").unwrap().as_ref(), "Apple");
    assert_eq!(
        catalog.lookup("commands.op.success").unwrap().as_ref(),
        "Opped: %s"
    );
    assert_eq!(catalog.lookup("missing.key"), None);
}

#[test]
fn unsorted_duplicate_and_oversized_inputs_fail_closed() {
    // Encoding rejects unsorted keys (the decoder's binary search depends on
    // strict ordering).
    let unsorted = [entry("zebra", "z"), entry("apple", "a")];
    assert!(encode_lang_catalog([0; 32], &unsorted).is_err());

    let duplicate = [entry("apple", "a"), entry("apple", "b")];
    assert!(encode_lang_catalog([0; 32], &duplicate).is_err());

    let oversized_value = "v".repeat(MAX_LANG_VALUE_BYTES + 1);
    let oversized = [entry("apple", &oversized_value)];
    assert!(encode_lang_catalog([0; 32], &oversized).is_err());

    // A truncated carrier and a corrupted envelope both fail closed.
    let bytes = encode_lang_catalog([1; 32], &[entry("apple", "a")]).unwrap();
    assert!(RuntimeLangCatalog::decode(&bytes[..bytes.len() - 1]).is_err());
    let mut corrupted = bytes.clone();
    let flip = corrupted.len() / 2;
    corrupted[flip] ^= 0xff;
    assert!(RuntimeLangCatalog::decode(&corrupted).is_err());
}

#[test]
fn entry_count_bound_is_enforced_at_encode_time() {
    let mut entries = Vec::with_capacity(MAX_LANG_ENTRIES + 1);
    for index in 0..=MAX_LANG_ENTRIES {
        entries.push(entry(&format!("key.{index:06}"), "value"));
    }
    assert!(encode_lang_catalog([0; 32], &entries).is_err());
}
