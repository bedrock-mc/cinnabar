//! Offline -> connecting -> ready -> playing; closure returns to offline, deliberate close
//! is terminal. Reconnect never replays play bytes. Current and one previous bearer keep
//! their own expiries; only a
//! received bearer is echoed in pong. Offline account intent occupies one slot. Caller time
//! drives six bounded retries with capped backoff and caller-supplied jitter.

use crate::{Channel, CloseReason, RelayTransport};
use bridge::{
    BridgeCodec,
    contract::{
        MAX_FRAME_LEN, RELAY_CHANNEL_CONTROL, RELAY_CHANNEL_PLAY, RELAY_CONTROL_BYTES,
        RELAY_EVENT_ACCOUNT, RELAY_EVENT_CONNECT, RELAY_EVENT_ERROR, RELAY_EVENT_PING,
        RELAY_EVENT_WELCOME, RELAY_OP_PONG, RELAY_OP_SIGN_IN, RELAY_OP_SIGN_OUT,
        RELAY_PLAY_MESSAGE_BYTES, RELAY_VERSION,
    },
};
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
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
    Closed,
}
/// Latest desired account operation, coalesced while offline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountOperation {
    SignIn,
    SignOut,
}
/// Control observations forwarded to the host's account and progress plumbing.
#[derive(Debug, PartialEq)]
pub enum ControlEvent {
    Account(Value),
    Connect(Value),
    Error(String),
    Keepalive,
}

/// One controller and its transport, with no retained gameplay on reconnect.
pub struct Session<T: RelayTransport> {
    transport: Option<T>,
    control: Option<Framed<T::Stream, BridgeCodec>>,
    state: State,
    bearers: [Option<(String, Duration)>; 2],
    desired: Option<AccountOperation>,
    attempts: u32,
    retry_at: Duration,
    join: u64,
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
            attempts: 0,
            retry_at: Duration::ZERO,
            join: 0,
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
            + Duration::from_secs(1 << self.attempts.min(4))
            + jitter.min(Duration::from_millis(500));
        self.attempts += 1;
        self.state = State::Connecting;
        true
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
        let stream = self
            .transport
            .as_mut()
            .ok_or(CloseReason::Transport)?
            .open(Channel::Control)
            .await?;
        let mut control = Framed::new(stream, BridgeCodec::with_max(RELAY_CONTROL_BYTES));
        control
            .send(Bytes::from(hello(Channel::Control, &resume, 0).to_string()))
            .await
            .map_err(|_| CloseReason::Transport)?;
        let frame = control
            .next()
            .await
            .ok_or(CloseReason::Transport)?
            .map_err(|_| CloseReason::Protocol)?;
        let welcome = parse(&frame)?;
        if welcome["type"] == RELAY_EVENT_ERROR {
            return Err(CloseReason::Refused);
        }
        if welcome["type"] != RELAY_EVENT_WELCOME {
            return Err(CloseReason::Protocol);
        }
        self.control = Some(control);
        self.apply_capability(&welcome, now())?;
        self.state = State::Ready(generation(&welcome)?);
        self.attempts = 0;
        Ok(())
    }
    /// Returns the current state without allowing callers to bypass transitions.
    pub fn state(&self) -> State {
        self.state
    }
    /// Flushes one coalesced account request; failure leaves its intent available for reconnect.
    pub async fn flush_account(&mut self) -> Result<(), CloseReason> {
        if let Some(operation) = self.desired {
            let op = match operation {
                AccountOperation::SignIn => RELAY_OP_SIGN_IN,
                AccountOperation::SignOut => RELAY_OP_SIGN_OUT,
            };
            if let Err(reason) = self.send(json!({"op": op})).await {
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
            .map_err(|_| CloseReason::Protocol)?;
        let event = parse(&frame)?;
        match event["type"].as_str().ok_or(CloseReason::Protocol)? {
            RELAY_EVENT_PING => {
                let active = match self.state {
                    State::Ready(g) | State::Playing(g) => g,
                    _ => return Err(CloseReason::Protocol),
                };
                if generation(&event)? != active {
                    return Err(CloseReason::Protocol);
                }
                self.apply_capability(&event, now())?;
                self.send(json!({"op": RELAY_OP_PONG, "resume": event["resume"]}))
                    .await?;
                Ok(Some(ControlEvent::Keepalive))
            }
            RELAY_EVENT_ACCOUNT | RELAY_EVENT_CONNECT => {
                let account = event["type"] == RELAY_EVENT_ACCOUNT;
                let value = &event[if account { "account" } else { "connect" }];
                if !value.is_object() {
                    return Err(CloseReason::Protocol);
                }
                Ok(Some(if account {
                    ControlEvent::Account(value.clone())
                } else {
                    ControlEvent::Connect(value.clone())
                }))
            }
            RELAY_EVENT_ERROR => Ok(Some(ControlEvent::Error(
                event["error"]
                    .as_str()
                    .ok_or(CloseReason::Protocol)?
                    .to_owned(),
            ))),
            _ => {
                self.unknown_events += 1;
                tracing::debug!(
                    unknown_events = self.unknown_events,
                    "skipping an optional relay event"
                );
                Ok(None)
            }
        }
    }
    /// Opens play once, writes the relay prefix, then returns the existing bounded bridge
    /// reader/queue/writer. The host spawns the writer locally and drops it on cancellation.
    pub async fn open_play<N: FnMut() -> Duration>(
        &mut self,
        mut now: N,
    ) -> Result<
        (
            bridge::FramedReader<tokio::io::ReadHalf<T::Stream>>,
            bridge::FrameQueue,
            impl std::future::Future<Output = ()> + use<T, N>,
        ),
        CloseReason,
    > {
        let State::Ready(generation) = self.state else {
            return Err(CloseReason::Protocol);
        };
        self.join = self.join.checked_add(1).ok_or(CloseReason::Protocol)?;
        self.state = State::Playing(generation);
        let result = async {
            let channel = Channel::Play { join: self.join };
            let stream = self
                .transport
                .as_mut()
                .ok_or(CloseReason::Transport)?
                .open(channel)
                .await?;
            let mut play = Framed::new(stream, BridgeCodec::with_max(RELAY_CONTROL_BYTES));
            let prefix = hello(channel, self.resume(now()), generation);
            play.send(Bytes::from(prefix.to_string()))
                .await
                .map_err(|_| CloseReason::Transport)?;
            let (read, write) = tokio::io::split(play.into_inner());
            Ok(bridge::tasks::prepare_bounded_connection(
                read,
                write,
                MAX_FRAME_LEN,
                RELAY_PLAY_MESSAGE_BYTES,
            ))
        }
        .await;
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
        if reason == CloseReason::Refused {
            self.bearers = [None, None];
        }
        self.state = if self.state == State::Closed || reason == CloseReason::Cancelled {
            State::Closed
        } else {
            State::Offline
        };
    }
    /// Writes a bounded control request through the same codec as Hello and gameplay.
    async fn send(&mut self, request: Value) -> Result<(), CloseReason> {
        self.control
            .as_mut()
            .ok_or(CloseReason::Transport)?
            .send(Bytes::from(request.to_string()))
            .await
            .map_err(|_| CloseReason::Transport)
    }
    /// Rotates only on a newly observed token; a delayed old observation cannot renew or replace it.
    fn apply_capability(&mut self, event: &Value, now: Duration) -> Result<(), CloseReason> {
        if event["version"] != RELAY_VERSION {
            return Err(CloseReason::Protocol);
        }
        let token = event["resume"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or(CloseReason::Protocol)?;
        let seconds = event["expires_seconds"]
            .as_u64()
            .ok_or(CloseReason::Protocol)?;
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
/// Builds both channel prefixes from the shared contract.
fn hello(channel: Channel, resume: &str, generation: u64) -> Value {
    let mut prefix = json!({"version": RELAY_VERSION, "channel": channel.name(), "channels": [RELAY_CHANNEL_CONTROL, RELAY_CHANNEL_PLAY], "resume": resume, "generation": generation});
    if let Channel::Play { join } = channel {
        prefix["join"] = join.into();
    }
    prefix
}
/// Admits an object with a typed event name; extension fields remain forward compatible.
fn parse(frame: &[u8]) -> Result<Value, CloseReason> {
    let value: Value = serde_json::from_slice(frame).map_err(|_| CloseReason::Protocol)?;
    if !value["type"].is_string() {
        return Err(CloseReason::Protocol);
    }
    Ok(value)
}
/// Requires a nonzero play generation in a Welcome or ping.
fn generation(event: &Value) -> Result<u64, CloseReason> {
    event["generation"]
        .as_u64()
        .filter(|g| *g != 0)
        .ok_or(CloseReason::Protocol)
}
