//! One actual account lease and exact upstream socket through Login, handoff and play retirement.

use super::{
    control,
    registry::Registry,
    session::{PlayLease, closed, progress},
    upstream,
};
use bridge::{FrameQueue, FrameRead, contract::*};
use bytes::Bytes;
use futures::StreamExt;
use protocol::{
    ClientDataRefusal, NativeRelayClosing, NativeRelayConnecting, NativeRelayDriver,
    NativeRelayRetired, RawClientData,
    session_wire::{self, CoreMessage},
};
use std::{io, sync::Arc, time::Instant};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::Framed;

mod duplex;
mod packs;
mod upload;

#[cfg(test)]
tokio::task_local! {
    static PREPARED_CLAIMS: tokio::sync::mpsc::Sender<RawClientData>;
}

/// The socket field drops before its account lease even if the caller cancels or unwinds.
struct Playing {
    socket: Option<Socket>,
    lease: PlayLease,
}

#[expect(
    clippy::large_enum_variant,
    reason = "the same owned connection moves between inline phases without per-phase allocation"
)]
enum Socket {
    Connecting(NativeRelayConnecting),
    Pumping(NativeRelayDriver),
    Closing(NativeRelayClosing),
    Retired(NativeRelayRetired),
}

/// Consumes the selected reliable play stream; account and transport drivers stay caller-owned.
pub(super) async fn run<S>(registry: &Arc<Registry>, stream: S) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    run_inner(registry, stream, None, packs::Delivery::Serial).await
}

/// Reuses this QUIC connection's original controller instead of a global bearer lookup.
#[cfg(feature = "webtransport")]
pub(super) async fn run_bound<S>(
    registry: &Arc<Registry>,
    stream: S,
    controller: &super::registry::Attachment,
    transport: Arc<super::backend::webtransport::Transport>,
) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    run_inner(
        registry,
        stream,
        Some(controller),
        packs::Delivery::WebTransport(transport),
    )
    .await
}

/// Keeps both transport entrypoints on the same account, negotiated socket and closing owner.
async fn run_inner<S>(
    registry: &Arc<Registry>,
    stream: S,
    bound: Option<&super::registry::Attachment>,
    delivery: packs::Delivery,
) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    // Hello is bounded independently before the play codec admits a larger Connect/game frame.
    let mut hello_io = Framed::new(stream, bridge::BridgeCodec::with_max(RELAY_CONTROL_BYTES));
    let hello = control::read_hello(&mut hello_io, RELAY_CHANNEL_PLAY).await?;
    let attachment = match bound {
        #[cfg(feature = "webtransport")]
        Some(controller) => registry.bound_play(controller, hello.generation)?,
        _ => registry.open_play(&hello.resume, hello.generation, Instant::now())?,
    };
    registry.allow_play(&attachment, Instant::now())?;
    let deadline = Instant::now()
        .checked_add(protocol::NATIVE_JOIN_TIMEOUT)
        .ok_or_else(closed)?;
    let acquiring = attachment
        .session
        .begin_play(attachment.generation, hello.join);
    let lease = tokio::select! { biased;
        () = attachment.cancel.cancelled() => return Err(closed()),
        result = tokio::time::timeout_at(deadline.into(), acquiring) => result.map_err(|_| elapsed())??,
    };
    let mut playing = Playing {
        socket: None,
        lease,
    };
    let parts = hello_io.into_parts();
    let stream = super::prefix::Prefix::new(parts.io, parts.read_buf.freeze());
    let (read, write) = tokio::io::split(stream);
    let (reader, frames, writer) = bridge::tasks::prepare_bounded_connection(
        upload::Upload::new(read),
        write,
        bridge::MAX_FRAME_LEN,
        RELAY_QUEUED_FRAME_BYTES,
    );
    let result = {
        let serving = playing.serve_with(reader, frames, deadline, delivery);
        tokio::pin!(serving, writer);
        tokio::select! { biased;
            () = registry.cancelled.cancelled() => Err(closed()),
            () = attachment.cancel.cancelled() => Err(closed()),
            result = &mut serving => result,
            () = &mut writer => Err(closed()),
        }
    };
    playing.begin_close();
    let cleanup = playing.close().await;
    result.and(cleanup)
}

impl Playing {
    /// Keeps the original serial fixture entrypoint on the same production serve path.
    #[cfg(test)]
    async fn serve(
        &mut self,
        reader: impl FrameRead,
        frames: FrameQueue,
        deadline: Instant,
    ) -> io::Result<()> {
        self.serve_with(reader, frames, deadline, packs::Delivery::Serial)
            .await
    }

    /// Keeps original negotiated-socket controls on the same production negotiation path.
    #[cfg(test)]
    async fn negotiate(
        &mut self,
        reader: impl FrameRead,
        frames: FrameQueue,
        claims: RawClientData,
        client_cache: bool,
        authentication: Option<protocol::NativeAuthentication>,
        deadline: Instant,
    ) -> io::Result<()> {
        self.negotiate_with(
            reader,
            frames,
            claims,
            client_cache,
            authentication,
            deadline,
            packs::Delivery::Serial,
        )
        .await
    }

    /// Retains the actual SDK owner across mint and the same socket across every later phase.
    async fn serve_with(
        &mut self,
        mut reader: impl FrameRead,
        frames: FrameQueue,
        deadline: Instant,
        delivery: packs::Delivery,
    ) -> io::Result<()> {
        upstream::current(&self.lease, deadline)?;
        let frame = tokio::time::timeout_at(deadline.into(), reader.next())
            .await
            .map_err(|_| elapsed())?
            .ok_or_else(closed)?
            .map_err(invalid)?;
        upstream::current(&self.lease, deadline)?;
        let request = match session_wire::decode_native_connect(&frame) {
            Ok(request) => request,
            Err(_) => {
                return self
                    .refuse(&frames, ClientDataRefusal::Generic, deadline)
                    .await;
            }
        };
        let checked = if let Some(account) = &self.lease.account {
            account.validate(request.client_data, request.protocol)
        } else {
            self.lease
                .session
                .validate_guest(request.client_data, request.protocol)
        };
        upstream::current(&self.lease, deadline)?;
        let claims = match checked {
            Ok(claims) => claims,
            Err(refusal) => return self.refuse(&frames, refusal, deadline).await,
        };
        self.lease
            .progress(progress(bridge::ConnectStage::Connecting))?;
        let (claims, authentication) = if let Some(account) = &mut self.lease.account {
            let preparing = account.prepare(claims, deadline);
            tokio::pin!(preparing);
            let authentication = tokio::select! { biased;
                () = self.lease.cancel.cancelled() => return Err(closed()),
                result = &mut preparing => result?,
            };
            (RawClientData::default(), Some(authentication))
        } else {
            (self.lease.session.guest_claims(claims)?, None)
        };
        upstream::current(&self.lease, deadline)?;
        #[cfg(test)]
        let _ = PREPARED_CLAIMS.try_with(|prepared| {
            prepared
                .try_send(claims.clone())
                .expect("fixture prepared-claims capacity");
        });
        let connected = upstream::connect(&self.lease, request.target, deadline).await?;
        self.socket = Some(Socket::Connecting(connected));
        self.negotiate_with(
            reader,
            frames,
            claims,
            request.client_cache,
            authentication,
            deadline,
            delivery,
        )
        .await
    }

    /// Publishes the generated refusal before cleanup, under the original caller's bounded write.
    async fn refuse(
        &mut self,
        frames: &FrameQueue,
        refusal: ClientDataRefusal,
        deadline: Instant,
    ) -> io::Result<()> {
        upstream::current(&self.lease, deadline)?;
        let terminal = CoreMessage::Disconnect(session_wire::SessionDisconnect {
            reason: 0,
            message: refusal.disconnect_key().into(),
            filtered_message: String::new(),
            hide_screen: false,
        });
        let frame = session_wire::encode_core_message(&terminal).map_err(invalid)?;
        tokio::select! { biased;
            () = self.lease.cancel.cancelled() => return Err(closed()),
            result = tokio::time::timeout_at(deadline.into(), frames.send(frame)) => {
                result.map_err(|_| elapsed())?.map_err(invalid)?;
            }
        }
        upstream::current(&self.lease, deadline)?;
        Err(io::Error::new(io::ErrorKind::InvalidInput, refusal))
    }

    /// Negotiates the actual retained preconnected owner; it performs no replacement dial or admission.
    #[expect(
        clippy::too_many_arguments,
        reason = "one retained negotiation carries its original I/O, claims, authentication and output owner"
    )]
    async fn negotiate_with(
        &mut self,
        reader: impl FrameRead,
        frames: FrameQueue,
        claims: RawClientData,
        client_cache: bool,
        authentication: Option<protocol::NativeAuthentication>,
        deadline: Instant,
        delivery: packs::Delivery,
    ) -> io::Result<()> {
        upstream::current(&self.lease, deadline)?;
        let Some(Socket::Connecting(socket)) = self.socket.as_mut() else {
            unreachable!("actual preconnected owner")
        };
        let negotiated = tokio::time::timeout_at(
            deadline.into(),
            socket.negotiate(claims, client_cache, authentication),
        )
        .await;
        if let Err(error) = negotiated
            .map_err(|_| elapsed())
            .and_then(|result| result.map_err(invalid))
        {
            if let Some(terminal) = socket.take_terminal() {
                duplex::write(
                    &self.lease,
                    &frames,
                    session_wire::encode_core_message(&terminal).map_err(invalid)?,
                    Some(deadline),
                )
                .await?;
            }
            return Err(error);
        }
        upstream::current(&self.lease, deadline)?;
        let Some(Socket::Connecting(socket)) = self.socket.take() else {
            unreachable!("completed same socket")
        };
        let (socket, handoff) = match socket.try_finish() {
            Ok(complete) => complete,
            Err(socket) => {
                self.socket = Some(Socket::Connecting(socket));
                return Err(invalid(()));
            }
        };
        let (sender, incoming, driver) = socket.into_pump();
        self.socket = Some(Socket::Pumping(driver));
        let result = {
            let Some(Socket::Pumping(driver)) = self.socket.as_mut() else {
                unreachable!("same established driver")
            };
            duplex::run(
                &self.lease,
                driver,
                (sender, incoming),
                reader,
                frames,
                handoff,
                deadline,
                delivery,
            )
            .await
        };
        self.begin_close();
        result
    }

    /// Revokes admission and captures the first close budget before another await.
    fn begin_close(&mut self) {
        let Some(socket) = self.socket.take() else {
            return;
        };
        self.socket = Some(match socket {
            Socket::Connecting(socket) => Socket::Retired(socket.begin_close()),
            Socket::Pumping(driver) => Socket::Closing(driver.begin_close()),
            closing @ (Socket::Closing(_) | Socket::Retired(_)) => closing,
        });
    }

    /// Retains the actual retirement owner and original deadline across canceled cleanup waits.
    async fn close(&mut self) -> io::Result<()> {
        self.begin_close();
        match self.socket.as_mut() {
            Some(Socket::Retired(socket)) => socket.finish_close().await,
            Some(Socket::Closing(driver)) => driver.finish_close().await,
            None => Ok(()),
            Some(Socket::Connecting(_) | Socket::Pumping(_)) => {
                unreachable!("synchronous consuming close")
            }
        }
    }
}

/// Redacts peer/account/provider/input diagnostics at the browser stream boundary.
fn invalid<T>(_error: T) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "gateway play failed")
}

/// Reports only the original finite join budget.
fn elapsed() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "gateway join timed out")
}

/// Keeps original finite join/drain waits; a play phase without one has no invented idle timer.
async fn expires(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline.into()).await,
        None => std::future::pending().await,
    }
}

/// Rechecks account/controller authority and the original deadline after actual output completion.
fn current(lease: &PlayLease, deadline: Option<Instant>) -> io::Result<()> {
    match deadline {
        Some(deadline) => upstream::current(lease, deadline),
        None => lease.current(),
    }
}

#[cfg(test)]
mod tests;
