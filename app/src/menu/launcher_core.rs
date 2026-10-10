//! The launcher's long-lived core: one `-control-status` core serves account,
//! catalog, connect and local-world control for the whole launcher run. Joins
//! pick their target over `connect.v1` and dial this core's session socket, so it
//! restarts when the validated sign-in changes or its child exits unexpectedly.

use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use anyhow::{Context, Result, anyhow};
use bevy::prelude::{Commands, Resource};
use bridge::{self, ConnectTarget};

use {
    super::{
        AuthSupervisor, CoreProcessGuard, MenuRuntime, account,
        core_process::{clear_stale_bridge_endpoint, core_executable},
        launcher_account::LauncherAccount,
        wait_for_core,
    },
    launcher::menu::auth::AuthState,
};
use {
    crate::{
        local_worlds::LocalWorlds, runtime::endpoint::bridge_endpoint_exists,
        session_cleanup::SessionDirectoryGuard,
    },
    launcher::install_layout::InstallLayout,
};

/// How long a join waits for the core to answer `connect.v1`.
const SELECT_TIMEOUT: Duration = Duration::from_secs(3);
const DEFAULT_PORT: u16 = 19132;
const LOCAL_SERVER: &str = if cfg!(windows) {
    "bedrock-local-server.exe"
} else {
    "bedrock-local-server"
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

struct LauncherCore {
    _guard: CoreProcessGuard,
    _directory: SessionDirectoryGuard,
    socket_dir: PathBuf,
    authenticated: bool,
    auth_cache: Option<PathBuf>,
    /// Account and local-world clients are attached once the game socket is up.
    attached: bool,
}

impl Drop for LauncherCore {
    fn drop(&mut self) {
        let stopped = self._guard.stop();
        #[cfg(unix)]
        if stopped != super::core_process::CoreStopOutcome::Unreaped {
            // Long Unix socket paths live outside the owned session directory.
            for endpoint in protocol::core_endpoint_paths(&self.socket_dir) {
                if let Err(error) = std::fs::remove_file(&endpoint)
                    && error.kind() != std::io::ErrorKind::NotFound
                {
                    bevy::log::warn!("remove core endpoint {}: {error}", endpoint.display());
                }
            }
        }
        #[cfg(not(unix))]
        let _ = stopped;
    }
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
            commands.insert_resource(LauncherAccount::new(
                core.socket_dir.clone(),
                menu.layout.launcher_artwork_dir(),
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
        let (sender, receiver) = crossbeam_channel::bounded(1);
        let (socket_dir, target) = (core.socket_dir.clone(), target_for(address));
        std::thread::Builder::new()
            .name("launcher-join".to_owned())
            .spawn(move || {
                let _ = sender.send(prepare(socket_dir, target, local_world));
            })
            .ok()?;
        Some(receiver)
    }
}

impl LauncherCore {
    fn spawn(
        layout: &InstallLayout,
        auth_cache: Option<&Path>,
        upstream_client_cache: bool,
        lease_game_cache: bool,
    ) -> Result<Self> {
        let executable =
            core_executable(layout).ok_or_else(|| anyhow!("bedrock-core executable not found"))?;
        let socket_dir = next_account_socket_dir(layout);
        let directory =
            SessionDirectoryGuard::bind(socket_dir.clone()).map_err(|error| anyhow!("{error}"))?;
        clear_stale_bridge_endpoint(&socket_dir)?;
        let child = crate::lifecycle::children::spawn(&mut launcher_command(
            layout,
            &executable,
            &socket_dir,
            auth_cache,
            upstream_client_cache,
            Some(&crate::runtime::network::active_language_code()),
            lease_game_cache,
        ))
        .with_context(|| format!("spawn {} for the launcher", executable.display()))?;
        let mut guard = CoreProcessGuard::default();
        guard.replace(child);
        Ok(Self {
            _guard: guard,
            _directory: directory,
            socket_dir,
            authenticated: auth_cache.is_some(),
            auth_cache: auth_cache.map(Path::to_path_buf),
            attached: false,
        })
    }
}

fn next_account_socket_dir(layout: &InstallLayout) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    layout.account_socket_dir(std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))
}

/// Waits for the core and selects `target`; blocks, so it runs on the join worker.
fn prepare(
    socket_dir: PathBuf,
    target: ConnectTarget,
    local_world: bool,
) -> Result<PathBuf, String> {
    wait_for_core(&socket_dir).map_err(|error| error.to_string())?;
    // An opened local world is already the core's route.
    if !local_world {
        select(&socket_dir, target)?;
    }
    Ok(socket_dir)
}

fn launcher_command(
    layout: &InstallLayout,
    executable: &Path,
    socket_dir: &Path,
    auth_cache: Option<&Path>,
    upstream_client_cache: bool,
    language: Option<&str>,
    lease_game_cache: bool,
) -> Command {
    let mut command = Command::new(executable);
    command
        .arg("-socket-dir")
        .arg(socket_dir)
        .arg("-control-status")
        .arg("-xbox-presence")
        .arg("-server-trust-file")
        .arg(layout.server_trust_file())
        .arg("-device-file")
        .arg(layout.device_profile_file())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(
            crate::lifecycle::core_health::open_core_log(layout)
                .map_or_else(Stdio::null, Stdio::from),
        );
    // Direct-mode account cores run alongside game cores that own this exclusive lease.
    if lease_game_cache {
        command
            .arg("-resource-pack-cache-dir")
            .arg(layout.resource_pack_cache_dir());
    }
    // The core refuses to start local worlds without their server binary.
    if executable.with_file_name(LOCAL_SERVER).is_file() {
        command.args(crate::local_worlds::core_args(layout));
    }
    if upstream_client_cache {
        command.arg("-upstream-client-cache");
    }
    if let Some(auth_cache) = auth_cache {
        command.arg("-auth-cache").arg(auth_cache);
    }
    if let Some(language) = language {
        command.arg("-language").arg(language.replace('_', "-"));
    }
    command
}

/// Sends `connect.v1`, waiting a bounded time for the answer.
fn select(socket_dir: &Path, target: ConnectTarget) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    // The timer must be created inside the runtime; building it outside panics.
    runtime
        .block_on(async {
            tokio::time::timeout(SELECT_TIMEOUT, bridge::connect_target(socket_dir, &target)).await
        })
        .map_err(|_| "the launcher core did not answer".to_owned())?
        .map_err(|error| error.to_string())
}

/// The kind of join `address` starts, for its progress titles.
pub(super) fn join_kind(address: &str, local_world: bool) -> launcher::menu::view::JoinKind {
    use launcher::menu::view::JoinKind;
    match target_for(address) {
        _ if local_world => JoinKind::Local,
        ConnectTarget::Realm(_) => JoinKind::Realm,
        // Friend worlds and experiences use the external-server title until vanilla's is confirmed.
        ConnectTarget::RakNet(_) | ConnectTarget::Friend(_) | ConnectTarget::Gathering(_) => {
            JoinKind::External
        }
    }
}

/// The `connect.v1` target for a menu address (the proxy's realm and friend
/// prefixes, else a server that gets the default port when it names none).
pub(crate) fn target_for(address: &str) -> ConnectTarget {
    let address = address.trim();
    if let Some(id) = address.strip_prefix(launcher::menu::EXPERIENCE_ADDRESS_PREFIX) {
        return ConnectTarget::Gathering(id.to_owned());
    }
    if let Some(id) = address.strip_prefix("realm_id/") {
        return ConnectTarget::Realm(id.to_owned());
    }
    if let Some(xuid) = address.strip_prefix(launcher::menu::FRIEND_ADDRESS_PREFIX) {
        return ConnectTarget::Friend(xuid.to_owned());
    }
    let has_port = address.rsplit_once(':').is_some_and(|(host, port)| {
        port.parse::<u16>().is_ok() && (host.ends_with(']') || !host.contains(':'))
    });
    ConnectTarget::RakNet(if has_port {
        address.to_owned()
    } else if address.contains(':') && !address.starts_with('[') {
        format!("[{address}]:{DEFAULT_PORT}")
    } else {
        format!("{address}:{DEFAULT_PORT}")
    })
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
        let layout = crate::install_layout::scratch("direct-local-world-routing");
        let socket_dir = layout.account_socket_dir(std::process::id(), 0);
        let directory = SessionDirectoryGuard::bind(socket_dir.clone()).unwrap();
        // Readiness only: no game connection or server process is needed to select the route.
        std::fs::write(
            crate::runtime::endpoint::bridge_endpoint_path(&socket_dir),
            [],
        )
        .unwrap();
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
    fn launcher_core_remembers_trusted_servers_in_user_data() {
        let layout = crate::install_layout::scratch("server-trust-file");
        let command = launcher_command(
            &layout,
            Path::new("/fixture/core"),
            Path::new("/fixture/socket"),
            None,
            false,
            None,
            false,
        );
        let args: Vec<_> = command.get_args().collect();
        assert!(args.windows(2).any(|pair| pair[0] == "-server-trust-file"
            && pair[1] == layout.server_trust_file().as_os_str()));
    }
    #[test]
    fn direct_account_core_leaves_pack_cache_for_game_cores() {
        let layout = crate::install_layout::scratch("direct-cache-ownership");
        for auth in [None, Some(Path::new("/fixture/auth.json"))] {
            let account = launcher_command(
                &layout,
                Path::new("/fixture/core"),
                Path::new("/fixture/socket"),
                auth,
                false,
                None,
                false,
            );
            let account_args: Vec<_> = account.get_args().collect();
            assert!(
                !account_args.contains(&std::ffi::OsStr::new("-resource-pack-cache-dir")),
                "account-only startup or auth restart must not lease the shared pack cache"
            );
        }
        let game = super::super::core_process::core_command_for_address(
            &layout,
            Path::new("/fixture/core"),
            Path::new("/fixture/socket"),
            "example.invalid",
            None,
            false,
        );
        let args: Vec<_> = game.get_args().collect();
        assert!(
            args.windows(2)
                .any(|pair| pair[0] == "-resource-pack-cache-dir"
                    && pair[1] == layout.resource_pack_cache_dir().as_os_str())
        );
        let launcher = launcher_command(
            &layout,
            Path::new("/fixture/core"),
            Path::new("/fixture/socket"),
            None,
            false,
            None,
            true,
        );
        assert!(
            launcher
                .get_args()
                .any(|arg| arg == "-resource-pack-cache-dir")
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

    #[test]
    fn menu_addresses_map_to_connect_targets() {
        assert_eq!(target_for("realm_id/42"), ConnectTarget::Realm("42".into()));
        assert_eq!(
            target_for("gathering/5b0f2bd4-8a8e-4a6e-9d3c-0a1b2c3d4e5f"),
            ConnectTarget::Gathering("5b0f2bd4-8a8e-4a6e-9d3c-0a1b2c3d4e5f".into())
        );
        assert_eq!(
            target_for("friend_xuid/2535"),
            ConnectTarget::Friend("2535".into())
        );
        assert_eq!(
            target_for("play.example.net:19133"),
            ConnectTarget::RakNet("play.example.net:19133".into())
        );
        assert_eq!(
            target_for("play.example.net"),
            ConnectTarget::RakNet("play.example.net:19132".into())
        );
        assert_eq!(
            target_for("[::1]:19134"),
            ConnectTarget::RakNet("[::1]:19134".into())
        );
        assert_eq!(
            target_for("::1"),
            ConnectTarget::RakNet("[::1]:19132".into())
        );
    }

    #[test]
    fn the_launcher_core_serves_control_and_signs_in_only_when_validated() {
        let layout = crate::install_layout::scratch("launcher-args");
        let args = |auth: Option<&Path>| -> Vec<String> {
            launcher_command(
                &layout,
                Path::new("/opt/core"),
                Path::new("/run/s"),
                auth,
                false,
                Some("pt_BR"),
                true,
            )
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
        };
        let offline = args(None);
        assert!(offline.iter().any(|arg| arg == "-control-status"));
        assert!(offline.iter().any(|arg| arg == "-xbox-presence"));
        assert!(
            offline
                .windows(2)
                .any(|args| args == ["-language", "pt-BR"])
        );
        assert!(
            !offline
                .iter()
                .any(|arg| arg == "-upstream" || arg == "-auth-cache")
        );
        let signed_in = args(Some(Path::new("/data/auth.json")));
        assert!(signed_in.iter().any(|arg| arg == "-auth-cache"));
    }

    // Adding an account must not make the install report a second device.
    #[test]
    fn every_core_names_the_install_device_whichever_account_signs_in() {
        let layout = crate::install_layout::scratch("launcher-device");
        let pending = launcher::accounts::AccountStore::new(layout.auth_cache()).pending_cache();
        let device_file = |command: Command| {
            let args: Vec<_> = command.get_args().map(ToOwned::to_owned).collect();
            args.windows(2)
                .find(|pair| pair[0] == "-device-file")
                .map(|pair| PathBuf::from(&pair[1]))
        };
        for auth in [None, Some(layout.auth_cache()), Some(pending)] {
            let auth = auth.as_deref();
            let launcher = launcher_command(
                &layout,
                Path::new("/opt/core"),
                Path::new("/run/s"),
                auth,
                false,
                None,
                true,
            );
            let direct = super::super::core_process::core_command_for_address(
                &layout,
                Path::new("/opt/core"),
                Path::new("/run/s"),
                "example.test",
                auth,
                false,
            );
            for command in [launcher, direct] {
                assert_eq!(device_file(command), Some(layout.device_profile_file()));
            }
        }
    }
}
