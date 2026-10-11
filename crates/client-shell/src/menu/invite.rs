//! The pause screen's "Invite to Game" while the open world is hosted for Xbox friends: opens
//! vanilla's invite screen, asks the core for the account's friends, and hands the picked
//! friends' invites to the account worker, all off the frame.

use std::sync::Arc;

use launcher::menu::invite::{Action, InviteState};

use launcher_host::account_control::AccountControl;
use {
    super::MenuRuntime,
    launcher::menu::{MenuAction, MenuScreen},
};

/// The invite screen's state, the friends request waiting to go out and the invites to send.
#[derive(Debug, Default)]
pub(super) struct InviteUi {
    state: Arc<InviteState>,
    fetch: bool,
    sends: Vec<String>,
}

impl MenuRuntime {
    pub(super) fn activate_invite(&mut self, action: Action) {
        match action {
            Action::Open if self.hosting_world() => {
                self.invite.state = Arc::default();
                self.invite.fetch = true;
                self.enter(MenuScreen::Invite);
            }
            Action::Open => {}
            Action::Toggle(section, index) => {
                Arc::make_mut(&mut self.invite.state).toggle(section, index);
            }
            // Vanilla sends and returns to the pause screen; nothing picked just closes it.
            Action::Send => {
                let picked = self.invite.state.selected_xuids();
                self.invite.sends.extend(picked);
                self.go_back();
            }
        }
    }

    /// Requests the friends list once per opening, installs the answer, and queues the sends.
    pub(super) fn sync_invites(&mut self, control: &mut dyn AccountControl) {
        if std::mem::take(&mut self.invite.fetch) {
            control.request_people();
        }
        if let Some(friends) = control.people() {
            Arc::make_mut(&mut self.invite.state).set_friends(friends.ok());
        }
        if !self.invite.sends.is_empty() {
            control.send_invites(std::mem::take(&mut self.invite.sends));
        }
    }

    /// The invite screen's state for the view while it is up.
    pub(super) fn invite_view(&self) -> Option<Arc<InviteState>> {
        (self.screen == MenuScreen::Invite).then(|| Arc::clone(&self.invite.state))
    }

    /// Keyboard and gamepad order: each friend's checkbox, send, then close.
    pub(super) fn invite_focus_actions(&self) -> Vec<MenuAction> {
        self.invite
            .state
            .actions()
            .map(MenuAction::Invite)
            .chain(std::iter::once(MenuAction::AddBack))
            .collect()
    }
}

#[cfg(test)]
mod tests;
