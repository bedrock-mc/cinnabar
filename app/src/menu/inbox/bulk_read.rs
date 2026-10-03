//! Suppress the unread totals of the cached feed affected by ReadAll.

use super::{BTreeMap, BTreeSet, MenuHome, category_index};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct BulkRead {
    counts: BTreeMap<usize, u32>,
    identities: BTreeSet<String>,
    unread: BTreeSet<String>,
    fresh_unread: BTreeMap<String, usize>,
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
        self.fresh_unread.clear();
    }

    /// Remembers newly unread service rows and retires caught-up category totals.
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
        self.fresh_unread = home
            .inbox
            .iter()
            .filter(|item| item.unread && !self.identities.contains(&item.instance_id))
            .filter_map(|item| {
                category_index(&item.category).map(|category| (item.instance_id.clone(), category))
            })
            .collect();
    }

    /// Whether this category's local reads are already included in the bulk adjustment.
    pub(super) fn covers(&self, category: &str) -> bool {
        category_index(category).is_some_and(|index| self.counts.contains_key(&index))
    }

    /// Keeps newly arriving identities unread even when cached totals have not increased yet.
    pub(super) fn apply(
        &self,
        home: &mut MenuHome,
        service_counts: &BTreeMap<usize, u32>,
        read: &BTreeSet<String>,
        deleted: &BTreeSet<String>,
    ) {
        for (&category, &baseline) in &self.counts {
            let mut fresh = 0;
            let mut locally_read = 0;
            for (identity, &item_category) in &self.fresh_unread {
                if item_category != category {
                    continue;
                }
                if read.contains(identity) || deleted.contains(identity) {
                    locally_read += 1;
                } else {
                    fresh += 1;
                }
            }
            if let Some(count) = home.inbox_counts.get_mut(&category) {
                // Reapply against the service reply, before any local read/delete mutations.
                *count = service_counts
                    .get(&category)
                    .copied()
                    .unwrap_or(baseline)
                    .saturating_sub(baseline)
                    .saturating_sub(locally_read)
                    .max(fresh);
            }
        }
    }
}
