use super::*;
use std::{cell::Cell, cmp::Ordering};

thread_local! {
    static COMPARISONS: Cell<usize> = const { Cell::new(0) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Counted(u32);

impl Ord for Counted {
    /// Counts priority comparisons independently of wall-clock timing.
    fn cmp(&self, other: &Self) -> Ordering {
        COMPARISONS.set(COMPARISONS.get() + 1);
        self.0.cmp(&other.0)
    }
}

impl PartialOrd for Counted {
    /// Uses the same total order for both comparison traits.
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Reconciles a full active set and returns its retained order.
fn reconcile<I: Copy + Eq + std::hash::Hash, P: Copy + Ord>(
    retained: &mut RetainedUploadPriorities<I, P>,
    active: &[(I, P)],
) -> Vec<I> {
    retained.begin();
    for &(id, priority) in active {
        retained.update(id, priority);
    }
    retained.finish();
    retained.ordered().collect()
}

#[test]
fn retained_distance_order_matches_total_cmp_including_nonfinite_values() {
    let values = [
        f32::from_bits(0xffc0_0001),
        f32::NEG_INFINITY,
        -1.0,
        -0.0,
        0.0,
        1.0,
        f32::INFINITY,
        f32::from_bits(0x7fc0_0001),
    ];
    for left in values {
        for right in values {
            assert_eq!(
                upload_distance_key(left).cmp(&upload_distance_key(right)),
                left.total_cmp(&right)
            );
        }
    }
}

#[test]
fn retained_buckets_reconcile_reprioritization_removal_and_stable_ties() {
    let mut retained = RetainedUploadPriorities::default();
    assert_eq!(
        reconcile(&mut retained, &[(1, 2), (2, 1), (3, 1)]),
        [2, 3, 1]
    );
    assert_eq!(
        reconcile(&mut retained, &[(3, 1), (2, 1), (1, 0)]),
        [1, 3, 2]
    );
    assert_eq!(reconcile(&mut retained, &[(4, 1), (1, 2)]), [4, 1]);
    assert_eq!(retained.entries.len(), 2);
    assert_eq!(retained.buckets.len(), 2);
    assert!(reconcile(&mut retained, &[]).is_empty());
    assert!(retained.entries.is_empty());
    assert!(retained.buckets.is_empty());
    assert_eq!(reconcile(&mut retained, &[(1, 7)]), [1]);
}

#[test]
fn retained_selection_work_at_fixed_backlogs() {
    for size in [512, 4_096, 65_536] {
        let active = (0..size)
            .map(|id| (id, Counted(((id * 4051) % size) as u32)))
            .collect::<Vec<_>>();
        let mut before = active.clone();
        COMPARISONS.set(0);
        before.sort_by_key(|&(_, priority)| priority);
        let before_comparisons = COMPARISONS.get();
        let expected = before.iter().map(|&(id, _)| id).collect::<Vec<_>>();
        let mut retained = RetainedUploadPriorities::default();
        assert_eq!(reconcile(&mut retained, &active), expected);
        COMPARISONS.set(0);
        assert_eq!(reconcile(&mut retained, &active), expected);
        let after_comparisons = COMPARISONS.get();
        assert!(before_comparisons >= size - 1);
        assert_eq!(after_comparisons, 0);
        eprintln!(
            "selection backlog={size}: sort comparisons={before_comparisons}, retained comparisons={after_comparisons}, reconciled={size}"
        );
        // One replacement reindexes one identity, rather than sorting the backlog.
        let mut changed = active.clone();
        changed[0].1 = Counted(u32::MAX);
        COMPARISONS.set(0);
        reconcile(&mut retained, &changed);
        let replacement_comparisons = COMPARISONS.get();
        assert!(replacement_comparisons < size);
        eprintln!(
            "selection backlog={size}: one replacement comparisons={replacement_comparisons}"
        );
    }
}
