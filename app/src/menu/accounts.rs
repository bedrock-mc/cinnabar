use launcher::accounts::{AccountProfile, AccountStore};

use super::{AuthState, MenuAction, MenuDialog, MenuRuntime};

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
    #[cfg(any(test, feature = "developer-control"))]
    pub(crate) fn set_presentation_accounts(&mut self, enabled: bool) {
        self.presentation_accounts = enabled;
        self.feeds.account_error = None;
        self.reload_accounts();
    }

    /// The feeds the UI sees; presentation mode hides the live profile behind the placeholders.
    pub(super) fn presented_feeds(&self) -> launcher::menu::view::MenuFeeds {
        let mut feeds = self.feeds.clone();
        if self.presentation_accounts {
            feeds.profile = Default::default();
        }
        feeds
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
            MenuAction::StartSignIn | MenuAction::AddAccount | MenuAction::SignOut => true,
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
        self.sign_in_page_code = None;
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
        self.accounts.operation = Some(Operation::Switch(account.id.clone()));
        self.focused = 0;
    }

    pub(super) fn dismiss_accounts(&mut self) {
        if self.dialog == Some(MenuDialog::Accounts) && self.feeds.account_adding {
            self.cancel_add_account();
        }
        self.dialog = None;
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
                Some(AuthState::Failed(_)) | None => {
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
        self.sign_in_page_code = None;
        self.reload_accounts();
        if !signed_out {
            self.start_sign_in();
        }
        if self.dialog.is_some() {
            self.dialog = Some(MenuDialog::Accounts);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presentation_accounts_never_sign_in_or_queue_store_changes() {
        let mut menu = MenuRuntime::new(true, 2, "First".into());
        menu.set_presentation_accounts(true);
        assert_eq!(menu.view().auth_state, AuthState::Authenticated);
        menu.activate(MenuAction::OpenAccounts);
        assert_eq!(menu.dialog, Some(MenuDialog::Accounts));
        assert_eq!(menu.feeds.accounts.len(), PRESENTATION_ACCOUNTS.len());
        menu.activate(MenuAction::AddAccount);
        menu.activate(MenuAction::StartSignIn);
        menu.activate(MenuAction::SignOut);
        assert!(menu.auth_process.is_none());
        assert!(!menu.feeds.account_adding && !menu.sign_out_requested);
        menu.activate(MenuAction::SwitchAccount(2));
        assert!(menu.accounts.operation.is_none());
        menu.activate(MenuAction::OpenAccounts);
        assert_eq!(
            menu.feeds.account_active_id.as_deref(),
            Some(PRESENTATION_ACCOUNTS[2].0)
        );
        menu.feeds.profile.gamertag = "RealGamertag".into();
        menu.feeds.profile.picture_path = "/real/picture.png".into();
        let view = menu.view();
        assert!(view.feeds.profile.gamertag.is_empty());
        assert!(view.feeds.profile.picture_path.is_empty());
        assert_eq!(menu.feeds.profile.gamertag, "RealGamertag");
        menu.set_presentation_accounts(false);
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
