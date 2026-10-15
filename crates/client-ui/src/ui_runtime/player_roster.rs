//! Shared roster answers and display policy for tab-list and name-tag badges.

use std::{collections::HashSet, sync::Arc};

use ui_screens::player_list::RosterRow;

/// The session's named roster entries and their tab-list rows.
#[derive(Clone, Debug, Default)]
pub struct PlayerRoster {
    entries: Vec<(Arc<str>, Arc<str>)>,
    revision: u64,
    reported: Arc<HashSet<String>>,
    hide_badges: bool,
    rows: Vec<RosterRow>,
}

impl PlayerRoster {
    /// Replaces the `(username, xuid)` roster; the revision advances only when it changed.
    pub fn set_entries(&mut self, entries: Vec<(Arc<str>, Arc<str>)>) {
        if self.entries != entries {
            self.entries = entries;
            self.revision = self.revision.wrapping_add(1);
            self.rebadge();
        }
    }

    /// Advances with every roster change, so lookups are sent only for new rosters.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The roster's nonempty XUIDs.
    pub fn xuids(&self) -> impl Iterator<Item = &str> {
        self.entries
            .iter()
            .map(|(_, xuid)| &**xuid)
            .filter(|xuid| !xuid.is_empty())
    }

    /// Applies the XUIDs the API reported as Cinnabar players.
    pub fn set_cinnabar(&mut self, reported: Arc<HashSet<String>>) {
        if !Arc::ptr_eq(&self.reported, &reported) {
            self.reported = reported;
            self.rebadge();
        }
    }

    /// One tab-list row per entry, in roster order.
    pub fn rows(&self) -> &[RosterRow] {
        &self.rows
    }

    /// Controls both tab-list and name-tag badges without changing API visibility or answers.
    pub fn set_show_badges(&mut self, show: bool) {
        let hide = !show;
        if self.hide_badges != hide {
            self.hide_badges = hide;
            self.rebadge();
        }
    }

    /// Whether this roster identity has a visible, currently reported badge.
    pub fn has_badge(&self, xuid: &str) -> bool {
        !self.hide_badges && !xuid.is_empty() && self.reported.contains(xuid)
    }

    /// Rebuilds tab rows only when roster identities, API answers or display policy change.
    fn rebadge(&mut self) {
        self.rows = self
            .entries
            .iter()
            .map(|(name, xuid)| RosterRow {
                name: Arc::clone(name),
                badged: self.has_badge(xuid),
            })
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, xuid: &str) -> (Arc<str>, Arc<str>) {
        (name.into(), xuid.into())
    }

    fn badges(roster: &PlayerRoster) -> Vec<(&str, bool)> {
        roster
            .rows()
            .iter()
            .map(|row| (&*row.name, row.badged))
            .collect()
    }

    /// Badges follow both the roster and the reported set, and only roster changes advance the revision.
    #[test]
    fn badges_join_reported_xuids_to_roster_entries() {
        let mut roster = PlayerRoster::default();
        roster.set_entries(vec![
            entry("Alex", "11"),
            entry("Steve", ""),
            entry("Zoe", "33"),
        ]);
        let revision = roster.revision();
        assert_eq!(roster.xuids().collect::<Vec<_>>(), ["11", "33"]);
        roster.set_cinnabar(Arc::new(HashSet::from(["33".to_owned(), String::new()])));
        assert_eq!(
            badges(&roster),
            [("Alex", false), ("Steve", false), ("Zoe", true)]
        );
        roster.set_entries(vec![
            entry("Alex", "11"),
            entry("Steve", ""),
            entry("Zoe", "33"),
        ]);
        assert_eq!(roster.revision(), revision);
        roster.set_entries(vec![entry("Alex", "11")]);
        assert_ne!(roster.revision(), revision);
        assert_eq!(badges(&roster), [("Alex", false)]);
    }

    /// Two players sharing a display name are badged by their own XUIDs, never by the name.
    #[test]
    fn same_name_entries_keep_their_own_identity() {
        let mut roster = PlayerRoster::default();
        roster.set_entries(vec![
            entry("Alex", ""),
            entry("Alex", "11"),
            entry("Alex", "22"),
        ]);
        roster.set_cinnabar(Arc::new(HashSet::from(["11".to_owned()])));
        assert_eq!(
            badges(&roster),
            [("Alex", false), ("Alex", true), ("Alex", false)]
        );
    }

    /// Both surfaces use the same setting and answer, including the cache's expiry publication.
    #[test]
    fn display_setting_and_expiry_hide_both_surfaces() {
        let mut roster = PlayerRoster::default();
        roster.set_entries(vec![entry("ExamplePlayer", "11")]);
        roster.set_cinnabar(Arc::new(HashSet::from(["11".to_owned()])));
        assert!(roster.rows()[0].badged && roster.has_badge("11"));
        roster.set_show_badges(false);
        assert!(!roster.rows()[0].badged && !roster.has_badge("11"));
        roster.set_show_badges(true);
        assert!(roster.rows()[0].badged && roster.has_badge("11"));
        roster.set_cinnabar(Arc::default());
        assert!(!roster.rows()[0].badged && !roster.has_badge("11"));
        roster.set_show_badges(false);
        roster.set_cinnabar(Arc::new(HashSet::from(["11".to_owned()])));
        assert!(!roster.rows()[0].badged && !roster.has_badge("11"));
    }
}
