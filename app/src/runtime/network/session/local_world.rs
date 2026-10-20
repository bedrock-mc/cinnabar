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
        let Some(account) = self.account.take() else {
            return Ok(None);
        };
        let claims = self
            .claims
            .take()
            .ok_or_else(|| io::Error::other("local world preparation was already used"))?;
        let preparing = account.prepare_connection(target, claims, self.random_id);
        tokio::pin!(preparing);
        loop {
            tokio::select! {
                biased;
                changed = self.selection.changed() => changed?,
                result = &mut preparing => {
                    let prepared = result?;
                    self.selection.validate()?;
                    return Ok(Some(prepared));
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
