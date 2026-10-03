//! Suppress the unread totals of the cached feed affected by ReadAll.

use super::{BTreeMap, BTreeSet, MenuHome, category_index};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct BulkRead {
    counts: BTreeMap<usize, u32>,
    identities: BTreeSet<String>,
    unread: BTreeSet<String>,
}

impl BulkRead {
    /// Remembers service totals, including unread messages outside the loaded page.
    pub(super) fn begin(
        &mut self,
        home: &MenuHome,
        service_counts: &BTreeMap<usize, u32>,
        read: &BTreeSet<String>,
    ) {
        self.counts = home
            .inbox_counts
            .iter()
            .map(|(&category, &count)| {
                (
                    category,
                    service_counts.get(&category).copied().unwrap_or(count),
                )
            })
            .collect();
        self.identities = home
            .inbox
            .iter()
            .map(|item| item.instance_id.clone())
            .collect();
        self.identities.extend(read.iter().cloned());
        self.unread = home
            .inbox
            .iter()
            .filter(|item| item.unread)
            .map(|item| item.instance_id.clone())
            .collect();
    }

    /// Retires a category's stale total once the service reports its read-state change.
    pub(super) fn observe(&mut self, home: &MenuHome) {
        self.counts.retain(|category, baseline| {
            let mut covered = home
                .inbox
                .iter()
                .filter(|item| {
                    category_index(&item.category) == Some(*category)
                        && self.unread.contains(&item.instance_id)
                })
                .peekable();
            let caught_up = covered.peek().is_some() && covered.all(|item| !item.unread);
            home.inbox_counts
                .get(category)
                .is_some_and(|count| count >= baseline)
                && !caught_up
        });
    }

    /// Whether this category's local reads are already included in the bulk adjustment.
    pub(super) fn covers(&self, category: &str) -> bool {
        category_index(category).is_some_and(|index| self.counts.contains_key(&index))
    }

    /// Keeps newly arriving identities unread even when cached totals have not increased yet.
    pub(super) fn apply(
        &self,
        home: &mut MenuHome,
        read: &BTreeSet<String>,
        deleted: &BTreeSet<String>,
    ) {
        for (&category, &baseline) in &self.counts {
            let mut fresh = 0;
            let mut locally_read = 0;
            for item in &home.inbox {
                if category_index(&item.category) != Some(category)
                    || !item.unread
                    || self.identities.contains(&item.instance_id)
                {
                    continue;
                }
                if read.contains(&item.instance_id) || deleted.contains(&item.instance_id) {
                    locally_read += 1;
                } else {
                    fresh += 1;
                }
            }
            if let Some(count) = home.inbox_counts.get_mut(&category) {
                *count = count
                    .saturating_sub(baseline)
                    .saturating_sub(locally_read)
                    .max(fresh);
            }
        }
    }
}
