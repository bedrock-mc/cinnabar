use super::*;
use bridge::contract::{RELAY_VERSION, RefusalCode};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::VecDeque,
    io,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use web_transport::Channel;

#[derive(Default)]
struct Observations {
    account: RefCell<RelayAccount>,
    ready: Rc<Cell<bool>>,
    now: Cell<Duration>,
    opens: Cell<usize>,
    closes: Cell<usize>,
    refused: Cell<bool>,
    live: Cell<bool>,
    hold_welcome: Cell<bool>,
    hold_writes: Cell<bool>,
    failed: Cell<bool>,
    cancelled_timers: Cell<usize>,
    output: RefCell<Vec<u8>>,
    incoming: RefCell<VecDeque<u8>>,
    events: RefCell<Vec<Value>>,
}
struct FakeHost(Rc<Observations>);
struct Transport(Rc<Observations>);
struct Stream {
    host: Rc<Observations>,
    input: VecDeque<u8>,
    control: bool,
}

impl AsyncRead for Stream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if !self.input.is_empty() && self.host.hold_welcome.get() {
            return Poll::Pending;
        }
        if self.control && self.input.is_empty() {
            let incoming: VecDeque<_> = self.host.incoming.borrow_mut().drain(..).collect();
            self.input.extend(incoming);
        }
        if self.input.is_empty() && self.host.live.get() {
            return Poll::Pending;
        }
        while output.remaining() > 0 {
            let Some(byte) = self.input.pop_front() else {
                break;
            };
            output.put_slice(&[byte]);
        }
        Poll::Ready(Ok(()))
    }
}
impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.host.hold_writes.get() {
            return Poll::Pending;
        }
        self.host.output.borrow_mut().extend_from_slice(data);
        Poll::Ready(Ok(data.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
impl RelayTransport for Transport {
    type Stream = Stream;
    async fn open(&mut self, channel: Channel) -> Result<Stream, CloseReason> {
        self.0.opens.set(self.0.opens.get() + 1);
        if self.0.failed.get() {
            return Err(CloseReason::Transport);
        }
        let event = if self.0.refused.get() {
            json!({"type":"error","error": RefusalCode::ResumeRefused})
        } else {
            json!({"type":"welcome", "version":RELAY_VERSION, "resume":"retained", "generation": self.0.opens.get(), "expires_seconds":120})
        };
        let frame = serde_json::to_vec(&event).unwrap();
        let mut input = VecDeque::from((frame.len() as u32).to_be_bytes().to_vec());
        input.extend(frame);
        Ok(Stream {
            host: Rc::clone(&self.0),
            input,
            control: matches!(channel, Channel::Control),
        })
    }
    fn close(&mut self, _: CloseReason) {
        self.0.closes.set(self.0.closes.get() + 1);
    }
}

/// A manually advanced timer with observable cancellation, without sleeping or spawning.
struct Wait {
    host: Rc<Observations>,
    due: Duration,
    polled: bool,
}
impl Future for Wait {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        self.polled = true;
        if self.host.now.get() >= self.due {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}
impl Drop for Wait {
    fn drop(&mut self) {
        if self.polled && self.host.now.get() < self.due {
            self.host
                .cancelled_timers
                .set(self.host.cancelled_timers.get() + 1);
        }
    }
}
impl Host for FakeHost {
    type Transport = Transport;
    fn now(&self) -> Duration {
        self.0.now.get()
    }
    fn jitter(&self) -> Duration {
        Duration::from_millis(250)
    }
    fn sleep(&self, duration: Duration) -> impl Future<Output = ()> {
        Wait {
            host: Rc::clone(&self.0),
            due: self.now() + duration,
            polled: false,
        }
    }
    fn transports(&self) -> Result<(Option<Transport>, Transport), CloseReason> {
        Ok((None, Transport(Rc::clone(&self.0))))
    }
    fn observe(&self, event: Value) {
        self.0.events.borrow_mut().push(event);
    }
    fn account(&self) -> &RefCell<RelayAccount> {
        &self.0.account
    }
}

/// Reads complete client Hello frames so reconnect assertions cover the real wire output.
fn requests(host: &Observations) -> Vec<Value> {
    let output = host.output.borrow();
    let mut bytes = output.as_slice();
    let mut requests = Vec::new();
    while !bytes.is_empty() {
        let len = u32::from_be_bytes(bytes[..4].try_into().unwrap()) as usize;
        requests.push(serde_json::from_slice(&bytes[4..4 + len]).unwrap());
        bytes = &bytes[4 + len..];
    }
    requests
}

/// Delivers one relay event through the production framing and typed decoder.
fn deliver(host: &Observations, event: Value) {
    let frame = serde_json::to_vec(&event).unwrap();
    host.incoming
        .borrow_mut()
        .extend((frame.len() as u32).to_be_bytes());
    host.incoming.borrow_mut().extend(frame);
}

#[test]
fn menu_tracks_typed_events_across_suspend_and_resume() {
    use ::launcher::menu::auth::AuthState;
    let host = Rc::new(Observations::default());
    host.live.set(true);
    host.hold_welcome.set(true);
    let (mut commands, pending) = mpsc::channel(4);
    let (mut lifecycle, changed) = mpsc::channel(1);
    let refresh = Rc::new(Intent::default());
    let paused = Rc::new(Cell::new(false));
    let (cancel, cancelled) = oneshot::channel();
    let (_joins, join_requests) = mpsc::channel(1);
    let mut task = Box::pin(run(
        FakeHost(Rc::clone(&host)),
        pending,
        join_requests,
        changed,
        Rc::clone(&paused),
        Rc::clone(&refresh),
        cancelled,
    ));
    let mut cx = Context::from_waker(std::task::Waker::noop());
    assert!(task.as_mut().poll(&mut cx).is_pending());
    commands
        .try_send(refresh.account(AccountOperation::SignIn))
        .unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    host.hold_welcome.set(false);
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert_eq!(host.account.borrow().state(), AuthState::Checking);
    deliver(
        &host,
        json!({"type":"account", "account":{"state":"awaiting_code", "verification_uri":"https://example.invalid/device", "user_code":"TEST-CODE"}}),
    );
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert!(matches!(
        host.account.borrow().state(),
        AuthState::AwaitingCode { .. }
    ));
    deliver(&host, json!({"type":"error", "error":"admission_refused"}));
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert!(
        host.account
            .borrow()
            .operation_error()
            .unwrap()
            .contains("Use Sign in to retry")
    );
    commands
        .try_send(refresh.account(AccountOperation::SignIn))
        .unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert!(host.account.borrow().operation_error().is_none());
    deliver(
        &host,
        json!({"type":"connect", "connect":{"stage":"packs", "packs_done":1, "packs_total":2}}),
    );
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert!(host.account.borrow().join_stage().is_some());
    deliver(&host, json!({"type":"connect", "connect":{"stage":""}}));
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert!(host.account.borrow().join_stage().is_none());
    deliver(
        &host,
        json!({"type":"account", "account":{"state":"signed_in", "gamertag":"Fixture"}}),
    );
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert_eq!(host.account.borrow().state(), AuthState::Authenticated);
    paused.set(true);
    lifecycle.try_send(()).unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert!(host.account.borrow().profile().gamertag.is_empty());
    host.now.set(Duration::from_secs(2));
    paused.set(false);
    lifecycle.try_send(()).unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert_eq!(requests(&host).last().unwrap()["resume"], "retained");
    deliver(
        &host,
        json!({"type":"account", "account":{"state":"signed_in", "gamertag":"Fixture"}}),
    );
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert_eq!(host.account.borrow().state(), AuthState::Authenticated);
    cancel.send(()).unwrap();
    assert!(task.as_mut().poll(&mut cx).is_ready());
    assert!(host.account.borrow().profile().gamertag.is_empty());
}

#[test]
fn menu_fresh_session_restores_only_the_current_generation_command() {
    use ::launcher::menu::auth::AuthState;
    let host = Rc::new(Observations::default());
    host.live.set(true);
    let (mut commands, pending) = mpsc::channel(4);
    let (mut lifecycle, changed) = mpsc::channel(1);
    let refresh = Rc::new(Intent::default());
    let (cancel, cancelled) = oneshot::channel();
    let (_joins, join_requests) = mpsc::channel(1);
    let mut task = Box::pin(run(
        FakeHost(Rc::clone(&host)),
        pending,
        join_requests,
        changed,
        Rc::new(Cell::new(false)),
        Rc::clone(&refresh),
        cancelled,
    ));
    let mut cx = Context::from_waker(std::task::Waker::noop());
    assert!(task.as_mut().poll(&mut cx).is_pending());
    commands
        .try_send(refresh.account(AccountOperation::SignIn))
        .unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    host.hold_writes.set(true);
    commands
        .try_send(refresh.account(AccountOperation::SignOut))
        .unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    commands
        .try_send(refresh.account(AccountOperation::SignOut))
        .unwrap();
    refresh.request(Refresh::Fresh);
    commands
        .try_send(refresh.account(AccountOperation::SignIn))
        .unwrap();
    lifecycle.try_send(()).unwrap();
    host.hold_writes.set(false);
    assert!(task.as_mut().poll(&mut cx).is_pending());
    let written = requests(&host);
    assert_eq!(written.len(), 4);
    assert!(written[2].get("resume").is_none());
    assert_eq!(written[3]["op"], "sign_in");
    assert_eq!(host.account.borrow().state(), AuthState::Checking);
    host.account
        .borrow_mut()
        .request(::launcher::menu::auth::AccountIntent::Cancel);
    assert_eq!(
        host.account.borrow_mut().take_operation(),
        Some(AccountOperation::SignOut)
    );
    cancel.send(()).unwrap();
    assert!(task.as_mut().poll(&mut cx).is_ready());
}

#[test]
fn driver_reconnects_with_the_same_bearer_and_cancels_backoff_on_navigation() {
    let host = Rc::new(Observations::default());
    let (_commands, pending) = mpsc::channel(4);
    let (mut lifecycle, changed) = mpsc::channel(1);
    let paused = Rc::new(Cell::new(false));
    let refresh = Rc::new(Intent::default());
    let (cancel, cancelled) = oneshot::channel();
    let (_joins, join_requests) = mpsc::channel(1);
    let task = run(
        FakeHost(Rc::clone(&host)),
        pending,
        join_requests,
        changed,
        Rc::clone(&paused),
        refresh,
        cancelled,
    );
    let mut task = Box::pin(task);
    let mut cx = Context::from_waker(std::task::Waker::noop());
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert_eq!(host.opens.get(), 1);
    host.now.set(Duration::from_millis(1249));
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert_eq!(host.opens.get(), 1);
    host.now.set(Duration::from_millis(1250));
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert_eq!(host.opens.get(), 2);
    let hello = requests(&host);
    assert!(hello[0].get("resume").is_none());
    assert_eq!(hello[1]["resume"], "retained");
    paused.set(true);
    lifecycle.try_send(()).unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert_eq!(host.cancelled_timers.get(), 1);
    host.now.set(Duration::from_secs(50));
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert_eq!(host.opens.get(), 2);
    paused.set(false);
    lifecycle.try_send(()).unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert_eq!(host.opens.get(), 3);
    cancel.send(()).unwrap();
    assert!(task.as_mut().poll(&mut cx).is_ready());
    assert_eq!(host.cancelled_timers.get(), 2);
}

#[test]
fn refusal_requires_an_explicit_fresh_session() {
    let host = Rc::new(Observations::default());
    host.refused.set(true);
    let (_commands, pending) = mpsc::channel(4);
    let (mut lifecycle, changed) = mpsc::channel(1);
    let refresh = Rc::new(Intent::default());
    let (cancel, cancelled) = oneshot::channel();
    let (_joins, join_requests) = mpsc::channel(1);
    let task = run(
        FakeHost(Rc::clone(&host)),
        pending,
        join_requests,
        changed,
        Rc::new(Cell::new(false)),
        Rc::clone(&refresh),
        cancelled,
    );
    let mut task = Box::pin(task);
    let mut cx = Context::from_waker(std::task::Waker::noop());
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert_eq!(host.opens.get(), 1);
    assert!(
        host.events
            .borrow()
            .iter()
            .any(|event| event["code"] == "resume_refused")
    );
    refresh.request(Refresh::Reconnect);
    lifecycle.try_send(()).unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert_eq!(host.opens.get(), 1);
    host.refused.set(false);
    refresh.request(Refresh::Fresh);
    lifecycle.try_send(()).unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert_eq!(host.opens.get(), 2);
    assert!(requests(&host)[1].get("resume").is_none());
    cancel.send(()).unwrap();
    assert!(task.as_mut().poll(&mut cx).is_ready());
}

#[test]
fn account_writes_leave_the_command_lane_available_and_sign_out_stops_retries() {
    let host = Rc::new(Observations::default());
    host.live.set(true);
    let (mut commands, pending) = mpsc::channel(4);
    let refresh = Rc::new(Intent::default());
    let (_lifecycle, changed) = mpsc::channel(1);
    let (cancel, cancelled) = oneshot::channel();
    let (_joins, join_requests) = mpsc::channel(1);
    let task = run(
        FakeHost(Rc::clone(&host)),
        pending,
        join_requests,
        changed,
        Rc::new(Cell::new(false)),
        Rc::clone(&refresh),
        cancelled,
    );
    let mut task = Box::pin(task);
    let mut cx = Context::from_waker(std::task::Waker::noop());
    assert!(task.as_mut().poll(&mut cx).is_pending());
    commands
        .try_send(refresh.account(AccountOperation::SignIn))
        .unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    commands
        .try_send(refresh.account(AccountOperation::SignOut))
        .unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    let written = requests(&host);
    assert_eq!(written[1]["op"], "sign_in");
    assert_eq!(written[2]["op"], "sign_out");
    host.live.set(false);
    assert!(task.as_mut().poll(&mut cx).is_pending());
    host.now.set(Duration::from_secs(50));
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert_eq!(host.opens.get(), 1);
    cancel.send(()).unwrap();
    assert!(task.as_mut().poll(&mut cx).is_ready());
}

#[test]
fn account_commands_during_setup_preserve_the_handshake_and_coalesce() {
    for restored in [false, true] {
        for operation in [AccountOperation::SignIn, AccountOperation::SignOut] {
            let host = Rc::new(Observations::default());
            host.live.set(true);
            host.hold_welcome.set(!restored);
            let (mut commands, pending) = mpsc::channel(4);
            let refresh = Rc::new(Intent::default());
            let (mut lifecycle, changed) = mpsc::channel(1);
            let paused = Rc::new(Cell::new(false));
            let (cancel, cancelled) = oneshot::channel();
            let (_joins, join_requests) = mpsc::channel(1);
            let mut task = Box::pin(run(
                FakeHost(Rc::clone(&host)),
                pending,
                join_requests,
                changed,
                Rc::clone(&paused),
                Rc::clone(&refresh),
                cancelled,
            ));
            let mut cx = Context::from_waker(std::task::Waker::noop());
            assert!(task.as_mut().poll(&mut cx).is_pending());
            if restored {
                paused.set(true);
                lifecycle.try_send(()).unwrap();
                assert!(task.as_mut().poll(&mut cx).is_pending());
                host.hold_welcome.set(true);
                host.now.set(Duration::from_secs(2));
                paused.set(false);
                lifecycle.try_send(()).unwrap();
                assert!(task.as_mut().poll(&mut cx).is_pending());
                assert_eq!(requests(&host)[1]["resume"], "retained");
            }
            let opens = host.opens.get();
            let closes = host.closes.get();
            for next in [
                AccountOperation::SignIn,
                AccountOperation::SignOut,
                operation,
            ] {
                commands.try_send(refresh.account(next)).unwrap();
                assert!(task.as_mut().poll(&mut cx).is_pending());
                assert_eq!(host.opens.get(), opens);
                assert_eq!(host.closes.get(), closes);
            }
            host.hold_welcome.set(false);
            host.now.set(Duration::from_secs(3));
            assert!(task.as_mut().poll(&mut cx).is_pending());
            let written = requests(&host);
            let expected = match operation {
                AccountOperation::SignIn => "sign_in",
                AccountOperation::SignOut => "sign_out",
            };
            assert_eq!(written.len(), opens + 1);
            assert_eq!(written.last().unwrap()["op"], expected);
            host.hold_welcome.set(true);
            host.live.set(false);
            assert!(task.as_mut().poll(&mut cx).is_pending());
            host.now.set(Duration::from_secs(6));
            assert!(task.as_mut().poll(&mut cx).is_pending());
            match operation {
                AccountOperation::SignIn => {
                    assert_eq!(host.opens.get(), opens + 1);
                    assert_eq!(requests(&host).last().unwrap()["resume"], "retained");
                }
                AccountOperation::SignOut => assert_eq!(host.opens.get(), opens),
            }
            cancel.send(()).unwrap();
            assert!(task.as_mut().poll(&mut cx).is_ready());
        }
    }
}

#[test]
fn explicit_reconnect_retains_the_current_controller() {
    for handshaking in [false, true] {
        let host = Rc::new(Observations::default());
        host.live.set(true);
        host.hold_welcome.set(handshaking);
        let (mut commands, pending) = mpsc::channel(4);
        let (mut lifecycle, changed) = mpsc::channel(1);
        let refresh = Rc::new(Intent::default());
        let (cancel, cancelled) = oneshot::channel();
        let (_joins, join_requests) = mpsc::channel(1);
        let mut task = Box::pin(run(
            FakeHost(Rc::clone(&host)),
            pending,
            join_requests,
            changed,
            Rc::new(Cell::new(false)),
            Rc::clone(&refresh),
            cancelled,
        ));
        let mut cx = Context::from_waker(std::task::Waker::noop());
        assert!(task.as_mut().poll(&mut cx).is_pending());
        for _ in 0..3 {
            refresh.request(Refresh::Reconnect);
            lifecycle.try_send(()).unwrap();
            lifecycle.try_send(()).unwrap();
            assert!(task.as_mut().poll(&mut cx).is_pending());
            assert_eq!(host.opens.get(), 1);
            assert_eq!(host.closes.get(), 0);
        }
        host.hold_welcome.set(false);
        commands
            .try_send(refresh.account(AccountOperation::SignIn))
            .unwrap();
        assert!(task.as_mut().poll(&mut cx).is_pending());
        assert_eq!(requests(&host)[1]["op"], "sign_in");
        cancel.send(()).unwrap();
        assert!(task.as_mut().poll(&mut cx).is_ready());
        assert_eq!(host.closes.get(), 1);
    }
}

#[test]
fn fresh_session_discards_account_intent_queued_during_setup() {
    let host = Rc::new(Observations::default());
    host.live.set(true);
    host.hold_welcome.set(true);
    let (mut commands, pending) = mpsc::channel(4);
    let (mut lifecycle, changed) = mpsc::channel(1);
    let refresh = Rc::new(Intent::default());
    let (cancel, cancelled) = oneshot::channel();
    let (_joins, join_requests) = mpsc::channel(1);
    let mut task = Box::pin(run(
        FakeHost(Rc::clone(&host)),
        pending,
        join_requests,
        changed,
        Rc::new(Cell::new(false)),
        Rc::clone(&refresh),
        cancelled,
    ));
    let mut cx = Context::from_waker(std::task::Waker::noop());
    assert!(task.as_mut().poll(&mut cx).is_pending());
    commands
        .try_send(refresh.account(AccountOperation::SignOut))
        .unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    refresh.request(Refresh::Fresh);
    lifecycle.try_send(()).unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert_eq!(host.opens.get(), 2);
    assert_eq!(host.closes.get(), 1);
    host.hold_welcome.set(false);
    assert!(task.as_mut().poll(&mut cx).is_pending());
    let written = requests(&host);
    assert_eq!(written.len(), 2);
    assert!(
        written
            .iter()
            .all(|request| request.get("resume").is_none())
    );
    cancel.send(()).unwrap();
    assert!(task.as_mut().poll(&mut cx).is_ready());
}

#[test]
fn fresh_session_discards_account_commands_pending_behind_a_blocked_write() {
    for operation in [AccountOperation::SignIn, AccountOperation::SignOut] {
        let host = Rc::new(Observations::default());
        host.live.set(true);
        let (mut commands, pending) = mpsc::channel(4);
        let (mut lifecycle, changed) = mpsc::channel(1);
        let refresh = Rc::new(Intent::default());
        let (cancel, cancelled) = oneshot::channel();
        let (_joins, join_requests) = mpsc::channel(1);
        let mut task = Box::pin(run(
            FakeHost(Rc::clone(&host)),
            pending,
            join_requests,
            changed,
            Rc::new(Cell::new(false)),
            Rc::clone(&refresh),
            cancelled,
        ));
        let mut cx = Context::from_waker(std::task::Waker::noop());
        assert!(task.as_mut().poll(&mut cx).is_pending());
        host.hold_writes.set(true);
        commands
            .try_send(refresh.account(AccountOperation::SignIn))
            .unwrap();
        assert!(task.as_mut().poll(&mut cx).is_pending());
        commands.try_send(refresh.account(operation)).unwrap();
        assert!(task.as_mut().poll(&mut cx).is_pending());
        refresh.request(Refresh::Fresh);
        lifecycle.try_send(()).unwrap();
        assert!(task.as_mut().poll(&mut cx).is_pending());
        host.hold_writes.set(false);
        assert!(task.as_mut().poll(&mut cx).is_pending());
        let written = requests(&host);
        assert_eq!(written.len(), 2, "only the old and fresh Hello may be sent");
        assert!(
            written
                .iter()
                .all(|request| request.get("resume").is_none())
        );
        commands
            .try_send(refresh.account(AccountOperation::SignIn))
            .unwrap();
        assert!(task.as_mut().poll(&mut cx).is_pending());
        assert_eq!(requests(&host).last().unwrap()["op"], "sign_in");
        cancel.send(()).unwrap();
        assert!(task.as_mut().poll(&mut cx).is_ready());
    }
}

#[test]
fn fresh_session_preserves_only_intent_issued_after_the_latest_reset() {
    let host = Rc::new(Observations::default());
    host.live.set(true);
    host.hold_welcome.set(true);
    let (mut commands, pending) = mpsc::channel(4);
    let (mut lifecycle, changed) = mpsc::channel(1);
    let refresh = Rc::new(Intent::default());
    let (cancel, cancelled) = oneshot::channel();
    let (_joins, join_requests) = mpsc::channel(1);
    let mut task = Box::pin(run(
        FakeHost(Rc::clone(&host)),
        pending,
        join_requests,
        changed,
        Rc::new(Cell::new(false)),
        Rc::clone(&refresh),
        cancelled,
    ));
    let mut cx = Context::from_waker(std::task::Waker::noop());
    assert!(task.as_mut().poll(&mut cx).is_pending());
    commands
        .try_send(refresh.account(AccountOperation::SignIn))
        .unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    refresh.request(Refresh::Fresh);
    commands
        .try_send(refresh.account(AccountOperation::SignIn))
        .unwrap();
    refresh.request(Refresh::Fresh);
    commands
        .try_send(refresh.account(AccountOperation::SignOut))
        .unwrap();
    refresh.request(Refresh::Reconnect);
    lifecycle.try_send(()).unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    host.hold_welcome.set(false);
    assert!(task.as_mut().poll(&mut cx).is_pending());
    let written = requests(&host);
    assert_eq!(written.len(), 3);
    assert!(
        written[..2]
            .iter()
            .all(|request| request.get("resume").is_none())
    );
    assert_eq!(written[2]["op"], "sign_out");
    cancel.send(()).unwrap();
    assert!(task.as_mut().poll(&mut cx).is_ready());
}

#[test]
fn sign_in_admission_refusal_directs_a_working_request_retry() {
    let host = Rc::new(Observations::default());
    host.live.set(true);
    let (mut commands, pending) = mpsc::channel(4);
    let refresh = Rc::new(Intent::default());
    let (_lifecycle, changed) = mpsc::channel(1);
    let (cancel, cancelled) = oneshot::channel();
    let (_joins, join_requests) = mpsc::channel(1);
    let mut task = Box::pin(run(
        FakeHost(Rc::clone(&host)),
        pending,
        join_requests,
        changed,
        Rc::new(Cell::new(false)),
        Rc::clone(&refresh),
        cancelled,
    ));
    let mut cx = Context::from_waker(std::task::Waker::noop());
    assert!(task.as_mut().poll(&mut cx).is_pending());
    commands
        .try_send(refresh.account(AccountOperation::SignIn))
        .unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    let error = serde_json::to_vec(&json!({"type":"error", "error":"admission_refused"})).unwrap();
    host.incoming
        .borrow_mut()
        .extend((error.len() as u32).to_be_bytes());
    host.incoming.borrow_mut().extend(error);
    assert!(task.as_mut().poll(&mut cx).is_pending());
    let event = host.events.borrow().last().unwrap().clone();
    assert_eq!(event["code"], "admission_refused");
    assert!(
        event["error"]
            .as_str()
            .unwrap()
            .contains("Use Sign in to retry")
    );
    commands
        .try_send(refresh.account(AccountOperation::SignIn))
        .unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert_eq!(
        host.opens.get(),
        1,
        "request retry keeps the ready controller"
    );
    assert_eq!(
        requests(&host)
            .iter()
            .filter(|request| request["op"] == "sign_in")
            .count(),
        2
    );
    cancel.send(()).unwrap();
    assert!(task.as_mut().poll(&mut cx).is_ready());
}

#[test]
fn sign_out_queued_before_opening_is_sent_before_retries_stop() {
    let host = Rc::new(Observations::default());
    let (mut commands, pending) = mpsc::channel(4);
    let refresh = Rc::new(Intent::default());
    commands
        .try_send(refresh.account(AccountOperation::SignOut))
        .unwrap();
    let (_lifecycle, changed) = mpsc::channel(1);
    let (cancel, cancelled) = oneshot::channel();
    let (_joins, join_requests) = mpsc::channel(1);
    let mut task = Box::pin(run(
        FakeHost(Rc::clone(&host)),
        pending,
        join_requests,
        changed,
        Rc::new(Cell::new(false)),
        Rc::clone(&refresh),
        cancelled,
    ));
    let mut cx = Context::from_waker(std::task::Waker::noop());
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert_eq!(
        requests(&host).last().map(|r| r["op"].clone()),
        Some(json!("sign_out"))
    );
    host.now.set(Duration::from_secs(50));
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert_eq!(host.opens.get(), 1);
    cancel.send(()).unwrap();
    assert!(task.as_mut().poll(&mut cx).is_ready());
}

#[test]
fn repeated_transport_loss_stops_at_the_shared_retry_ceiling() {
    let host = Rc::new(Observations::default());
    host.failed.set(true);
    let (_commands, pending) = mpsc::channel(4);
    let (_lifecycle, changed) = mpsc::channel(1);
    let (cancel, cancelled) = oneshot::channel();
    let (_joins, join_requests) = mpsc::channel(1);
    let task = run(
        FakeHost(Rc::clone(&host)),
        pending,
        join_requests,
        changed,
        Rc::new(Cell::new(false)),
        Rc::new(Intent::default()),
        cancelled,
    );
    let mut task = Box::pin(task);
    let mut cx = Context::from_waker(std::task::Waker::noop());
    for step in 0..20 {
        host.now.set(Duration::from_secs(step * 100));
        assert!(task.as_mut().poll(&mut cx).is_pending());
    }
    assert_eq!(host.opens.get(), 6);
    assert!(
        host.events
            .borrow()
            .iter()
            .any(|event| event["state"] == "retry_exhausted")
    );
    cancel.send(()).unwrap();
    assert!(task.as_mut().poll(&mut cx).is_ready());
}

mod leases;
