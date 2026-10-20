//! One selected launcher service lifetime and its optional legacy Core process.

use super::{MenuRuntime, account, launcher_account::LauncherAccount};
use crate::local_worlds::LocalWorlds;
use bevy::prelude::{Commands, Resource};
use bridge::bridge_endpoint_exists;
use launcher::menu::auth::AuthState;
use launcher_host::{
    auth::AuthSupervisor,
    core_process::CoreStopOutcome,
    launcher_core::{AccountCorePolicy, LauncherCore, TitlePresenceOwner},
    lifecycle::core_health::RestartBackoff,
};
#[cfg(test)]
use launcher_host::{core_process::CoreProcessGuard, session_cleanup::SessionDirectoryGuard};
#[cfg(test)]
use std::process::{Command, Stdio};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
/// Allows auth updates beside a separate direct game core, but protects games this core owns.
pub(super) fn account_core_idle(
    launcher: bool,
    connecting: bool,
    in_session: bool,
    local_world: bool,
) -> bool {
    !connecting && (!in_session || !(launcher || local_world))
}

/// The account core's readiness, for a held join that should go through it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct AccountCore {
    pub(super) serving: Option<bool>, // Sign-in of the attached core.
    pub(super) failed: Option<bool>,  // Sign-in whose core could not start.
}

impl AccountCore {
    /// Whether a join with this sign-in has nothing left to wait for from the core.
    pub(super) fn ready_for(self, authenticated: bool) -> bool {
        self.serving == Some(authenticated) || self.failed == Some(authenticated)
    }
}

/// Owns one selected account-service lifetime, with a real child only for the legacy backend.
#[derive(Default, Resource)]
pub struct LauncherServices {
    running: Option<Running>,
    /// Sign-in mode whose spawn failed, or whose core kept exiting; retried only once the mode
    /// changes or a profile refresh asks for it.
    failed: Option<bool>,
    retiring: Option<crossbeam_channel::Receiver<CoreStopOutcome>>,
    retirement_blocked: bool,
    last_retirement: Option<CoreStopOutcome>,
    title_presence_owner: TitlePresenceOwner,
    presence_lifetime: Option<launcher_host::launcher_account::TitlePresenceLifetime>,
    /// Paces restarts of a core that exits on its own.
    backoff: RestartBackoff,
    /// When the current core was spawned, by the `now` that spawned it.
    started: Option<Instant>,
    /// The earliest restart after such an exit.
    restart_at: Option<Instant>,
}

enum Running {
    Core(LauncherCore),
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    Native {
        cache: Option<PathBuf>,
        identity: Option<String>,
        account: Option<std::sync::Arc<launcher_host::native_account::NativeAccount>>,
    },
}

impl LauncherServices {
    /// Borrows only a real legacy child; native attachment has no process readiness.
    fn core(&self) -> Option<&LauncherCore> {
        match self.running.as_ref()? {
            Running::Core(core) => Some(core),
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            Running::Native { .. } => None,
        }
    }

    /// Mutable access remains exclusive to the original legacy child owner.
    fn core_mut(&mut self) -> Option<&mut LauncherCore> {
        match self.running.as_mut()? {
            Running::Core(core) => Some(core),
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            Running::Native { .. } => None,
        }
    }

    /// Takes a known real Core owner before starting its original retirement.
    fn take_core(&mut self) -> LauncherCore {
        let Some(Running::Core(core)) = self.running.take() else {
            unreachable!("the selected legacy Core exists");
        };
        core
    }

    /// Keep the core matching the validated sign-in while idle, and attach the
    /// account and local-world clients once it is serving. A core that exited is restarted
    /// after its [`RestartBackoff`] delay, measured from `now`, and not again once the backoff
    /// gives up.
    pub(super) fn drive(
        &mut self,
        commands: &mut Commands,
        menu: &mut MenuRuntime,
        idle: bool,
        upstream_client_cache: bool,
        mut worlds: Option<&mut LocalWorlds>,
        now: Instant,
    ) {
        if !self.retirement_complete(commands, menu) {
            return;
        }
        if let Some((status, authenticated)) = self
            .core_mut()
            .and_then(|core| Some((core._guard.exit_status()?, core.authenticated)))
        {
            let ran_for = self.started.map_or(Duration::ZERO, |started| {
                now.saturating_duration_since(started)
            });
            match self.backoff.next_delay(ran_for) {
                Some(delay) => {
                    bevy::log::warn!(
                        "launcher core exited ({status}); restarting it in {delay:?} when idle"
                    );
                    self.restart_at = Some(now + delay);
                    self.failed = None;
                }
                None => {
                    bevy::log::warn!(
                        "launcher core exited ({status}) again soon after starting; not restarting it until sign-in changes or a refresh"
                    );
                    self.restart_at = None;
                    self.failed = Some(authenticated);
                }
            }
            self.retire(commands, menu, worlds.as_deref_mut());
            return;
        }
        if self.running.is_none() && std::mem::take(&mut menu.feeds.profile_refresh_requested) {
            self.failed = None;
            self.restart_at = None;
        }
        if idle
            && menu.accounts.operation.is_some()
            && menu.accounts.remember.is_none()
            && menu
                .auth_process
                .as_ref()
                .is_none_or(|p| p.cleanup_complete())
        {
            if !menu.account_session_retired() {
                return;
            }
            if !self.retire_presence(commands, menu) {
                return;
            }
            let job = menu.account_operation_job();
            if let Some(old) = self.running.take() {
                commands.remove_resource::<LauncherAccount>();
                menu.close_realm_membership();
                if let Some(worlds) = worlds.as_deref_mut() {
                    worlds.detach();
                }
                menu.control_auth = None;
                menu.accounts.skip_control = true;
                match old {
                    Running::Core(old) => self.begin_retirement(old, job),
                    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
                    Running::Native { .. } => {
                        let _ = std::thread::Builder::new()
                            .name("account-switch".into())
                            .spawn(job);
                    }
                }
            } else {
                let _ = std::thread::Builder::new()
                    .name("account-switch".into())
                    .spawn(job);
            }
            self.failed = None;
            return;
        }
        if menu.accounts.work.is_some() {
            return;
        }
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        if idle && !menu.sign_in_in_flight() {
            if menu.native_sign_in {
                self.drive_native(commands, menu, worlds.as_deref_mut());
                return;
            }
            if matches!(self.running, Some(Running::Native { .. })) {
                if !self.retire_presence(commands, menu) {
                    return;
                }
                self.detach_clients(commands, menu, worlds.as_deref_mut());
                self.running = None;
                self.failed = None;
                self.restart_at = None;
                return;
            }
        }
        if idle && !menu.sign_in_in_flight() {
            let auth_cache = menu.launcher_auth_cache();
            let wanted = auth_cache.is_some();
            let current = self.core().map(|core| core.authenticated);
            let path_changed = self
                .core()
                .is_some_and(|core| core.auth_cache != auth_cache);
            if path_changed && self.core().is_some() && menu.accounts.pending_ready {
                if !self.retire_presence(commands, menu) {
                    return;
                }
                let old = self.take_core();
                commands.remove_resource::<LauncherAccount>();
                menu.close_realm_membership();
                if let Some(worlds) = worlds.as_deref_mut() {
                    worlds.detach();
                }
                menu.control_auth = None;
                menu.accounts.skip_control = true;
                menu.feeds.profile = Default::default();
                self.begin_retirement(old, || {});
                return;
            }
            let restart_due = self.restart_at.is_none_or(|at| now >= at);
            let presence_changed = self
                .core()
                .is_some_and(|core| core.title_presence_owner != self.title_presence_owner);
            if (current != Some(wanted) || path_changed || presence_changed)
                && self.failed != Some(wanted)
                && restart_due
            {
                if self.core().is_some() {
                    if !self.retire_presence(commands, menu) {
                        return;
                    }
                    let old = self.take_core();
                    self.detach_clients(commands, menu, worlds.as_deref_mut());
                    self.begin_retirement(old, || {});
                    return;
                }
                match LauncherCore::spawn(
                    &menu.layout,
                    auth_cache.as_deref(),
                    &pack_runtime::pack_language::active_language_code(),
                    AccountCorePolicy {
                        upstream_client_cache,
                        lease_game_cache: menu.is_launcher(),
                        title_presence_owner: self.title_presence_owner,
                    },
                ) {
                    Ok(core) => {
                        self.running = Some(Running::Core(core));
                        self.started = Some(now);
                        self.restart_at = None;
                        self.failed = None;
                    }
                    Err(error) => {
                        bevy::log::warn!("launcher core unavailable: {error:#}");
                        self.failed = Some(wanted);
                    }
                }
            }
        }
        let directory = self
            .core()
            .filter(|core| !core.attached && bridge_endpoint_exists(&core.socket_dir))
            .map(|core| core.socket_dir.clone());
        if let Some(directory) = directory {
            let (runtime, tasks) = match menu.io_execution() {
                Ok(execution) => execution,
                Err(error) => {
                    menu.catalog_message = Some(error.to_string());
                    return;
                }
            };
            self.core_mut().expect("original Core attachment").attached = true;
            let account = launcher_host::launcher_account::LauncherAccount::new(
                directory.clone(),
                menu.layout.launcher_artwork_dir(),
                runtime,
                tasks,
            );
            self.presence_lifetime = Some(account.title_presence_lifetime());
            commands.insert_resource(LauncherAccount(account));
            if let Some(worlds) = worlds
                && let Err(error) = worlds.attach(directory, &menu.layout)
            {
                bevy::log::warn!("local worlds unavailable: {error}");
            }
        }
    }

    fn retire(
        &mut self,
        commands: &mut Commands,
        menu: &mut MenuRuntime,
        worlds: Option<&mut LocalWorlds>,
    ) {
        if let Some(lifetime) = &self.presence_lifetime {
            lifetime.retire();
        }
        if let Some(old) = self.running.take() {
            debug_assert!(matches!(old, Running::Core(_)));
            drop(old);
            self.last_retirement = Some(CoreStopOutcome::ExitedAfterGracefulClose);
            self.detach_clients(commands, menu, worlds);
        }
    }

    /// The account core as this frame left it.
    pub(super) fn account_core(&self) -> AccountCore {
        AccountCore {
            serving: self
                .core()
                .filter(|core| core.attached)
                .map(|core| core.authenticated),
            failed: self.failed,
        }
    }

    /// Selects a join's target on the launcher core off the frame; the receiver
    /// yields the session endpoint to dial. Local worlds always use their owning core;
    /// direct remote joins stay on a separate per-session core.
    pub fn begin_join(
        &self,
        address: &str,
        local_world: bool,
        authenticated: bool,
        launcher_mode: bool,
    ) -> Option<crossbeam_channel::Receiver<Result<PathBuf, String>>> {
        if !launcher_mode && !local_world {
            return None;
        }
        let core = self.core()?;
        if !local_world && core.authenticated != authenticated {
            return None;
        }
        core.begin_join(address, local_world)
    }
}

#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
mod native;
mod presence;

impl MenuRuntime {
    /// The validated sign-in's auth cache, for the launcher core and joins.
    pub fn launcher_auth_cache(&self) -> Option<PathBuf> {
        if self.external.is_some() {
            return None;
        }
        if self.feeds.account_adding && self.accounts.pending_ready {
            return Some(
                launcher::accounts::AccountStore::new(self.layout.auth_cache()).pending_cache(),
            );
        }
        account::validated_auth_cache(
            &self.layout,
            self.auth_process.as_ref().map(AuthSupervisor::state),
        )
    }

    pub(super) fn sign_in_in_flight(&self) -> bool {
        matches!(
            self.auth_process.as_ref().map(AuthSupervisor::state),
            Some(
                AuthState::Checking
                    | AuthState::AwaitingCode { .. }
                    | AuthState::AwaitingXboxSignup { .. }
            )
        )
    }
}

#[cfg(test)]
mod dead_child_tests;

#[cfg(all(test, feature = "native-networking", not(target_arch = "wasm32")))]
mod retirement_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_startup_local_world_uses_its_owning_core_after_save_and_quit() {
        let layout = launcher::test_support::scratch("direct-local-world-routing");
        let socket_dir = layout.account_socket_dir(std::process::id(), 0);
        let directory = SessionDirectoryGuard::bind(socket_dir.clone()).unwrap();
        // Readiness only: no game connection or server process is needed to select the route.
        std::fs::write(bridge::session_endpoint_path(&socket_dir), []).unwrap();
        let slot = LauncherServices {
            running: Some(Running::Core(LauncherCore {
                _guard: CoreProcessGuard::default(),
                _directory: directory,
                socket_dir: socket_dir.clone(),
                authenticated: false,
                auth_cache: None,
                title_presence_owner: TitlePresenceOwner::Core,
                attached: true,
            })),
            failed: None,
            ..Default::default()
        };
        let mut menu = MenuRuntime::new_with_layout(
            false,
            Some(2),
            "Fixture".into(),
            layout,
            crate::player_skin::LocalPlayerSkin::generated_default("Fixture"),
        );
        menu.show_home();
        assert!(!menu.is_launcher());
        assert_eq!(menu.screen(), launcher::menu::MenuScreen::Home);
        assert!(
            slot.begin_join("example.invalid", false, false, menu.is_launcher())
                .is_none(),
            "direct remote joins must keep their separate game core"
        );
        let selected = slot
            .begin_join("Local fixture", true, false, menu.is_launcher())
            .expect("Save & Quit must retain the owning local-world core");
        assert_eq!(
            selected
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .unwrap(),
            socket_dir
        );
    }

    #[test]
    fn account_core_idle_keeps_direct_play_independent_from_account_startup() {
        for launcher in [false, true] {
            assert!(account_core_idle(launcher, false, false, false));
            for in_session in [false, true] {
                assert!(!account_core_idle(launcher, true, in_session, false));
            }
        }
        assert!(account_core_idle(false, false, true, false));
        assert!(!account_core_idle(true, false, true, false));
    }

    #[test]
    fn direct_local_world_prevents_an_account_restart_during_play() {
        assert!(
            !account_core_idle(false, false, true, true),
            "the account core owns this local game and cannot restart for auth changes"
        );
        assert!(
            account_core_idle(false, false, false, true),
            "after quitting the local game, account updates may resume"
        );
    }
}
