//! Actual public UDP negotiation and browser TCP EOF under real output backpressure.

use super::*;
use bedrock_net::raknet::{RakNetConnection, RakNetOptions};
use bridge::{BridgeCodec, contract::KIND_HANDOFF};
use futures::StreamExt;
use std::{
    net::Ipv4Addr,
    pin::Pin,
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncWrite, AsyncWriteExt},
    net::{TcpSocket, tcp::OwnedWriteHalf},
    sync::oneshot,
};
use tokio_util::codec::FramedRead;

mod peer;
mod semantic;
mod terminal;
mod upload;

/// EOF on an empty upload retires the actual generation even while a native output write is blocked.
/// This private preconnected fixture does not enable the held HTTP play route or authorize metadata.
#[tokio::test]
async fn empty_upload_half_eof_retires_the_actual_pump_while_output_is_blocked() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let peer = peer::Peer::new().await;
        let address = peer.socket.local_addr().unwrap();
        let listener = TcpSocket::new_v4().unwrap();
        listener.set_send_buffer_size(1024).unwrap();
        listener.bind((Ipv4Addr::LOCALHOST, 0).into()).unwrap();
        let listener = listener.listen(1).unwrap();
        let browser = TcpSocket::new_v4().unwrap();
        browser.set_recv_buffer_size(1024).unwrap();
        let (browser, accepted) = tokio::join!(browser.connect(listener.local_addr().unwrap()), listener.accept());
        let browser = browser.unwrap();
        let accepted = accepted.unwrap().0;
        let (browser_read, mut browser_write) = browser.into_split();
        let (read, write) = accepted.into_split();
        let (blocked, observed_block) = oneshot::channel();
        let armed = Arc::new(AtomicBool::new(false));
        let write = ObservedWrite { write, blocked: Some(blocked), armed: Arc::clone(&armed) };
        let (reader, frames, writer) = bridge::tasks::prepare_bounded_connection(read, write, bridge::MAX_FRAME_LEN, RELAY_QUEUED_FRAME_BYTES);
        let (retired, observed_retirement) = oneshot::channel();
        let registry = Registry::new();
        let attachment = registry.open_control(Ipv4Addr::LOCALHOST.into(), "", Instant::now()).unwrap();
        let session = Arc::clone(&attachment.session);
        let generation = attachment.generation;
        let lease = session.begin_play(generation, 1).await.unwrap();
        let serving = async {
            let connection = RakNetConnection::connect(address, RakNetOptions::default()).await.unwrap();
            let connection = NativeRelayConnecting::from_raknet(connection, address.to_string());
            let mut playing = Playing { socket: Some(Socket::Connecting(connection)), lease };
            let random = protocol::ClientRandomId::new(1).unwrap();
            let value = protocol::login_client_data("Fixture", random, None, &protocol::LoginSettings::default());
            let raw = serde_json::value::RawValue::from_string(serde_json::to_string(&value).unwrap()).unwrap();
            let claims = RawClientData::from_raw(&raw).unwrap();
            let claims = session.validate_guest(claims, protocol::PROTOCOL_VERSION).unwrap();
            let claims = session.guest_claims(claims).unwrap();
            let deadline = Instant::now().checked_add(protocol::NATIVE_JOIN_TIMEOUT).unwrap();
            let result = {
                let negotiating = playing.negotiate(reader, frames, claims, false, None, deadline);
                tokio::pin!(negotiating, writer);
                tokio::select! { biased;
                    result = &mut negotiating => result,
                    () = &mut writer => panic!("browser writer ended before the original empty-upload EOF"),
                }
            };
            assert!(result.is_err(), "actual browser EOF must end its play attempt");
            assert!(playing.lease.current().is_ok());
            let newer = session.begin_play(generation, 2);
            tokio::pin!(newer);
            assert!(futures::poll!(&mut newer).is_pending(), "new join stole the lease before physical upstream retirement");
            playing.close().await.unwrap();
            playing.close().await.unwrap();
            assert!(playing.lease.current().is_ok(), "cleanup must retain the exact account lease");
            drop(playing);
            let newer = newer.await.unwrap();
            assert!(newer.current().is_ok());
            drop(newer);
            retired.send(()).unwrap();
        };
        let browser = async {
            let mut frames = FramedRead::new(browser_read, BridgeCodec::with_max(bridge::MAX_FRAME_LEN));
            let handoff = frames.next().await.unwrap().unwrap();
            assert_eq!(handoff.first(), Some(&KIND_HANDOFF));
            armed.store(true, Ordering::Release);
            observed_block.await.expect("actual TCP output never became pending");
            browser_write.shutdown().await.unwrap();
            // Keep the actual read half open and unread while the gateway owns its blocked output.
            observed_retirement.await.unwrap();
            drop(frames);
        };
        tokio::join!(peer.serve(), serving, browser);
        drop(attachment);
        registry.close();
    }).await.expect("real native/browser pump EOF fixture stalled");
}

/// Observes only the real socket's Pending write; it neither gates nor replaces socket progress.
struct ObservedWrite {
    write: OwnedWriteHalf,
    blocked: Option<oneshot::Sender<()>>,
    armed: Arc<AtomicBool>,
}

impl AsyncWrite for ObservedWrite {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut self.write).poll_write(cx, bytes);
        if result.is_pending()
            && self.armed.load(Ordering::Acquire)
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
