//! The launcher account core process, endpoint lifetime and join worker.

use crate::{
    core_process::{CoreProcessGuard, clear_stale_bridge_endpoint, core_executable, wait_for_core},
    session_cleanup::SessionDirectoryGuard,
};
use anyhow::{Context, Result, anyhow};
use bridge::ConnectTarget;
use launcher::{install_layout::InstallLayout, menu::DEFAULT_PORT};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// Chooses the sole title presence publisher for one account-core incarnation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TitlePresenceOwner {
    /// The existing Go account worker owns updates and cleanup.
    #[default]
    Core,
    /// An application-owned native lifetime will own updates and cleanup.
    /// The core starts no presence worker; this policy does not start a native publisher.
    Native,
}

/// Explicit service and cache ownership for one account-core incarnation.
#[derive(Clone, Copy, Default)]
pub struct AccountCorePolicy {
    /// Whether a verified client blob-cache resolver is active for upstream traffic.
    pub upstream_client_cache: bool,
    /// Whether this core owns the game resource-pack cache lease.
    pub lease_game_cache: bool,
    /// The sole title presence publisher selected before process startup.
    pub title_presence_owner: TitlePresenceOwner,
}

pub struct LauncherCore {
    pub _guard: CoreProcessGuard,
    pub _directory: SessionDirectoryGuard,
    pub socket_dir: PathBuf,
    pub authenticated: bool,
    pub auth_cache: Option<PathBuf>,
    /// The actual spawn policy, retained as part of this core's restart identity.
    pub title_presence_owner: TitlePresenceOwner,
    /// Account and local-world clients are attached once the session endpoint is up.
    pub attached: bool,
}
impl Drop for LauncherCore {
    fn drop(&mut self) {
        let stopped = self._guard.stop();
        #[cfg(unix)]
        if stopped != super::core_process::CoreStopOutcome::Unreaped {
            // Long Unix socket paths live outside the owned session directory.
            for endpoint in bridge::core_endpoint_paths(&self.socket_dir) {
                if let Err(error) = std::fs::remove_file(&endpoint)
                    && error.kind() != std::io::ErrorKind::NotFound
                {
                    tracing::warn!("remove core endpoint {}: {error}", endpoint.display());
                }
            }
        }
        #[cfg(not(unix))]
        let _ = stopped;
    }
}
impl LauncherCore {
    /// Starts a tracked account core using the current language and sign-in.
    pub fn spawn(
        layout: &InstallLayout,
        auth_cache: Option<&Path>,
        language: &str,
        policy: AccountCorePolicy,
    ) -> Result<Self> {
        let executable =
            core_executable(layout).ok_or_else(|| anyhow!("bedrock-core executable not found"))?;
        let socket_dir = next_account_socket_dir(layout);
        let directory =
            SessionDirectoryGuard::bind(socket_dir.clone()).map_err(|error| anyhow!("{error}"))?;
        clear_stale_bridge_endpoint(&socket_dir)?;
        let child = crate::lifecycle::core_output::spawn(
            &mut launcher_command(
                layout,
                &executable,
                &socket_dir,
                auth_cache,
                Some(language),
                policy,
            ),
            crate::lifecycle::core_output::CoreRole::Account,
        )
        .with_context(|| format!("spawn {} for the launcher", executable.display()))?;
        let mut guard = CoreProcessGuard::default();
        guard.replace(child);
        Ok(Self {
            _guard: guard,
            _directory: directory,
            socket_dir,
            authenticated: auth_cache.is_some(),
            auth_cache: auth_cache.map(Path::to_path_buf),
            title_presence_owner: policy.title_presence_owner,
            attached: false,
        })
    }
}
impl LauncherCore {
    /// Selects a target off the frame and returns the endpoint when ready.
    pub fn begin_join(
        &self,
        address: &str,
        local_world: bool,
    ) -> Option<crossbeam_channel::Receiver<Result<PathBuf, String>>> {
        let (sender, receiver) = crossbeam_channel::bounded(1);
        let (socket_dir, target) = (self.socket_dir.clone(), target_for(address));
        std::thread::Builder::new()
            .name("launcher-join".to_owned())
            .spawn(move || {
                let _ = sender.send(prepare(socket_dir, target, local_world));
            })
            .ok()?;
        Some(receiver)
    }
}

/// Gives each account-core incarnation its own runtime directory.
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

/// Builds the account core command from its install and session settings.
fn launcher_command(
    layout: &InstallLayout,
    executable: &Path,
    socket_dir: &Path,
    auth_cache: Option<&Path>,
    language: Option<&str>,
    policy: AccountCorePolicy,
) -> Command {
    let mut command = Command::new(executable);
    command
        .arg("-socket-dir")
        .arg(socket_dir)
        .arg("-control-status")
        .arg("-cinnabar-api")
        .arg("-server-trust-file")
        .arg(layout.server_trust_file())
        .arg("-device-file")
        .arg(layout.device_profile_file())
        .stdin(Stdio::piped())
        .stdout(Stdio::null());
    if policy.title_presence_owner == TitlePresenceOwner::Core {
        command.arg("-xbox-presence");
    }
    // Direct-mode account cores run alongside game cores that own this exclusive lease.
    if policy.lease_game_cache {
        command
            .arg("-resource-pack-cache-dir")
            .arg(layout.resource_pack_cache_dir());
    }
    if policy.upstream_client_cache {
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

/// Sends `connect.v1` and leaves service deadlines with the account core.
fn select(socket_dir: &Path, target: ConnectTarget) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    runtime.block_on(select_target(socket_dir, target))
}

/// Waits for the selected target's account-service result on the join worker runtime.
async fn select_target(socket_dir: &Path, target: ConnectTarget) -> Result<(), String> {
    bridge::connect_target(socket_dir, &target)
        .await
        .map_err(|error| error.to_string())
}

/// The kind of join `address` starts, for its progress titles.
pub fn join_kind(address: &str, local_world: bool) -> launcher::menu::view::JoinKind {
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
pub fn target_for(address: &str) -> ConnectTarget {
    let address = address.trim();
    if let Some(id) = address.strip_prefix(launcher::menu::EXPERIENCE_ADDRESS_PREFIX) {
        return ConnectTarget::Gathering(id.to_owned());
    }
    if let Some(id) = address.strip_prefix(bridge::contract::REALM_TARGET_PREFIX) {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A responsive account core may still be resolving its selected experience after three seconds.
    #[cfg(unix)]
    #[tokio::test(start_paused = true)]
    async fn selection_waits_for_the_account_service_result() {
        use std::time::Duration;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let directory = tempfile::tempdir().unwrap();
        let endpoint = bridge::control_endpoint_path(directory.path());
        std::fs::create_dir_all(endpoint.parent().unwrap()).unwrap();
        let listener = tokio::net::UnixListener::bind(&endpoint).unwrap();
        let socket_dir = directory.path().to_path_buf();
        let selecting = tokio::spawn(async move {
            select_target(&socket_dir, ConnectTarget::Gathering("fixture".into())).await
        });
        let (mut core, _) = listener.accept().await.unwrap();
        let length = core.read_u32().await.unwrap();
        let mut request = vec![0; length as usize];
        core.read_exact(&mut request).await.unwrap();
        let request: serde_json::Value = serde_json::from_slice(&request).unwrap();
        assert_eq!(request["method"], bridge::contract::METHOD_CONNECT);
        tokio::time::advance(Duration::from_secs(4)).await;
        tokio::task::yield_now().await;
        assert!(
            !selecting.is_finished(),
            "an active service request must remain pending"
        );
        let response = serde_json::to_vec(&serde_json::json!({
            "jsonrpc": bridge::contract::JSON_RPC_VERSION,
            "id": request["id"],
            "result": {"schema_version": bridge::contract::CONTROL_SCHEMA_VERSION}
        }))
        .unwrap();
        core.write_u32(response.len() as u32).await.unwrap();
        core.write_all(&response).await.unwrap();
        assert_eq!(selecting.await.unwrap(), Ok(()));
    }

    #[test]
    fn launcher_core_remembers_trusted_servers_in_user_data() {
        let layout = launcher::test_support::scratch("server-trust-file");
        let command = launcher_command(
            &layout,
            Path::new("/fixture/core"),
            Path::new("/fixture/socket"),
            None,
            None,
            AccountCorePolicy::default(),
        );
        let args: Vec<_> = command.get_args().collect();
        assert!(args.windows(2).any(|pair| pair[0] == "-server-trust-file"
            && pair[1] == layout.server_trust_file().as_os_str()));
    }
    #[test]
    fn direct_account_core_leaves_pack_cache_for_game_cores() {
        let layout = launcher::test_support::scratch("direct-cache-ownership");
        for auth in [None, Some(Path::new("/fixture/auth.json"))] {
            let account = launcher_command(
                &layout,
                Path::new("/fixture/core"),
                Path::new("/fixture/socket"),
                auth,
                None,
                AccountCorePolicy::default(),
            );
            let account_args: Vec<_> = account.get_args().collect();
            assert!(
                !account_args.contains(&std::ffi::OsStr::new("-resource-pack-cache-dir")),
                "account-only startup or auth restart must not lease the shared pack cache"
            );
        }
        let game = crate::core_process::core_command_for_address(
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
            None,
            AccountCorePolicy {
                lease_game_cache: true,
                ..Default::default()
            },
        );
        assert!(
            launcher
                .get_args()
                .any(|arg| arg == "-resource-pack-cache-dir")
        );
    }
    #[test]
    fn title_presence_has_one_explicit_process_owner() {
        let layout = launcher::test_support::scratch("presence-owner");
        for owner in [TitlePresenceOwner::Core, TitlePresenceOwner::Native] {
            let command = launcher_command(
                &layout,
                Path::new("/fixture/core"),
                Path::new("/fixture/socket"),
                None,
                None,
                AccountCorePolicy {
                    title_presence_owner: owner,
                    ..Default::default()
                },
            );
            assert_eq!(
                command
                    .get_args()
                    .any(|argument| argument == "-xbox-presence"),
                owner == TitlePresenceOwner::Core,
            );
            assert!(
                command
                    .get_args()
                    .any(|argument| argument == "-control-status")
            );
        }
    }
    #[test]
    fn menu_addresses_map_to_connect_targets() {
        assert_eq!(
            target_for(&format!("{}42", bridge::contract::REALM_TARGET_PREFIX)),
            ConnectTarget::Realm("42".into())
        );
        assert_eq!(
            target_for(&format!(
                "{}5b0f2bd4-8a8e-4a6e-9d3c-0a1b2c3d4e5f",
                bridge::contract::GATHERING_TARGET_PREFIX
            )),
            ConnectTarget::Gathering("5b0f2bd4-8a8e-4a6e-9d3c-0a1b2c3d4e5f".into())
        );
        assert_eq!(
            target_for(&format!("{}2535", bridge::contract::FRIEND_TARGET_PREFIX)),
            ConnectTarget::Friend("2535".into())
        );
        assert_eq!(
            target_for("play.example.net:19133"),
            ConnectTarget::RakNet("play.example.net:19133".into())
        );
        assert_eq!(
            target_for("play.example.net"),
            ConnectTarget::RakNet(format!(
                "play.example.net:{}",
                bridge::contract::DEFAULT_BEDROCK_PORT_TEXT
            ))
        );
        assert_eq!(
            target_for("[::1]:19134"),
            ConnectTarget::RakNet("[::1]:19134".into())
        );
        assert_eq!(
            target_for("::1"),
            ConnectTarget::RakNet(format!(
                "[::1]:{}",
                bridge::contract::DEFAULT_BEDROCK_PORT_TEXT
            ))
        );
    }
    #[test]
    fn the_launcher_core_serves_control_and_signs_in_only_when_validated() {
        let layout = launcher::test_support::scratch("launcher-args");
        let args = |auth: Option<&Path>| -> Vec<String> {
            launcher_command(
                &layout,
                Path::new("/opt/core"),
                Path::new("/run/s"),
                auth,
                Some("pt_BR"),
                AccountCorePolicy {
                    lease_game_cache: true,
                    ..Default::default()
                },
            )
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
        };
        let offline = args(None);
        assert!(offline.iter().any(|arg| arg == "-control-status"));
        assert!(offline.iter().any(|arg| arg == "-xbox-presence"));
        assert!(offline.iter().any(|arg| arg == "-cinnabar-api"));
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
    #[test]
    fn every_core_names_the_install_device_whichever_account_signs_in() {
        let layout = launcher::test_support::scratch("launcher-device");
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
                None,
                AccountCorePolicy {
                    lease_game_cache: true,
                    ..Default::default()
                },
            );
            let direct = crate::core_process::core_command_for_address(
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
    fn retired_account_control_paths_cannot_reach_a_new_core_or_game_session() {
        let layout = launcher::test_support::scratch("account-endpoint-isolation");
        let first = next_account_socket_dir(&layout);
        let second = next_account_socket_dir(&layout);
        assert_ne!(first, second);
        for generation in 0..3 {
            let game = layout.connect_socket_dir(std::process::id(), generation);
            assert_ne!(first, game);
            assert_ne!(second, game);
        }
    }
}
