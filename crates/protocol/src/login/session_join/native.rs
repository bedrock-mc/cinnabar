//! Native endpoint and blocking-job composition for the shared login sequence.

use super::{JoinPreparation, SessionIo, SessionLogin};
use crate::{
    ClientBlobCache, ClientRandomId, ClientSkin, GameData, LoginSequence, LoginSettings,
    PlaySession, ProtocolError, ResourcePackHandoff, packet_tap::PacketTap,
};
use std::{future::Future, path::Path, pin::Pin, sync::Arc};

/// Preserves native startup offloading and panic propagation on the Tokio runtime.
pub struct NativeJoinPreparation;

impl JoinPreparation for NativeJoinPreparation {
    type Task<T: Send + 'static> = Pin<Box<dyn Future<Output = T> + Send>>;

    fn submit<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Self::Task<T> {
        // Spawn at submission, before the spawn sequence reads more server packets.
        let profile = bridge::join_profile::JoinProfile::current();
        let task = tokio::task::spawn_blocking(move || profile.run(work));
        Box::pin(async move {
            match task.await {
                Ok(value) => value,
                Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
                Err(_) => panic!("startup worker cancelled by its runtime"),
            }
        })
    }
}

impl LoginSequence {
    /// Opens the native core endpoint, offloads startup work and continues its only login.
    /// Pack preparation overlaps server input; loading ends when the caller presents the world.
    #[expect(
        clippy::too_many_arguments,
        reason = "independent caller choices for one join"
    )]
    pub async fn connect_session<P: Send + 'static>(
        socket_dir: &Path,
        display_name: &str,
        client_random_id: ClientRandomId,
        cache: Option<ClientBlobCache>,
        skin: Option<ClientSkin>,
        settings: &LoginSettings,
        tap: Option<Arc<dyn PacketTap>>,
        prepare: impl FnOnce(ResourcePackHandoff, GameData) -> P + Send + 'static,
    ) -> Result<(PlaySession, P), ProtocolError> {
        let profile = bridge::join_profile::JoinProfile::current();
        let endpoint = profile.stage("core_endpoint_connect");
        let (reader, frames) = bridge::connect_session(socket_dir)
            .await
            .map_err(ProtocolError::Bridge)?;
        drop(endpoint);
        Self::connect_prepared(
            SessionIo {
                reader,
                frames,
                timers: bridge::tasks::Timers::tokio(),
            },
            SessionLogin {
                display_name,
                client_random_id,
                cache,
                skin,
                settings,
                tap,
                target: None,
            },
            &NativeJoinPreparation,
            prepare,
        )
        .await
    }
}
