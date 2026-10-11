use launcher::accounts::{AccountProfile, AccountStore};

use {
    super::MenuRuntime,
    launcher::menu::{MenuAction, MenuDialog, auth::AuthState},
};

/// Placeholder identities shown while developer recordings present accounts.
const PRESENTATION_ACCOUNTS: [(&str, &str); 4] = [
    ("2535400000000001", "CinnabarDemo"),
    ("2535400000000002", "PixelPioneer"),
    ("2535400000000003", "BlockBuilder"),
    ("2535400000000004", "SkylineSurfer"),
];

#[derive(Debug, Default)]
pub(super) struct Manager {
    pub operation: Option<Operation>,
    remembered: Option<(String, Option<String>)>,
    pub pending_ready: bool,
    pub skip_control: bool,
    pub work: Option<crossbeam_channel::Receiver<(bool, bool)>>,
    pub(super) remember: Option<crossbeam_channel::Receiver<bool>>,
    remember_retry: Option<std::time::Instant>,
}

#[derive(Debug)]
pub(super) enum Operation {
    Switch(String),
    Commit(AccountProfile),
    Restore,
    SignOut,
}

impl MenuRuntime {
    pub(super) fn account_change_pending(&self) -> bool {
        self.feeds.account_adding
            || self.accounts.operation.is_some()
            || self.accounts.work.is_some()
    }

    fn account_store(&self) -> AccountStore {
        AccountStore::new(self.layout.auth_cache())
    }

    pub(super) fn open_accounts(&mut self) {
        if self.over_world() || self.is_connecting() {
            return;
        }
        self.reload_accounts();
        self.dialog = Some(MenuDialog::Accounts);
        self.focused = 0;
        self.feeds.account_error = None;
    }

    /// Presents a signed-in launcher with placeholder accounts, or restores the saved ones.
    /// Enabling needs a signed-out install with no sign-in under way, so no live account data
    /// or action can exist behind the placeholders.
    #[cfg(any(test, feature = "developer-control"))]
    pub(crate) fn set_presentation_accounts(&mut self, enabled: bool) -> bool {
        if enabled
            && (self.account_change_pending()
                || self.auth_restart_requested
                || self.auth_process.as_ref().is_some_and(|process| {
                    matches!(
                        process.state(),
                        AuthState::Checking
                            | AuthState::AwaitingCode { .. }
                            | AuthState::Authenticated
                    )
                })
                || self.layout.auth_cache().is_file())
        {
            return false;
        }
        self.presentation_accounts = enabled;
        self.feeds.account_error = None;
        self.reload_accounts();
        true
    }

    /// The feeds the UI sees; presentation mode shows the selected placeholder's loaded profile.
    pub(super) fn presented_feeds(&self) -> launcher::menu::view::MenuFeeds {
        let mut feeds = self.feeds.clone();
        if let std::borrow::Cow::Owned(profile) = self.presented_profile() {
            feeds.profile = profile;
        }
        feeds
    }

    /// The live profile, or the selected placeholder's completed one in presentation mode.
    pub(super) fn presented_profile(
        &self,
    ) -> std::borrow::Cow<'_, launcher::menu::view::MenuProfile> {
        if !self.presentation_accounts {
            return std::borrow::Cow::Borrowed(&self.feeds.profile);
        }
        std::borrow::Cow::Owned(launcher::menu::view::MenuProfile {
            loaded: true,
            xuid: self.feeds.account_active_id.clone().unwrap_or_default(),
            gamertag: self.presented_display_name(),
            statistics_loaded: true,
            achievements_loaded: true,
            avatar_loaded: true,
            avatar_error: true,
            featured_screenshot_loaded: true,
            featured_screenshot_error: true,
            ..Default::default()
        })
    }

    /// The selected placeholder's name in presentation mode, otherwise the live display name.
    pub(super) fn presented_display_name(&self) -> String {
        self.presentation_accounts
            .then_some(self.feeds.account_active_id.as_deref())
            .flatten()
            .and_then(|active| PRESENTATION_ACCOUNTS.iter().find(|(id, _)| *id == active))
            .map_or_else(|| self.display_name.clone(), |(_, name)| (*name).to_owned())
    }

    /// Account actions only change the in-memory presentation while it is shown.
    pub(super) fn presentation_blocks(&mut self, action: MenuAction) -> bool {
        if !self.presentation_accounts {
            return false;
        }
        match action {
            MenuAction::SwitchAccount(index) => {
                if let Some(account) = self.feeds.accounts.get(index) {
                    self.feeds.account_active_id = Some(account.id.clone());
                }
                self.dialog = None;
                true
            }
            MenuAction::StartSignIn
            | MenuAction::CancelSignIn
            | MenuAction::CloseSignIn
            | MenuAction::AddAccount
            | MenuAction::SignOut => true,
            _ => false,
        }
    }

    fn reload_accounts(&mut self) {
        if self.presentation_accounts {
            self.feeds.accounts = PRESENTATION_ACCOUNTS
                .iter()
                .map(|(id, gamertag)| AccountProfile {
                    id: (*id).into(),
                    gamertag: (*gamertag).into(),
                    picture_path: None,
                })
                .collect();
            let shown = self
                .feeds
                .account_active_id
                .as_deref()
                .is_some_and(|active| PRESENTATION_ACCOUNTS.iter().any(|(id, _)| *id == active));
            if !shown {
                self.feeds.account_active_id = Some(PRESENTATION_ACCOUNTS[0].0.into());
            }
            return;
        }
        let store = self.account_store();
        match store.list() {
            Ok(accounts) => {
                self.feeds.accounts = accounts;
                self.feeds.account_active_id = self
                    .layout
                    .auth_cache()
                    .is_file()
                    .then(|| store.active_id().ok().flatten())
                    .flatten();
            }
            Err(_) => self.feeds.account_error = Some("Could not load saved accounts.".into()),
        }
    }

    pub(super) fn add_account(&mut self) {
        if self.over_world()
            || self.is_connecting()
            || self.feeds.account_adding
            || self.accounts.work.is_some()
            || self.accounts.operation.is_some()
        {
            return;
        }
        self.dialog = Some(MenuDialog::Accounts);
        if self.layout.auth_cache().is_file()
            && self.feeds.account_active_id.is_none()
            && matches!(
                self.auth_process.as_ref().map(|p| p.state()),
                Some(AuthState::Authenticated)
            )
        {
            self.feeds.account_error =
                Some("Your Xbox profile is loading. Try again shortly.".into());
            self.feeds.profile_refresh_requested = true;
            return;
        }
        self.feeds.account_error = None;
        self.feeds.account_adding = true;
        self.accounts.pending_ready = false;
        // Each Add account starts a fresh device flow, even after an abandoned attempt.
        let _ = self.account_store().discard_pending();
        self.start_sign_in();
    }

    pub(super) fn switch_account(&mut self, index: usize) {
        if self.over_world()
            || self.is_connecting()
            || self.feeds.account_adding
            || self.accounts.work.is_some()
            || self.accounts.operation.is_some()
        {
            return;
        }
        let Some(account) = self.feeds.accounts.get(index) else {
            return;
        };
        if self.feeds.account_active_id.as_deref() == Some(&account.id) {
            self.dialog = None;
            return;
        }
        self.sign_in_cancelled = false;
        self.accounts.operation = Some(Operation::Switch(account.id.clone()));
        self.retry_target = None;
        self.focused = 0;
    }

    pub(super) fn dismiss_accounts(&mut self) {
        if self.dialog == Some(MenuDialog::Accounts) && self.feeds.account_adding {
            self.cancel_add_account();
        }
        self.dialog = None;
        if self.sign_in_focus().is_some() {
            self.focus_sign_in_prompt();
        }
    }

    pub(super) fn cancel_add_account(&mut self) {
        self.stop_sign_in();
        self.accounts.operation = Some(Operation::Restore);
    }

    pub(super) fn poll_accounts(&mut self) {
        if let Some(receiver) = &self.accounts.work {
            match receiver.try_recv() {
                Ok((success, signed_out)) => {
                    self.accounts.work = None;
                    self.finish_account_operation(success, signed_out);
                }
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    self.accounts.work = None;
                    self.finish_account_operation(false, false);
                }
                Err(crossbeam_channel::TryRecvError::Empty) => {}
            }
            return;
        }
        if let Some(receiver) = &self.accounts.remember {
            let result = receiver.try_recv();
            if !matches!(result, Err(crossbeam_channel::TryRecvError::Empty)) {
                let success = result.unwrap_or(false);
                self.accounts.remember = None;
                if !success {
                    self.accounts.remembered = None;
                    self.accounts.remember_retry =
                        Some(std::time::Instant::now() + std::time::Duration::from_secs(1));
                } else {
                    self.reload_accounts();
                }
            }
        }
        if self.accounts.operation.is_some() {
            return;
        }
        if self.feeds.account_adding {
            match self.auth_process.as_ref().map(|p| p.state()) {
                Some(AuthState::Authenticated) if !self.accounts.pending_ready => {
                    self.accounts.pending_ready = true;
                    self.feeds.profile = Default::default();
                    self.control_auth = None;
                }
                None if !matches!(self.current_auth().as_ref(), AuthState::Failed(_)) => {
                    self.feeds.account_error = Some("Sign-in did not complete. Try again.".into());
                    self.accounts.operation = Some(Operation::Restore);
                }
                _ => {}
            }
        }
        let profile = &self.feeds.profile;
        if self.feeds.account_adding && self.accounts.pending_ready && profile.unavailable {
            self.feeds.account_error = Some("Xbox profile unavailable. Retrying…".into());
        }
        if !profile.loaded || profile.xuid.is_empty() || profile.gamertag.is_empty() {
            return;
        }
        let identity = (
            profile.xuid.clone(),
            (!profile.picture_path.is_empty()).then(|| profile.picture_path.clone()),
        );
        if self.feeds.account_adding {
            if self.accounts.pending_ready && self.accounts.operation.is_none() {
                self.accounts.operation = Some(Operation::Commit(AccountProfile {
                    id: profile.xuid.clone(),
                    gamertag: profile.gamertag.clone(),
                    picture_path: (!profile.picture_path.is_empty())
                        .then(|| profile.picture_path.clone()),
                }));
            }
        } else if self
            .accounts
            .remember_retry
            .is_none_or(|retry| std::time::Instant::now() >= retry)
            && self.accounts.remember.is_none()
            && self.accounts.remembered.as_ref() != Some(&identity)
        {
            let store = self.account_store();
            let profile = profile.clone();
            let (sender, receiver) = crossbeam_channel::bounded(1);
            if std::thread::Builder::new()
                .name("account-save".into())
                .spawn(move || {
                    let success = store
                        .remember_current(
                            &profile.xuid,
                            &profile.gamertag,
                            (!profile.picture_path.is_empty())
                                .then_some(profile.picture_path.as_str()),
                        )
                        .is_ok();
                    let _ = sender.send(success);
                })
                .is_ok()
            {
                self.accounts.remembered = Some(identity);
                self.accounts.remember = Some(receiver);
            }
        }
    }

    /// Runs only after the core releases its active credential cache.
    pub(super) fn account_operation_job(&mut self) -> impl FnOnce() + Send + 'static {
        let operation = self
            .accounts
            .operation
            .take()
            .expect("queued account operation");
        let store = self.account_store();
        let (sender, receiver) = crossbeam_channel::bounded(1);
        self.accounts.work = Some(receiver);
        move || {
            let signed_out = matches!(&operation, Operation::SignOut);
            let result = match operation {
                Operation::Switch(id) => store.activate(&id),
                Operation::Commit(profile) => store
                    .commit_pending(
                        &profile.id,
                        &profile.gamertag,
                        profile.picture_path.as_deref(),
                    )
                    .map(|_| ()),
                Operation::Restore => Ok(()),
                Operation::SignOut => store.sign_out(),
            };
            let _ = store.discard_pending();
            let _ = sender.send((result.is_ok(), signed_out));
        }
    }

    fn finish_account_operation(&mut self, success: bool, signed_out: bool) {
        if !success {
            self.feeds.account_error = Some("Could not switch accounts. Try again.".into());
        }
        self.feeds.account_adding = false;
        self.accounts.pending_ready = false;
        self.accounts.remembered = None;
        self.feeds.profile = Default::default();
        self.feeds.home = Default::default();
        self.feeds.inbox_state = Default::default();
        self.feeds.selected_realm = None;
        self.feeds.details.clear();
        self.realms.clear();
        self.friends.clear();
        self.catalog_started = false;
        self.control_auth = None;
        self.reload_accounts();
        if !signed_out {
            if self.sign_in_cancelled && !self.layout.auth_cache().is_file() {
                self.auth_process = None;
            } else {
                let cancelled = self.sign_in_cancelled;
                self.start_sign_in();
                if cancelled {
                    self.sign_in_cancelled = true;
                    self.sign_in_requested = false;
                }
            }
        }
        if self.dialog.is_some() {
            self.dialog = Some(MenuDialog::Accounts);
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        launcher::menu::auth::AuthState,
        launcher::menu::{MenuAction, MenuDialog},
    };

    #[test]
    fn presentation_accounts_never_sign_in_or_queue_store_changes() {
        let mut menu = MenuRuntime::new(true, 2, "First".into());
        menu.feeds.account_adding = true;
        assert!(!menu.set_presentation_accounts(true));
        menu.feeds.account_adding = false;
        let cache = menu.layout.auth_cache();
        std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
        std::fs::write(&cache, b"{}").unwrap();
        assert!(
            !menu.set_presentation_accounts(true),
            "a signed-in install is refused"
        );
        std::fs::remove_file(&cache).unwrap();
        menu.auth_restart_requested = true;
        assert!(
            !menu.set_presentation_accounts(true),
            "a queued sign-in is refused"
        );
        menu.auth_restart_requested = false;
        assert!(menu.set_presentation_accounts(true));
        assert_eq!(menu.view().auth_state, AuthState::Authenticated);
        menu.activate(MenuAction::OpenAccounts);
        assert_eq!(menu.dialog, Some(MenuDialog::Accounts));
        assert_eq!(menu.feeds.accounts.len(), PRESENTATION_ACCOUNTS.len());
        menu.activate(MenuAction::AddAccount);
        menu.activate(MenuAction::StartSignIn);
        menu.activate(MenuAction::SignOut);
        menu.activate(MenuAction::CancelSignIn);
        assert!(menu.auth_process.is_none());
        assert!(menu.accounts.operation.is_none());
        assert!(!menu.feeds.account_adding && !menu.sign_out_requested);
        menu.activate(MenuAction::SwitchAccount(2));
        assert!(menu.accounts.operation.is_none());
        menu.activate(MenuAction::OpenAccounts);
        assert_eq!(
            menu.feeds.account_active_id.as_deref(),
            Some(PRESENTATION_ACCOUNTS[2].0)
        );
        let view = menu.view();
        assert_eq!(view.display_name, PRESENTATION_ACCOUNTS[2].1);
        assert!(view.feeds.profile.loaded);
        assert_eq!(view.feeds.profile.gamertag, PRESENTATION_ACCOUNTS[2].1);
        assert!(
            menu.feeds.profile.gamertag.is_empty(),
            "the live profile is untouched"
        );
        menu.activate(MenuAction::Navigate(launcher::menu::MenuScreen::Profile));
        assert!(menu.focus_actions().contains(&MenuAction::SelectProfileTab(
            launcher::menu::ProfileTab::Stats
        )));
        menu.set_presentation_accounts(false);
        assert_eq!(menu.view().display_name, "First");
        assert_ne!(menu.view().auth_state, AuthState::Authenticated);
        assert!(menu.feeds.accounts.iter().all(|account| {
            PRESENTATION_ACCOUNTS
                .iter()
                .all(|(id, _)| account.id != *id)
        }));
    }

    #[test]
    fn account_picker_queues_switch_without_replacing_live_credentials() {
        let mut menu = MenuRuntime::new(true, 2, "First".into());
        menu.feeds.accounts = vec![AccountProfile {
            id: "42".into(),
            gamertag: "Second".into(),
            picture_path: None,
        }];
        menu.feeds.account_active_id = Some("41".into());
        menu.dialog = Some(MenuDialog::Accounts);
        menu.activate(MenuAction::SwitchAccount(0));
        assert!(matches!(&menu.accounts.operation, Some(Operation::Switch(id)) if id == "42"));
        assert_eq!(menu.feeds.account_active_id.as_deref(), Some("41"));
        assert!(menu.focus_actions().contains(&MenuAction::AddAccount));
    }

    #[test]
    fn dismissing_an_add_account_flow_restores_instead_of_signing_out() {
        let mut menu = MenuRuntime::new(true, 2, "First".into());
        menu.feeds.account_active_id = Some("41".into());
        menu.feeds.account_adding = true;
        menu.dialog = Some(MenuDialog::Accounts);
        menu.go_back();
        assert!(menu.dialog.is_none());
        assert!(matches!(menu.accounts.operation, Some(Operation::Restore)));
        assert!(!menu.sign_out_requested);
        assert_eq!(menu.feeds.account_active_id.as_deref(), Some("41"));
    }

    #[test]
    fn back_cancels_add_account_without_restarting_a_signed_out_flow() {
        let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
        menu.feeds.account_adding = true;
        menu.dialog = Some(MenuDialog::Accounts);
        menu.apply_control_auth(AuthState::AwaitingCode {
            uri: "https://example.invalid".into(),
            code: "TEST-CODE".into(),
        });
        menu.go_back();
        assert!(menu.dialog.is_none());
        menu.account_operation_job()();
        menu.poll_accounts();
        assert!(!menu.sign_in_requested);
        assert!(menu.sign_in_cancelled);
        assert_eq!(menu.current_auth().as_ref(), &AuthState::SignedOut);
        assert!(menu.auth_process.is_none());
    }

    #[test]
    fn joining_waits_until_the_selected_accounts_credentials_are_active() {
        let mut menu = MenuRuntime::new(true, 2, "First".into());
        menu.accounts.operation = Some(Operation::Switch("2".into()));
        menu.request_connect("example.invalid".into());
        assert!(menu.intents.join.is_none());
        assert!(!menu.is_connecting());
        menu.accounts.operation = None;
        menu.request_connect("example.invalid".into());
        assert!(menu.intents.join.is_some());
    }

    #[test]
    fn changing_accounts_drops_inbox_actions_and_read_state_from_the_previous_account() {
        let mut menu = MenuRuntime::new(true, 2, "First".into());
        let item = launcher::menu::view::InboxItem {
            instance_id: "shared-message".into(),
            category: "News".into(),
            unread: true,
            ..Default::default()
        };
        menu.feeds.home.inbox = vec![item.clone()];
        menu.feeds
            .activate_inbox(launcher::menu::inbox::Action::Delete(0));
        menu.feeds
            .activate_inbox(launcher::menu::inbox::Action::ConfirmDelete);
        assert!(!menu.feeds.inbox_state.pending.is_empty());
        menu.finish_account_operation(true, true);
        menu.feeds.home.inbox = vec![item];
        menu.feeds.inbox_state.reconcile(&mut menu.feeds.home);
        assert_eq!(menu.feeds.home.inbox.len(), 1);
        assert!(menu.feeds.home.inbox[0].unread);
        assert!(menu.feeds.inbox_state.pending.is_empty());
    }

    #[test]
    fn account_management_cannot_change_the_account_owning_a_live_world() {
        let mut menu = MenuRuntime::new(true, 2, "First".into());
        menu.set_visible(false);
        menu.open_pause();
        menu.feeds.accounts = vec![AccountProfile {
            id: "42".into(),
            gamertag: "Second".into(),
            picture_path: None,
        }];
        menu.open_accounts();
        menu.add_account();
        menu.switch_account(0);
        assert!(menu.dialog.is_none());
        assert!(!menu.feeds.account_adding);
        assert!(menu.accounts.operation.is_none());
    }
}
