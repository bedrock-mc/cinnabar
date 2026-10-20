//! Explicit account and preference handoffs for a host without native launcher services.

use super::MenuRuntime;
use launcher::menu::{
    MenuAction, MenuScreen,
    auth::{AccountIntent, AuthState},
    view::{JoinStage, MenuProfile, SavedServer},
};

/// The browser cannot use account-service join targets yet.
pub(super) const SERVICE_JOINS_UNAVAILABLE: &str =
    "Realms, friends' worlds and featured experiences cannot be joined from a browser yet.";

/// Plain menu data only; the platform host owns all runtime and storage handles.
#[derive(Debug, Default)]
pub(super) struct ExternalServices {
    intent: Option<AccountIntent>,
    pub(super) preferences_dirty: bool,
    gui_scale_offset: i8,
    identity: Option<String>,
}

impl MenuRuntime {
    /// Selects externally driven services before the first menu frame.
    pub fn use_external_services(&mut self, servers: Vec<SavedServer>, gui_scale_offset: i8) {
        self.stop_catalog();
        self.auth_process = None;
        self.auth_attempted = true;
        self.auth_restart_requested = false;
        self.external = Some(ExternalServices {
            gui_scale_offset,
            ..Default::default()
        });
        self.servers.replace(servers);
        self.gui_scale_preference = None;
        self.gui_scale_offset = gui_scale_offset;
        self.gui_scale_display_offset = gui_scale_offset;
        self.control_auth = Some(AuthState::SignedOut);
        self.show_home();
    }

    /// Takes one coalesced interactive request for the external account owner.
    pub fn take_account_intent(&mut self) -> Option<AccountIntent> {
        self.external.as_mut()?.intent.take()
    }

    /// Publishes the external account without launching helpers or storing credentials.
    pub fn sync_external_account(
        &mut self,
        state: AuthState,
        profile: MenuProfile,
        stage: Option<JoinStage>,
    ) {
        let Some(external) = self.external.as_mut() else {
            return;
        };
        let retired = match state {
            AuthState::Authenticated => {
                let retired = external
                    .identity
                    .as_ref()
                    .is_none_or(|old| old != &profile.gamertag);
                external.identity = Some(profile.gamertag.clone());
                retired
            }
            AuthState::SignedOut => external.identity.take().is_some(),
            _ => false,
        };
        if retired {
            self.clear_retry_target();
        }
        self.feeds.profile = profile;
        self.apply_control_auth(state);
        if self.is_connecting() {
            self.feeds.join.observe(stage);
        }
    }

    /// Returns the active sign-in link only for a freshly resolved menu action.
    pub fn external_sign_in_link(&self, action: MenuAction) -> Option<String> {
        if self.external.is_none()
            || action != MenuAction::OpenSignInLink
            || !self.visible
            || !self.sign_in_focus()?.contains(&action)
        {
            return None;
        }
        match self.current_auth().as_ref() {
            AuthState::AwaitingCode { code, .. } => launcher::menu::sign_in::device_code_url(code),
            AuthState::AwaitingXboxSignup { uri } => Some(uri.clone()),
            _ => None,
        }
    }

    /// Takes the current durable subset after a menu edit; the host reports write failures.
    pub fn take_external_preferences(&mut self) -> Option<(Vec<SavedServer>, i8)> {
        let external = self.external.as_mut()?;
        external.preferences_dirty |= external.gui_scale_offset != self.gui_scale_offset;
        external.gui_scale_offset = self.gui_scale_offset;
        if !std::mem::take(&mut external.preferences_dirty) {
            return None;
        }
        Some((self.servers.to_vec(), self.gui_scale_offset))
    }

    /// Makes external storage/admission failures visible in the menu that requested them.
    pub fn external_error(&mut self, message: &str) {
        self.message = Some(message.to_owned());
        self.dialog = Some(launcher::menu::MenuDialog::Notice);
        self.focused = 0;
        self.field = None;
    }

    /// Routes account actions to the external host and refuses unsupported native services.
    pub(super) fn external_action(&mut self, action: MenuAction) -> bool {
        let Some(external) = &mut self.external else {
            return false;
        };
        match action {
            MenuAction::StartSignIn => {
                external.intent = Some(AccountIntent::SignIn);
                self.sign_in_requested = true;
                self.sign_in_cancelled = false;
                self.control_auth = Some(AuthState::Checking);
                self.focus_sign_in_prompt();
            }
            MenuAction::CancelSignIn | MenuAction::CloseSignIn => {
                external.intent = Some(AccountIntent::Cancel);
                self.sign_in_requested = false;
                self.sign_in_cancelled = true;
                self.dialog = None;
            }
            MenuAction::SignOut => {
                external.intent = Some(AccountIntent::SignOut);
                self.enter(MenuScreen::Profile);
            }
            MenuAction::OpenSignInLink => {} // The host opens it within the browser gesture.
            MenuAction::AddAccount | MenuAction::SwitchAccount(_) => {
                self.external_error("Sign out before using another account in this browser tab.");
            }
            MenuAction::PlayRealm(_) | MenuAction::PlayFriend(_) => {
                self.external_error(SERVICE_JOINS_UNAVAILABLE);
            }
            _ => return false,
        }
        true
    }

    /// Queues addressed joins without a native account cache or helper cleanup barrier.
    pub(super) fn request_external_connect(&mut self, address: String) {
        if !matches!(
            super::target_for(&address),
            bridge::contract::ConnectTarget::RakNet(_)
        ) {
            self.external_error(SERVICE_JOINS_UNAVAILABLE);
            return;
        }
        if address.trim().is_empty() {
            self.external_error("That server has no address.");
            return;
        }
        bridge::join_profile::JoinProfile::begin();
        self.remember_session_origin();
        self.remember_retry_target(&address, None, false);
        self.intents.join = Some(client_state::JoinIntent {
            address,
            auth_cache: None,
            local_world: false,
        });
        self.disconnect_message = None;
        self.show_connecting();
    }
}

#[cfg(test)]
mod tests;
