//! Inbox selection and optimistic message state, keyed by the service instance identity.
use super::view::{MenuFeeds, MenuHome};
use bridge::MessageEvent;
use std::collections::{BTreeMap, BTreeSet};

mod bulk_read;
use bulk_read::BulkRead;

#[cfg(test)]
mod regressions;

pub const CATEGORIES: [&str; 5] = ["News", "Realms", "Invites", "Marketplace Pass", "Feedback"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    Category(usize),
    Open(usize),
    Delete(usize),
    Filters,
    MarkAllRead,
    DeleteAllRead,
    ConfirmDelete,
    Cancel,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InboxState {
    pub category: usize,
    pub filters: bool,
    pub delete_pending: Option<Vec<String>>,
    pub opened: Option<String>,
    pub pending: Vec<MessageEvent>,
    read: BTreeSet<String>,
    deleted: BTreeSet<String>,
    delete_all_read: bool,
    bulk_read: BulkRead,
    service_counts: BTreeMap<usize, u32>,
}

impl InboxState {
    /// Keeps a stale polled feed from undoing locally submitted read/delete actions.
    pub fn reconcile(&mut self, home: &mut MenuHome) {
        self.service_counts.clone_from(&home.inbox_counts);
        self.bulk_read.observe(home);
        self.apply(home);
    }

    /// Applies local actions without treating their optimistic result as a service reply.
    fn apply(&mut self, home: &mut MenuHome) {
        self.bulk_read
            .apply(home, &self.service_counts, &self.read, &self.deleted);
        for item in &mut home.inbox {
            if item.unread
                && (self.read.contains(&item.instance_id)
                    || self.deleted.contains(&item.instance_id))
            {
                if let Some(count) = category_index(&item.category)
                    .and_then(|index| home.inbox_counts.get_mut(&index))
                    && !self.bulk_read.covers(&item.category)
                {
                    *count = count.saturating_sub(1);
                }
                item.unread = false;
            }
        }
        home.inbox
            .retain(|item| !self.deleted.contains(&item.instance_id));
        if self
            .opened
            .as_ref()
            .is_some_and(|opened| !home.inbox.iter().any(|item| &item.instance_id == opened))
        {
            self.opened = None;
        }
        home.inbox_unread = if home.inbox_counts.is_empty() {
            home.inbox.iter().filter(|item| item.unread).count() as u32
        } else {
            home.inbox_counts.values().sum()
        };
    }
}

impl MenuFeeds {
    /// Applies one inbox control action and queues its service report exactly once.
    pub fn activate_inbox(&mut self, action: Action) {
        let state = &mut self.inbox_state;
        match action {
            Action::Category(index) if index < CATEGORIES.len() => {
                state.category = index;
                state.opened = None;
            }
            Action::Filters => state.filters = !state.filters,
            Action::Cancel => {
                if state.delete_pending.take().is_some() {
                    return;
                }
                state.filters = false;
                state.opened = None;
            }
            Action::Delete(index) => {
                state.delete_all_read = false;
                state.delete_pending = self
                    .home
                    .inbox
                    .get(index)
                    .map(|item| vec![item.instance_id.clone()]);
            }
            Action::DeleteAllRead => {
                state.delete_all_read = true;
                state.delete_pending = Some(
                    self.home
                        .inbox
                        .iter()
                        .filter(|item| !item.unread)
                        .map(|item| item.instance_id.clone())
                        .collect(),
                );
            }
            Action::MarkAllRead | Action::ConfirmDelete => {
                let deleting = matches!(action, Action::ConfirmDelete);
                let bulk = deleting && state.delete_all_read && state.delete_pending.is_some();
                if bulk {
                    state.pending.push(MessageEvent {
                        event_type: "DeleteAllRead".into(),
                        instance_id: String::new(),
                        report_id: String::new(),
                        button_id: String::new(),
                    });
                }
                let identities = if deleting {
                    state.delete_pending.take().unwrap_or_default()
                } else {
                    state
                        .bulk_read
                        .begin(&self.home, &state.service_counts, &state.read);
                    state.pending.push(MessageEvent {
                        event_type: "ReadAll".into(),
                        instance_id: String::new(),
                        report_id: String::new(),
                        button_id: String::new(),
                    });
                    self.home
                        .inbox
                        .iter()
                        .map(|item| item.instance_id.clone())
                        .collect()
                };
                for identity in identities {
                    let Some(item) = self
                        .home
                        .inbox
                        .iter()
                        .find(|item| item.instance_id == identity && !identity.is_empty())
                    else {
                        continue;
                    };
                    if deleting && state.deleted.insert(identity.clone()) && !bulk {
                        state.pending.push(MessageEvent {
                            event_type: "Delete".into(),
                            instance_id: identity,
                            report_id: item.report_id.clone(),
                            button_id: String::new(),
                        });
                    } else if !deleting {
                        state.read.insert(identity);
                    }
                }
                state.apply(&mut self.home);
            }
            Action::Open(index) => {
                let Some(item) = self.home.inbox.get(index) else {
                    return;
                };
                if item.instance_id.is_empty() {
                    return;
                }
                state.opened = Some(item.instance_id.clone());
                let changed = state.read.insert(item.instance_id.clone()) && item.unread;
                if changed {
                    state.pending.push(MessageEvent {
                        event_type: "Click".into(),
                        instance_id: item.instance_id.clone(),
                        report_id: item.report_id.clone(),
                        button_id: String::new(),
                    });
                }
                state.apply(&mut self.home);
            }
            _ => {}
        }
    }
}

/// Matches service spelling without inventing an extra All category.
pub fn category_index(category: &str) -> Option<usize> {
    let normalized: String = category
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    CATEGORIES
        .iter()
        .position(|candidate| candidate.replace(' ', "").eq_ignore_ascii_case(&normalized))
}

/// Formats the service ISO date independently of its time zone suffix.
pub fn date(value: &str) -> String {
    let Some(day) = value.get(..10) else {
        return String::new();
    };
    let parts: Vec<_> = day.split('-').collect();
    if parts.len() != 3 || parts.iter().any(|p| !p.bytes().all(|b| b.is_ascii_digit())) {
        return String::new();
    }
    format!("{}/{}/{}", parts[1], parts[2], parts[0])
}

/// Day numbers used to split date groups, never read state.
pub fn day(value: &str) -> Option<i64> {
    let parts: Vec<i64> = value
        .get(..10)?
        .split('-')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    let [mut y, m, d]: [i64; 3] = parts.try_into().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    y -= i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yo = y - era * 400;
    let shifted = m + if m > 2 { -3 } else { 9 };
    Some(era * 146097 + yo * 365 + yo / 4 - yo / 100 + (153 * shifted + 2) / 5 + d - 1 - 719468)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancelling_deletion_keeps_inbox_settings_open() {
        let mut menu = MenuFeeds::default();
        menu.activate_inbox(Action::Filters);
        menu.activate_inbox(Action::DeleteAllRead);
        menu.activate_inbox(Action::Cancel);
        assert!(menu.inbox_state.filters);
        assert!(menu.inbox_state.delete_pending.is_none());
        menu.activate_inbox(Action::Cancel);
        assert!(!menu.inbox_state.filters);
    }
    #[test]
    fn dates_and_category_names_survive_the_feed() {
        assert_eq!(date("2026-10-02T10:00:00Z"), "10/02/2026");
        assert_eq!(category_index("MarketplacePass"), Some(3));
        assert_eq!(day("1970-01-01"), Some(0));
        assert_eq!(day("2026-10-03").unwrap() - day("2026-09-26").unwrap(), 7);
    }
    #[test]
    fn read_and_delete_survive_stale_polls_without_duplicate_reports() {
        let mut menu = MenuFeeds::default();
        menu.home.inbox.push(super::super::InboxItem {
            instance_id: "i".into(),
            category: "News".into(),
            unread: true,
            ..Default::default()
        });
        menu.home.inbox_counts.insert(0, 30);
        let stale = menu.home.clone();
        menu.activate_inbox(Action::Open(0));
        menu.activate_inbox(Action::Open(0));
        assert_eq!(menu.inbox_state.pending.len(), 1);
        assert!(!menu.home.inbox[0].unread);
        menu.activate_inbox(Action::Delete(0));
        assert_eq!(menu.home.inbox.len(), 1);
        menu.activate_inbox(Action::ConfirmDelete);
        menu.home = stale;
        menu.inbox_state.reconcile(&mut menu.home);
        assert!(menu.home.inbox.is_empty());
        assert_eq!(menu.home.inbox_unread, 29);
    }

    /// Builds a partial inbox page with a service total larger than its loaded rows.
    fn partial_feed() -> MenuHome {
        MenuHome {
            inbox: vec![super::super::InboxItem {
                instance_id: "old".into(),
                category: "News".into(),
                unread: true,
                ..Default::default()
            }],
            inbox_counts: [(0, 30), (1, 5)].into(),
            inbox_unread: 35,
            ..Default::default()
        }
    }

    #[test]
    fn mark_all_read_clears_partial_page_totals_and_preserves_new_messages() {
        let mut menu = MenuFeeds::default();
        let stale = partial_feed();
        menu.home = stale.clone();
        menu.activate_inbox(Action::MarkAllRead);
        assert_eq!(menu.home.inbox_unread, 0);
        assert!(menu.home.inbox_counts.values().all(|count| *count == 0));
        assert_eq!(menu.inbox_state.pending[0].event_type, "ReadAll");
        for _ in 0..2 {
            menu.home = stale.clone();
            menu.inbox_state.reconcile(&mut menu.home);
            assert_eq!(menu.home.inbox_unread, 0);
        }
        let mut fresh = stale;
        fresh.inbox.push(super::super::InboxItem {
            instance_id: "new".into(),
            category: "News".into(),
            unread: true,
            ..Default::default()
        });
        for count in [30, 31] {
            fresh.inbox_counts.insert(0, count);
            menu.home = fresh.clone();
            menu.inbox_state.reconcile(&mut menu.home);
            assert_eq!(menu.home.inbox_unread, 1);
            assert!(menu.home.inbox[1].unread);
        }
        menu.activate_inbox(Action::Open(1));
        assert_eq!(menu.home.inbox_unread, 0);
        menu.home = fresh.clone();
        menu.inbox_state.reconcile(&mut menu.home);
        assert_eq!(menu.home.inbox_unread, 0);

        // The service catches up, then reports new unread messages outside the loaded page.
        fresh.inbox.iter_mut().for_each(|item| item.unread = false);
        fresh.inbox_counts = [(0, 0), (1, 0)].into();
        menu.home = fresh.clone();
        menu.inbox_state.reconcile(&mut menu.home);
        fresh.inbox_counts.insert(0, 2);
        menu.home = fresh;
        menu.inbox_state.reconcile(&mut menu.home);
        assert_eq!(menu.home.inbox_unread, 2);
    }

    #[test]
    fn repeated_mark_all_uses_service_totals_and_ignores_already_read_rows() {
        let mut menu = MenuFeeds::default();
        let mut stale = partial_feed();
        stale.inbox.push(super::super::InboxItem {
            instance_id: "already-read".into(),
            category: "News".into(),
            ..Default::default()
        });
        menu.home = stale.clone();
        menu.inbox_state.reconcile(&mut menu.home);
        menu.activate_inbox(Action::Open(0));
        menu.activate_inbox(Action::MarkAllRead);
        menu.activate_inbox(Action::MarkAllRead);
        menu.home = stale;
        menu.inbox_state.reconcile(&mut menu.home);
        assert_eq!(menu.home.inbox_unread, 0);
    }
}
