//! Original join cutoff while a genuine pre-handoff terminal is blocked in FrameQueue.

use super::*;
use std::task::Poll;
use tokio::io::AsyncWrite;

struct Observation {
    completed: Option<io::ErrorKind>,
    terminal: bool,
    held: bool,
}

/// A blocked original terminal must not turn its completed negotiation into an indefinite wait.
#[tokio::test]
async fn prehandoff_terminal_forwarding_keeps_the_original_join_cutoff() {
    tokio::time::timeout(
        protocol::NATIVE_JOIN_TIMEOUT + Duration::from_secs(20),
        async {
            let flushed = forward(false).await;
            assert!(flushed.terminal && flushed.held);
            assert_eq!(flushed.completed, Some(io::ErrorKind::InvalidData));

            let expired = forward(true).await;
            assert!(
                expired.held,
                "the original account lease escaped its socket"
            );
            assert_eq!(
                expired.completed,
                Some(io::ErrorKind::TimedOut),
                "the actual terminal forwarding ignored its original join cutoff"
            );
        },
    )
    .await
    .expect("actual terminal forwarding fixture stalled");
}

/// Uses actual UDP negotiation and a one-byte duplex writer, without fabricating a terminal owner.
async fn forward(expire: bool) -> Observation {
    let peer = peer::Peer::new().await;
    let address = peer.socket.local_addr().unwrap();
    let registry = Registry::new();
    let attachment = registry
        .open_control(Ipv4Addr::LOCALHOST.into(), "", Instant::now())
        .unwrap();
    let session = Arc::clone(&attachment.session);
    let generation = attachment.generation;
    let lease = session.begin_play(generation, 1).await.unwrap();
    let forwarding = async {
        let connection = RakNetConnection::connect(address, RakNetOptions::default())
            .await
            .unwrap();
        let connection = NativeRelayConnecting::from_raknet(connection, address.to_string());
        let mut playing = Playing {
            socket: Some(Socket::Connecting(connection)),
            lease,
        };
        let random = protocol::ClientRandomId::new(1).unwrap();
        let value = protocol::login_client_data(
            "Fixture",
            random,
            None,
            &protocol::LoginSettings::default(),
        );
        let raw = serde_json::value::RawValue::from_string(serde_json::to_string(&value).unwrap())
            .unwrap();
        let claims = RawClientData::from_raw(&raw).unwrap();
        let claims = session
            .validate_guest(claims, protocol::PROTOCOL_VERSION)
            .unwrap();
        let claims = session.guest_claims(claims).unwrap();
        let deadline = Instant::now()
            .checked_add(protocol::NATIVE_JOIN_TIMEOUT)
            .unwrap();
        let (local, browser) = tokio::io::duplex(1);
        let (read, write) = tokio::io::split(local);
        let (blocked, observed_block) = oneshot::channel();
        let (reader, frames, writer) = bridge::tasks::prepare_bounded_connection(
            read,
            Observed {
                write,
                blocked: Some(blocked),
            },
            bridge::MAX_FRAME_LEN,
            RELAY_QUEUED_FRAME_BYTES,
        );
        let (completed, terminal) = {
            let negotiating = playing.negotiate(reader, frames, claims, false, None, deadline);
            tokio::pin!(negotiating, writer);
            tokio::select! { biased;
                observed = observed_block => observed.expect("actual terminal write never blocked"),
                result = &mut negotiating => panic!("negotiation ended before its actual terminal blocked: {result:?}"),
                () = &mut writer => panic!("actual frame writer ended before backpressure"),
            }
            if expire {
                // Real UDP/Settings/Login and the terminal write have already happened.
                tokio::time::pause();
                let remaining =
                    deadline.saturating_duration_since(tokio::time::Instant::now().into_std());
                tokio::time::advance(remaining + Duration::from_nanos(1)).await;
                // Advancing paused time alone does not guarantee expired timers were dispatched.
                let completed = tokio::time::timeout_at(
                    tokio::time::Instant::from_std(deadline) + Duration::from_secs(1),
                    &mut negotiating,
                )
                .await;
                tokio::time::resume();
                let completed = match completed {
                    Ok(Err(error)) => Some(error.kind()),
                    Ok(Ok(())) => panic!("refused Login completed successfully"),
                    Err(_) => None,
                };
                (completed, false)
            } else {
                let mut browser =
                    FramedRead::new(browser, BridgeCodec::with_max(bridge::MAX_FRAME_LEN));
                let terminal = tokio::select! { biased;
                    frame = browser.next() => frame.unwrap().unwrap(),
                    () = &mut writer => panic!("frame writer ended before its actual terminal was read"),
                };
                let terminal = matches!(
                    session_wire::decode_core_message(terminal).unwrap(),
                    CoreMessage::Disconnect(_)
                );
                let result = tokio::select! { biased;
                    result = &mut negotiating => result,
                    () = &mut writer => panic!("frame writer ended before its flush receipt"),
                };
                (Some(result.unwrap_err().kind()), terminal)
            }
        };
        let held = playing.lease.current().is_ok();
        playing.close().await.unwrap();
        drop(playing);
        let newer = session.begin_play(generation, 2).await.unwrap();
        assert!(newer.current().is_ok());
        drop(newer);
        Observation {
            completed,
            terminal,
            held,
        }
    };
    let ((), observation) = tokio::join!(peer.refuse_login(), forwarding);
    drop(attachment);
    registry.close();
    observation
}

/// Reports only actual write backpressure; it never delays or gates the underlying stream.
struct Observed<W> {
    write: W,
    blocked: Option<oneshot::Sender<()>>,
}

impl<W: AsyncWrite + Unpin> AsyncWrite for Observed<W> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut self.write).poll_write(cx, bytes);
        if result.is_pending()
            && let Some(blocked) = self.blocked.take()
        {
            let _ = blocked.send(());
        }
        result
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.write).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.write).poll_shutdown(cx)
    }
}
