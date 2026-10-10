//! Bevy attachment and menu policy for the launcher account core.

use super::{MenuRuntime, account, launcher_account::LauncherAccount};
use crate::local_worlds::LocalWorlds;
use bevy::prelude::{Commands, Resource};
use bridge::bridge_endpoint_exists;
use launcher::menu::auth::AuthState;
use launcher_host::{
    auth::AuthSupervisor, core_process::CoreProcessGuard, launcher_core::LauncherCore,
    session_cleanup::SessionDirectoryGuard,
};
use std::path::PathBuf;
#[cfg(test)]
use std::{
    path::Path,
    process::{Command, Stdio},
    time::Duration,
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

/// Holds the account core in every startup mode, including direct connections.
#[derive(Default, Resource)]
pub(crate) struct LauncherCoreSlot {
    core: Option<LauncherCore>,
    /// Sign-in mode whose spawn failed; retried only once the mode changes.
    failed: Option<bool>,
    retiring: Option<crossbeam_channel::Receiver<()>>,
}

impl LauncherCoreSlot {
    /// Keep the core matching the validated sign-in while idle, and attach the
    /// account and local-world clients once it is serving.
    pub(super) fn drive(
        &mut self,
        commands: &mut Commands,
        menu: &mut MenuRuntime,
        idle: bool,
        upstream_client_cache: bool,
        mut worlds: Option<&mut LocalWorlds>,
    ) {
        if let Some(retiring) = &self.retiring {
            if matches!(
                retiring.try_recv(),
                Err(crossbeam_channel::TryRecvError::Empty)
            ) {
                return;
            }
            self.retiring = None;
        }
        if self.core.as_mut().is_some_and(|core| core._guard.exited()) {
            bevy::log::warn!("launcher core exited; reconnecting its control clients when idle");
            self.retire(commands, menu, worlds.as_deref_mut());
            self.failed = None;
        }
        if self.core.is_none() && std::mem::take(&mut menu.feeds.profile_refresh_requested) {
            self.failed = None;
        }
        if idle
            && menu.accounts.operation.is_some()
            && menu.accounts.remember.is_none()
            && menu
                .auth_process
                .as_ref()
                .is_none_or(|p| p.cleanup_complete())
        {
            let job = menu.account_operation_job();
            if let Some(mut old) = self.core.take() {
                commands.remove_resource::<LauncherAccount>();
                menu.close_realm_membership();
                if let Some(worlds) = worlds.as_deref_mut() {
                    worlds.detach();
                }
                menu.control_auth = None;
                menu.accounts.skip_control = true;
                let mut guard = std::mem::take(&mut old._guard);
                guard.stop_detached(move || {
                    drop(old);
                    job();
                });
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
        if idle && !menu.sign_in_in_flight() {
            let auth_cache = menu.launcher_auth_cache();
            let wanted = auth_cache.is_some();
            let current = self.core.as_ref().map(|core| core.authenticated);
            let path_changed = self
                .core
                .as_ref()
                .is_some_and(|core| core.auth_cache != auth_cache);
            if path_changed && self.core.is_some() && menu.accounts.pending_ready {
                let mut old = self.core.take().expect("account core");
                commands.remove_resource::<LauncherAccount>();
                menu.close_realm_membership();
                if let Some(worlds) = worlds.as_deref_mut() {
                    worlds.detach();
                }
                menu.control_auth = None;
                menu.accounts.skip_control = true;
                menu.feeds.profile = Default::default();
                let (done, retired) = crossbeam_channel::bounded(1);
                self.retiring = Some(retired);
                let mut guard = std::mem::take(&mut old._guard);
                guard.stop_detached(move || {
                    drop(old);
                    let _ = done.send(());
                });
                return;
            }
            if (current != Some(wanted) || path_changed) && self.failed != Some(wanted) {
                self.retire(commands, menu, worlds.as_deref_mut());
                match LauncherCore::spawn(
                    &menu.layout,
                    auth_cache.as_deref(),
                    upstream_client_cache,
                    menu.is_launcher(),
                    &crate::runtime::network::active_language_code(),
                ) {
                    Ok(core) => {
                        self.core = Some(core);
                        self.failed = None;
                    }
                    Err(error) => {
                        bevy::log::warn!("launcher core unavailable: {error:#}");
                        self.failed = Some(wanted);
                    }
                }
            }
        }
        if let Some(core) = self.core.as_mut()
            && !core.attached
            && bridge_endpoint_exists(&core.socket_dir)
        {
            core.attached = true;
            commands.insert_resource(LauncherAccount(
                launcher_host::launcher_account::LauncherAccount::new(
                    core.socket_dir.clone(),
                    menu.layout.launcher_artwork_dir(),
                ),
            ));
            if let Some(worlds) = worlds
                && let Err(error) = worlds.attach(core.socket_dir.clone())
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
        if let Some(old) = self.core.take() {
            drop(old);
            commands.remove_resource::<LauncherAccount>();
            menu.close_realm_membership();
            if let Some(worlds) = worlds {
                worlds.detach();
            }
            menu.control_auth = None;
            menu.accounts.skip_control = true;
            menu.feeds.profile = Default::default();
        }
    }

    /// Selects a join's target on the launcher core off the frame; the receiver
    /// yields the game socket to dial. Local worlds always use their owning core;
    /// direct remote joins stay on a separate per-session core.
    pub(crate) fn begin_join(
        &self,
        address: &str,
        local_world: bool,
        authenticated: bool,
        launcher_mode: bool,
    ) -> Option<crossbeam_channel::Receiver<Result<PathBuf, String>>> {
        if !launcher_mode && !local_world {
            return None;
        }
        let core = self.core.as_ref()?;
        if !local_world && core.authenticated != authenticated {
            return None;
        }
        core.begin_join(address, local_world)
    }
}

impl MenuRuntime {
    /// The validated sign-in's auth cache, for the launcher core and joins.
    pub(crate) fn launcher_auth_cache(&self) -> Option<PathBuf> {
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

    fn sign_in_in_flight(&self) -> bool {
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
        let slot = LauncherCoreSlot {
            core: Some(LauncherCore {
                _guard: CoreProcessGuard::default(),
                _directory: directory,
                socket_dir: socket_dir.clone(),
                authenticated: false,
                auth_cache: None,
                attached: true,
            }),
            failed: None,
            retiring: None,
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

    // The launcher core, whose events the menu polls, is the one that asks about server trust.

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

    // A missing launcher socket is an error, not a panic on a timer built outside the runtime.
    #[test]
    fn select_without_a_launcher_core_errors_instead_of_panicking() {
        let missing = std::env::temp_dir().join("cinnabar-no-launcher-core-here");
        assert!(
            select(
                &missing,
                ConnectTarget::RakNet("example.invalid:19132".into())
            )
            .is_err()
        );
    }

    // Adding an account must not make the install report a second device.
}
