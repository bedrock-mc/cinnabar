//! Browser relay channels. The host drives deadlines and reconnects and feeds the prepared
//! play connection into the existing session plumbing; gameplay always stays reliable.

#[cfg(target_arch = "wasm32")]
pub mod browser;
pub mod control;
pub mod session;

#[cfg(any(test, target_arch = "wasm32"))]
mod receive;

use session::{Session, State};

use std::future::Future;
use tokio::io::{AsyncRead, AsyncWrite};

/// Reliable channel names from the shared relay contract.
#[derive(Clone, Copy)]
pub enum Channel {
    Control,
    Play { join: u64 },
}
impl Channel {
    /// Returns the channel's wire name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Control => bridge::contract::RELAY_CHANNEL_CONTROL,
            Self::Play { .. } => bridge::contract::RELAY_CHANNEL_PLAY,
        }
    }
}

/// Public close outcomes, without account or endpoint details.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseReason {
    Cancelled,
    Transport,
    Protocol,
    Refused,
    RetryAt(std::time::Duration),
    RetryExhausted,
}

/// Browser-local transport boundary; JS stream handles never need `Send` wrappers.
#[allow(async_fn_in_trait)]
pub trait RelayTransport {
    type Stream: AsyncRead + AsyncWrite + Unpin + 'static;
    /// Opens a reliable channel; the caller writes its framed Hello next.
    async fn open(&mut self, channel: Channel) -> Result<Self::Stream, CloseReason>;
    /// Waits for this play's independent receive stream; fallback returns inline delivery.
    fn accept_packs(
        &self,
    ) -> impl Future<Output = Result<Option<Self::Stream>, CloseReason>> + 'static + use<Self> {
        async { Ok(None) }
    }
    /// Resets the completed play and any outstanding archive reader, retaining control.
    fn finish_play(&mut self) {}
    /// Cancels pending I/O immediately, including a throttled send.
    fn close(&mut self, reason: CloseReason);
}

/// Selects a transport only after its control Hello succeeds. A failed QUIC candidate is
/// closed before fallback; authentication refusal never triggers a second transport.
/// The host clock callback is sampled again when each Welcome arrives.
pub async fn select<T: RelayTransport, D: Future<Output = ()>>(
    session: &mut Session<T>,
    quic: Option<T>,
    socket: impl FnOnce() -> T,
    mut now: impl FnMut() -> std::time::Duration,
    mut deadline: impl FnMut() -> D,
    mut jitter: impl FnMut() -> std::time::Duration,
) -> Result<(), CloseReason> {
    match session.state() {
        State::Refused(_) => return Err(CloseReason::Refused),
        State::Closed => return Err(CloseReason::Cancelled),
        _ => {}
    }
    if session.state() == State::Offline {
        let due = session.next_retry()?;
        let time = now();
        if time < due {
            return Err(CloseReason::RetryAt(due));
        }
        if !session.retry(time, jitter()) {
            return Err(CloseReason::RetryExhausted);
        }
    }
    if session.state() != State::Connecting {
        return Err(CloseReason::Protocol);
    }
    if let Some(quic) = quic {
        session.replace_transport(quic);
        match bridge::tasks::until(session.handshake(&mut now), deadline()).await {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(CloseReason::Refused)) => {
                session.close(CloseReason::Refused);
                return Err(CloseReason::Refused);
            }
            _ => session.close(CloseReason::Transport),
        }
    }
    session.replace_transport(socket());
    let result = bridge::tasks::until(session.handshake(&mut now), deadline())
        .await
        .unwrap_or(Err(CloseReason::Transport));
    if let Err(reason) = result {
        session.close(reason);
    }
    result
}

#[cfg(test)]
mod tests;
