use super::*;

/// Encodes a fixed little-endian root with damage and an empty enchantment list.
fn source(damage: i32) -> Arc<[u8]> {
    let mut bytes = vec![255, 255, 1, 10, 0, 0, 3, 6, 0];
    bytes.extend_from_slice(b"Damage");
    bytes.extend_from_slice(&damage.to_le_bytes());
    bytes.extend_from_slice(&[9, 4, 0]);
    bytes.extend_from_slice(b"ench");
    bytes.extend_from_slice(&[10, 0, 0, 0, 0, 0]);
    bytes.into()
}

#[test]
fn unchanged_source_shares_facts_and_changed_source_refreshes_damage() {
    let cache = ItemStackFactsCache::default();
    let first = source(7);
    let facts = cache.get(&first, false);
    assert_eq!(facts.damage, Some(7));
    assert!(facts.has_enchantment_list);
    assert!(facts.display.enchantments.is_empty());
    assert!(Arc::ptr_eq(&facts, &cache.get(&Arc::clone(&first), false)));
    assert!(Arc::ptr_eq(&facts, &cache.clone().get(&first, false)));
    let changed = source(9);
    assert_eq!(cache.get(&changed, false).damage, Some(9));
    assert_eq!(facts.damage, Some(7));
    assert!(!Arc::ptr_eq(&facts, &cache.get(&changed, false)));
}

#[test]
fn tail_is_borrowed_and_projectile_work_is_limited_to_crossbows() {
    let source = source(7);
    assert_eq!(
        super::super::decode_extra_nbt(&source).unwrap().as_ptr(),
        source[3..].as_ptr()
    );
    let mut bytes = vec![255, 255, 1, 10, 0, 0, 10, 11, 0];
    bytes.extend_from_slice(b"chargedItem");
    bytes.extend_from_slice(&[8, 4, 0]);
    bytes.extend_from_slice(b"Name");
    let name = b"minecraft:arrow";
    bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
    bytes.extend_from_slice(name);
    bytes.extend_from_slice(&[0, 0]);
    let extra = Arc::from(bytes);
    let cache = ItemStackFactsCache::default();
    let ordinary = cache.get(&extra, false);
    assert!(ordinary.charged_projectile().is_none());
    let charged = cache.get(&extra, true);
    assert!(Arc::ptr_eq(&ordinary, &charged));
    assert_eq!(
        charged.charged_projectile().as_deref(),
        Some("minecraft:arrow")
    );
    let ((ordinary_again, charged_again), allocations) =
        crate::test_allocations::measure(|| (cache.get(&extra, false), cache.get(&extra, true)));
    assert!(Arc::ptr_eq(&ordinary_again, &charged_again));
    assert!(Arc::ptr_eq(&ordinary, &ordinary_again));
    assert_eq!(allocations, 0);
}

#[test]
fn cache_bounds_entries_and_retained_bytes_without_changing_returned_facts() {
    let cache = ItemStackFactsCache::default();
    let first = source(7);
    let retained = cache.get(&first, false);
    for damage in 0..MAX_FACT_ENTRIES as i32 + 1 {
        assert_eq!(
            cache.get(&source(damage), false).damage,
            Some(damage as u32)
        );
    }
    assert_eq!(retained.damage, Some(7));
    let state = cache.0.lock().unwrap();
    assert!(state.entries.len() <= MAX_FACT_ENTRIES);
    assert!(state.bytes <= MAX_FACT_BYTES);
}

#[test]
fn warmed_presentation_facts_do_not_allocate() {
    let mut bytes = vec![255, 255, 1, 10, 0, 0, 10, 7, 0];
    bytes.extend_from_slice(b"display");
    bytes.extend_from_slice(&[8, 4, 0]);
    bytes.extend_from_slice(b"Name");
    bytes.extend_from_slice(&[6, 0]);
    bytes.extend_from_slice(b"A tool");
    bytes.extend_from_slice(&[0, 0]);
    let source = Arc::from(bytes);
    let cache = ItemStackFactsCache::default();
    let facts = cache.get(&source, false);
    assert_eq!(facts.display.name.as_deref(), Some("A tool"));
    let (uncached, fresh_allocations) = crate::test_allocations::measure(|| item_display(&source));
    let (shared, allocations) = crate::test_allocations::measure(|| cache.get(&source, false));
    assert_eq!(uncached.name, shared.display.name);
    assert!(Arc::ptr_eq(&facts, &shared));
    assert_eq!(allocations, 0);
    println!("named-stack display allocations: fresh={fresh_allocations}, cached={allocations}");
}

#[test]
fn headerless_sources_share_empty_facts_without_spending_cache_entries() {
    let cache = ItemStackFactsCache::default();
    let empty = cache.get(&Arc::from([]), false);
    let odd = cache.get(&Arc::from([1, 2, 3]), true);
    assert!(Arc::ptr_eq(&empty, &odd));
    assert!(empty.display.name.is_none());
    assert!(empty.charged_projectile().is_none());
    assert!(empty.damage.is_none());
    assert!(cache.0.lock().unwrap().entries.is_empty());
}
