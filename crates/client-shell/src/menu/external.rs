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
            AuthState::AwaitingCode { uri, .. } | AuthState::AwaitingXboxSignup { uri } => {
                Some(uri.clone())
            }
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
                self.clear_retry_target();
                self.enter(MenuScreen::Profile);
            }
            MenuAction::OpenSignInLink => {} // The host opens it within the browser gesture.
            MenuAction::AddAccount | MenuAction::SwitchAccount(_) => {
                self.message =
                    Some("Sign out before using another account in this browser tab.".into());
            }
            MenuAction::PlayRealm(_) | MenuAction::PlayFriend(_) => {
                self.message = Some(SERVICE_JOINS_UNAVAILABLE.into());
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
            self.message = Some(SERVICE_JOINS_UNAVAILABLE.into());
            return;
        }
        if address.trim().is_empty() {
            self.message = Some("That server has no address.".into());
            return;
        }
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
mod tests {
    use super::*;

    #[test]
    fn external_account_actions_never_start_native_helpers() {
        let mut menu = MenuRuntime::new(true, 2, "Fixture".into());
        menu.use_external_services(vec![], 1);
        assert_eq!(menu.view().screen, MenuScreen::Home);
        menu.activate(MenuAction::StartSignIn);
        assert_eq!(menu.take_account_intent(), Some(AccountIntent::SignIn));
        assert_eq!(menu.current_auth().as_ref(), &AuthState::Checking);
        assert!(menu.auth_process.is_none());
        menu.sync_external_account(
            AuthState::AwaitingCode {
                uri: "https://example.invalid".into(),
                code: "TEST".into(),
            },
            MenuProfile::default(),
            None,
        );
        assert!(
            menu.external_sign_in_link(MenuAction::OpenSignInLink)
                .is_some()
        );
        assert!(
            menu.external_sign_in_link(MenuAction::StartSignIn)
                .is_none()
        );
        menu.activate(MenuAction::CancelSignIn);
        assert_eq!(menu.take_account_intent(), Some(AccountIntent::Cancel));
        assert!(
            menu.external_sign_in_link(MenuAction::OpenSignInLink)
                .is_none()
        );
        menu.activate(MenuAction::SignOut);
        assert_eq!(menu.take_account_intent(), Some(AccountIntent::SignOut));
        assert!(menu.accounts.operation.is_none());
    }

    #[test]
    fn external_menu_refuses_service_targets_and_keeps_addressed_join_intent() {
        let mut menu = MenuRuntime::new(true, 2, "Fixture".into());
        menu.use_external_services(vec![], 0);
        for address in [
            format!("{}1", bridge::contract::REALM_TARGET_PREFIX),
            format!("{}1", launcher::menu::FRIEND_ADDRESS_PREFIX),
            format!("{}1", launcher::menu::EXPERIENCE_ADDRESS_PREFIX),
        ] {
            menu.request_connect(address);
            assert!(menu.take_join_intent().is_none());
            assert!(menu.view().message.is_some());
        }
        menu.request_connect("example.invalid".into());
        let join = menu.take_join_intent().unwrap();
        assert_eq!(join.address, "example.invalid");
        assert!(join.auth_cache.is_none());
        assert!(!join.local_world);
    }

    #[test]
    fn external_preferences_emit_only_edits_and_surface_storage_failures() {
        let mut menu = MenuRuntime::new(true, 2, "Fixture".into());
        menu.use_external_services(vec![], 0);
        assert!(menu.take_external_preferences().is_none());
        menu.servers.push(SavedServer {
            name: "Fixture".into(),
            address: "example.invalid".into(),
            favorite: false,
            last_joined_unix: 0,
        });
        menu.save_servers();
        let (servers, offset) = menu.take_external_preferences().unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(offset, 0);
        assert!(menu.take_external_preferences().is_none());
        menu.gui_scale_offset = -1;
        assert_eq!(menu.take_external_preferences().unwrap().1, -1);
        assert!(menu.take_external_preferences().is_none());
        menu.external_error("Browser preferences could not be saved.");
        assert_eq!(
            menu.view().message.as_deref(),
            Some("Browser preferences could not be saved.")
        );
    }

    #[test]
    fn external_retry_survives_transport_loss_but_not_an_account_change() {
        let mut menu = MenuRuntime::new(true, 2, "Fixture".into());
        menu.use_external_services(vec![], 0);
        let profile = MenuProfile {
            gamertag: "Fixture".into(),
            ..MenuProfile::unavailable()
        };
        menu.sync_external_account(AuthState::Authenticated, profile.clone(), None);
        assert_eq!(menu.sign_in_status(), crate::menu::SignInStatus::SignedIn);
        assert!(!menu.view().catalog_loading);
        assert_eq!(
            menu.view().catalog_message.as_deref(),
            Some(SERVICE_JOINS_UNAVAILABLE)
        );
        menu.activate(MenuAction::RefreshCatalog);
        assert!(!menu.view().catalog_loading);
        assert!(menu.view().catalog_message.is_some());
        menu.request_connect("example.invalid".into());
        assert!(menu.take_join_intent().is_some());
        menu.show_join_failure("Fixture failure".into());
        assert!(!menu.view().catalog_loading);
        menu.sync_external_account(
            AuthState::Failed("Disconnected".into()),
            MenuProfile::default(),
            None,
        );
        assert!(menu.retry_target.is_some());
        menu.sync_external_account(AuthState::Authenticated, profile, None);
        assert!(menu.retry_target.is_some());
        menu.sync_external_account(AuthState::SignedOut, MenuProfile::default(), None);
        assert!(menu.retry_target.is_none());
    }
}
