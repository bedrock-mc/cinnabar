//! Shared roster answers and display policy for tab-list and name-tag badges.

use std::{collections::HashSet, sync::Arc};

use ui_screens::player_list::RosterRow;
mod counters;

/// The session's named roster entries and their tab-list rows.
#[derive(Clone, Debug, Default)]
pub struct PlayerRoster {
    entries: Vec<(Arc<str>, Arc<str>, [u8; 16])>,
    revision: u64,
    reported: Arc<HashSet<String>>,
    hide_badges: bool,
    rows: Vec<RosterRow>,
    counters: counters::Counters,
}

impl PlayerRoster {
    /// Publishes one native interval's counts to both roster rows and eligible world nametags.
    pub fn set_totem_annotations(
        &mut self,
        style: Option<&ui::mod_hud::TotemCounterStyle>,
        facts: impl Iterator<Item = ([u8; 16], u32, bool)>,
    ) {
        if self.counters.update(style, facts) {
            self.rebadge();
        }
    }

    /// Looks up a UUID-bound nametag decoration independently of the tab visibility switch.
    pub fn totem_annotation(
        &self,
        uuid: &[u8; 16],
    ) -> Option<&Arc<ui::mod_hud::CounterAnnotation>> {
        self.counters.annotation(uuid)
    }
    /// Replaces the `(username, xuid, uuid)` roster; the revision advances only when it changed.
    pub fn set_entries(&mut self, entries: Vec<(Arc<str>, Arc<str>, [u8; 16])>) {
        if self.entries != entries {
            self.entries = entries;
            self.rows.clear();
            self.revision = self.revision.wrapping_add(1);
            tracing::debug!(
                players = self.entries.len(),
                identities = self.xuids().count(),
                uuid_keys = self
                    .entries
                    .iter()
                    .filter(|(_, xuid, _)| !bridge::valid_cinnabar_xuid(xuid))
                    .count(),
                "Cinnabar UI roster changed"
            );
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
            .map(|(_, xuid, _)| &**xuid)
            .filter(|xuid| !xuid.is_empty())
    }

    /// Chooses one lookup key per player, preferring a valid XUID over their UUID.
    pub fn lookup_ids(&self) -> impl Iterator<Item = String> + '_ {
        self.entries.iter().map(|(_, xuid, uuid)| {
            if bridge::valid_cinnabar_xuid(xuid) {
                xuid.to_string()
            } else {
                uuid::Uuid::from_bytes(*uuid).to_string()
            }
        })
    }

    /// Applies the XUIDs and UUIDs the API reported as Cinnabar players.
    pub fn set_cinnabar(&mut self, reported: Arc<HashSet<String>>) {
        if !Arc::ptr_eq(&self.reported, &reported) {
            self.reported = reported;
            self.rebadge();
            tracing::debug!(
                found = self.reported.len(),
                rows = self.rows.iter().filter(|row| row.badged).count(),
                "Cinnabar UI badges updated"
            );
        }
    }

    /// One tab-list row per entry, in roster order.
    pub fn rows(&self) -> &[RosterRow] {
        &self.rows
    }

    /// Refreshes skins by UUID, including listed players outside actor render distance.
    pub fn set_skins(&mut self, skin: impl Fn(&[u8; 16]) -> Option<render_api::StandardSkin>) {
        for ((_, _, uuid), row) in self.entries.iter().zip(&mut self.rows) {
            row.skin = skin(uuid);
        }
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
    pub fn has_badge(&self, xuid: &str, uuid: &[u8; 16]) -> bool {
        let mut buffer = [0; 36];
        let uuid = uuid::Uuid::from_bytes(*uuid);
        !self.hide_badges
            && ((!xuid.is_empty() && self.reported.contains(xuid))
                || self
                    .reported
                    .contains(&*uuid.hyphenated().encode_lower(&mut buffer)))
    }

    /// Rebuilds tab rows only when roster identities, API answers or display policy change.
    fn rebadge(&mut self) {
        self.rows = self
            .entries
            .iter()
            .enumerate()
            .map(|(index, (name, xuid, uuid))| RosterRow {
                name: Arc::clone(name),
                badged: self.has_badge(xuid, uuid),
                annotation: self
                    .counters
                    .style
                    .as_ref()
                    .filter(|style| style.show_tab)
                    .and_then(|_| self.counters.annotation(uuid))
                    .cloned(),
                skin: self.rows.get(index).and_then(|row| row.skin.clone()),
            })
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a named roster fixture with a fixed UUID.
    fn entry(name: &str, xuid: &str) -> (Arc<str>, Arc<str>, [u8; 16]) {
        (name.into(), xuid.into(), [0; 16])
    }

    /// Reads the rendered badge decision for each tab row.
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
        assert!(roster.rows()[0].badged && roster.has_badge("11", &[0; 16]));
        roster.set_show_badges(false);
        assert!(!roster.rows()[0].badged && !roster.has_badge("11", &[0; 16]));
        roster.set_show_badges(true);
        assert!(roster.rows()[0].badged && roster.has_badge("11", &[0; 16]));
        roster.set_cinnabar(Arc::default());
        assert!(!roster.rows()[0].badged && !roster.has_badge("11", &[0; 16]));
        roster.set_show_badges(false);
        roster.set_cinnabar(Arc::new(HashSet::from(["11".to_owned()])));
        assert!(!roster.rows()[0].badged && !roster.has_badge("11", &[0; 16]));
    }

    /// Servers that omit XUIDs still badge by UUID, and mixed rosters key each player by XUID first.
    #[test]
    fn missing_xuids_fall_back_to_uuid() {
        let uuid = uuid::Uuid::from_u128(0x0011_2233_4455_6677_8899_aabb_ccdd_eeff);
        let mut roster = PlayerRoster::default();
        roster.set_entries(vec![
            ("Alex".into(), "11".into(), [1; 16]),
            ("Steve".into(), "".into(), uuid.into_bytes()),
        ]);
        assert_eq!(
            roster.lookup_ids().collect::<Vec<_>>(),
            ["11", "00112233-4455-6677-8899-aabbccddeeff"]
        );
        roster.set_cinnabar(Arc::new(HashSet::from([uuid.to_string()])));
        assert_eq!(badges(&roster), [("Alex", false), ("Steve", true)]);
        assert!(roster.has_badge("", uuid.as_bytes()));
    }

    #[test]
    fn skins_follow_uuids_and_survive_badge_changes() {
        let mut roster = PlayerRoster::default();
        roster.set_entries(vec![
            ("Alex".into(), "11".into(), [1; 16]),
            ("Alex".into(), "22".into(), [2; 16]),
        ]);
        let skin = render_api::StandardSkin {
            width: 64,
            height: 64,
            rgba8: vec![255; 64 * 64 * 4].into(),
            cape: None,
            geometry: None,
        };
        let revision = roster.revision();
        roster.set_skins(|uuid| (*uuid == [1; 16]).then(|| skin.clone()));
        assert_eq!(roster.rows()[0].skin, Some(skin.clone()));
        assert!(roster.rows()[1].skin.is_none());
        roster.set_cinnabar(Arc::new(HashSet::from(["11".into()])));
        roster.set_show_badges(false);
        assert_eq!(roster.rows()[0].skin, Some(skin));
        assert_eq!(roster.revision(), revision);
        roster.set_entries(vec![("Alex".into(), "22".into(), [2; 16])]);
        assert!(roster.rows()[0].skin.is_none());
    }
}
