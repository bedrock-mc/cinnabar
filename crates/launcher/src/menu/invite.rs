//! The pause screen's "Invite to Game": the account's Xbox friends split into vanilla's online
//! and offline lists, the friends the player picked, and the XUIDs a send invites.

use std::collections::BTreeSet;

use bridge::Person;

/// The vanilla screen the pause menu's invite button opens.
pub const SCREEN: &str = "invite.invite_screen";

/// One Xbox friend; `picture_path` is the cached gamerpic, empty when it has none.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Friend {
    pub xuid: String,
    pub gamertag: String,
    pub online: bool,
    pub picture_path: String,
}

impl From<Person> for Friend {
    fn from(person: Person) -> Self {
        Self {
            xuid: person.xuid,
            gamertag: person.gamertag,
            online: person.online,
            picture_path: person.gamerpic.path,
        }
    }
}

/// Which of vanilla's two Xbox Live friend lists a row sits in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Section {
    Online,
    Offline,
}

impl Section {
    fn holds(self, friend: &Friend) -> bool {
        friend.online == (self == Self::Online)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    /// The pause screen's "Invite to Game".
    Open,
    /// A friend row's checkbox, by the row's index in its list.
    Toggle(Section, usize),
    /// Invite every picked friend and return to the pause screen.
    Send,
}

/// The invite screen's friends and picks; the list is `None` until the core answers.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InviteState {
    friends: Option<Vec<Friend>>,
    failed: bool,
    selected: BTreeSet<String>,
}

impl InviteState {
    /// The friends list is still loading.
    pub fn loading(&self) -> bool {
        self.friends.is_none() && !self.failed
    }

    /// The core could not list the friends.
    pub fn failed(&self) -> bool {
        self.failed
    }

    /// Installs the core's answer, `None` when it failed; picks of friends no longer listed drop.
    pub fn set_friends(&mut self, friends: Option<Vec<Friend>>) {
        self.failed = friends.is_none();
        let listed = friends.as_deref().unwrap_or_default();
        self.selected
            .retain(|xuid| listed.iter().any(|friend| &friend.xuid == xuid));
        self.friends = friends;
    }

    /// The rows of one list, in the core's order.
    pub fn section(&self, section: Section) -> impl Iterator<Item = &Friend> {
        self.friends
            .iter()
            .flatten()
            .filter(move |friend| section.holds(friend))
    }

    pub fn is_selected(&self, friend: &Friend) -> bool {
        self.selected.contains(&friend.xuid)
    }

    /// Flips the pick of the row at `index` in `section`; a row that is not listed is ignored.
    pub fn toggle(&mut self, section: Section, index: usize) {
        let Some(xuid) = self
            .section(section)
            .nth(index)
            .map(|friend| friend.xuid.clone())
        else {
            return;
        };
        if !self.selected.remove(&xuid) {
            self.selected.insert(xuid);
        }
    }

    pub fn selected_count(&self) -> usize {
        self.selected.len()
    }

    /// The picked friends' XUIDs, online list first, each in row order.
    pub fn selected_xuids(&self) -> Vec<String> {
        [Section::Online, Section::Offline]
            .into_iter()
            .flat_map(|section| self.section(section))
            .filter(|friend| self.is_selected(friend))
            .map(|friend| friend.xuid.clone())
            .collect()
    }

    /// Every row's checkbox, online list first, then the send button: the screen's focus order.
    pub fn actions(&self) -> impl Iterator<Item = Action> + '_ {
        [Section::Online, Section::Offline]
            .into_iter()
            .flat_map(|section| {
                (0..self.section(section).count()).map(move |index| Action::Toggle(section, index))
            })
            .chain(std::iter::once(Action::Send))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn friend(xuid: &str, online: bool) -> Friend {
        Friend {
            xuid: xuid.to_owned(),
            gamertag: format!("gamer{xuid}"),
            online,
            picture_path: String::new(),
        }
    }

    fn loaded(friends: &[Friend]) -> InviteState {
        let mut state = InviteState::default();
        state.set_friends(Some(friends.to_vec()));
        state
    }

    #[test]
    fn rows_index_within_their_own_list() {
        let mut state = loaded(&[friend("1", false), friend("2", true), friend("3", false)]);
        state.toggle(Section::Offline, 1);
        assert_eq!(state.selected_xuids(), ["3"]);
        state.toggle(Section::Online, 0);
        assert_eq!(state.selected_xuids(), ["2", "3"]);
    }

    #[test]
    fn a_second_press_unpicks_and_unlisted_rows_are_ignored() {
        let mut state = loaded(&[friend("1", true)]);
        state.toggle(Section::Online, 0);
        state.toggle(Section::Online, 0);
        state.toggle(Section::Online, 5);
        state.toggle(Section::Offline, 0);
        assert!(state.selected_xuids().is_empty());
        assert_eq!(state.selected_count(), 0);
    }

    #[test]
    fn a_new_list_keeps_picks_of_friends_still_listed() {
        let mut state = loaded(&[friend("1", true), friend("2", true)]);
        state.toggle(Section::Online, 0);
        state.toggle(Section::Online, 1);
        // Friend 2 went offline and friend 1 left the list.
        state.set_friends(Some(vec![friend("2", false)]));
        assert_eq!(state.selected_xuids(), ["2"]);
        assert_eq!(state.selected_count(), 1);
        state.set_friends(None);
        assert!(state.failed() && !state.loading());
        assert!(state.selected_xuids().is_empty());
    }

    #[test]
    fn loading_lasts_until_the_core_answers() {
        let mut state = InviteState::default();
        assert!(state.loading());
        assert_eq!(state.actions().collect::<Vec<_>>(), [Action::Send]);
        state.set_friends(Some(vec![friend("1", false), friend("2", true)]));
        assert!(!state.loading() && !state.failed());
        assert_eq!(
            state.actions().collect::<Vec<_>>(),
            [
                Action::Toggle(Section::Online, 0),
                Action::Toggle(Section::Offline, 0),
                Action::Send
            ]
        );
    }
}
