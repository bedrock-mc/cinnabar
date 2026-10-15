use super::*;
use std::mem::{size_of, size_of_val};
use std::sync::atomic::Ordering;

/// Measures only documented physical allocations, with shared cache payload charged in full.
fn actual_heap(storage: &PalettedStorage) -> usize {
    size_of_val(storage.words.as_ref())
        + size_of_val(storage.palette.values.as_ref())
        + storage.edits.as_ref().map_or(0, |edits| {
            size_of_val(edits.slots.as_ref())
                + edits.canonical_words.get().map_or(0, |words| {
                    (size_of_val(words.as_ref()) + 2 * size_of::<usize>())
                        .next_multiple_of(std::mem::align_of::<usize>())
                })
        })
}

#[test]
fn retained_memory_counts_wide_physical_words_and_dead_slots_after_uniform_shrink() {
    let mut storage = packed(16, (0..BLOCKS_PER_SUB_CHUNK as u32).collect(), |cell| cell);
    for cell in 1..BLOCKS_PER_SUB_CHUNK {
        assert!(storage.apply_runtime_update(cell, 0));
    }
    assert_eq!(storage.bits_per_index(), 0);
    assert_eq!(storage.palette().len(), 1);
    assert_eq!(
        storage.edits.as_ref().unwrap().slots.len(),
        BLOCKS_PER_SUB_CHUNK
    );
    assert_eq!(storage.retained_heap_bytes(), actual_heap(&storage));
    assert!(storage.retained_heap_bytes() > size_of_val(storage.words.as_ref()));
    let bound = storage.retained_heap_bytes_bound();
    assert!(bound <= PalettedStorage::MAX_RETAINED_HEAP_BYTES);
    assert!(
        storage
            .edits
            .as_ref()
            .unwrap()
            .canonical_words
            .get()
            .is_none()
    );
    assert_eq!(storage.work.materializations.load(Ordering::Relaxed), 0);
    assert!(storage.words().is_empty());
    assert_eq!(storage.retained_heap_bytes(), actual_heap(&storage));
    assert_eq!(storage.retained_heap_bytes(), bound);
    assert_eq!(storage.retained_heap_bytes_bound(), bound);
}

#[test]
fn memory_reservation_covers_a_cache_materialized_between_measurement_and_clone() {
    let mut storage = packed(2, vec![0, 1, 2], |cell| if cell < 2 { cell + 1 } else { 0 });
    storage.apply_runtime_update(0, 3);
    let cold = storage.retained_heap_bytes();
    let reserved = storage.retained_heap_bytes_bound();
    assert_eq!(cold, actual_heap(&storage));
    assert!(reserved > cold);
    assert_eq!(storage.work.materializations.load(Ordering::Relaxed), 0);
    let words = storage.words();
    assert_eq!(reserved - cold, size_of_val(words) + 2 * size_of::<usize>());
    let mut cloned = storage.clone();
    assert_eq!(cloned.words().as_ptr(), words.as_ptr());
    assert_eq!(cloned.retained_heap_bytes(), reserved);
    assert_eq!(cloned.retained_heap_bytes_bound(), reserved);
    assert_eq!(storage.retained_heap_bytes(), reserved);
    assert!(cloned.apply_runtime_update(1, 4));
    assert!(
        cloned
            .edits
            .as_ref()
            .unwrap()
            .canonical_words
            .get()
            .is_none()
    );
    assert_eq!(cloned.retained_heap_bytes(), actual_heap(&cloned));
    assert!(cloned.retained_heap_bytes() < reserved);
    assert_eq!(storage.retained_heap_bytes(), reserved);
    assert_eq!(storage.work.materializations.load(Ordering::Relaxed), 1);
}

#[test]
fn decoded_and_edited_storage_reservations_fit_the_declared_maximum() {
    for bits in SUPPORTED_BITS {
        let len = (1_usize << bits).min(BLOCKS_PER_SUB_CHUNK);
        let mut storage = packed(bits, (0..len as u32).collect(), |cell| cell % len);
        assert_eq!(storage.retained_heap_bytes(), actual_heap(&storage));
        assert_eq!(
            storage.retained_heap_bytes_bound(),
            storage.retained_heap_bytes()
        );
        storage.apply_runtime_update(0, len as u32);
        let reserved = storage.retained_heap_bytes_bound();
        assert!(reserved <= PalettedStorage::MAX_RETAINED_HEAP_BYTES);
        assert_eq!(storage.work.materializations.load(Ordering::Relaxed), 0);
        let _ = storage.words();
        assert!(storage.retained_heap_bytes() <= reserved);
        assert_eq!(storage.retained_heap_bytes(), actual_heap(&storage));
    }
}
