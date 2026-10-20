use launcher::menu::auth::select_auth;
use launcher::menu::view::MenuProfile;
use launcher::menu::{MenuAction, MenuDialog, MenuScreen};
use {super::*, launcher::install_layout::InstallLayout, launcher::menu::auth::AuthState};

/// Longest a join waits for the saved account's check before going ahead signed out.
const JOIN_SIGN_IN_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

pub(super) fn validated_auth_cache(
    layout: &InstallLayout,
    state: Option<&AuthState>,
) -> Option<PathBuf> {
    matches!(state, Some(AuthState::Authenticated)).then(|| layout.auth_cache())
}

/// Coarse account state for automation; it never carries an identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignInStatus {
    SignedIn,
    Checking,
    SignedOut,
}

impl SignInStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SignedIn => "signed_in",
            Self::Checking => "checking",
            Self::SignedOut => "signed_out",
        }
    }
}

/// Where a queued join stands relative to the saved account's check.
pub(super) enum JoinSignIn {
    Waiting,
    /// The check settled, or none was pending; the join's account is final.
    Settled,
    /// The wait expired: the join goes ahead signed out while the check keeps running.
    Expired,
}

impl MenuRuntime {
    /// Installs the application's retained edge before any menu/account worker is admitted.
    /// Offline UI construction requires no runtime; it cannot start workers without this injection.
    pub fn with_io(
        mut self,
        runtime: tokio::runtime::Handle,
        tasks: tokio_util::task::TaskTracker,
    ) -> Self {
        self.native_io = Some((runtime, tasks));
        self
    }

    /// Returns execution/accounting capabilities, never ownership of the actual Runtime lifetime.
    pub fn io_execution(
        &self,
    ) -> std::io::Result<(tokio::runtime::Handle, tokio_util::task::TaskTracker)> {
        self.native_io.clone().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "application I/O owner is unavailable",
            )
        })
    }
    /// Selects the native sign-in worker before any account activity, preserving the existing presenter.
    #[cfg(feature = "native-networking")]
    pub fn with_native_sign_in(mut self, enabled: bool) -> Self {
        self.native_sign_in = enabled;
        self
    }

    fn start_catalog(&mut self) {
        if self.native_sign_in || self.catalog_started || !self.visible || self.is_connecting() {
            return;
        }
        if self.auth_attempted && self.auth_process.is_none() {
            return;
        }
        if matches!(
            self.auth_process.as_ref().map(AuthSupervisor::state),
            Some(
                AuthState::Checking
                    | AuthState::AwaitingCode { .. }
                    | AuthState::AwaitingXboxSignup { .. }
            )
        ) || matches!(
            self.auth_process.as_ref().map(AuthSupervisor::state),
            Some(AuthState::Failed(_) | AuthState::SignedOut)
        ) {
            return;
        }
        self.catalog_started = true;
        if let Err(message) = self.catalog.start(&self.layout) {
            self.catalog_message = Some(message);
        }
    }

    /// `core_feeds`: a launcher core serves the lists, so the one-shot catalog
    /// process (which would overwrite them with other join addresses) stays off.
    pub fn poll_catalog(&mut self, core_feeds: bool) {
        if self.external.is_some() {
            return;
        }
        self.poll_sign_in();
        self.cancel_join_fallback_prompt();
        // Cached validation also runs when a signed-out account core is already
        // attached, as happens before opening the menu in a direct session.
        if self.visible
            && !self.is_connecting()
            && self.should_auto_start_sign_in(auth_cache_path(&self.layout).is_some())
        {
            self.start_cached_sign_in();
        }
        self.update_sign_in_browser(false);
        self.poll_catalog_art();
        if core_feeds || self.native_sign_in {
            self.stop_catalog();
            return;
        }
        if self.screen == MenuScreen::Profile && !self.feeds.profile.loaded {
            self.feeds.profile = MenuProfile::unavailable();
            launcher_host::launcher_account::profile_worker::log_unavailable("worker_unavailable");
        }
        self.start_catalog();
        match self.catalog.poll() {
            Ok(Some(catalog)) => self.apply_catalog(catalog),
            Ok(None) => {}
            Err(message) => self.catalog_message = Some(message),
        }
    }

    pub(super) fn stop_catalog(&mut self) {
        self.catalog.stop();
    }

    fn apply_catalog(&mut self, catalog: CatalogFile) {
        let featured = self.catalog_cards(catalog.featured);
        self.featured.replace(featured);
        if !self.native_sign_in {
            self.realms.replace(catalog.realms);
            self.friends
                .replace(catalog.friends.into_iter().map(Into::into).collect());
        }
        // Service errors may contain URLs, response bodies, or account material.
        // Keep successful sections, but expose only controlled recovery copy.
        self.catalog_message =
            (!catalog.errors.is_empty()).then(|| "Social: Refresh to try again.".to_owned());
    }

    /// Validates the saved account in the background, without taking focus for a prompt.
    fn start_cached_sign_in(&mut self) {
        self.start_sign_in();
        self.sign_in_requested = false;
    }

    /// The saved account's background check is running, as opposed to an interactive prompt.
    fn cached_sign_in_checking(&self) -> bool {
        !self.sign_in_requested
            && !self.feeds.account_adding
            && !self.auth_restart_requested
            && matches!(
                self.auth_process.as_ref().map(AuthSupervisor::state),
                Some(AuthState::Checking)
            )
    }

    /// A new join waits on the saved account's check instead of cancelling it, starting the
    /// check if the menu has not yet; interactive sign-in is abandoned as before.
    pub(super) fn hold_join_for_sign_in(&mut self) {
        #[cfg(feature = "native-networking")]
        self.native_catalog.pause();
        self.intents.native_auth_required = self.native_sign_in
            && (self.feeds.account_active_id.is_some()
                || auth_cache_path(&self.layout).is_some()
                || matches!(
                    self.auth_process.as_ref().map(AuthSupervisor::state),
                    Some(
                        AuthState::Checking
                            | AuthState::AwaitingCode { .. }
                            | AuthState::AwaitingXboxSignup { .. }
                            | AuthState::Failed(_)
                    )
                ));
        if self.should_auto_start_sign_in(auth_cache_path(&self.layout).is_some()) {
            self.start_cached_sign_in();
        }
        if self.cached_sign_in_checking() {
            self.intents.join_sign_in_deadline =
                Some(std::time::Instant::now() + JOIN_SIGN_IN_WAIT);
            self.intents.join_sign_in_check = true;
        } else {
            self.stop_sign_in();
        }
    }

    /// Gives a held join the account its check settled on, once the check settles and the account
    /// core serves that account, or the wait expires.
    pub(super) fn settle_join_sign_in(&mut self, now: std::time::Instant) -> JoinSignIn {
        let Some(deadline) = self.intents.join_sign_in_deadline else {
            return JoinSignIn::Settled;
        };
        let checking = self.cached_sign_in_checking();
        if checking && now < deadline {
            return JoinSignIn::Waiting;
        }
        let auth_cache = self.launcher_auth_cache();
        if !checking && auth_cache.is_none() {
            // A check that failed or fell back to a prompt is abandoned, as for any join.
            self.stop_sign_in();
        }
        let native_direct = self.native_sign_in
            && self.intents.join.as_ref().is_some_and(|join| {
                !join.local_world
                    && matches!(
                        target_for(&join.address),
                        bridge::ConnectTarget::RakNet(_) | bridge::ConnectTarget::Realm(_)
                    )
            });
        let core_ready = native_direct
            || self
                .account_core
                .is_none_or(|core| core.ready_for(auth_cache.is_some()));
        if !checking && !core_ready && now < deadline {
            return JoinSignIn::Waiting;
        }
        self.intents.join_sign_in_deadline = None;
        if let Some(join) = self.intents.join.as_mut() {
            join.auth_cache = auth_cache;
        }
        if checking {
            if !self.intents.native_auth_required {
                bevy::log::warn!("sign-in check still running; joining signed out");
            }
            JoinSignIn::Expired
        } else {
            JoinSignIn::Settled
        }
    }

    /// The queued join, once the saved account's check has settled (bounded) and any
    /// sign-in helper the join does not outlive has finished cleaning up.
    pub fn take_join_intent(&mut self) -> Option<JoinIntent> {
        if self.external.is_some() {
            return self.intents.join.take();
        }
        if self.sign_out_requested {
            self.intents.join = None;
            return None;
        }
        if self.account_operation_pending() {
            return None;
        }
        let helper_outlives_join = match self.settle_join_sign_in(std::time::Instant::now()) {
            JoinSignIn::Waiting => return None,
            JoinSignIn::Settled => false,
            JoinSignIn::Expired => true,
        };
        if self.intents.native_auth_required
            && self
                .intents
                .join
                .as_ref()
                .is_some_and(|join| !join.local_world)
            && self.launcher_auth_cache().is_none()
        {
            self.stop_sign_in();
            self.cancel_join();
            self.show_join_failure(
                "Could not verify the selected account. Sign in again before joining.".into(),
            );
            return None;
        }
        if !helper_outlives_join
            && self
                .auth_process
                .as_ref()
                .is_some_and(|process| !process.cleanup_complete())
        {
            return None;
        }
        let join = self.intents.join.take();
        if join.is_some() {
            self.intents.native_auth_required = false;
        }
        join
    }

    /// Cancels a fallback prompt from a check serving a join before any browser opens for it.
    fn cancel_join_fallback_prompt(&mut self) {
        if !self.intents.join_sign_in_check {
            return;
        }
        match self.auth_process.as_ref().map(AuthSupervisor::state) {
            Some(AuthState::Checking) => return,
            Some(state) if state.awaiting_browser() => self.stop_sign_in(),
            _ => {}
        }
        self.intents.join_sign_in_check = false;
    }

    /// A join is under way beyond waiting for the saved account, so the account core stays put.
    pub(super) fn joining_past_sign_in(&self) -> bool {
        self.is_connecting() && self.intents.join_sign_in_deadline.is_none()
    }

    /// Whether joins currently use the saved account, are waiting on its check, or go signed out.
    pub fn sign_in_status(&self) -> SignInStatus {
        if self.external.is_some() {
            return match self.current_auth().as_ref() {
                AuthState::Authenticated => SignInStatus::SignedIn,
                AuthState::Checking
                | AuthState::AwaitingCode { .. }
                | AuthState::AwaitingXboxSignup { .. } => SignInStatus::Checking,
                _ => SignInStatus::SignedOut,
            };
        }
        if self.launcher_auth_cache().is_some() {
            SignInStatus::SignedIn
        } else if self.auth_restart_requested
            || self.sign_in_in_flight()
            || self.should_auto_start_sign_in(auth_cache_path(&self.layout).is_some())
        {
            SignInStatus::Checking
        } else {
            SignInStatus::SignedOut
        }
    }

    pub(super) fn start_sign_in(&mut self) {
        if self.external.is_some() {
            self.external_action(launcher::menu::MenuAction::StartSignIn);
            return;
        }
        self.sign_in_failure = None;
        self.sign_in_requested = true;
        self.sign_in_cancelled = false;
        self.focus_sign_in_prompt();
        self.auth_attempted = true;
        self.stop_catalog();
        self.catalog_started = false;
        self.catalog_message = None;
        if let Some(process) = self.auth_process.as_mut()
            && !process.cleanup_complete()
        {
            process.request_cancel();
            self.auth_restart_requested = true;
            return;
        }
        self.auth_process = None;
        self.auth_restart_requested = false;
        self.control_auth = None;
        self.spawn_sign_in();
    }

    fn spawn_sign_in(&mut self) {
        #[cfg(feature = "native-networking")]
        if self.native_sign_in {
            let result = self
                .io_execution()
                .map_err(Into::into)
                .and_then(|(runtime, tasks)| {
                    AuthSupervisor::spawn_native(
                        self.account_store(),
                        self.layout.device_profile_file(),
                        self.feeds.account_active_id.clone(),
                        self.feeds.account_adding,
                        runtime,
                        tasks,
                    )
                });
            match result {
                Ok(process) => self.auth_process = Some(process),
                Err(_) => {
                    self.auth_process = None;
                    self.sign_in_failure = Some(AuthState::Failed(
                        "Sign-in could not start. Try again.".into(),
                    ));
                }
            }
            return;
        }
        let Some(executable) = core_executable(&self.layout) else {
            self.auth_process = None;
            let failure = "Sign-in is unavailable. Try again.";
            self.sign_in_failure = Some(AuthState::Failed(failure.into()));
            self.message = Some(failure.into());
            return;
        };
        let cache = if self.feeds.account_adding {
            launcher::accounts::AccountStore::new(self.layout.auth_cache()).pending_cache()
        } else {
            self.layout.auth_cache()
        };
        match AuthSupervisor::spawn(&executable, &cache) {
            Ok(process) => self.auth_process = Some(process),
            Err(error) => {
                self.auth_process = None;
                bevy::log::warn!(%error, "sign-in could not start");
                let failure = "Sign-in could not start. Try again.";
                self.sign_in_failure = Some(AuthState::Failed(failure.into()));
                self.message = Some(failure.into());
            }
        }
    }

    /// Uses the same auth precedence as presentation, including launcher-core codes.
    pub(super) fn update_sign_in_browser(&mut self, explicit: bool) {
        if self.external.is_some() {
            return;
        }
        #[cfg(feature = "developer-control")]
        if self.fixture_active() {
            return;
        }
        self.sign_in_browser.poll();
        if self.auth_restart_requested
            || self.sign_in_cancelled
            || self.owned_sign_in_failure().is_some()
        {
            return;
        }
        let state = select_auth(
            self.auth_process.as_ref().map(AuthSupervisor::state),
            self.control_auth.as_ref(),
        );
        if state.awaiting_browser() {
            #[cfg(not(test))]
            self.sign_in_browser
                .open(state, explicit, launcher_host::desktop::open_sign_in_link);
            #[cfg(test)]
            self.sign_in_browser.open(state, explicit, |_| true);
        }
    }

    /// The active auth prompt, with a pending helper ahead of the launcher core.
    pub(super) fn current_auth(&self) -> std::borrow::Cow<'_, AuthState> {
        #[cfg(feature = "developer-control")]
        if let Some(state) = self.sign_in_fixture {
            return std::borrow::Cow::Owned(if self.dialog == Some(MenuDialog::Accounts) {
                super::sign_in_fixture::auth_state(state)
            } else {
                AuthState::SignedOut
            });
        }
        if self.auth_restart_requested {
            return std::borrow::Cow::Borrowed(&AuthState::Checking);
        }
        if let Some(state) = self.owned_sign_in_failure() {
            return std::borrow::Cow::Borrowed(state);
        }
        let state = select_auth(
            self.auth_process.as_ref().map(AuthSupervisor::state),
            self.control_auth.as_ref(),
        );
        std::borrow::Cow::Borrowed(
            if self.sign_in_cancelled && !matches!(state, AuthState::Authenticated) {
                &AuthState::SignedOut
            } else {
                state
            },
        )
    }

    /// Interactive failures own presentation; background cache checks leave core status visible.
    fn owned_sign_in_failure(&self) -> Option<&AuthState> {
        self.sign_in_failure
            .as_ref()
            .filter(|_| self.sign_in_requested || self.feeds.account_adding)
    }

    /// New core prompts start at the primary action; repeated status keeps the selection.
    pub(super) fn apply_control_auth(&mut self, state: AuthState) {
        if self.sign_in_cancelled || self.owned_sign_in_failure().is_some() {
            self.control_auth = Some(state);
            return;
        }
        let next = select_auth(
            self.auth_process.as_ref().map(AuthSupervisor::state),
            Some(&state),
        );
        let ready = next.awaiting_browser();
        let finished = !self.auth_restart_requested
            && matches!(next, AuthState::Authenticated | AuthState::SignedOut);
        let prompt = ready
            || ((self.sign_in_requested || self.feeds.account_adding)
                && matches!(next, AuthState::Checking | AuthState::Failed(_)))
            || (self.feeds.account_adding && matches!(next, AuthState::Authenticated));
        let reset = !self.auth_restart_requested && self.current_auth().as_ref() != next && prompt;
        self.control_auth = Some(state);
        if ready {
            self.sign_in_requested = true;
        } else if finished {
            self.sign_in_requested = false;
        }
        if reset {
            self.focus_sign_in_prompt();
        }
    }

    /// A new prompt releases input held by the route underneath it.
    pub(super) fn focus_sign_in_prompt(&mut self) {
        if !self.sign_in_prompt_layer_available() {
            return;
        }
        self.focused = 0;
        self.field = None;
        self.hovered = None;
        self.pressed = None;
        self.pointer_down = false;
        self.settings.key_remap = None;
        self.clear_settings_slider_selection();
    }

    /// Progress and existing dialogs keep their input until the sign-in prompt is visible.
    pub(super) fn sign_in_prompt_layer_available(&self) -> bool {
        !self.is_connecting()
            && matches!(self.dialog, None | Some(MenuDialog::Accounts))
            && (self.dialog.is_some()
                || (self.disconnect_message.is_none() && !self.local_ui.progress_open()))
    }

    /// Focus follows the sign-in prompt before any controls underneath it.
    pub(super) fn sign_in_focus(&self) -> Option<Vec<MenuAction>> {
        if !self.sign_in_prompt_layer_available() {
            return None;
        }
        let auth = self.current_auth();
        if !self.sign_in_requested
            && !self.feeds.account_adding
            && !auth.as_ref().awaiting_browser()
        {
            return None;
        }
        Some(match auth.as_ref() {
            AuthState::Checking => vec![MenuAction::CancelSignIn],
            AuthState::Authenticated if self.feeds.account_adding => vec![MenuAction::CancelSignIn],
            AuthState::AwaitingCode { .. } | AuthState::AwaitingXboxSignup { .. } => {
                vec![MenuAction::OpenSignInLink, MenuAction::CancelSignIn]
            }
            AuthState::Failed(_) => vec![MenuAction::StartSignIn, MenuAction::CancelSignIn],
            _ => return None,
        })
    }

    fn poll_sign_in(&mut self) {
        let Some(process) = self.auth_process.as_mut() else {
            return;
        };
        let was_authenticated = matches!(process.state(), AuthState::Authenticated);
        let before = std::mem::discriminant(process.state());
        process.poll();
        if self.sign_in_cancelled && process.state().awaiting_browser() {
            process.cancel_prompt();
        }
        if !self.auth_restart_requested {
            match process.state() {
                AuthState::AwaitingCode { .. } | AuthState::AwaitingXboxSignup { .. } => {
                    self.sign_in_requested = true
                }
                AuthState::Authenticated | AuthState::SignedOut => self.sign_in_requested = false,
                _ => {}
            }
        }
        let reset_focus = !self.auth_restart_requested
            && (self.sign_in_requested || self.feeds.account_adding)
            && before != std::mem::discriminant(process.state())
            && (process.state().awaiting_browser()
                || matches!(process.state(), AuthState::Checking | AuthState::Failed(_))
                || (self.feeds.account_adding
                    && matches!(process.state(), AuthState::Authenticated)));
        if process.cleanup_complete() && self.auth_restart_requested {
            self.auth_process = None;
            self.auth_restart_requested = false;
            self.spawn_sign_in();
            return;
        }
        if !was_authenticated && matches!(process.state(), AuthState::Authenticated) {
            self.catalog_started = false;
            self.catalog_message = Some("Signed in. Loading account destinations…".to_owned());
        }
        #[cfg(feature = "native-networking")]
        if let Some((profile, pending)) = process.take_native_profile() {
            self.feeds.profile = MenuProfile {
                unavailable: false,
                xuid: profile.id.clone(),
                gamertag: profile.gamertag.clone(),
                picture_path: profile.picture_path.clone().unwrap_or_default(),
                ..MenuProfile::unavailable()
            };
            if let Some(pending) = pending {
                self.accounts.pending_ready = true;
                self.accounts.operation = Some(launcher_host::accounts::Operation::CommitNative(
                    profile, pending,
                ));
            } else {
                self.feeds.account_active_id = Some(profile.id);
            }
        }
        if reset_focus {
            self.focus_sign_in_prompt();
        }
    }

    pub(super) fn stop_sign_in(&mut self) {
        if self.external.is_some() {
            self.external_action(launcher::menu::MenuAction::CancelSignIn);
            return;
        }
        self.sign_in_failure = None;
        self.sign_in_requested = false;
        self.sign_in_cancelled = true;
        // Cancellation is sticky. Only the explicit StartSignIn action may
        // create another helper after this point.
        if matches!(
            self.control_auth,
            Some(
                AuthState::Checking
                    | AuthState::AwaitingCode { .. }
                    | AuthState::AwaitingXboxSignup { .. }
                    | AuthState::Failed(_)
            )
        ) {
            self.control_auth = None;
        }
        self.auth_attempted = true;
        self.auth_restart_requested = false;
        if let Some(mut process) = self.auth_process.take() {
            process.cancel_prompt();
            self.auth_process = Some(process);
        }
    }

    fn should_auto_start_sign_in(&self, cache_exists: bool) -> bool {
        cache_exists && !self.auth_attempted && self.auth_process.is_none()
    }
}

#[cfg(all(test, unix))]
mod profile_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod auth_tests;

#[cfg(all(test, feature = "native-networking"))]
mod native_tests;
