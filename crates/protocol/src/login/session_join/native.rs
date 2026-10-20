//! Native endpoint and blocking-job composition for the shared login sequence.

use super::{JoinPreparation, SessionIo, SessionLogin};
use crate::{
    ClientBlobCache, ClientRandomId, ClientSkin, GameData, LoginSequence, LoginSettings,
    PlaySession, ProtocolError, packet_tap::PacketTap,
};
use bedrock_packs::ResourcePackHandoff;
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
    /// Selects the actual connection backend while retaining shared spawn and preparation.
    pub async fn connect_endpoint<P: Send + 'static>(
        endpoint: &crate::SessionEndpoint,
        login: SessionLogin<'_>,
        prepare: impl FnOnce(ResourcePackHandoff, GameData) -> P + Send + 'static,
    ) -> Result<(PlaySession, P), ProtocolError> {
        match endpoint {
            crate::SessionEndpoint::Core(directory) => {
                let profile = bridge::join_profile::JoinProfile::current();
                let (reader, frames) = {
                    let _endpoint = profile.stage("core_endpoint_connect");
                    bridge::connect_session(directory)
                        .await
                        .map_err(ProtocolError::Bridge)?
                };
                Self::connect_prepared(
                    SessionIo {
                        reader,
                        frames,
                        timers: bridge::tasks::Timers::tokio(),
                    },
                    login,
                    &NativeJoinPreparation,
                    prepare,
                )
                .await
            }
            #[cfg(feature = "native-networking")]
            crate::SessionEndpoint::Native(target) => {
                Self::connect_native(target, login, prepare).await
            }
        }
    }

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
        relay: Option<Arc<dyn crate::PacketRelay>>,
        tap: Option<Arc<dyn PacketTap>>,
        prepare: impl FnOnce(ResourcePackHandoff, GameData) -> P + Send + 'static,
    ) -> Result<(PlaySession, P), ProtocolError> {
        let profile = bridge::join_profile::JoinProfile::current();
        let (reader, frames) = {
            let _endpoint = profile.stage("core_endpoint_connect");
            bridge::connect_session(socket_dir)
                .await
                .map_err(ProtocolError::Bridge)?
        };
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
                relay,
                target: None,
            },
            &NativeJoinPreparation,
            prepare,
        )
        .await
    }
}
