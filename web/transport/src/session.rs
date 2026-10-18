//! Offline -> connecting -> ready -> playing; closure returns to offline, deliberate close
//! is terminal. Reconnect never replays play bytes. Current and one previous bearer keep
//! their own expiries; only a
//! received bearer is echoed in pong. Offline account intent occupies one slot. Caller time
//! drives six bounded retries with capped backoff and caller-supplied jitter.

use crate::control::{Capability, Event, Hello, Request, parse};
use crate::{Channel, CloseReason, RelayTransport};
use bridge::{
    BridgeCodec,
    contract::{
        MAX_FRAME_LEN, RELAY_CONTROL_BYTES, RELAY_QUEUED_FRAME_BYTES, RELAY_VERSION, RefusalCode,
    },
};
use bytes::Bytes;
use futures::{SinkExt, StreamExt};

use std::time::Duration;
use tokio_util::codec::Framed;

pub(crate) const RECONNECT_ATTEMPTS: u32 = 6;

/// Session lifecycle; only a ready controller may start one play generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Offline,
    Connecting,
    Ready(u64),
    Playing(u64),
    Refused(RefusalCode),
    Closed,
}
/// Latest desired account operation, coalesced while offline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountOperation {
    SignIn,
    SignOut,
}
/// A request refusal keeps control ready and identifies the operation the player can retry.
#[derive(Debug, PartialEq)]
pub enum RequestRefusal {
    RetrySignIn,
    Refused(RefusalCode),
}
impl RequestRefusal {
    /// Describes the available request recovery without exposing provider errors.
    pub fn message(&self) -> &'static str {
        match self {
            Self::RetrySignIn => "The relay cannot accept sign-in right now. Use Sign in to retry.",
            Self::Refused(RefusalCode::SessionBusy) => "Leave the server before signing in or out.",
            Self::Refused(_) => "The relay refused this request.",
        }
    }
}
/// Control observations forwarded to the host's account and progress plumbing.
#[derive(Debug, PartialEq)]
pub enum ControlEvent {
    Account(bridge::Account),
    Connect(bridge::ConnectProgress),
    RequestRefused(RequestRefusal),
    Keepalive,
}

/// One controller and its transport, with no retained gameplay on reconnect.
pub struct Session<T: RelayTransport> {
    transport: Option<T>,
    control: Option<Framed<T::Stream, BridgeCodec>>,
    state: State,
    bearers: [Option<(String, Duration)>; 2],
    desired: Option<AccountOperation>,
    resumed_account: bool,
    attempts: u32,
    retry_at: Duration,
    join: u64,
    refusal: Option<RefusalCode>,
    pub unknown_events: u64,
}
impl<T: RelayTransport> Session<T> {
    /// Creates an offline controller; it owns and closes its transport.
    pub fn new() -> Self {
        Self {
            transport: None,
            control: None,
            state: State::Offline,
            bearers: [None, None],
            desired: None,
            resumed_account: false,
            attempts: 0,
            retry_at: Duration::ZERO,
            join: 0,
            refusal: None,
            unknown_events: 0,
        }
    }
    /// Returns the newest unexpired capability without renewing its expiry.
    pub fn resume(&self, now: Duration) -> &str {
        self.bearers
            .iter()
            .flatten()
            .find(|(_, expires)| now < *expires)
            .map_or("", |(token, _)| token)
    }
    /// Replaces offline account intent, so obsolete sign-in/out requests cannot accumulate.
    pub fn desire_account(&mut self, operation: AccountOperation) {
        self.desired = Some(operation);
    }
    /// Claims a due reconnect; callers replace the failed transport before handshaking.
    pub fn retry(&mut self, now: Duration, jitter: Duration) -> bool {
        if self.state != State::Offline
            || now < self.retry_at
            || self.attempts == RECONNECT_ATTEMPTS
        {
            return false;
        }
        self.retry_at = now
            .saturating_add(Duration::from_secs(1 << self.attempts.min(4)))
            .saturating_add(jitter.min(Duration::from_millis(500)));
        self.attempts += 1;
        self.state = State::Connecting;
        true
    }
    /// Observes the next allowed attempt without creating a transport or consuming a retry.
    pub fn next_retry(&self) -> Result<Duration, CloseReason> {
        match self.state {
            State::Offline if self.attempts < RECONNECT_ATTEMPTS => Ok(self.retry_at),
            State::Offline => Err(CloseReason::RetryExhausted),
            State::Refused(_) => Err(CloseReason::Refused),
            State::Closed => Err(CloseReason::Cancelled),
            _ => Err(CloseReason::Protocol),
        }
    }

    /// Returns a safe application refusal independently of transport failure.
    pub fn refusal(&self) -> Option<RefusalCode> {
        self.refusal
    }

    /// Starts a deliberate new account session after refusal or expiry, discarding old intent.
    pub fn fresh_session(&mut self) {
        *self = Self::new();
    }

    /// Restarts an explicit transport/admission retry without bypassing a resume refusal.
    pub fn restart_retries(&mut self) {
        if matches!(
            self.state,
            State::Offline
                | State::Refused(RefusalCode::AdmissionRefused | RefusalCode::SessionBusy)
        ) {
            self.state = State::Offline;
            self.refusal = None;
            self.attempts = 0;
            self.retry_at = Duration::ZERO;
        }
    }

    /// Replaces a failed candidate while retaining capability expiry and latest account intent.
    pub(crate) fn replace_transport(&mut self, transport: T) {
        if let Some(old) = &mut self.transport {
            old.close(CloseReason::Transport);
        }
        self.control = None;
        self.transport = Some(transport);
    }
    /// Performs the shared control handshake with a host clock callback and deadline.
    pub(crate) async fn handshake(
        &mut self,
        now: &mut impl FnMut() -> Duration,
    ) -> Result<(), CloseReason> {
        if !matches!(self.state, State::Offline | State::Connecting) {
            return Err(CloseReason::Protocol);
        }
        self.state = State::Connecting;
        let resume = self.resume(now()).to_owned();
        if resume.is_empty() && self.bearers.iter().any(Option::is_some) {
            self.refusal = Some(RefusalCode::ResumeRefused);
            return Err(CloseReason::Refused);
        }
        let stream = self
            .transport
            .as_mut()
            .ok_or(CloseReason::Transport)?
            .open(Channel::Control)
            .await?;
        let mut control = Framed::new(stream, BridgeCodec::with_max(RELAY_CONTROL_BYTES));
        control
            .send(encode(&Hello::new(Channel::Control, &resume, 0))?)
            .await
            .map_err(|_| CloseReason::Transport)?;
        let frame = control
            .next()
            .await
            .ok_or(CloseReason::Transport)?
            .map_err(read_failure)?;
        let welcome = match parse(&frame)? {
            Event::Welcome { capability } => capability,
            Event::Error { error } => {
                self.refusal = Some(error);
                return Err(CloseReason::Refused);
            }
            _ => return Err(CloseReason::Protocol),
        };
        self.apply_capability(&welcome, now())?;
        self.state = State::Ready(welcome.generation);
        self.resumed_account = !resume.is_empty();
        self.control = Some(control);
        self.attempts = 0;
        Ok(())
    }
    /// Returns the current state without allowing callers to bypass transitions.
    pub fn state(&self) -> State {
        self.state
    }
    /// Reports whether a command still needs a cancellation-safe write boundary.
    pub fn has_account_intent(&self) -> bool {
        self.desired.is_some()
    }

    /// Whether the last successful control Hello resumed an existing relay account.
    pub fn resumed_account(&self) -> bool {
        self.resumed_account
    }
    /// The coalesced account operation still retained for the next flush.
    pub fn pending_account(&self) -> Option<AccountOperation> {
        self.desired
    }
    /// Flushes one coalesced account request; failure leaves its intent available for reconnect.
    pub async fn flush_account(&mut self) -> Result<(), CloseReason> {
        if let Some(operation) = self.desired {
            let op = match operation {
                AccountOperation::SignIn => Request::SignIn,
                AccountOperation::SignOut => Request::SignOut,
            };
            if let Err(reason) = self.send(&op).await {
                self.close(reason);
                return Err(reason);
            }
            self.desired = None;
        }
        Ok(())
    }
    /// Reads one control observation. Malformed framing/JSON closes the generation; unknown
    /// well-formed optional events are counted and skipped. The host clock is sampled after receipt.
    pub async fn next_event(
        &mut self,
        mut now: impl FnMut() -> Duration,
    ) -> Result<ControlEvent, CloseReason> {
        loop {
            match self.read_event(&mut now).await {
                Ok(Some(event)) => return Ok(event),
                Ok(None) => continue,
                Err(reason) => {
                    self.close(reason);
                    return Err(reason);
                }
            }
        }
    }

    /// Applies an event and echoes the exact capability from each keepalive.
    async fn read_event(
        &mut self,
        now: &mut impl FnMut() -> Duration,
    ) -> Result<Option<ControlEvent>, CloseReason> {
        let frame = self
            .control
            .as_mut()
            .ok_or(CloseReason::Transport)?
            .next()
            .await
            .ok_or(CloseReason::Transport)?
            .map_err(read_failure)?;
        match parse(&frame)? {
            Event::Ping { capability } => {
                let active = match self.state {
                    State::Ready(g) | State::Playing(g) => g,
                    _ => return Err(CloseReason::Protocol),
                };
                if capability.generation != active {
                    return Err(CloseReason::Protocol);
                }
                self.apply_capability(&capability, now())?;
                self.send(&Request::Pong {
                    resume: capability.resume,
                })
                .await?;
                Ok(Some(ControlEvent::Keepalive))
            }
            Event::Account { account } => Ok(Some(ControlEvent::Account(account))),
            Event::Connect { connect } => Ok(Some(ControlEvent::Connect(connect))),
            Event::Error { error } => Ok(Some(ControlEvent::RequestRefused(match error {
                // Only sign-in consumes admission within an established control session.
                RefusalCode::AdmissionRefused => RequestRefusal::RetrySignIn,
                code => RequestRefusal::Refused(code),
            }))),
            Event::Unknown => {
                self.unknown_events += 1;
                tracing::debug!(
                    unknown_events = self.unknown_events,
                    "skipping an optional relay event"
                );
                Ok(None)
            }
            Event::Welcome { .. } => Err(CloseReason::Protocol),
        }
    }
    /// Opens play and writes Hello within the host's deadline, then returns the bounded bridge
    /// reader/queue/writer. Expiry closes the transport, cancelling stream-credit waits too.
    /// The host spawns the writer locally and drops it on cancellation.
    pub async fn open_play<N: FnMut() -> Duration, D: std::future::Future<Output = ()>>(
        &mut self,
        mut now: N,
        expires: D,
    ) -> Result<
        (
            bridge::FramedReader<tokio::io::ReadHalf<T::Stream>>,
            bridge::FrameQueue,
            std::pin::Pin<Box<dyn std::future::Future<Output = ()>>>,
        ),
        CloseReason,
    >
    where
        T::Stream: 'static,
    {
        let State::Ready(generation) = self.state else {
            return Err(CloseReason::Protocol);
        };
        self.join = self.join.checked_add(1).ok_or(CloseReason::Protocol)?;
        self.state = State::Playing(generation);
        let result = bridge::tasks::until(
            async {
                let channel = Channel::Play { join: self.join };
                let stream = self
                    .transport
                    .as_mut()
                    .ok_or(CloseReason::Transport)?
                    .open(channel)
                    .await?;
                let mut play = Framed::new(stream, BridgeCodec::with_max(RELAY_CONTROL_BYTES));
                play.send(encode(&Hello::new(
                    channel,
                    self.resume(now()),
                    generation,
                ))?)
                .await
                .map_err(|_| CloseReason::Transport)?;
                let (read, write) = tokio::io::split(play.into_inner());
                let (reader, frames, writer) = bridge::tasks::prepare_bounded_connection(
                    read,
                    write,
                    MAX_FRAME_LEN,
                    RELAY_QUEUED_FRAME_BYTES,
                );
                Ok((
                    reader,
                    frames,
                    Box::pin(writer) as std::pin::Pin<Box<dyn std::future::Future<Output = ()>>>,
                ))
            },
            expires,
        )
        .await
        .unwrap_or(Err(CloseReason::Transport));
        if let Err(reason) = &result {
            self.close(*reason);
        }
        result
    }
    /// Returns the identity expected in this play's pack stream and handoff.
    pub fn play_join(&self) -> Option<u64> {
        matches!(self.state, State::Playing(_)).then_some(self.join)
    }

    /// Obtains a receive future without borrowing control, so keepalives remain independent.
    pub fn pack_stream(
        &self,
    ) -> impl std::future::Future<Output = Result<Option<T::Stream>, CloseReason>> + use<T> {
        let source = self
            .transport
            .as_ref()
            .filter(|_| self.play_join().is_some())
            .map(RelayTransport::accept_packs);
        async move { source.ok_or(CloseReason::Protocol)?.await }
    }

    /// Ends exactly the current join; later joins never inherit archive handles or observations.
    pub fn finish_play(&mut self, join: u64) -> Result<(), CloseReason> {
        let State::Playing(generation) = self.state else {
            return Err(CloseReason::Protocol);
        };
        if join != self.join {
            return Err(CloseReason::Protocol);
        }
        if let Some(transport) = &mut self.transport {
            transport.finish_play();
        }
        self.state = State::Ready(generation);
        Ok(())
    }

    /// Closes the controller and its play channels immediately; deliberate cancellation is final.
    pub fn close(&mut self, reason: CloseReason) {
        if let Some(mut transport) = self.transport.take() {
            transport.close(reason);
        }
        self.control = None;
        if reason == CloseReason::Refused && self.refusal == Some(RefusalCode::ResumeRefused) {
            self.bearers = [None, None];
        }
        self.state = match (self.state, reason) {
            (State::Closed, _) | (_, CloseReason::Cancelled) => State::Closed,
            (State::Refused(code), _) => State::Refused(code),
            (_, CloseReason::Refused) => {
                State::Refused(self.refusal.unwrap_or(RefusalCode::Refused))
            }
            _ => State::Offline,
        };
    }
    /// Writes a bounded control request through the same codec as Hello and gameplay.
    async fn send(&mut self, request: &Request) -> Result<(), CloseReason> {
        self.control
            .as_mut()
            .ok_or(CloseReason::Transport)?
            .send(encode(request)?)
            .await
            .map_err(|_| CloseReason::Transport)
    }
    /// Rotates only on a newly observed token; a delayed old observation cannot renew or replace it.
    fn apply_capability(&mut self, event: &Capability, now: Duration) -> Result<(), CloseReason> {
        if event.version != RELAY_VERSION || event.generation == 0 {
            return Err(CloseReason::Protocol);
        }
        let token = event.resume.as_str();
        if token.is_empty() {
            return Err(CloseReason::Protocol);
        }
        let seconds = event.expires_seconds;
        if !self
            .bearers
            .iter()
            .flatten()
            .any(|(known, _)| known == token)
        {
            self.bearers.swap(0, 1);
            self.bearers[0] = Some((
                token.to_owned(),
                now.checked_add(Duration::from_secs(seconds))
                    .ok_or(CloseReason::Protocol)?,
            ));
        }
        Ok(())
    }
}
impl<T: RelayTransport> Default for Session<T> {
    fn default() -> Self {
        Self::new()
    }
}
impl<T: RelayTransport> Drop for Session<T> {
    fn drop(&mut self) {
        self.close(CloseReason::Cancelled);
    }
}
/// Serializes only typed bounded requests and prefixes; the codec enforces the wire byte limit.
fn encode(value: &impl serde::Serialize) -> Result<Bytes, CloseReason> {
    serde_json::to_vec(value)
        .map(Bytes::from)
        .map_err(|_| CloseReason::Protocol)
}

/// Distinguishes a lost byte stream from framing that violates the control contract.
fn read_failure(error: bridge::BridgeError) -> CloseReason {
    if matches!(error, bridge::BridgeError::Io(_)) {
        CloseReason::Transport
    } else {
        CloseReason::Protocol
    }
}
