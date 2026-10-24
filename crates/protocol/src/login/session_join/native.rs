//! Native endpoint and blocking-job composition for the shared login sequence.

use super::JoinPreparation;
#[cfg(feature = "native-networking")]
use super::SessionLogin;
#[cfg(feature = "native-networking")]
use crate::{GameData, LoginSequence, PlaySession, ProtocolError};
#[cfg(feature = "native-networking")]
use bedrock_packs::ResourcePackHandoff;
use std::{future::Future, pin::Pin};

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

#[cfg(feature = "native-networking")]
impl LoginSequence {
    /// Connects the captured native target and retains shared spawn and preparation ownership.
    pub async fn connect_endpoint<P: Send + 'static>(
        endpoint: &crate::SessionEndpoint,
        login: SessionLogin<'_>,
        prepare: impl FnOnce(ResourcePackHandoff, GameData) -> P + Send + 'static,
    ) -> Result<(PlaySession, P), ProtocolError> {
        let crate::SessionEndpoint::Native(target) = endpoint;
        Self::connect_native(target, login, prepare).await
    }
}
