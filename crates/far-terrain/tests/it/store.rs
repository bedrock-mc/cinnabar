use far_terrain::{Admission, ColumnPos, SummaryStore};

use crate::summary::hilly;

#[test]
fn resent_column_replaces_only_when_content_differs() {
    let mut store = SummaryStore::new(usize::MAX);
    let pos = ColumnPos::new(3, -4);
    let summary = hilly();
    assert_eq!(store.admit(pos, &summary), Admission::Inserted);
    assert_eq!(store.admit(pos, &summary), Admission::Unchanged);
    let mut edited = summary.clone();
    edited.cells_mut()[0].height += 1;
    assert_eq!(store.admit(pos, &edited), Admission::Replaced);
    assert_eq!(store.take_changed(), vec![pos, pos]);
    assert_eq!(store.summary(pos), Some(Ok(edited)));
    assert_eq!(store.len(), 1);
}

#[test]
fn byte_budget_evicts_least_recently_used_first() {
    let summary = hilly();
    let mut probe = SummaryStore::new(usize::MAX);
    probe.admit(ColumnPos::new(0, 0), &summary);
    let mut store = SummaryStore::new(probe.bytes() * 2);
    store.admit(ColumnPos::new(0, 0), &summary);
    store.admit(ColumnPos::new(1, 0), &summary);
    assert!(store.summary(ColumnPos::new(0, 0)).is_some());
    store.admit(ColumnPos::new(2, 0), &summary);
    assert!(store.contains(ColumnPos::new(0, 0)));
    assert!(!store.contains(ColumnPos::new(1, 0)));
    assert!(store.contains(ColumnPos::new(2, 0)));
    assert!(store.bytes() <= probe.bytes() * 2);
}
