//! Host adapter for the Realm invitation flow. Requests run on the account worker.

use launcher::menu::realm_membership::{Action, State};

use launcher_host::account_control::AccountControl;
use {
    super::MenuRuntime,
    launcher::menu::{MenuAction, MenuField, MenuScreen, auth::AuthState, view::MenuRealmCard},
};

/// A request generation prevents a closed or reopened flow from consuming an old answer.
#[derive(Debug)]
pub(super) struct RealmUi {
    pub(super) state: Option<State>,
    pub(super) code: ui::ChatEditor,
    ticket: u64,
    request: Option<bool>,
    cancel: bool,
    account_generation: Option<u64>,
}

impl Default for RealmUi {
    fn default() -> Self {
        Self {
            state: None,
            code: super::input::field_editor(MenuField::RealmCode),
            ticket: 0,
            request: None,
            cancel: false,
            account_generation: None,
        }
    }
}

impl MenuRuntime {
    /// Opens or advances membership without connecting to a game before it has been accepted.
    pub(super) fn activate_realm_membership(&mut self, action: Action) {
        if action == Action::Open {
            if self.current_auth().as_ref() != &AuthState::Authenticated || self.over_world() {
                return;
            }
            self.enter(MenuScreen::Social);
            self.realm_membership.ticket = self.realm_membership.ticket.wrapping_add(1);
            self.realm_membership.code.clear();
            self.realm_membership.account_generation = None;
            self.realm_membership.state = Some(State::default());
            self.focused = 0;
            self.focus_field(MenuField::RealmCode);
            return;
        }
        let Some(state) = self.realm_membership.state.as_mut() else {
            return;
        };
        match action {
            Action::EditCode
                if state.can_verify()
                    || state.stage == launcher::menu::realm_membership::Stage::Code =>
            {
                self.focus_field(MenuField::RealmCode)
            }
            Action::Verify | Action::Accept => {
                let accept = action == Action::Accept;
                if state.begin(accept) {
                    self.realm_membership.ticket = self.realm_membership.ticket.wrapping_add(1);
                    self.realm_membership.request = Some(accept);
                    self.field = None;
                    self.focused = 0;
                }
            }
            Action::Back if state.can_back() => {
                use launcher::menu::realm_membership::Stage;
                if state.stage == Stage::Confirm {
                    state.code = self.realm_membership.code.as_str().to_owned();
                    state.stage = Stage::Code;
                    state.realm = None;
                    state.error = None;
                    self.focus_field(MenuField::RealmCode);
                } else {
                    self.close_realm_membership();
                }
            }
            Action::Play if state.can_play() => {
                let target = state.realm.as_ref().map(|realm| realm.target.clone());
                self.close_realm_membership();
                if let Some(index) = self
                    .realms
                    .iter()
                    .position(|realm| Some(&realm.target) == target.as_ref())
                {
                    self.activate(MenuAction::PlayRealm(index));
                }
            }
            _ => {}
        }
    }

    /// Retires pending previews on navigation or account changes.
    pub(super) fn close_realm_membership(&mut self) {
        if self.realm_membership.state.take().is_some() {
            self.realm_membership.ticket = self.realm_membership.ticket.wrapping_add(1);
            self.realm_membership.request = None;
            self.realm_membership.cancel = true;
            self.realm_membership.account_generation = None;
            self.field = None;
            self.focused = 0;
        }
    }

    /// Sends a queued request once and installs only the current signed-in flow's reply.
    pub(super) fn sync_realm_membership(&mut self, control: &mut dyn AccountControl) {
        let generation = control.account_generation();
        if self.current_auth().as_ref() != &AuthState::Authenticated
            || self
                .realm_membership
                .account_generation
                .zip(generation)
                .is_some_and(|(old, current)| old != current)
        {
            self.close_realm_membership();
        }
        if self.realm_membership.state.is_some() {
            self.realm_membership.account_generation = generation;
        }
        if std::mem::take(&mut self.realm_membership.cancel) {
            control.cancel_realm_membership();
        }
        if let Some(accept) = self.realm_membership.request.take()
            && let Some(state) = &self.realm_membership.state
        {
            let ticket = self.realm_membership.ticket;
            if !control.request_realm_membership(ticket, state.code.clone(), accept) {
                self.receive_realm_membership(ticket, accept, Err(()));
            }
        }
        if let Some((ticket, accept, result)) = control.realm_membership() {
            self.receive_realm_membership(ticket, accept, result);
        }
    }

    /// Rejects replies from earlier openings and publishes accepted membership into the catalog.
    fn receive_realm_membership(
        &mut self,
        ticket: u64,
        accept: bool,
        result: Result<(String, MenuRealmCard), ()>,
    ) {
        if ticket != self.realm_membership.ticket
            || self.current_auth().as_ref() != &AuthState::Authenticated
        {
            return;
        }
        let Some(state) = self.realm_membership.state.as_mut() else {
            return;
        };
        let success = state.finish(accept, result);
        self.focused = 0;
        if success
            && accept
            && let Some(realm) = &state.realm
        {
            match self
                .realms
                .iter()
                .position(|old| old.target == realm.target)
            {
                Some(index) => {
                    self.realms[index] = realm.clone();
                    self.feeds.selected_realm = Some(index);
                }
                None => {
                    self.feeds.selected_realm = Some(self.realms.len());
                    self.realms.push(realm.clone());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
