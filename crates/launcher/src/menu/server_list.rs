//! Saved section visibility and ordering never change server identities or selection.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerGroup {
    Featured,
    Creator,
    Saved,
}

impl ServerGroup {
    pub const ALL: [Self; 3] = [Self::Featured, Self::Creator, Self::Saved];

    const fn index(self) -> usize {
        match self {
            Self::Featured => 0,
            Self::Creator => 1,
            Self::Saved => 2,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Featured => "Featured experiences",
            Self::Creator => "Creator experiences",
            Self::Saved => "Custom servers",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServerListAction {
    Toggle(ServerGroup),
    MoveBefore(ServerGroup, Option<ServerGroup>),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerListPreferences {
    order: [ServerGroup; 3],
    collapsed: [bool; 3],
}

impl Default for ServerListPreferences {
    fn default() -> Self {
        Self {
            order: ServerGroup::ALL,
            collapsed: [false; 3],
        }
    }
}

impl ServerListPreferences {
    /// A malformed saved permutation falls back without hiding a section.
    pub fn order(&self) -> [ServerGroup; 3] {
        if ServerGroup::ALL
            .iter()
            .all(|group| self.order.contains(group))
        {
            self.order
        } else {
            ServerGroup::ALL
        }
    }

    pub fn collapsed(&self, group: ServerGroup) -> bool {
        self.collapsed[group.index()]
    }

    pub(super) fn apply(&mut self, action: ServerListAction) -> bool {
        match action {
            ServerListAction::Toggle(group) => {
                self.collapsed[group.index()] = !self.collapsed(group);
            }
            ServerListAction::MoveBefore(group, before) => {
                if before == Some(group) {
                    return false;
                }
                self.order = self.order();
                let index = |group| self.order.iter().position(|value| *value == group).unwrap();
                let from = index(group);
                let to = before.map_or(self.order.len(), index);
                let to = to - usize::from(to > from);
                if from == to {
                    return false;
                }
                if from < to {
                    self.order[from..=to].rotate_left(1);
                } else {
                    self.order[to..=from].rotate_right(1);
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::settings_options::SettingsOptions;

    #[test]
    fn moving_sections_retains_other_sections_and_does_not_save_a_noop() {
        use ServerGroup::{Creator, Featured, Saved};
        let mut prefs = ServerListPreferences::default();
        assert!(prefs.apply(ServerListAction::MoveBefore(Saved, Some(Featured))));
        assert_eq!(prefs.order(), [Saved, Featured, Creator]);
        assert!(!prefs.apply(ServerListAction::MoveBefore(Saved, Some(Featured))));
        assert!(!prefs.apply(ServerListAction::MoveBefore(Saved, Some(Saved))));
        assert!(prefs.apply(ServerListAction::MoveBefore(Featured, None)));
        assert_eq!(prefs.order(), [Saved, Creator, Featured]);
        assert!(!prefs.apply(ServerListAction::MoveBefore(Featured, None)));
    }

    #[test]
    fn server_list_visibility_and_order_survive_a_settings_round_trip() {
        let mut settings = SettingsOptions::default();
        settings.apply_server_list(ServerListAction::Toggle(ServerGroup::Featured));
        settings.apply_server_list(ServerListAction::MoveBefore(
            ServerGroup::Saved,
            Some(ServerGroup::Creator),
        ));
        settings.apply_server_list(ServerListAction::MoveBefore(
            ServerGroup::Saved,
            Some(ServerGroup::Featured),
        ));
        let loaded = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert_eq!(
            loaded.server_list().order(),
            [
                ServerGroup::Saved,
                ServerGroup::Featured,
                ServerGroup::Creator
            ]
        );
        assert!(loaded.server_list().collapsed(ServerGroup::Featured));
        assert!(!loaded.server_list().collapsed(ServerGroup::Creator));
        assert!(!loaded.server_list().collapsed(ServerGroup::Saved));
    }

    #[test]
    fn legacy_settings_expand_all_groups_and_invalid_order_keeps_every_group() {
        let loaded = SettingsOptions::decode(br#"{"values":{"gamma":40}}"#).unwrap();
        assert_eq!(loaded.server_list().order(), ServerGroup::ALL);
        for group in ServerGroup::ALL {
            assert!(!loaded.server_list().collapsed(group));
        }
        let loaded =
            SettingsOptions::decode(br#"{"server_list":{"order":["saved","saved","creator"]}}"#)
                .unwrap();
        assert_eq!(loaded.server_list().order(), ServerGroup::ALL);
    }
}
