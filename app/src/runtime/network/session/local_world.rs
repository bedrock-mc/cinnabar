//! Caller-driven lifetime for the original local-world process and selected account.

use client_session::SessionCompanion;
use launcher_host::{local_worlds::NativeLocalWorldSelection, native_account::NativeAccount};
use std::{io, sync::Arc};

/// The menu's exact process opening; no world name or later manager lookup resolves its endpoint.
pub(crate) struct WorldConnectionRequest {
    pub(crate) selection: NativeLocalWorldSelection,
    pub(crate) account: Option<Arc<NativeAccount>>,
    pub(crate) retirement: client_shell::local_worlds::NativeLocalWorldRetirement,
}

impl WorldConnectionRequest {
    /// Moves only caller-owned state onto the existing session worker.
    pub(super) fn into_companion(
        self,
        claims: serde_json::Value,
        random_id: protocol::ClientRandomId,
    ) -> LocalWorldCompanion {
        LocalWorldCompanion {
            selection: self.selection,
            account: self.account,
            retirement: Some(self.retirement),
            claims: Some(claims),
            random_id,
        }
    }
}

pub(super) struct LocalWorldCompanion {
    selection: NativeLocalWorldSelection,
    account: Option<Arc<NativeAccount>>,
    retirement: Option<client_shell::local_worlds::NativeLocalWorldRetirement>,
    claims: Option<serde_json::Value>,
    random_id: protocol::ClientRandomId,
}

impl SessionCompanion for LocalWorldCompanion {
    /// Preserves the selected account through minting and watches the same opening across every wait.
    async fn prepare(
        &mut self,
    ) -> io::Result<
        Option<(
            protocol::NativeConnectionTarget,
            protocol::NativeAuthentication,
        )>,
    > {
        let target = self.selection.connection_target()?;
        if target.transport == protocol::NativeTransport::LocalNetherNetLan {
            target.local_lan_route()?;
            self.account = None;
            self.claims = None;
            return Ok(None);
        }
        if target.transport == protocol::NativeTransport::LocalNetherNetHttp {
            target.local_http_address()?;
        }
        let Some(account) = self.account.take() else {
            return Ok(None);
        };
        let claims = self
            .claims
            .take()
            .ok_or_else(|| io::Error::other("local world preparation was already used"))?;
        let preparing = account.prepare_connection(target.clone(), claims, self.random_id);
        tokio::pin!(preparing);
        loop {
            tokio::select! {
                biased;
                changed = self.selection.changed() => changed?,
                result = &mut preparing => {
                    return complete_preparation(&target, result, self.selection.validate());
                }
            }
        }
    }

    /// Runs beside transport connect, bootstrap backpressure and play without a polling task.
    async fn drive(&mut self) -> io::Result<()> {
        self.selection.changed().await
    }

    /// Queues the original process retirement; no pipe write blocks the session's reactor.
    async fn close(&mut self) {
        self.retirement = None;
    }
}

/// Admits only the completed preparation of the original local-world selection.
fn complete_preparation(
    target: &protocol::NativeConnectionTarget,
    prepared: io::Result<(
        protocol::NativeConnectionTarget,
        protocol::NativeAuthentication,
    )>,
    selection: io::Result<()>,
) -> io::Result<
    Option<(
        protocol::NativeConnectionTarget,
        protocol::NativeAuthentication,
    )>,
> {
    selection?;
    match prepared {
        Ok(prepared) => Ok(Some(prepared)),
        Err(error) if target.local_http_address().is_ok() => {
            bevy::log::warn!(
                kind = ?error.kind(),
                "local account preparation failed; using local server identity"
            );
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod preparation_tests {
    use super::*;

    struct RunningWorld(launcher_host::local_worlds::Manager);

    impl Drop for RunningWorld {
        fn drop(&mut self) {
            self.0.close();
        }
    }

    /// The actual Dragonfly opening must reach preparation through its original RakNet route.
    #[tokio::test]
    async fn dragonfly_local_world_admits_native_raknet_preparation() {
        const SERVER: &str = "CINNABAR_NATIVE_LOCAL_PREPARATION_SERVER";
        let Some(server) = std::env::var_os(SERVER) else {
            eprintln!("missing fixture: {SERVER} (build tools/localserver)");
            return;
        };
        let directory = tempfile::tempdir().unwrap();
        let mut layout = launcher::test_support::scratch_at(
            launcher::install_layout::Platform::Linux,
            directory.path(),
        );
        layout.local_server_executable = server.into();
        assert!(layout.local_server_executable.is_absolute());
        assert!(layout.local_server_executable.is_file());
        let world = RunningWorld(launcher_host::local_worlds::open(&layout).unwrap());
        let saved = world
            .0
            .create(&launcher::local_worlds::world::NewWorld {
                name: "Local preparation fixture".into(),
                generator: launcher::local_worlds::world::Generator::Flat,
                backend: Some(bridge::Backend::Dragonfly),
                ..Default::default()
            })
            .unwrap();
        world.0.open(&saved.id, Some(2)).unwrap();
        test_time::eventually("actual Dragonfly opening ready", || {
            world.0.status().state == bridge::WorldState::Running
        });
        let selection = world.0.native_local_world(&saved.id).unwrap();
        assert_eq!(
            selection.connection_target().unwrap().transport,
            protocol::NativeTransport::RakNet
        );
        let mut companion = LocalWorldCompanion {
            selection,
            account: None,
            retirement: None,
            claims: Some(serde_json::json!({})),
            random_id: protocol::ClientRandomId::new(1).unwrap(),
        };
        let prepared = companion.prepare().await;
        let original_running = companion.selection.validate();
        drop(companion);
        world.0.close();
        test_time::eventually("original Dragonfly opening stopped", || {
            world.0.status().state == bridge::WorldState::Idle
        });
        assert!(
            prepared
                .expect("the original Dragonfly RakNet route must admit preparation")
                .is_none()
        );
        original_running.unwrap();
    }

    /// A failed online service request must not prevent the original offline local server login.
    #[test]
    fn completed_local_http_failure_allows_offline_login() {
        for address in ["127.0.0.1:19132", "[::1]:19132"] {
            let target = protocol::NativeConnectionTarget::new(
                address.into(),
                protocol::NativeTransport::LocalNetherNetHttp,
            );
            let prepared = complete_preparation(
                &target,
                Err(io::Error::new(
                    io::ErrorKind::NotConnected,
                    "account service unavailable",
                )),
                Ok(()),
            );
            assert!(matches!(prepared, Ok(None)));
        }
    }

    /// Completion cannot revive a stopped or replaced local-world opening.
    #[test]
    fn failed_preparation_cannot_outlive_original_selection() {
        let target = protocol::NativeConnectionTarget::new(
            "127.0.0.1:19132".into(),
            protocol::NativeTransport::LocalNetherNetHttp,
        );
        let result = complete_preparation(
            &target,
            Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "account service unavailable",
            )),
            Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "original opening retired",
            )),
        );
        assert_eq!(result.err().unwrap().kind(), io::ErrorKind::Interrupted);
    }

    /// A local service error grants no offline login to another route or transport.
    #[test]
    fn nonlocal_preparation_failure_stays_an_error() {
        for target in [
            protocol::NativeConnectionTarget::new(
                "127.0.0.1:19132".into(),
                protocol::NativeTransport::Addressed,
            ),
            protocol::NativeConnectionTarget::new(
                "192.0.2.1:19132".into(),
                protocol::NativeTransport::LocalNetherNetHttp,
            ),
            protocol::NativeConnectionTarget::new(
                "127.0.0.1:0".into(),
                protocol::NativeTransport::LocalNetherNetHttp,
            ),
            protocol::NativeConnectionTarget::new(
                "fixture.invalid:19132".into(),
                protocol::NativeTransport::LocalNetherNetHttp,
            ),
        ] {
            let result = complete_preparation(
                &target,
                Err(io::Error::new(
                    io::ErrorKind::NotConnected,
                    "account service unavailable",
                )),
                Ok(()),
            );
            assert_eq!(result.err().unwrap().kind(), io::ErrorKind::NotConnected);
        }
    }
}
