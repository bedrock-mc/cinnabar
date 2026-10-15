use super::MenuList;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Debug)]
struct Card {
    value: usize,
    copies: Arc<AtomicUsize>,
}

impl Clone for Card {
    /// Counts each copied card independently of the shared list handles.
    fn clone(&self) -> Self {
        self.copies.fetch_add(1, Ordering::Relaxed);
        Self {
            value: self.value,
            copies: Arc::clone(&self.copies),
        }
    }
}

impl PartialEq for Card {
    /// The counter records work and is not part of a card's presented value.
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl Eq for Card {}

/// Creates distinct cards sharing a copy counter, without copying any card.
fn cards(count: usize, copies: &Arc<AtomicUsize>) -> MenuList<Card> {
    (0..count)
        .map(|value| Card {
            value,
            copies: Arc::clone(copies),
        })
        .collect()
}

#[test]
fn repeated_publication_keeps_storage_and_does_not_copy_entries() {
    let copies = Arc::new(AtomicUsize::new(0));
    let owner = cards(512, &copies);
    for _ in 0..240 {
        let view = owner.clone();
        assert!(owner.shares_storage(&view));
        assert_eq!(view.len(), 512);
        assert_eq!(view[511].value, 511);
    }
    assert_eq!(copies.load(Ordering::Relaxed), 0);
}

#[test]
fn edits_detach_once_and_leave_retained_entries_in_their_original_order() {
    let copies = Arc::new(AtomicUsize::new(0));
    let mut owner = cards(3, &copies);
    let previous = owner.clone();
    owner[1].value = 7;
    assert!(!owner.shares_storage(&previous));
    assert_eq!(copies.load(Ordering::Relaxed), 3);
    owner.swap(0, 2);
    owner.remove(1);
    assert_eq!(copies.load(Ordering::Relaxed), 3);
    assert_eq!(
        previous.iter().map(|card| card.value).collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert_eq!(
        owner.iter().map(|card| card.value).collect::<Vec<_>>(),
        [2, 0]
    );
    let current = owner.clone();
    assert!(current.shares_storage(&owner));
    owner.clear();
    assert_eq!(
        current.iter().map(|card| card.value).collect::<Vec<_>>(),
        [2, 0]
    );
}

#[test]
fn equal_deliveries_keep_the_generation_and_changed_deliveries_replace_it() {
    let copies = Arc::new(AtomicUsize::new(0));
    let mut owner = cards(3, &copies);
    let previous = owner.clone();
    owner.replace(cards(3, &copies).into_iter().collect());
    assert!(owner.shares_storage(&previous));
    owner.replace(cards(2, &copies).into_iter().collect());
    assert!(!owner.shares_storage(&previous));
    assert_eq!(owner.len(), 2);
    assert_eq!(previous.len(), 3);
    assert_eq!(copies.load(Ordering::Relaxed), 0);
    let empty = MenuList::<Card>::default();
    owner.replace(Vec::new());
    let cleared = owner.clone();
    owner.replace(Vec::new());
    assert!(owner.shares_storage(&cleared));
    assert_eq!(owner, empty);
}

#[test]
fn equality_compares_order_and_values_across_independent_generations() {
    let first: MenuList<_> = vec!["a", "b"].into();
    let equal: MenuList<_> = vec!["a", "b"].into();
    let reordered: MenuList<_> = vec!["b", "a"].into();
    assert!(!first.shares_storage(&equal));
    assert_eq!(first, equal);
    assert_ne!(first, reordered);
}

#[test]
fn borrowed_and_owned_iteration_preserve_value_semantics() {
    let mut owner: MenuList<_> = vec![1, 2, 3].into();
    let previous = owner.clone();
    for value in &mut owner {
        *value *= 2;
    }
    assert_eq!(
        (&previous).into_iter().copied().collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert_eq!(owner.into_iter().collect::<Vec<_>>(), [2, 4, 6]);
    let retained = previous.clone();
    assert_eq!(previous.into_iter().collect::<Vec<_>>(), [1, 2, 3]);
    assert_eq!(retained.as_slice(), [1, 2, 3]);
}

#[test]
fn retaining_a_list_does_not_require_cloneable_entries() {
    struct NotClone;
    let owner: MenuList<_> = vec![NotClone].into();
    let view = owner.clone();
    assert!(owner.shares_storage(&view));
}
