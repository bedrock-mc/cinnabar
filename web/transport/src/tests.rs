use super::session::{AccountOperation, ControlEvent};
use super::*;
use bridge::{BridgeCodec, contract::*};
use bytes::{Bytes, BytesMut};
use futures::StreamExt;
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    io,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_util::codec::{Decoder, Encoder};

struct Fake {
    channels: VecDeque<Vec<u8>>,
    failed: bool,
    stall_play: bool,
    closed: Rc<Cell<bool>>,
    written: Rc<RefCell<Vec<u8>>>,
}
struct Stream {
    input: VecDeque<u8>,
    pending: bool,
    written: Rc<RefCell<Vec<u8>>>,
}
impl AsyncRead for Stream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        out: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.pending && self.input.is_empty() {
            return Poll::Pending;
        }
        for _ in 0..out.remaining().min(self.input.len()).min(2) {
            let Some(byte) = self.input.pop_front() else {
                break;
            };
            out.put_slice(&[byte]);
        }
        Poll::Ready(Ok(()))
    }
}
impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.written.borrow_mut().extend_from_slice(bytes);
        Poll::Ready(Ok(bytes.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
impl RelayTransport for Fake {
    type Stream = Stream;
    async fn open(&mut self, channel: Channel) -> Result<Stream, CloseReason> {
        if self.stall_play && matches!(channel, Channel::Play) {
            return std::future::pending().await;
        }
        if self.failed {
            return Err(CloseReason::Transport);
        }
        let input = self.channels.pop_front().unwrap_or_default();
        Ok(Stream {
            pending: input.is_empty(),
            input: input.into(),
            written: self.written.clone(),
        })
    }
    fn max_datagram_size(&self) -> Option<usize> {
        None
    }
    async fn send_datagram(&mut self, _: &[u8]) -> Result<(), CloseReason> {
        Err(CloseReason::Protocol)
    }
    async fn recv_datagram(&mut self) -> Result<Option<Vec<u8>>, CloseReason> {
        Ok(None)
    }
    fn close(&mut self, _: CloseReason) {
        self.closed.set(true);
    }
}
/// Encodes a peer fixture through the actual shared codec.
fn wire(value: Value) -> Vec<u8> {
    let mut buffer = BytesMut::new();
    BridgeCodec::with_max(RELAY_CONTROL_BYTES)
        .encode(Bytes::from(value.to_string()), &mut buffer)
        .unwrap();
    buffer.to_vec()
}
/// Supplies the relay's negotiated fields with arbitrary opaque bearer fixtures.
fn capability(kind: &str, token: &str, seconds: u64) -> Value {
    json!({"type": kind, "version": RELAY_VERSION, "resume": token, "generation": 7, "expires_seconds": seconds})
}
/// Builds an in-memory reliable channel fixture with observable close and wire output.
fn fake(input: Vec<u8>, failed: bool) -> Fake {
    Fake {
        channels: [input, vec![0, 0, 0, 1, KIND_BATCH]].into(),
        failed,
        stall_play: false,
        closed: Rc::new(Cell::new(false)),
        written: Rc::new(RefCell::new(Vec::new())),
    }
}
/// Decodes all complete client frames for behavioral assertions.
fn sent(output: &Rc<RefCell<Vec<u8>>>) -> Vec<Value> {
    let mut bytes = BytesMut::from(output.borrow().as_slice());
    let mut codec = BridgeCodec::with_max(RELAY_CONTROL_BYTES);
    let mut frames = Vec::new();
    while let Some(frame) = codec.decode(&mut bytes).unwrap() {
        frames.push(serde_json::from_slice(&frame).unwrap());
    }
    frames
}

#[tokio::test]
async fn selection_closes_failed_quic_and_never_falls_back_on_refusal() {
    let failed = fake(Vec::new(), true);
    let closed = failed.closed.clone();
    let mut session = Session::new();
    select(
        &mut session,
        Some(failed),
        || {
            assert!(closed.get());
            fake(wire(capability(RELAY_EVENT_WELCOME, "a", 20)), false)
        },
        || Duration::ZERO,
        std::future::pending,
        || Duration::ZERO,
    )
    .await
    .unwrap();
    assert_eq!(session.state(), State::Ready(7));
    session.close(CloseReason::Transport);
    let refused = fake(
        wire(json!({"type": RELAY_EVENT_ERROR, "error": "resume_refused"})),
        false,
    );
    assert_eq!(
        select(
            &mut session,
            Some(refused),
            || panic!("refusal must not fall back"),
            || Duration::from_secs(1),
            std::future::pending,
            || Duration::ZERO,
        )
        .await,
        Err(CloseReason::Refused)
    );
    assert_eq!(session.refusal(), Some(RefusalCode::ResumeRefused));
    assert_eq!(session.next_retry(), Err(CloseReason::Refused));
    session.fresh_session();
    let stalled = fake(Vec::new(), false);
    let closed = stalled.closed.clone();
    select(
        &mut session,
        Some(stalled),
        || {
            assert!(closed.get());
            fake(wire(capability(RELAY_EVENT_WELCOME, "b", 20)), false)
        },
        || Duration::from_secs(2),
        || std::future::ready(()),
        || Duration::ZERO,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn both_candidates_use_the_codec_and_generation_prefix() {
    for quic in [true, false] {
        let transport = fake(wire(capability(RELAY_EVENT_WELCOME, "a", 20)), false);
        let output = transport.written.clone();
        let mut candidate = Some(transport);
        let mut session = Session::new();
        let first = if quic { candidate.take() } else { None };
        select(
            &mut session,
            first,
            || candidate.take().unwrap(),
            || Duration::ZERO,
            std::future::pending,
            || Duration::ZERO,
        )
        .await
        .unwrap();
        let (mut read, frames, writer) = session
            .open_play(|| Duration::ZERO, std::future::pending())
            .await
            .unwrap();
        assert_eq!(read.next().await.unwrap().unwrap(), [KIND_BATCH][..]);
        assert!(
            session
                .open_play(|| Duration::ZERO, std::future::pending())
                .await
                .is_err()
        );
        let requests = sent(&output);
        let contract: Value =
            serde_json::from_str(include_str!("../../../crates/bridge/data/contract.json"))
                .unwrap();
        let version = contract["constants"]
            .as_array()
            .unwrap()
            .iter()
            .find(|constant| constant["rust_name"] == "RELAY_VERSION")
            .unwrap();
        assert_eq!(requests[0]["version"], version["value"]);
        assert_eq!(
            requests[0]["channels"],
            json!([RELAY_CHANNEL_CONTROL, RELAY_CHANNEL_PLAY])
        );
        assert_eq!(requests[1]["channel"], RELAY_CHANNEL_PLAY);
        assert_eq!(requests[1]["generation"], 7);
        assert_eq!(requests[1]["resume"], "a");
        drop(writer);
        assert!(
            frames
                .send(Bytes::from_static(&[KIND_BATCH]))
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn rotations_echo_received_bearers_and_keep_independent_expiry() {
    let mut input = wire(capability(RELAY_EVENT_WELCOME, "a", 20));
    for (token, seconds) in [("b", 30), ("a", 18), ("b", 29)] {
        input.extend(wire(capability(RELAY_EVENT_PING, token, seconds)));
    }
    let transport = fake(input, false);
    let output = transport.written.clone();
    let mut session = Session::new();
    select(
        &mut session,
        None,
        || transport,
        || Duration::from_secs(10),
        std::future::pending,
        || Duration::ZERO,
    )
    .await
    .unwrap();
    for now in [11, 12, 13] {
        assert_eq!(
            session
                .next_event(|| Duration::from_secs(now))
                .await
                .unwrap(),
            ControlEvent::Keepalive
        );
    }
    assert_eq!(session.resume(Duration::from_secs(40)), "b");
    assert_eq!(session.resume(Duration::from_secs(41)), "");
    let requests = sent(&output);
    assert_eq!(
        requests
            .iter()
            .skip(1)
            .map(|v| v["resume"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["b", "a", "b"]
    );
    session.close(CloseReason::Transport);
    session.desire_account(AccountOperation::SignIn);
    session.desire_account(AccountOperation::SignOut);
    let transport = fake(wire(capability(RELAY_EVENT_WELCOME, "c", 20)), false);
    let output = transport.written.clone();
    select(
        &mut session,
        None,
        || transport,
        || Duration::from_secs(20),
        std::future::pending,
        || Duration::ZERO,
    )
    .await
    .unwrap();
    session.flush_account().await.unwrap();
    session.flush_account().await.unwrap();
    let requests = sent(&output);
    assert_eq!(requests[0]["resume"], "b");
    assert_eq!(requests[1]["op"], RELAY_OP_SIGN_OUT);
    assert_eq!(requests.len(), 2);
}

#[tokio::test]
async fn unknown_events_are_counted_but_malformed_frames_end_the_session() {
    for bad in [
        vec![0, 0, 0, 0],
        vec![0, 0, 0, 3, b'{'],
        wire(json!({"type": RELAY_EVENT_PING})),
    ] {
        let mut input = wire(capability(RELAY_EVENT_WELCOME, "a", 20));
        input.extend(wire(json!({"type": "future_optional", "extra": true})));
        input.extend(wire(
            json!({"type": RELAY_EVENT_ACCOUNT, "account": {"state": "signed_out"}}),
        ));
        input.extend(bad);
        let transport = fake(input, false);
        let closed = transport.closed.clone();
        let mut session = Session::new();
        select(
            &mut session,
            None,
            || transport,
            || Duration::ZERO,
            std::future::pending,
            || Duration::ZERO,
        )
        .await
        .unwrap();
        assert_eq!(
            session.next_event(|| Duration::ZERO).await.unwrap(),
            ControlEvent::Account(serde_json::from_value(json!({"state": "signed_out"})).unwrap())
        );
        assert_eq!(session.unknown_events, 1);
        assert_eq!(
            session.next_event(|| Duration::ZERO).await,
            Err(CloseReason::Protocol)
        );
        assert!(closed.get());
        assert_eq!(session.state(), State::Offline);
    }
}

#[test]
fn reconnect_is_bounded_and_deliberate_close_is_terminal() {
    let mut session: Session<Fake> = Session::new();
    assert!(session.retry(Duration::ZERO, Duration::ZERO));
    session.close(CloseReason::Transport);
    assert!(!session.retry(Duration::ZERO, Duration::ZERO));
    for attempt in 1..super::session::RECONNECT_ATTEMPTS {
        assert!(session.retry(
            Duration::from_secs(u64::from(attempt) * 100),
            Duration::ZERO
        ));
        session.close(CloseReason::Transport);
    }
    assert!(!session.retry(Duration::from_secs(1000), Duration::ZERO));
    session.close(CloseReason::Cancelled);
    assert!(!session.retry(Duration::from_secs(2000), Duration::ZERO));
}

#[tokio::test]
async fn received_capabilities_use_observation_time_and_closed_stays_terminal() {
    let mut input = wire(capability(RELAY_EVENT_WELCOME, "welcome", 120));
    input.extend(wire(capability(RELAY_EVENT_PING, "ping", 120)));
    let transport = fake(input, false);
    let mut session = Session::new();
    let calls = Cell::new(0);
    select(
        &mut session,
        None,
        || transport,
        || {
            let call = calls.get();
            calls.set(call + 1);
            Duration::from_secs(if call < 2 { 0 } else { 30 })
        },
        std::future::pending,
        || Duration::ZERO,
    )
    .await
    .unwrap();
    assert_eq!(
        calls.get(),
        3,
        "sample admission, Hello and received Welcome separately"
    );
    assert_eq!(session.resume(Duration::from_secs(149)), "welcome");
    assert_eq!(session.resume(Duration::from_secs(150)), "");
    assert_eq!(
        session
            .next_event(|| Duration::from_secs(60))
            .await
            .unwrap(),
        ControlEvent::Keepalive
    );
    assert_eq!(session.resume(Duration::from_secs(179)), "ping");
    assert_eq!(session.resume(Duration::from_secs(180)), "");
    session.desire_account(AccountOperation::SignIn);
    session.close(CloseReason::Cancelled);
    assert!(session.next_event(|| Duration::ZERO).await.is_err());
    assert!(session.flush_account().await.is_err());
    session.close(CloseReason::Refused);
    assert_eq!(session.state(), State::Closed);
    assert!(!session.retry(Duration::from_secs(1000), Duration::ZERO));
    assert!(
        select(
            &mut session,
            None,
            || panic!("closed session must not open"),
            || Duration::from_secs(1000),
            std::future::pending,
            || Duration::ZERO,
        )
        .await
        .is_err()
    );
    assert_eq!(session.state(), State::Closed);
}

#[tokio::test]
async fn early_retry_reports_due_time_and_a_play_open_deadline_ends_only_that_join() {
    let mut transport = fake(
        wire(capability(RELAY_EVENT_WELCOME, "retained", 120)),
        false,
    );
    transport.stall_play = true;
    let mut session = Session::new();
    select(
        &mut session,
        None,
        || transport,
        || Duration::ZERO,
        std::future::pending,
        || Duration::from_millis(250),
    )
    .await
    .unwrap();
    assert!(matches!(
        session
            .open_play(|| Duration::ZERO, std::future::ready(()))
            .await,
        Err(CloseReason::Transport)
    ));
    assert_eq!(session.state(), State::Offline);
    assert_eq!(session.resume(Duration::ZERO), "retained");
    let due = Duration::from_millis(1250);
    assert_eq!(session.next_retry(), Ok(due));
    assert_eq!(
        select(
            &mut session,
            None,
            || panic!("early wake cannot open a socket"),
            || Duration::from_millis(1249),
            std::future::pending,
            || Duration::ZERO
        )
        .await,
        Err(CloseReason::RetryAt(due))
    );
    assert_eq!(session.next_retry(), Ok(due));
}

#[tokio::test]
async fn sequential_joins_reopen_control_with_the_retained_account() {
    let transport = fake(
        wire(capability(RELAY_EVENT_WELCOME, "retained", 120)),
        false,
    );
    let mut session = Session::new();
    select(
        &mut session,
        None,
        || transport,
        || Duration::ZERO,
        std::future::pending,
        || Duration::ZERO,
    )
    .await
    .unwrap();
    let (reader, frames, writer) = session
        .open_play(|| Duration::ZERO, std::future::pending())
        .await
        .unwrap();
    drop((reader, frames, writer));
    session.finish_play().unwrap();
    assert_eq!(session.state(), State::Offline);
    let mut welcome = capability(RELAY_EVENT_WELCOME, "rotated", 120);
    welcome["generation"] = json!(8);
    let transport = fake(wire(welcome), false);
    let output = transport.written.clone();
    select(
        &mut session,
        None,
        || transport,
        || Duration::from_secs(1),
        std::future::pending,
        || Duration::ZERO,
    )
    .await
    .unwrap();
    assert_eq!(session.state(), State::Ready(8));
    assert_eq!(sent(&output)[0]["resume"], "retained");
    assert_eq!(
        sent(&output).len(),
        1,
        "a new generation never replays gameplay"
    );
}

#[tokio::test]
async fn expired_resume_requires_a_deliberate_fresh_session() {
    let mut session = Session::new();
    let transport = fake(wire(capability(RELAY_EVENT_WELCOME, "expires", 1)), false);
    select(
        &mut session,
        None,
        || transport,
        || Duration::ZERO,
        std::future::pending,
        || Duration::ZERO,
    )
    .await
    .unwrap();
    session.close(CloseReason::Transport);
    let transport = fake(
        wire(capability(RELAY_EVENT_WELCOME, "must-not-create", 120)),
        false,
    );
    let output = transport.written.clone();
    assert_eq!(
        select(
            &mut session,
            Some(transport),
            || panic!("expiry cannot fall back"),
            || Duration::from_secs(2),
            std::future::pending,
            || Duration::ZERO
        )
        .await,
        Err(CloseReason::Refused)
    );
    assert_eq!(session.refusal(), Some(RefusalCode::ResumeRefused));
    assert!(
        output.borrow().is_empty(),
        "local expiry never sends a fresh Hello"
    );
    session.fresh_session();
    assert_eq!(session.next_retry(), Ok(Duration::ZERO));
}

#[tokio::test]
async fn admission_refusal_keeps_the_account_for_an_explicit_retry() {
    let mut session = Session::new();
    let transport = fake(wire(capability(RELAY_EVENT_WELCOME, "account", 120)), false);
    select(
        &mut session,
        None,
        || transport,
        || Duration::ZERO,
        std::future::pending,
        || Duration::ZERO,
    )
    .await
    .unwrap();
    session.close(CloseReason::Transport);
    let transport = fake(
        wire(json!({"type":"error", "error":"admission_refused"})),
        false,
    );
    assert_eq!(
        select(
            &mut session,
            None,
            || transport,
            || Duration::from_secs(1),
            std::future::pending,
            || Duration::ZERO
        )
        .await,
        Err(CloseReason::Refused)
    );
    assert_eq!(session.resume(Duration::from_secs(1)), "account");
    assert_eq!(session.next_retry(), Err(CloseReason::Refused));
    session.restart_retries();
    let transport = fake(wire(capability(RELAY_EVENT_WELCOME, "rotated", 120)), false);
    let output = transport.written.clone();
    select(
        &mut session,
        None,
        || transport,
        || Duration::from_secs(2),
        std::future::pending,
        || Duration::ZERO,
    )
    .await
    .unwrap();
    assert_eq!(sent(&output)[0]["resume"], "account");
}
